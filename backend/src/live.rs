//! The live channel: one WebSocket per open tab, carrying notices rather than
//! content.
//!
//! What goes down it is "the comments on this publication changed", "Ivy is
//! in Midpoint", "this scene was just saved". Never the comments or the scene
//! themselves: a client that hears something changed fetches it through the
//! ordinary API, where the ordinary rules decide what it may see — spoilers,
//! filters, blocks and all. A notice cannot leak what a fetch would refuse.
//!
//! Rooms are `publication:<id>` (readers of one review) and `work:<id>` (the
//! people writing one book). Joining a room is checked against the same rules
//! as reading it. Everything is in memory: one process, like the rate limiter.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::routes::support::{self, Ability, AuthorCard};
use crate::state::AppState;

/// Tells a work's room that a scene's text changed, and who changed it — if
/// anyone is there to hear. Autosave calls this every couple of seconds, so an
/// empty room costs nothing, not even the lookup of a name.
pub async fn announce_save(state: &AppState, scene: &crate::db::models::Scene, user_id: &str) {
    if !state.hub.occupied(&format!("work:{}", scene.work_id)) {
        return;
    }
    let uid = user_id.to_string();
    let card = crate::db::blocking(&state.pool, move |conn| {
        Ok(support::author_cards(conn, std::slice::from_ref(&uid))?.remove(&uid))
    })
    .await;
    if let Ok(Some(by)) = card {
        state
            .hub
            .scene_saved(&scene.work_id, &scene.id, &scene.content_hash, by);
    }
}

/// How long a ticket stays good: long enough to open a socket, no longer.
const TICKET_LIFE: Duration = Duration::from_secs(30);
/// Rooms one connection may be in at once.
const MAX_ROOMS: usize = 16;
/// Notices queued for one slow connection before it is dropped.
const QUEUE: usize = 64;
/// How often a quiet socket is pinged, so a proxy does not close it as idle.
const PING_EVERY: Duration = Duration::from_secs(25);

struct Member {
    tx: mpsc::Sender<String>,
    person: Option<AuthorCard>,
    scene_id: Option<String>,
}

#[derive(Default)]
pub struct Hub {
    rooms: Mutex<HashMap<String, HashMap<u64, Member>>>,
    tickets: Mutex<HashMap<String, (Grant, Instant)>>,
    next: AtomicU64,
}

impl Hub {
    fn rooms(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<u64, Member>>> {
        self.rooms.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tells everyone in a room something happened. Never blocks a handler: a
    /// connection too far behind to take another notice is left to time out.
    pub fn publish(&self, room: &str, notice: Value) {
        let text = notice.to_string();
        if let Some(members) = self.rooms().get(room) {
            for member in members.values() {
                let _ = member.tx.try_send(text.clone());
            }
        }
    }

    /// The comments on a publication changed; readers should fetch them.
    pub fn comments_changed(&self, publication_id: &str) {
        let room = format!("publication:{publication_id}");
        self.publish(
            &room,
            json!({ "type": "comments", "room": room, "publicationId": publication_id }),
        );
    }

    /// A scene was saved. The hash lets the tab that saved it recognise its
    /// own save and stay quiet.
    pub fn scene_saved(&self, work_id: &str, scene_id: &str, hash: &str, by: AuthorCard) {
        let room = format!("work:{work_id}");
        self.publish(
            &room,
            json!({
                "type": "scene_saved", "room": room, "sceneId": scene_id,
                "contentHash": hash, "by": by,
            }),
        );
    }

    fn occupied(&self, room: &str) -> bool {
        self.rooms().get(room).is_some_and(|m| !m.is_empty())
    }

    fn presence(&self, room: &str) {
        let notice = {
            let rooms = self.rooms();
            let Some(members) = rooms.get(room) else {
                return;
            };
            if room.starts_with("work:") {
                // The people writing a book see each other by name, and where.
                let mut seen: HashMap<String, Value> = HashMap::new();
                for member in members.values() {
                    if let Some(person) = &member.person {
                        seen.insert(
                            person.id.clone(),
                            json!({ "id": person.id, "displayName": person.display_name, "sceneId": member.scene_id }),
                        );
                    }
                }
                json!({ "type": "presence", "room": room, "people": seen.into_values().collect::<Vec<_>>() })
            } else {
                // Readers of a review are a number. Who else is reading a draft
                // is not something one reader is owed about another.
                json!({ "type": "presence", "room": room, "readers": members.len() })
            }
        };
        self.publish(room, notice);
    }

    fn join(&self, room: &str, id: u64, member: Member) {
        self.rooms()
            .entry(room.to_string())
            .or_default()
            .insert(id, member);
        self.presence(room);
    }

    fn here(&self, room: &str, id: u64, scene_id: Option<String>) {
        if let Some(member) = self.rooms().get_mut(room).and_then(|m| m.get_mut(&id)) {
            member.scene_id = scene_id;
        }
        self.presence(room);
    }

    fn leave(&self, room: &str, id: u64) {
        let emptied = {
            let mut rooms = self.rooms();
            let Some(members) = rooms.get_mut(room) else {
                return;
            };
            members.remove(&id);
            let empty = members.is_empty();
            if empty {
                rooms.remove(room);
            }
            empty
        };
        if !emptied {
            self.presence(room);
        }
    }

    fn issue(&self, user: Option<AuthUser>, share_keys: Vec<String>) -> String {
        let ticket = crate::auth::generate_link_token();
        let mut tickets = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        tickets.retain(|_, (_, at)| now.duration_since(*at) < TICKET_LIFE);
        tickets.insert(ticket.clone(), (Grant { user, share_keys }, now));
        ticket
    }

    /// Spends a ticket.
    fn redeem(&self, ticket: &str) -> Option<Grant> {
        let mut tickets = self.tickets.lock().unwrap_or_else(|e| e.into_inner());
        let (grant, at) = tickets.remove(ticket)?;
        (at.elapsed() < TICKET_LIFE).then_some(grant)
    }

    #[cfg(test)]
    fn members(&self, room: &str) -> usize {
        self.rooms().get(room).map_or(0, |m| m.len())
    }
}

/// Who a ticket was issued to, and the share keys they held then: the socket
/// runs on its own task, away from the request that carried them.
struct Grant {
    user: Option<AuthUser>,
    share_keys: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LiveTicket {
    /// Single use, good for thirty seconds: `GET /api/live?ticket=…`.
    pub ticket: String,
}

/// A ticket to open the live channel with. A WebSocket cannot carry an
/// Authorization header from a browser, and a token in a URL ends up in logs,
/// so the socket is opened with this instead: single use, and dead in thirty
/// seconds whether or not it was used.
#[utoipa::path(
    post,
    path = "/api/live/ticket",
    tag = "live",
    responses(
        (status = 200, description = "A ticket", body = LiveTicket),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn ticket(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    headers: axum::http::HeaderMap,
) -> ApiResult<Json<LiveTicket>> {
    // A lapsed token is refused rather than read as nobody: an anonymous
    // ticket would be turned away from a private review's room without a
    // word, and the tab would never hear another note. Refused, the client
    // refreshes its token and asks again.
    if user.is_none() && headers.contains_key(axum::http::header::AUTHORIZATION) {
        return Err(ApiError::Unauthorized("that token has lapsed".into()));
    }
    Ok(Json(LiveTicket {
        ticket: state
            .hub
            .issue(user, crate::routes::share::keys_from(&headers)),
    }))
}

#[derive(Deserialize)]
pub struct Connect {
    ticket: String,
}

/// Opens the live channel. Not in the OpenAPI document: it is a WebSocket,
/// described in `docs/API.md`.
pub async fn connect(
    State(state): State<AppState>,
    Query(query): Query<Connect>,
    upgrade: WebSocketUpgrade,
) -> ApiResult<Response> {
    let grant = state
        .hub
        .redeem(&query.ticket)
        .ok_or_else(|| ApiError::Unauthorized("that ticket is spent or expired".into()))?;
    Ok(upgrade
        .max_message_size(4096)
        .on_upgrade(move |socket| serve(state, grant, socket)))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Incoming {
    Join {
        room: String,
        #[serde(default, rename = "sceneId")]
        scene_id: Option<String>,
    },
    Leave {
        room: String,
    },
    Here {
        room: String,
        #[serde(default, rename = "sceneId")]
        scene_id: Option<String>,
    },
}

async fn serve(state: AppState, grant: Grant, mut socket: WebSocket) {
    let Grant { user, share_keys } = grant;
    let id = state.hub.next.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel::<String>(QUEUE);
    let person = match &user {
        Some(u) => {
            let uid = u.id.clone();
            crate::db::blocking(&state.pool, move |conn| {
                Ok(support::author_cards(conn, std::slice::from_ref(&uid))?.remove(&uid))
            })
            .await
            .ok()
            .flatten()
        }
        None => None,
    };
    let mut joined: Vec<String> = Vec::new();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;

    loop {
        tokio::select! {
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            outgoing = rx.recv() => {
                let Some(text) = outgoing else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                let Ok(command) = serde_json::from_str::<Incoming>(&text) else {
                    let _ = tx.try_send(json!({ "type": "error", "message": "not a command this channel knows" }).to_string());
                    continue;
                };
                match command {
                    Incoming::Join { room, scene_id } => {
                        if joined.contains(&room) {
                            continue;
                        }
                        if joined.len() >= MAX_ROOMS {
                            let _ = tx.try_send(json!({ "type": "denied", "room": room, "message": "in too many rooms at once" }).to_string());
                            continue;
                        }
                        let check = may_join(&state, user.as_ref(), &room);
                        match crate::routes::share::PRESENTED.scope(share_keys.clone(), check).await {
                            Ok(()) => {
                                let _ = tx.try_send(json!({ "type": "joined", "room": room }).to_string());
                                state.hub.join(&room, id, Member { tx: tx.clone(), person: person.clone(), scene_id });
                                joined.push(room);
                            }
                            Err(e) => {
                                let _ = tx.try_send(json!({ "type": "denied", "room": room, "message": e.to_string() }).to_string());
                            }
                        }
                    }
                    Incoming::Leave { room } => {
                        if let Some(i) = joined.iter().position(|r| r == &room) {
                            joined.remove(i);
                            state.hub.leave(&room, id);
                        }
                    }
                    Incoming::Here { room, scene_id } => {
                        if joined.contains(&room) {
                            state.hub.here(&room, id, scene_id);
                        }
                    }
                }
            }
        }
    }

    for room in joined {
        state.hub.leave(&room, id);
    }
}

/// Joining a room is reading it: the same rules decide.
async fn may_join(state: &AppState, user: Option<&AuthUser>, room: &str) -> ApiResult<()> {
    let user = user.cloned();
    let room = room.to_string();
    crate::db::blocking(&state.pool, move |conn| match room.split_once(':') {
        Some(("publication", id)) => {
            support::publication_for_reader(conn, id, user.as_ref())?;
            Ok(())
        }
        Some(("work", id)) => {
            let user =
                user.ok_or_else(|| ApiError::Unauthorized("sign in to join a work".into()))?;
            support::work_for(conn, id, &user, Ability::Read)?;
            Ok(())
        }
        _ => Err(ApiError::BadRequest("no such room".into())),
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str) -> AuthorCard {
        AuthorCard {
            id: id.into(),
            display_name: id.to_uppercase(),
        }
    }

    #[tokio::test]
    async fn a_room_hears_its_own_notices_and_no_one_else_s() {
        let hub = Hub::default();
        let (a_tx, mut a_rx) = mpsc::channel(8);
        let (b_tx, mut b_rx) = mpsc::channel(8);
        hub.join(
            "work:w1",
            1,
            Member {
                tx: a_tx,
                person: Some(card("ada")),
                scene_id: Some("s1".into()),
            },
        );
        hub.join(
            "work:w2",
            2,
            Member {
                tx: b_tx,
                person: Some(card("bo")),
                scene_id: None,
            },
        );
        // Joining announces who is there, names and scenes for a work.
        let presence: Value = serde_json::from_str(&a_rx.recv().await.unwrap()).unwrap();
        assert_eq!(presence["people"][0]["displayName"], "ADA");
        assert_eq!(presence["people"][0]["sceneId"], "s1");
        b_rx.recv().await.unwrap();

        hub.scene_saved("w1", "s1", "hash", card("ada"));
        let saved: Value = serde_json::from_str(&a_rx.recv().await.unwrap()).unwrap();
        assert_eq!(saved["type"], "scene_saved");
        assert!(b_rx.try_recv().is_err(), "another work's room heard it");

        hub.leave("work:w1", 1);
        assert_eq!(hub.members("work:w1"), 0);
    }

    #[tokio::test]
    async fn readers_of_a_review_are_a_count_not_a_list() {
        let hub = Hub::default();
        let (tx, mut rx) = mpsc::channel(8);
        hub.join(
            "publication:p",
            1,
            Member {
                tx,
                person: Some(card("ada")),
                scene_id: None,
            },
        );
        let presence: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(presence["readers"], 1);
        assert!(presence.get("people").is_none());
    }

    #[test]
    fn a_ticket_is_good_once() {
        let hub = Hub::default();
        let ticket = hub.issue(None, Vec::new());
        assert!(hub.redeem(&ticket).is_some_and(|g| g.user.is_none()));
        assert!(hub.redeem(&ticket).is_none());
        assert!(hub.redeem("made-up").is_none());
    }
}

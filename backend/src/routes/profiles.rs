//! Author profiles, and following the people whose work you want to see next.
//!
//! A profile is assembled from rows that already exist — what someone has put
//! up for review, what they have said about other people's work — rather than
//! a second copy of either. The only thing it adds is a bio and a follow.
use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{comments, follows, publications, users, works};
use crate::domain::filter::Surface;
use crate::domain::moderation;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::publications::{feed_cards, FeedQuery, PublicationCard};
use super::support::{self, AuthorCard, Standing};

const ABOUT_LIMIT: usize = 3000;
const BIO_LIMIT: usize = 2_000;
const NAME_LIMIT: usize = 80;
/// Enough history to judge a reviewer by, without a profile turning into an
/// archive nobody scrolls.
const REVIEW_HISTORY: usize = 20;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    pub bio: Option<String>,
    pub joined_at: String,
    pub followers: i64,
    pub following: i64,
    /// Whether the person asking follows this author. Always false signed out.
    pub viewer_follows: bool,
    pub standing: Standing,
    /// What this author has on offer that the viewer could open — the same
    /// rule the feed applies, so a profile never lists a locked door.
    pub publications: Vec<PublicationCard>,
    /// Other people's work this person has given notes on, newest first.
    pub reviews: Vec<ReviewHistoryItem>,
}

/// One publication someone reviewed, as their profile shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHistoryItem {
    pub publication_id: String,
    pub work_title: String,
    pub author: AuthorCard,
    pub notes: i64,
    /// Notes the author marked resolved — that they acted on it.
    pub resolved: i64,
    pub last_at: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    /// An empty string clears it.
    #[serde(default)]
    pub bio: Option<String>,
    /// A paragraph about the writer, offered as back matter in any book's
    /// export. An empty string clears it.
    #[serde(default)]
    pub about_author: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FollowState {
    pub following: bool,
    pub followers: i64,
}

#[utoipa::path(
    get,
    path = "/api/users/{user_id}/profile",
    tag = "profiles",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Profile),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn profile(
    State(state): State<AppState>,
    MaybeAuthUser(viewer): MaybeAuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<Profile>> {
    let viewer_id = viewer.as_ref().map(|v| v.id.clone());
    let mut profile = blocking(&state.pool, {
        let user_id = user_id.clone();
        move |conn| {
            let user = support::find_user(conn, &user_id)?;
            // A deactivated or permanently banned account is not on the site.
            if viewer_id.as_deref() != Some(user.id.as_str())
                && crate::domain::restrictions::hidden_users(conn)?.contains(&user.id)
            {
                return Err(ApiError::NotFound("user not found".into()));
            }
            let followers = followers(conn, &user.id)?;
            let following: i64 = follows::table
                .filter(follows::follower_id.eq(&user.id))
                .count()
                .get_result(conn)?;
            let viewer_follows = match &viewer_id {
                Some(viewer) => {
                    follows::table
                        .filter(follows::follower_id.eq(viewer))
                        .filter(follows::followee_id.eq(&user.id))
                        .count()
                        .get_result::<i64>(conn)?
                        > 0
                }
                None => false,
            };
            let standing = support::standings(conn, std::slice::from_ref(&user.id))?
                .remove(&user.id)
                .unwrap_or_default();
            let reviews = review_history(conn, &user.id, viewer_id.as_deref())?;
            Ok(Profile {
                id: user.id,
                display_name: user.display_name,
                bio: user.bio,
                joined_at: user.created_at,
                followers,
                following,
                viewer_follows,
                standing,
                publications: Vec::new(),
                reviews,
            })
        }
    })
    .await?;

    profile.publications = feed_cards(
        &state,
        viewer,
        FeedQuery {
            author: Some(user_id),
            ..Default::default()
        },
    )
    .await?;
    Ok(Json(profile))
}

/// The notes someone has left on other people's work.
///
/// Only where the viewer could see the work too: public and platform
/// publications, or the viewer's own. A note on a draft shared with named
/// readers stays between them and the author — listing it on a profile would
/// say the draft exists, to people it was not shown to.
fn review_history(
    conn: &mut crate::db::Conn,
    reviewer_id: &str,
    viewer_id: Option<&str>,
) -> ApiResult<Vec<ReviewHistoryItem>> {
    let mut visible = vec!["public"];
    if viewer_id.is_some() {
        visible.push("platform");
    }
    let rows: Vec<(String, String, String, String, String)> = comments::table
        .inner_join(publications::table.on(publications::id.eq(comments::publication_id)))
        .inner_join(works::table.on(works::id.eq(publications::work_id)))
        .filter(comments::author_id.eq(reviewer_id))
        .filter(comments::state.ne("deleted"))
        // Replying under your own book is not reviewing.
        .filter(works::owner_id.ne(reviewer_id))
        .filter(
            publications::visibility
                .eq_any(visible)
                .or(works::owner_id.eq(viewer_id.unwrap_or_default())),
        )
        .order(comments::created_at.desc())
        .select((
            publications::id,
            works::title,
            works::owner_id,
            comments::state,
            comments::created_at,
        ))
        .load(conn)?;

    // Newest first, one entry per publication; the first row seen for each is
    // its latest note.
    let mut order: Vec<String> = Vec::new();
    let mut items: HashMap<String, ReviewHistoryItem> = HashMap::new();
    for (publication_id, work_title, owner_id, state, created_at) in rows {
        let item = items.entry(publication_id.clone()).or_insert_with(|| {
            order.push(publication_id.clone());
            ReviewHistoryItem {
                publication_id,
                work_title,
                author: AuthorCard {
                    id: owner_id,
                    display_name: String::new(),
                },
                notes: 0,
                resolved: 0,
                last_at: created_at,
            }
        });
        item.notes += 1;
        if state == "resolved" {
            item.resolved += 1;
        }
    }
    order.truncate(REVIEW_HISTORY);

    let ids: Vec<String> = order
        .iter()
        .filter_map(|id| items.get(id).map(|i| i.author.id.clone()))
        .collect();
    let cards = support::author_cards(conn, &ids)?;
    Ok(order
        .into_iter()
        .filter_map(|id| items.remove(&id))
        .map(|mut item| {
            if let Some(card) = cards.get(&item.author.id) {
                item.author = card.clone();
            }
            item
        })
        .collect())
}

fn followers(conn: &mut crate::db::Conn, user_id: &str) -> ApiResult<i64> {
    Ok(follows::table
        .filter(follows::followee_id.eq(user_id))
        .count()
        .get_result(conn)?)
}

#[utoipa::path(
    patch,
    path = "/api/me/profile",
    tag = "profiles",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Success", body = User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateProfileRequest>,
) -> ApiResult<Json<User>> {
    let display_name = match body.display_name.as_deref().map(str::trim) {
        Some("") => return Err(ApiError::BadRequest("display name is required".into())),
        Some(name) if name.chars().count() > NAME_LIMIT => {
            return Err(ApiError::BadRequest(format!(
                "display name must be at most {NAME_LIMIT} characters"
            )))
        }
        other => other.map(str::to_string),
    };
    let bio = body.bio.as_deref().map(str::trim).map(str::to_string);
    if bio
        .as_deref()
        .is_some_and(|b| b.chars().count() > BIO_LIMIT)
    {
        return Err(ApiError::BadRequest(format!(
            "a bio must be at most {BIO_LIMIT} characters"
        )));
    }

    let about_author = body
        .about_author
        .as_deref()
        .map(str::trim)
        .map(str::to_string);
    if about_author
        .as_deref()
        .is_some_and(|a| a.chars().count() > ABOUT_LIMIT)
    {
        return Err(ApiError::BadRequest(format!(
            "the about-the-author text must be at most {ABOUT_LIMIT} characters"
        )));
    }

    // Public text on a profile is screened like anything else public. A
    // profile can't be held back (it is already there), so a hold is a flag.
    let mut display_name = display_name;
    let mut bio = bio;
    let mut about_author = about_author;
    let mut merged = moderation::Screen::default();
    for (surface, field) in [
        (Surface::DisplayName, &mut display_name),
        (Surface::Bio, &mut bio),
        (Surface::AboutAuthor, &mut about_author),
    ] {
        if let Some(text) = field.as_deref().filter(|t| !t.is_empty()) {
            let screen = moderation::screen(&state, surface, &user.id, text).await?;
            *field = Some(screen.text.clone());
            merged = merged.merge(screen);
        }
    }
    merged.hold = false;

    let updated = blocking(&state.pool, move |conn| {
        let now = now_iso();
        if let Some(about) = &about_author {
            diesel::update(users::table.find(&user.id))
                .set(users::about_author.eq((!about.is_empty()).then_some(about)))
                .execute(conn)?;
        }
        if let Some(name) = &display_name {
            diesel::update(users::table.find(&user.id))
                .set(users::display_name.eq(name))
                .execute(conn)?;
        }
        if let Some(bio) = &bio {
            diesel::update(users::table.find(&user.id))
                .set(users::bio.eq((!bio.is_empty()).then_some(bio)))
                .execute(conn)?;
        }
        diesel::update(users::table.find(&user.id))
            .set(users::updated_at.eq(&now))
            .execute(conn)?;
        let provider = moderation::provider_name(conn)?;
        moderation::record(conn, &merged, "profile", &user.id, &user.id, &provider)?;
        support::find_user(conn, &user.id)
    })
    .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    put,
    path = "/api/users/{user_id}/follow",
    tag = "profiles",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Following", body = FollowState),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn follow(
    State(state): State<AppState>,
    user: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<FollowState>> {
    let followers = blocking(&state.pool, move |conn| {
        if user_id == user.id {
            return Err(ApiError::BadRequest("you cannot follow yourself".into()));
        }
        support::find_user(conn, &user_id)?;
        // Following someone who has shut their door on you would fill a feed
        // with work you cannot open.
        if support::blocked(conn, &user_id, &user.id)? {
            return Err(ApiError::Forbidden(
                "this author has asked that you not read their work".into(),
            ));
        }
        // Following twice is following.
        diesel::insert_or_ignore_into(follows::table)
            .values(&NewFollow {
                id: new_id(),
                follower_id: user.id.clone(),
                followee_id: user_id.clone(),
                created_at: now_iso(),
            })
            .execute(conn)?;
        followers(conn, &user_id)
    })
    .await?;
    Ok(Json(FollowState {
        following: true,
        followers,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/users/{user_id}/follow",
    tag = "profiles",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Not following", body = FollowState),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn unfollow(
    State(state): State<AppState>,
    user: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<FollowState>> {
    let followers = blocking(&state.pool, move |conn| {
        diesel::delete(
            follows::table
                .filter(follows::follower_id.eq(&user.id))
                .filter(follows::followee_id.eq(&user_id)),
        )
        .execute(conn)?;
        followers(conn, &user_id)
    })
    .await?;
    Ok(Json(FollowState {
        following: false,
        followers,
    }))
}

#[utoipa::path(
    get,
    path = "/api/me/following",
    tag = "profiles",
    responses(
        (status = 200, description = "Authors you follow", body = Vec<AuthorCard>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn following(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<AuthorCard>>> {
    let cards = blocking(&state.pool, move |conn| {
        let ids: Vec<String> = follows::table
            .filter(follows::follower_id.eq(&user.id))
            .order(follows::created_at.desc())
            .select(follows::followee_id)
            .load(conn)?;
        let mut cards = support::author_cards(conn, &ids)?;
        Ok(ids.iter().filter_map(|id| cards.remove(id)).collect())
    })
    .await?;
    Ok(Json(cards))
}

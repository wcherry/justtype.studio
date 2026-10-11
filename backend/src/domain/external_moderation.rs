//! External moderation: a hosted content-moderation platform, behind a trait.
//!
//! The platform classifies public text with AI and its own blocklists,
//! recommends an action, and has a review dashboard where outside moderators
//! work. Nothing here is specific to one vendor: [`ModerationProvider`] is the
//! whole surface, [`StreamProvider`] is the first implementation (field and
//! endpoint names follow the vendor's API docs once the account exists),
//! [`NoProvider`] is the default, and [`StubProvider`] runs entirely locally
//! for development and tests.
//!
//! What is sent is the text, the surface, our id for the thing and an opaque
//! author reference (an HMAC of the user id under a key only this server has),
//! so the platform can spot repeat offenders without knowing who they are.
//! Never an email, never a name beyond the display name or bio being checked,
//! and never private writing — [`CheckItem`] has no way to carry any.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

use crate::error::{ApiError, ApiResult};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How long a check may take before the post goes ahead and the check is
/// retried later: a platform that is down never stops anyone posting.
pub const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recommended {
    Keep,
    Flag,
    Remove,
}

#[derive(Debug, Clone)]
pub struct ProviderVerdict {
    pub recommended: Recommended,
    pub labels: Vec<String>,
    pub external_id: Option<String>,
}

/// What is sent about one thing.
#[derive(Debug, Clone)]
pub struct CheckItem {
    pub surface: String,
    pub subject_id: String,
    /// Opaque: see [`author_ref`].
    pub author_ref: String,
    pub text: String,
}

impl CheckItem {
    /// The request body, exactly as it goes out. One function, so a test that
    /// reads what the stub recorded is reading what the real one would send.
    pub fn body(&self) -> Value {
        json!({
            "surface": self.surface,
            "subjectId": self.subject_id,
            "authorRef": self.author_ref,
            "text": self.text,
        })
    }
}

/// The platform's name for a writer: an HMAC of their id, stable and
/// meaningless to anyone without the server's secret.
pub fn author_ref(server_secret: &str, user_id: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(server_secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(b"justtype.studio/moderation-author/v1:");
    mac.update(user_id.as_bytes());
    hex::encode(mac.finalize().into_bytes())[..32].to_string()
}

pub trait ModerationProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn check<'a>(&'a self, item: &'a CheckItem) -> BoxFuture<'a, ApiResult<ProviderVerdict>>;
    /// Tells the platform a decision made here, so its review queue closes.
    fn report_decision<'a>(
        &'a self,
        external_id: &'a str,
        decision: &'a str,
    ) -> BoxFuture<'a, ApiResult<()>>;
    /// Asks the platform to delete what it holds for an author reference.
    fn forget_author<'a>(&'a self, author_ref: &'a str) -> BoxFuture<'a, ApiResult<()>>;
}

/// The default: nothing is sent anywhere.
pub struct NoProvider;

impl ModerationProvider for NoProvider {
    fn name(&self) -> &'static str {
        "none"
    }
    fn check<'a>(&'a self, _: &'a CheckItem) -> BoxFuture<'a, ApiResult<ProviderVerdict>> {
        Box::pin(async {
            Ok(ProviderVerdict {
                recommended: Recommended::Keep,
                labels: vec![],
                external_id: None,
            })
        })
    }
    fn report_decision<'a>(&'a self, _: &'a str, _: &'a str) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn forget_author<'a>(&'a self, _: &'a str) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// A platform in the style of Stream's moderation product.
pub struct StreamProvider {
    pub client: reqwest::Client,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Deserialize)]
struct StreamCheck {
    #[serde(default)]
    id: Option<String>,
    /// `keep`, `flag` or `remove`.
    #[serde(default)]
    recommended_action: String,
    #[serde(default)]
    labels: Vec<String>,
}

impl StreamProvider {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url.trim_end_matches('/'))
    }

    async fn post(&self, path: &str, body: Value) -> ApiResult<reqwest::Response> {
        let response = self
            .client
            .post(self.url(path))
            .header("authorization", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| ApiError::internal(format!("the moderation platform: {e}")))?;
        if !response.status().is_success() {
            return Err(ApiError::internal(format!(
                "the moderation platform answered {}",
                response.status()
            )));
        }
        Ok(response)
    }
}

impl ModerationProvider for StreamProvider {
    fn name(&self) -> &'static str {
        "stream"
    }

    fn check<'a>(&'a self, item: &'a CheckItem) -> BoxFuture<'a, ApiResult<ProviderVerdict>> {
        Box::pin(async move {
            let response = self.post("/api/v2/moderation/check", item.body()).await?;
            let parsed: StreamCheck = response
                .json()
                .await
                .map_err(|e| ApiError::internal(format!("the moderation platform: {e}")))?;
            Ok(ProviderVerdict {
                recommended: match parsed.recommended_action.as_str() {
                    "remove" => Recommended::Remove,
                    "flag" => Recommended::Flag,
                    _ => Recommended::Keep,
                },
                labels: parsed.labels,
                external_id: parsed.id,
            })
        })
    }

    fn report_decision<'a>(
        &'a self,
        external_id: &'a str,
        decision: &'a str,
    ) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async move {
            self.post(
                "/api/v2/moderation/review",
                json!({ "id": external_id, "decision": decision }),
            )
            .await
            .map(|_| ())
        })
    }

    fn forget_author<'a>(&'a self, author_ref: &'a str) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async move {
            self.post(
                "/api/v2/moderation/forget",
                json!({ "authorRef": author_ref }),
            )
            .await
            .map(|_| ())
        })
    }
}

/// What the stub has been asked, newest last: `(endpoint, request body)`.
/// Tests read it to check what would have gone out.
pub static STUB_LOG: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());

/// Subjects the stub has already failed once, for `[[down-once]]`.
static DOWN_SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Runs locally and decides from markers in the text, so a test can say what
/// the platform should answer: `[[remove]]`, `[[flag]]`, `[[down]]` (an
/// error), `[[down-once]]` (an error the first time each thing is asked
/// about) or `[[slow]]` (longer than the timeout).
pub struct StubProvider;

impl StubProvider {
    fn log(endpoint: &str, body: Value) {
        STUB_LOG
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((endpoint.to_string(), body));
    }
}

impl ModerationProvider for StubProvider {
    fn name(&self) -> &'static str {
        "stub"
    }

    fn check<'a>(&'a self, item: &'a CheckItem) -> BoxFuture<'a, ApiResult<ProviderVerdict>> {
        Box::pin(async move {
            Self::log("check", item.body());
            if item.text.contains("[[slow]]") {
                tokio::time::sleep(CHECK_TIMEOUT * 3).await;
            }
            if item.text.contains("[[down]]") {
                return Err(ApiError::internal("the stub platform is down"));
            }
            if item.text.contains("[[down-once]]") {
                let mut seen = DOWN_SEEN.lock().unwrap_or_else(|e| e.into_inner());
                if !seen.contains(&item.subject_id) {
                    seen.push(item.subject_id.clone());
                    return Err(ApiError::internal("the stub platform is down, once"));
                }
            }
            let (recommended, labels) = if item.text.contains("[[remove]]") {
                (Recommended::Remove, vec!["harassment".to_string()])
            } else if item.text.contains("[[flag]]") {
                (Recommended::Flag, vec!["toxicity".to_string()])
            } else {
                (Recommended::Keep, vec![])
            };
            Ok(ProviderVerdict {
                recommended,
                labels,
                external_id: Some(format!("stub-{}", item.subject_id)),
            })
        })
    }

    fn report_decision<'a>(
        &'a self,
        external_id: &'a str,
        decision: &'a str,
    ) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async move {
            Self::log("review", json!({ "id": external_id, "decision": decision }));
            Ok(())
        })
    }

    fn forget_author<'a>(&'a self, author_ref: &'a str) -> BoxFuture<'a, ApiResult<()>> {
        Box::pin(async move {
            Self::log("forget", json!({ "authorRef": author_ref }));
            Ok(())
        })
    }
}

/// What `/admin/external` saves, in `moderation_settings` under `external`.
/// The two secrets are stored sealed (see `secrets`) and never sent back.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalConfig {
    /// `none`, `stream` or `stub`.
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub surfaces: Vec<String>,
    /// What the platform's *remove* does here: `hold` (the default) or `block`.
    #[serde(default = "default_remove_as")]
    pub remove_as: String,
    /// The scope a temporary ban or suspension from the platform is given:
    /// `discussion` (the default) or `site`.
    #[serde(default = "default_ban_scope")]
    pub ban_scope: String,
    #[serde(default, skip_serializing)]
    pub api_key_sealed: Option<String>,
    #[serde(default, skip_serializing)]
    pub webhook_secret_sealed: Option<String>,
}

fn default_remove_as() -> String {
    "hold".into()
}
fn default_ban_scope() -> String {
    "discussion".into()
}

impl Default for ExternalConfig {
    fn default() -> Self {
        ExternalConfig {
            provider: "none".into(),
            base_url: String::new(),
            surfaces: Vec::new(),
            remove_as: default_remove_as(),
            ban_scope: default_ban_scope(),
            api_key_sealed: None,
            webhook_secret_sealed: None,
        }
    }
}

impl ExternalConfig {
    pub fn enabled(&self) -> bool {
        self.provider != "none"
    }

    pub fn checks(&self, surface: &str) -> bool {
        self.enabled() && self.surfaces.iter().any(|s| s == surface)
    }

    pub fn provider(&self, server_secret: &str) -> ApiResult<Arc<dyn ModerationProvider>> {
        Ok(match self.provider.as_str() {
            "stub" => Arc::new(StubProvider),
            "stream" => {
                let key = match &self.api_key_sealed {
                    Some(sealed) => super::secrets::open(server_secret, sealed)?,
                    None => return Ok(Arc::new(NoProvider)),
                };
                Arc::new(StreamProvider {
                    client: reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .map_err(|e| ApiError::internal(e.to_string()))?,
                    base_url: self.base_url.clone(),
                    api_key: key,
                })
            }
            _ => Arc::new(NoProvider),
        })
    }

    pub fn webhook_secret(&self, server_secret: &str) -> Option<String> {
        self.webhook_secret_sealed
            .as_deref()
            .and_then(|s| super::secrets::open(server_secret, s).ok())
    }
}

/// Whether `signature` (hex) is the HMAC-SHA256 of `body` under `secret`.
pub fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let Ok(expected) = hex::decode(signature.trim()) else {
        return false;
    };
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

#[cfg(test)]
pub fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_author_reference_is_stable_opaque_and_not_the_id() {
        let a = author_ref("secret", "user-1");
        assert_eq!(a, author_ref("secret", "user-1"));
        assert_ne!(a, author_ref("secret", "user-2"));
        assert_ne!(a, author_ref("other", "user-1"));
        assert!(!a.contains("user-1"));
    }

    #[test]
    fn a_signature_verifies_only_for_the_same_body_and_secret() {
        let sig = sign("whsec", b"{\"a\":1}");
        assert!(verify_signature("whsec", b"{\"a\":1}", &sig));
        assert!(!verify_signature("whsec", b"{\"a\":2}", &sig));
        assert!(!verify_signature("nope", b"{\"a\":1}", &sig));
        assert!(!verify_signature("whsec", b"{\"a\":1}", "zz"));
    }

    #[test]
    fn the_request_body_carries_only_what_the_trait_allows() {
        let item = CheckItem {
            surface: "comment".into(),
            subject_id: "c1".into(),
            author_ref: "abc".into(),
            text: "hello".into(),
        };
        let body = item.body();
        let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 4);
        for k in ["surface", "subjectId", "authorRef", "text"] {
            assert!(body.get(k).is_some());
        }
    }
}

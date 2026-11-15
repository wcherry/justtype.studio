//! What stands between the public sign-up form and a script that wants ten
//! thousand accounts.
//!
//! Every public sign-up passes three things before its account is written:
//!
//! 1. **The address.** It has to look like one, and its domain must not be a
//!    throwaway-mail provider (the built-in list in `domain::bans`, plus the
//!    domains staff block). Those get the answer a taken address gets, as they
//!    always have, so the lists cannot be probed.
//! 2. **A CAPTCHA**, when Cloudflare Turnstile is configured. A valid token,
//!    or an address a writer invited, makes the sign-up *human-verified*. A
//!    bad token is refused; no token at all is only "not verified". If
//!    Cloudflare cannot be reached the sign-up counts as unverified, so an
//!    outage tightens the limits rather than closing sign-up.
//! 3. **Limits**, counted from `signup_events` so a restart does not reset
//!    them: per address per hour and per day, across the site per hour, and,
//!    with the CAPTCHA on, much lower ones for sign-ups that did not pass it.
//!
//! A limit is taken by writing the request's row first and counting after, so
//! two requests at once both see each other and neither slips past together.
//! The row is removed if the account is not made.
//!
//! The per-minute limit on the auth routes (`ops::per_address`) is still in
//! front of all this; it stops a burst, these stop a steady trickle.
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Duration, Utc};
use diesel::dsl::{count_star, min};
use diesel::prelude::*;
use serde::Deserialize;

use crate::db::schema::{signup_events, site_invites};
use crate::db::Conn;
use crate::error::{ApiError, ApiResult};

const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// Cloudflare Turnstile's two keys. The site key is public: the web app reads
/// it from `GET /api/auth/signup-policy`, so changing it needs no rebuild.
#[derive(Clone)]
pub struct TurnstileKeys {
    pub site_key: String,
    pub secret: String,
}

impl std::fmt::Debug for TurnstileKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnstileKeys")
            .field("site_key", &self.site_key)
            .field("secret", &"…")
            .finish()
    }
}

/// The sign-up limits. Zero turns one off.
#[derive(Debug, Clone)]
pub struct SignupConfig {
    /// Sign-ups from one address in any hour.
    pub ip_hourly: i64,
    /// Sign-ups from one address in any day.
    pub ip_daily: i64,
    /// Sign-ups across the site in any hour.
    pub global_hourly: i64,
    /// With the CAPTCHA on: sign-ups that did not pass it, from one address
    /// in any day.
    pub unverified_ip_daily: i64,
    /// With the CAPTCHA on: the same, across the site in any hour.
    pub unverified_global_hourly: i64,
    pub turnstile: Option<TurnstileKeys>,
}

impl Default for SignupConfig {
    fn default() -> Self {
        SignupConfig {
            ip_hourly: 3,
            ip_daily: 10,
            global_hourly: 60,
            unverified_ip_daily: 2,
            unverified_global_hourly: 10,
            turnstile: None,
        }
    }
}

fn limit_from_env(name: &str, default: i64) -> anyhow::Result<i64> {
    match std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        None => Ok(default),
        Some(v) => v.parse::<i64>().ok().filter(|n| *n >= 0).ok_or_else(|| {
            anyhow::anyhow!("{name} must be a whole number of sign-ups (0 for no limit), not `{v}`")
        }),
    }
}

impl SignupConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let d = Self::default();
        let key = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let turnstile = match (
            key("JUSTTYPE_TURNSTILE_SITE_KEY"),
            key("JUSTTYPE_TURNSTILE_SECRET_KEY"),
        ) {
            (Some(site_key), Some(secret)) => Some(TurnstileKeys { site_key, secret }),
            (None, None) => None,
            _ => anyhow::bail!(
                "JUSTTYPE_TURNSTILE_SITE_KEY and JUSTTYPE_TURNSTILE_SECRET_KEY go together: set both or neither"
            ),
        };
        Ok(SignupConfig {
            ip_hourly: limit_from_env("JUSTTYPE_SIGNUP_IP_HOURLY", d.ip_hourly)?,
            ip_daily: limit_from_env("JUSTTYPE_SIGNUP_IP_DAILY", d.ip_daily)?,
            global_hourly: limit_from_env("JUSTTYPE_SIGNUP_GLOBAL_HOURLY", d.global_hourly)?,
            unverified_ip_daily: limit_from_env(
                "JUSTTYPE_SIGNUP_UNVERIFIED_IP_DAILY",
                d.unverified_ip_daily,
            )?,
            unverified_global_hourly: limit_from_env(
                "JUSTTYPE_SIGNUP_UNVERIFIED_GLOBAL_HOURLY",
                d.unverified_global_hourly,
            )?,
            turnstile,
        })
    }

    /// Every limit off and no CAPTCHA: for tests that register many accounts
    /// from one address.
    #[cfg(test)]
    pub fn off() -> Self {
        SignupConfig {
            ip_hourly: 0,
            ip_daily: 0,
            global_hourly: 0,
            unverified_ip_daily: 0,
            unverified_global_hourly: 0,
            turnstile: None,
        }
    }

    /// One line for the startup log.
    pub fn summary(&self) -> String {
        let n = |v: i64| {
            if v == 0 {
                "no limit".to_string()
            } else {
                v.to_string()
            }
        };
        format!(
            "per address {}/hour {}/day, site {}/hour; CAPTCHA {}",
            n(self.ip_hourly),
            n(self.ip_daily),
            n(self.global_hourly),
            if self.turnstile.is_some() {
                format!(
                    "on (without it: {}/day per address, {}/hour site)",
                    n(self.unverified_ip_daily),
                    n(self.unverified_global_hourly)
                )
            } else {
                "off".to_string()
            },
        )
    }
}

/// What Cloudflare said about a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaOutcome {
    Passed,
    Failed,
    /// Cloudflare could not be asked, or refused our secret. Treated as no
    /// token, so an outage or a misconfiguration tightens the limits instead
    /// of turning everyone away.
    Unavailable,
}

enum Captcha {
    Off,
    Turnstile {
        secret: String,
        http: reqwest::Client,
    },
    /// Answers without the network, for tests.
    #[cfg(test)]
    Stub(CaptchaOutcome),
}

#[derive(Deserialize)]
struct SiteVerify {
    success: bool,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

pub struct SignupGuard {
    config: SignupConfig,
    captcha: Captcha,
}

fn limited(code: &'static str, message: &str, retry_after: Option<i64>) -> ApiError {
    ApiError::WithCode {
        status: StatusCode::TOO_MANY_REQUESTS,
        code,
        message: message.to_string(),
        retry_after,
    }
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl SignupGuard {
    pub fn new(config: SignupConfig) -> Self {
        let captcha = match &config.turnstile {
            Some(keys) => Captcha::Turnstile {
                secret: keys.secret.clone(),
                http: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .expect("an HTTP client"),
            },
            None => Captcha::Off,
        };
        SignupGuard { config, captcha }
    }

    /// A guard whose CAPTCHA always answers `outcome`.
    #[cfg(test)]
    pub fn with_stub_captcha(mut config: SignupConfig, outcome: CaptchaOutcome) -> Self {
        config.turnstile.get_or_insert(TurnstileKeys {
            site_key: "test-site-key".into(),
            secret: "test-secret".into(),
        });
        SignupGuard {
            config,
            captcha: Captcha::Stub(outcome),
        }
    }

    /// The key the web app shows the CAPTCHA with, when there is one.
    pub fn site_key(&self) -> Option<&str> {
        match self.captcha {
            Captcha::Off => None,
            _ => self.config.turnstile.as_ref().map(|k| k.site_key.as_str()),
        }
    }

    fn captcha_on(&self) -> bool {
        !matches!(self.captcha, Captcha::Off)
    }

    async fn check_token(&self, token: &str, ip: &str) -> CaptchaOutcome {
        let (secret, http) = match &self.captcha {
            Captcha::Off => return CaptchaOutcome::Unavailable,
            #[cfg(test)]
            Captcha::Stub(outcome) => return *outcome,
            Captcha::Turnstile { secret, http } => (secret, http),
        };
        let mut body = serde_json::json!({ "secret": secret, "response": token });
        // A /64 or "unknown" is not an address Cloudflare can use.
        if ip.parse::<IpAddr>().is_ok() {
            body["remoteip"] = serde_json::json!(ip);
        }
        let response = match http.post(SITEVERIFY_URL).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "could not reach Turnstile; counting the sign-up as unverified");
                return CaptchaOutcome::Unavailable;
            }
        };
        match response.json::<SiteVerify>().await {
            Ok(v) if v.success => CaptchaOutcome::Passed,
            Ok(v) if v.error_codes.iter().any(|c| c.contains("secret")) => {
                // Every sign-up would fail the same way: say it where it is seen.
                tracing::error!(codes = ?v.error_codes, "Turnstile refused the secret key; check JUSTTYPE_TURNSTILE_SECRET_KEY");
                CaptchaOutcome::Unavailable
            }
            Ok(v) => {
                tracing::info!(codes = ?v.error_codes, "a sign-up's CAPTCHA token was refused");
                CaptchaOutcome::Failed
            }
            Err(e) => {
                tracing::error!(error = %e, "Turnstile's answer could not be read; counting the sign-up as unverified");
                CaptchaOutcome::Unavailable
            }
        }
    }

    /// Whether a person is behind this sign-up: an address a writer invited,
    /// or a CAPTCHA passed. A token that fails is refused, so the form can
    /// show the check again; no token is only "not verified", and is how a
    /// sign-up arrives while the CAPTCHA is off.
    pub async fn human_verified(
        &self,
        invited: bool,
        token: Option<&str>,
        ip: &str,
    ) -> ApiResult<bool> {
        if invited {
            return Ok(true);
        }
        let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) else {
            return Ok(false);
        };
        if !self.captcha_on() {
            return Ok(false);
        }
        match self.check_token(token, ip).await {
            CaptchaOutcome::Passed => Ok(true),
            CaptchaOutcome::Unavailable => Ok(false),
            CaptchaOutcome::Failed => Err(ApiError::WithCode {
                status: StatusCode::BAD_REQUEST,
                code: "captcha_failed",
                message: "we couldn't confirm a person is signing up — try the check again".into(),
                retry_after: None,
            }),
        }
    }

    /// Writes this sign-up's row and refuses it, taking the row back out, if
    /// it is over a limit. Returns the row's id, to [`confirm`] once the
    /// account exists or [`release`] if it was not made.
    pub fn reserve(
        &self,
        conn: &mut Conn,
        ip: &str,
        email_domain: &str,
        verified: bool,
    ) -> ApiResult<String> {
        let id = crate::domain::new_id();
        let now = crate::clock::now();
        diesel::insert_into(signup_events::table)
            .values((
                signup_events::id.eq(&id),
                signup_events::ip.eq(ip),
                signup_events::email_domain.eq(email_domain),
                signup_events::human_verified.eq(verified),
                signup_events::created_at.eq(iso(now)),
            ))
            .execute(conn)?;
        if let Err(e) = self.check_limits(conn, ip, verified, now) {
            release(conn, &id);
            return Err(e);
        }
        Ok(id)
    }

    fn check_limits(
        &self,
        conn: &mut Conn,
        ip: &str,
        verified: bool,
        now: DateTime<Utc>,
    ) -> ApiResult<()> {
        let c = &self.config;
        // Whether the window holds more than `limit` rows (this request's own
        // is one of them), and if so how long until the oldest ages out.
        let mut over = |limit: i64,
                        window: Duration,
                        ip: Option<&str>,
                        unverified: bool|
         -> ApiResult<Option<i64>> {
            if limit <= 0 {
                return Ok(None);
            }
            let mut q = signup_events::table
                .filter(signup_events::created_at.ge(iso(now - window)))
                .into_boxed();
            if let Some(ip) = ip {
                q = q.filter(signup_events::ip.eq(ip.to_string()));
            }
            if unverified {
                q = q.filter(signup_events::human_verified.eq(false));
            }
            let (n, oldest): (i64, Option<String>) = q
                .select((count_star(), min(signup_events::created_at)))
                .first(conn)?;
            if n <= limit {
                return Ok(None);
            }
            let wait = oldest
                .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                .map(|t| (t.with_timezone(&Utc) + window - now).num_seconds())
                .unwrap_or(window.num_seconds())
                .max(1);
            Ok(Some(wait))
        };

        let hour = Duration::hours(1);
        let day = Duration::days(1);
        if let Some(wait) =
            over(c.ip_hourly, hour, Some(ip), false)?.or(over(c.ip_daily, day, Some(ip), false)?)
        {
            tracing::warn!(%ip, "sign-up refused: the per-address limit is reached");
            return Err(limited(
                "signup_rate_limited",
                "too many accounts have been made from your network lately — try again later",
                Some(wait),
            ));
        }
        if self.captcha_on() && !verified {
            if let Some(wait) = over(c.unverified_ip_daily, day, Some(ip), true)?.or(over(
                c.unverified_global_hourly,
                hour,
                None,
                true,
            )?) {
                tracing::warn!(%ip, "sign-up refused: the limit without a CAPTCHA is reached");
                return Err(limited(
                    "signup_captcha_required",
                    "sign-up needs the check that you're a person — complete it and try again",
                    Some(wait),
                ));
            }
        }
        if let Some(wait) = over(c.global_hourly, hour, None, false)? {
            tracing::warn!("sign-up refused: the site-wide hourly limit is reached");
            return Err(limited(
                "signup_rate_limited",
                "sign-up is very busy right now — try again in a little while",
                Some(wait),
            ));
        }
        Ok(())
    }
}

/// Ties a reserved row to the account it made.
pub fn confirm(conn: &mut Conn, reservation: &str, user_id: &str) -> ApiResult<()> {
    diesel::update(signup_events::table.find(reservation))
        .set(signup_events::user_id.eq(user_id))
        .execute(conn)?;
    Ok(())
}

/// Gives back a reservation whose account was not made. One already tied to
/// an account stays: the account exists, whatever failed after it.
pub fn release(conn: &mut Conn, reservation: &str) {
    let unclaimed = signup_events::table
        .filter(signup_events::id.eq(reservation))
        .filter(signup_events::user_id.is_null());
    if let Err(e) = diesel::delete(unclaimed).execute(conn) {
        tracing::error!(error = %e, "could not release a sign-up reservation");
    }
}

/// Whether a writer has invited this address, which is proof enough that a
/// person is behind it: only a confirmed address can invite, a few a day.
pub fn was_invited(conn: &mut Conn, email: &str) -> ApiResult<bool> {
    Ok(diesel::select(diesel::dsl::exists(
        site_invites::table.filter(site_invites::email.eq(email)),
    ))
    .get_result(conn)?)
}

/// The domain of an address that looks like one: one `@`, something either
/// side, a dotted domain, no spaces. `None` otherwise.
pub fn email_domain(email: &str) -> Option<String> {
    let email = email.trim();
    if email.is_empty() || email.len() > 254 || email.chars().any(char::is_whitespace) {
        return None;
    }
    let (local, domain) = email.split_once('@')?;
    let domain = domain.to_ascii_lowercase();
    let ok = !local.is_empty()
        && local.len() <= 64
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..");
    ok.then_some(domain)
}

/// The address a sign-up is counted against: the client's, as `ClientInfo`
/// found it, with an IPv6 address cut to its /64 — one machine usually holds
/// a whole /64 and could otherwise take a fresh address per account.
pub fn signup_ip(raw: Option<&str>) -> String {
    let Some(ip) = raw.and_then(|r| r.trim().parse::<IpAddr>().ok()) else {
        return "unknown".into();
    };
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

static WARNED_PROXY: AtomicBool = AtomicBool::new(false);

/// Says once when a sign-up arrives from a private address with a forwarding
/// header while `JUSTTYPE_TRUST_PROXY` is off: the server is behind a proxy,
/// and every sign-up is being counted against the proxy's address, which
/// turns the per-address limit into a site-wide one.
pub fn warn_if_proxied(trust_proxy: bool, ip: Option<&str>, headers: &HeaderMap) {
    if trust_proxy || WARNED_PROXY.load(Ordering::Relaxed) {
        return;
    }
    let private = match ip.and_then(|i| i.parse::<IpAddr>().ok()) {
        Some(IpAddr::V4(v4)) => v4.is_private() || v4.is_loopback(),
        Some(IpAddr::V6(v6)) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        None => false,
    };
    let header = ["x-forwarded-for", "x-real-ip", "cf-connecting-ip"]
        .into_iter()
        .find(|h| headers.contains_key(*h));
    if let (true, Some(header)) = (private, header) {
        if !WARNED_PROXY.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                "a sign-up came from {} with a {header} header: the server looks to be behind a proxy. \
                 Set JUSTTYPE_TRUST_PROXY=true, or every sign-up counts against the proxy's address",
                ip.unwrap_or("?")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_has_to_look_like_one() {
        assert_eq!(
            email_domain("Ann@Example.COM").as_deref(),
            Some("example.com")
        );
        for bad in [
            "",
            "ann",
            "ann@",
            "@x.com",
            "ann@x",
            "a b@x.com",
            "a@b@x.com",
            "a@.x.com",
            "a@x..com",
            "a@x.com.",
        ] {
            assert_eq!(email_domain(bad), None, "{bad}");
        }
    }

    #[test]
    fn ipv6_is_counted_by_its_slash_64_and_mapped_v4_as_v4() {
        assert_eq!(signup_ip(Some("203.0.113.7")), "203.0.113.7");
        assert_eq!(
            signup_ip(Some("2001:db8:1:2:aaaa::1")),
            signup_ip(Some("2001:db8:1:2:bbbb::9"))
        );
        assert_eq!(signup_ip(Some("2001:db8:1:2::1")), "2001:db8:1:2::/64");
        assert_eq!(signup_ip(Some("::ffff:203.0.113.7")), "203.0.113.7");
        assert_eq!(signup_ip(Some("nonsense")), "unknown");
        assert_eq!(signup_ip(None), "unknown");
    }

    #[test]
    fn the_turnstile_keys_go_together_and_a_limit_is_a_whole_number() {
        // Names no other test sets, since the environment is shared.
        std::env::set_var("JUSTTYPE_SIGNUP_LIMIT_TEST", "-1");
        assert!(limit_from_env("JUSTTYPE_SIGNUP_LIMIT_TEST", 3).is_err());
        std::env::set_var("JUSTTYPE_SIGNUP_LIMIT_TEST", " 7 ");
        assert_eq!(limit_from_env("JUSTTYPE_SIGNUP_LIMIT_TEST", 3).unwrap(), 7);
        assert_eq!(limit_from_env("JUSTTYPE_SIGNUP_LIMIT_UNSET", 3).unwrap(), 3);
    }
}

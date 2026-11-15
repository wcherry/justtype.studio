//! The permanent ban list: an address that can never register again.
//!
//! The row in `permanent_bans` has no foreign keys and is kept forever, like
//! `audit_events`: it has to outlive the account it records. Addresses are
//! compared in a canonical form, so `j.ane+x@gmail.com` is `jane@gmail.com`.
use diesel::prelude::*;

use crate::db::models::PermanentBan;
use crate::db::schema::{blocked_domains, permanent_bans};
use crate::db::Conn;
use crate::error::ApiResult;

/// Folds case; for Gmail also drops dots and `+tags` in the local part.
pub fn canonical(email: &str) -> String {
    let email = email.trim().to_lowercase();
    let Some((local, domain)) = email.rsplit_once('@') else {
        return email;
    };
    let local = local.split('+').next().unwrap_or(local);
    match domain {
        "gmail.com" | "googlemail.com" => {
            format!("{}@gmail.com", local.replace('.', ""))
        }
        other => format!("{local}@{other}"),
    }
}

pub fn domain_of(email: &str) -> String {
    email
        .trim()
        .to_lowercase()
        .rsplit_once('@')
        .map(|(_, d)| d.to_string())
        .unwrap_or_default()
}

/// The ban in force on this address, if there is one.
pub fn banned_address(conn: &mut Conn, email: &str) -> ApiResult<Option<PermanentBan>> {
    Ok(permanent_bans::table
        .filter(permanent_bans::email_canonical.eq(canonical(email)))
        .filter(permanent_bans::removed_at.is_null())
        .select(PermanentBan::as_select())
        .first(conn)
        .optional()?)
}

/// Widely used throwaway-mail providers, refused without anyone having to
/// list them. Not exhaustive, by design: the point is that the easy option for
/// a script costs something. Staff block whatever the next attack uses from
/// the admin area, without a release.
#[rustfmt::skip]
const THROWAWAY_DOMAINS: &[&str] = &[
    "10minutemail.com", "10minutemail.net", "20minutemail.com", "33mail.com",
    "anonaddy.me", "burnermail.io", "byom.de", "discard.email", "dispostable.com",
    "dropmail.me", "emailondeck.com", "fakeinbox.com", "fakemail.net", "getairmail.com",
    "getnada.com", "guerrillamail.biz", "guerrillamail.com", "guerrillamail.de",
    "guerrillamail.info", "guerrillamail.net", "guerrillamail.org", "guerrillamailblock.com",
    "grr.la", "harakirimail.com", "inboxbear.com", "incognitomail.org", "jetable.org",
    "mail-temp.com", "mail.tm", "mailcatch.com", "maildrop.cc", "mailinator.com",
    "mailinator.net", "mailinator2.com", "mailnesia.com", "mailpoof.com", "mailsac.com",
    "mintemail.com", "moakt.com", "mohmal.com", "mytemp.email", "nada.email",
    "sharklasers.com", "spam4.me", "spamgourmet.com", "temp-mail.io", "temp-mail.org",
    "tempail.com", "tempinbox.com", "tempmail.com", "tempmail.dev", "tempmail.net",
    "tempmailo.com", "tempr.email", "throwawaymail.com", "tmail.ws", "tmpmail.net",
    "tmpmail.org", "trashmail.com", "trashmail.de", "trashmail.net", "yopmail.com",
    "yopmail.fr", "yopmail.net", "emailfake.com", "fakemailgenerator.com", "luxusmail.org",
    "cyclelove.cc", "1secmail.com", "1secmail.net", "1secmail.org", "esiix.com",
    "wwjmp.com", "xojxe.com", "yoggm.com", "laafd.com", "txcct.com", "vjuum.com",
];

/// The domain and each domain above it: `a.b.example` gives `a.b.example`,
/// `b.example` and `example`.
fn domain_and_parents(domain: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(domain), |d| d.split_once('.').map(|(_, rest)| rest))
        .filter(|d| !d.is_empty())
}

/// The built-in throwaway provider this domain is, or is under.
fn throwaway(domain: &str) -> Option<&str> {
    domain_and_parents(domain).find(|d| THROWAWAY_DOMAINS.contains(d))
}

/// The blocked domain this address falls under: a built-in throwaway
/// provider, or one staff listed. Subdomains of either count.
pub fn blocked_domain(conn: &mut Conn, email: &str) -> ApiResult<Option<String>> {
    let domain = domain_of(email);
    if let Some(d) = throwaway(&domain) {
        return Ok(Some(d.to_string()));
    }
    let candidates: Vec<&str> = domain_and_parents(&domain).collect();
    Ok(blocked_domains::table
        .filter(blocked_domains::domain.eq_any(candidates))
        .select(blocked_domains::domain)
        .first(conn)
        .optional()?)
}

/// Whether the address may not be used to register, be invited, or be set as
/// an account's address. Callers that face the public give the same refusal
/// they give for any other address they won't use.
pub fn refused(conn: &mut Conn, email: &str) -> ApiResult<bool> {
    Ok(banned_address(conn, email)?.is_some() || blocked_domain(conn, email)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_dots_and_tags_fold_but_other_addresses_only_lose_tags_and_case() {
        assert_eq!(canonical("j.ane+x@gmail.com"), "jane@gmail.com");
        assert_eq!(canonical("Jane@GoogleMail.com"), "jane@gmail.com");
        assert_eq!(canonical("J.Ane+x@Example.com"), "j.ane@example.com");
        assert_eq!(canonical("nobody"), "nobody");
        assert_eq!(domain_of("a@Spam.Example"), "spam.example");
    }

    #[test]
    fn throwaway_providers_and_their_subdomains_are_known_but_lookalikes_are_not() {
        assert_eq!(throwaway("mailinator.com"), Some("mailinator.com"));
        assert_eq!(throwaway("eu.mailinator.com"), Some("mailinator.com"));
        assert_eq!(throwaway("notmailinator.com"), None);
        assert_eq!(throwaway("gmail.com"), None);
        assert_eq!(throwaway(""), None);
    }
}

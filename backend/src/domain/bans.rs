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

pub fn blocked_domain(conn: &mut Conn, email: &str) -> ApiResult<Option<String>> {
    let domain = domain_of(email);
    Ok(blocked_domains::table
        .find(&domain)
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
}

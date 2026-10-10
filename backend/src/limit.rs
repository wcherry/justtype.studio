//! A sliding-window rate limiter, in process and in memory.
//!
//! This is one process with one SQLite file behind it, so a shared counter
//! would be a second thing to run for no gain. If the deployment ever grows a
//! second process, this is the piece that moves — and until then a counter
//! that costs nothing is better than an architecture that does.
//!
//! The limits exist to stop a script, not a person: a reader who types fast is
//! never near them, and one that is near them is not reading.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::ApiError;

const WINDOW: Duration = Duration::from_secs(60);

/// What is being counted. Separate buckets, because voting is a click and
/// writing a comment is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Comment,
    Vote,
    React,
    /// Mail sent to one address: a reset link is cheap to ask for and a
    /// nuisance to receive forty of.
    Mail,
    /// Guesses at a second factor. A six-digit code is a million tries from
    /// open, so tries are what is limited.
    SecondFactor,
    /// Any request from one address. Keyed by IP, not account.
    Requests,
    /// Requests to the sign-in, sign-up and reset routes from one address.
    AuthRequests,
    /// Summaries asked for, per writer: each one is a paid call to a model.
    Summaries,
    /// Requests to the writing assistant, per writer: a paid call each.
    Assistant,
    /// Critiques and replies on Daily Prompts answers.
    Critique,
}

impl Action {
    fn noun(self) -> &'static str {
        match self {
            Action::Comment => "comments",
            Action::Vote => "votes",
            Action::React => "reactions",
            Action::Mail => "messages",
            Action::SecondFactor => "codes",
            Action::Requests => "requests",
            Action::AuthRequests => "sign-in requests",
            Action::Summaries => "summaries",
            Action::Assistant => "assistant requests",
            Action::Critique => "critiques",
        }
    }
}

#[derive(Default)]
pub struct RateLimiter {
    hits: Mutex<HashMap<(String, Action), Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one action and refuses it if the last minute already holds
    /// `limit` of them. A limit of zero or less means no limit.
    pub fn check(&self, user_id: &str, action: Action, limit: i64) -> Result<(), ApiError> {
        if limit <= 0 {
            return Ok(());
        }
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());

        // Old buckets are swept here rather than on a timer: a user who stops
        // asking stops costing anything, and there is no thread to own.
        hits.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < WINDOW);
            !times.is_empty()
        });

        let times = hits.entry((user_id.to_string(), action)).or_default();
        if (times.len() as i64) < limit {
            times.push(now);
            return Ok(());
        }

        let oldest = times.first().copied().unwrap_or(now);
        let retry_after = WINDOW
            .saturating_sub(now.duration_since(oldest))
            .as_secs()
            .max(1) as i64;
        Err(ApiError::TooManyRequests {
            message: format!(
                "that is {limit} {} in a minute — wait {retry_after}s",
                action.noun()
            ),
            retry_after,
        })
    }
}

/// Sign-in attempts per account, failed ones only.
///
/// The per-address limit stops one machine guessing fast; this stops many
/// machines guessing at one account slowly. After `LOCK_AFTER` wrong passwords
/// or codes inside `LOCK_WINDOW`, the account refuses every attempt — right or
/// wrong — until the oldest of them ages out. The same answer for an address
/// with no account, so the lock says nothing about which addresses exist.
/// A reset link still works: a locked-out owner is not stuck.
#[derive(Default)]
pub struct LoginGuard {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

pub const LOCK_AFTER: usize = 5;
pub const LOCK_WINDOW: Duration = Duration::from_secs(15 * 60);

impl LoginGuard {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Instant>>> {
        self.failures.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Refuses if the account is locked.
    pub fn check(&self, email: &str) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut failures = self.lock();
        failures.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < LOCK_WINDOW);
            !times.is_empty()
        });
        let Some(times) = failures.get(email) else {
            return Ok(());
        };
        if times.len() < LOCK_AFTER {
            return Ok(());
        }
        let oldest = times[times.len() - LOCK_AFTER];
        let retry_after = LOCK_WINDOW
            .saturating_sub(now.duration_since(oldest))
            .as_secs()
            .max(1) as i64;
        Err(ApiError::TooManyRequests {
            message: format!(
                "too many wrong attempts on this account — try again in {} minute{}, or reset the password",
                (retry_after + 59) / 60,
                if retry_after > 60 { "s" } else { "" }
            ),
            retry_after,
        })
    }

    pub fn failed(&self, email: &str) {
        self.lock()
            .entry(email.to_string())
            .or_default()
            .push(Instant::now());
    }

    pub fn succeeded(&self, email: &str) {
        self.lock().remove(email);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_locks_after_five_wrong_attempts_and_a_right_one_clears_it() {
        let guard = LoginGuard::default();
        for _ in 0..LOCK_AFTER - 1 {
            guard.failed("ada@example.com");
        }
        assert!(guard.check("ada@example.com").is_ok(), "four is not five");
        guard.failed("ada@example.com");
        match guard.check("ada@example.com") {
            Err(ApiError::TooManyRequests { retry_after, .. }) => {
                assert!(retry_after > 14 * 60 && retry_after <= 15 * 60)
            }
            other => panic!("expected a lock, got {other:?}"),
        }
        // Another account is untouched.
        assert!(guard.check("grace@example.com").is_ok());
        guard.succeeded("ada@example.com");
        assert!(guard.check("ada@example.com").is_ok());
    }

    #[test]
    fn a_burst_is_refused_once_the_window_is_full() {
        let limiter = RateLimiter::new();
        for _ in 0..3 {
            limiter.check("ada", Action::Comment, 3).unwrap();
        }
        let err = limiter.check("ada", Action::Comment, 3).unwrap_err();
        assert!(matches!(err, ApiError::TooManyRequests { .. }));
        // Someone else's burst is not this person's problem.
        limiter.check("grace", Action::Comment, 3).unwrap();
        // Nor is a different kind of action.
        limiter.check("ada", Action::Vote, 3).unwrap();
    }

    #[test]
    fn a_limit_of_zero_is_no_limit() {
        let limiter = RateLimiter::new();
        for _ in 0..100 {
            limiter.check("ada", Action::Comment, 0).unwrap();
        }
    }

    /// The refusal has to say how long to wait, or a client can only guess.
    #[test]
    fn the_refusal_carries_a_wait() {
        let limiter = RateLimiter::new();
        limiter.check("ada", Action::Vote, 1).unwrap();
        match limiter.check("ada", Action::Vote, 1).unwrap_err() {
            ApiError::TooManyRequests { retry_after, .. } => {
                assert!((1..=60).contains(&retry_after), "got {retry_after}")
            }
            other => panic!("expected a 429, got {other:?}"),
        }
    }
}

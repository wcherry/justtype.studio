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
}

impl Action {
    fn noun(self) -> &'static str {
        match self {
            Action::Comment => "comments",
            Action::Vote => "votes",
            Action::React => "reactions",
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

#[cfg(test)]
mod tests {
    use super::*;

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

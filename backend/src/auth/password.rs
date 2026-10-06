//! Password hashing with Argon2id, at a cost set for this machine.
//!
//! The parameters come from the environment (`JUSTTYPE_ARGON2_*`) and
//! default to more than the OWASP minimum; `justtype-backend --tune-argon2`
//! measures what this hardware can afford. A hash records the parameters it
//! was made with, so raising them does not lock anyone out: the next time
//! someone signs in, their password is checked against the old parameters and
//! stored again under the new ones.
use std::sync::OnceLock;

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::{ApiError, ApiResult};

/// Memory in KiB, iterations, lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl Default for Cost {
    /// 64 MiB, two passes, one lane: comfortably past OWASP's minimum
    /// (19 MiB, two passes) and still well under a second on a small server.
    fn default() -> Self {
        Self {
            memory_kib: 65_536,
            iterations: 2,
            lanes: 1,
        }
    }
}

impl Cost {
    pub fn from_env() -> anyhow::Result<Self> {
        let d = Self::default();
        let read = |key: &str, default: u32| -> anyhow::Result<u32> {
            Ok(match std::env::var(key) {
                Ok(v) if !v.is_empty() => v.parse()?,
                _ => default,
            })
        };
        let cost = Self {
            memory_kib: read("JUSTTYPE_ARGON2_MEMORY_KIB", d.memory_kib)?,
            iterations: read("JUSTTYPE_ARGON2_ITERATIONS", d.iterations)?,
            lanes: read("JUSTTYPE_ARGON2_LANES", d.lanes)?,
        };
        cost.hasher()?;
        Ok(cost)
    }

    fn params(&self) -> anyhow::Result<Params> {
        Params::new(self.memory_kib, self.iterations, self.lanes, None)
            .map_err(|e| anyhow::anyhow!("Argon2 parameters out of range: {e}"))
    }

    fn hasher(&self) -> anyhow::Result<Argon2<'static>> {
        Ok(Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            self.params()?,
        ))
    }
}

static COST: OnceLock<Cost> = OnceLock::new();

/// Sets the cost for this process; called once at startup.
pub fn configure(cost: Cost) {
    let _ = COST.set(cost);
}

fn cost() -> Cost {
    *COST.get_or_init(Cost::default)
}

pub fn hash(password: &str) -> ApiResult<String> {
    hash_with(password, cost())
}

fn hash_with(password: &str, cost: Cost) -> ApiResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    cost.hasher()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal(format!("password hashing failed: {e}")))
}

/// Whether the password matches. The parameters are read from the stored hash,
/// so a hash made under older settings still verifies.
pub fn verify(password: &str, encoded: &str) -> bool {
    match PasswordHash::new(encoded) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Whether a stored hash was made with anything other than the current
/// algorithm and cost — and so should be made again, now that the password is
/// in hand.
pub fn needs_rehash(encoded: &str) -> bool {
    needs_rehash_for(encoded, cost())
}

fn needs_rehash_for(encoded: &str, cost: Cost) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    if parsed.algorithm.as_str() != "argon2id" {
        return true;
    }
    let Ok(stored) = Params::try_from(&parsed) else {
        return true;
    };
    stored.m_cost() != cost.memory_kib
        || stored.t_cost() != cost.iterations
        || stored.p_cost() != cost.lanes
}

/// Times a range of costs on this machine and prints what it can afford, for
/// `justtype-backend --tune-argon2 [target ms]`.
pub fn tune(target_ms: u128) {
    println!("Argon2id on this machine, aiming under {target_ms} ms a hash:");
    let mut best: Option<Cost> = None;
    for memory_mib in [19u32, 32, 46, 64, 96, 128, 192, 256] {
        for iterations in [1u32, 2, 3] {
            let cost = Cost {
                memory_kib: memory_mib * 1024,
                iterations,
                lanes: 1,
            };
            let started = std::time::Instant::now();
            if hash_with("tuning-sample-password", cost).is_err() {
                continue;
            }
            let ms = started.elapsed().as_millis();
            let fits = ms <= target_ms;
            println!(
                "  {:>4} MiB × {} pass{}  {:>5} ms{}",
                memory_mib,
                iterations,
                if iterations == 1 { " " } else { "es" },
                ms,
                if fits { "" } else { "  — over" }
            );
            // Memory is what makes guessing on a GPU expensive; prefer more of
            // it over more passes at the same time.
            if fits
                && best.is_none_or(|b| {
                    (cost.memory_kib, cost.iterations) > (b.memory_kib, b.iterations)
                })
            {
                best = Some(cost);
            }
        }
    }
    match best {
        Some(cost) => println!(
            "\nSuggested:\n  JUSTTYPE_ARGON2_MEMORY_KIB={}\n  JUSTTYPE_ARGON2_ITERATIONS={}\n  JUSTTYPE_ARGON2_LANES=1\nTime it in a release build: debug builds are many times slower.",
            cost.memory_kib, cost.iterations
        ),
        None => println!("\nNothing fits {target_ms} ms; the OWASP floor is 19 MiB × 2 passes."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGHT: Cost = Cost {
        memory_kib: 8_192,
        iterations: 1,
        lanes: 1,
    };
    const HEAVIER: Cost = Cost {
        memory_kib: 16_384,
        iterations: 2,
        lanes: 1,
    };

    #[test]
    fn an_old_hash_still_verifies_and_asks_to_be_made_again() {
        let old = hash_with("correcthorse", LIGHT).unwrap();
        // Raised since: the old hash still works, and is flagged.
        assert!(verify("correcthorse", &old));
        assert!(!verify("wrong-horse", &old));
        assert!(needs_rehash_for(&old, HEAVIER));
        let new = hash_with("correcthorse", HEAVIER).unwrap();
        assert!(verify("correcthorse", &new));
        assert!(!needs_rehash_for(&new, HEAVIER));
    }

    #[test]
    fn the_crate_default_is_below_ours_and_gets_upgraded() {
        // Argon2::default() is the 19 MiB minimum — what accounts made before
        // this change were hashed with.
        let salt = SaltString::generate(&mut OsRng);
        let legacy = Argon2::default()
            .hash_password(b"correcthorse", &salt)
            .unwrap()
            .to_string();
        assert!(verify("correcthorse", &legacy));
        assert!(needs_rehash_for(&legacy, Cost::default()));
    }

    #[test]
    fn a_hash_that_is_not_one_is_never_rehashed() {
        // The "former member" placeholder's hash is "!", on purpose.
        assert!(!verify("anything", "!"));
        assert!(!needs_rehash_for("!", Cost::default()));
    }
}

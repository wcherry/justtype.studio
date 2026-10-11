//! Secrets kept in the database (the moderation platform's API key and
//! webhook secret): encrypted with a key derived from the server's own secret,
//! so a copy of the database alone does not hold them, and never shown again
//! once saved.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

use crate::error::{ApiError, ApiResult};

const PREFIX: &str = "enc1:";

fn key(server_secret: &str) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(server_secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(b"justtype.studio/stored-secrets/v1");
    mac.finalize().into_bytes().into()
}

pub fn seal(server_secret: &str, plain: &str) -> ApiResult<String> {
    let cipher = Aes256Gcm::new(&key(server_secret).into());
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
        .map_err(|_| ApiError::internal("could not encrypt a stored secret"))?;
    let mut bytes = nonce.to_vec();
    bytes.extend(sealed);
    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

pub fn open(server_secret: &str, stored: &str) -> ApiResult<String> {
    let bad = || ApiError::internal("a stored secret could not be read");
    let raw = stored.strip_prefix(PREFIX).ok_or_else(bad)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| bad())?;
    if bytes.len() < 13 {
        return Err(bad());
    }
    let (nonce, sealed) = bytes.split_at(12);
    let plain = Aes256Gcm::new(&key(server_secret).into())
        .decrypt(Nonce::from_slice(nonce), sealed)
        .map_err(|_| bad())?;
    String::from_utf8(plain).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_and_is_not_stored_in_the_clear() {
        let sealed = seal("server", "sk_live_abc123").unwrap();
        assert!(!sealed.contains("sk_live"));
        assert_eq!(open("server", &sealed).unwrap(), "sk_live_abc123");
        assert!(open("another server", &sealed).is_err());
        // Sealed twice, two different strings.
        assert_ne!(sealed, seal("server", "sk_live_abc123").unwrap());
    }
}

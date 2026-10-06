//! Time-based one-time passwords (RFC 6238), the codes an authenticator app
//! shows. Thirty-second steps, six digits, HMAC-SHA1 — the parameters every
//! authenticator assumes when the setup link does not say otherwise.
//!
//! Written out rather than pulled in: it is twenty lines of arithmetic over a
//! MAC, and the tests below pin it to the RFC's own vectors.
use hmac::{Hmac, Mac};
use sha1::Sha1;

const STEP_SECONDS: i64 = 30;
const DIGITS: u32 = 6;
/// One step either side, for a phone clock that has drifted or a code typed as
/// it turned over. Wider than that and a shoulder-surfed code stays good for
/// minutes.
const SKEW: i64 = 1;

/// A new random secret, as the base32 an authenticator app accepts.
pub fn generate_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut bytes);
    base32_encode(&bytes)
}

/// The `otpauth://` link an authenticator app reads, from a QR code or pasted.
pub fn setup_url(secret: &str, account: &str) -> String {
    // Encoded: the name has spaces, and a raw space ends the link for some
    // authenticator apps.
    let issuer = encode_label("Just Type Studio");
    format!(
        "otpauth://totp/{issuer}:{}?secret={secret}&issuer={issuer}&digits={DIGITS}&period={STEP_SECONDS}",
        encode_label(account)
    )
}

fn encode_label(account: &str) -> String {
    account
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_' => (b as char).to_string(),
            b'@' => "%40".into(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The code for one time step.
fn code_at(key: &[u8], step: i64, digits: u32) -> u32 {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[hash.len() - 1] & 0x0f) as usize;
    let binary = u32::from_be_bytes([
        hash[offset] & 0x7f,
        hash[offset + 1],
        hash[offset + 2],
        hash[offset + 3],
    ]);
    binary % 10u32.pow(digits)
}

/// Checks a code against the secret at `unix_time`.
///
/// Returns the step it matched, so the caller can refuse that step next time:
/// a code is good once, not for its whole thirty seconds. `last_step` is the
/// step accepted last; nothing at or before it matches again.
pub fn verify(secret: &str, code: &str, unix_time: i64, last_step: Option<i64>) -> Option<i64> {
    let code = code.trim().replace(' ', "");
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let wanted: u32 = code.parse().ok()?;
    let key = base32_decode(secret)?;
    let now = unix_time.div_euclid(STEP_SECONDS);
    (now - SKEW..=now + SKEW)
        .filter(|step| last_step.is_none_or(|last| *step > last))
        .find(|step| code_at(&key, *step, DIGITS) == wanted)
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32, without padding — what authenticator apps expect.
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Lenient on the way in: lower case, spaces and padding are what people
/// paste.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let value = ALPHABET
            .iter()
            .position(|a| *a as char == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SHA-1 seed from RFC 6238 appendix B.
    const RFC_KEY: &[u8] = b"12345678901234567890";

    #[test]
    fn matches_the_rfc_test_vectors() {
        // Appendix B lists eight-digit codes; the same arithmetic gives six.
        for (time, expected) in [
            (59, 94287082),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
            (20000000000, 65353130),
        ] {
            assert_eq!(
                code_at(RFC_KEY, time / STEP_SECONDS, 8),
                expected,
                "at {time}"
            );
        }
    }

    #[test]
    fn base32_round_trips_and_matches_rfc_4648() {
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32_decode("mzxw 6ytb oi======").unwrap(), b"foobar");
        let secret = generate_secret();
        assert_eq!(base32_decode(&secret).unwrap().len(), 20);
        assert!(base32_decode("not base32!").is_none());
    }

    #[test]
    fn a_code_is_good_once_and_only_near_its_time() {
        let secret = base32_encode(RFC_KEY);
        let time = 1234567890;
        let code = format!("{:06}", code_at(RFC_KEY, time / STEP_SECONDS, 6));

        let step = verify(&secret, &code, time, None).expect("the current code");
        // Thirty seconds late is still a typo away from on time.
        assert!(verify(&secret, &code, time + 30, None).is_some());
        // Five minutes late is not.
        assert!(verify(&secret, &code, time + 300, None).is_none());
        // And once accepted, never again.
        assert!(verify(&secret, &code, time, Some(step)).is_none());
        assert!(verify(&secret, "12345", time, None).is_none());
        assert!(verify(&secret, "abcdef", time, None).is_none());
    }

    #[test]
    fn the_setup_link_escapes_the_address() {
        let url = setup_url("ABC", "ada+books@example.com");
        assert!(url.starts_with(
            "otpauth://totp/Just%20Type%20Studio:ada%2Bbooks%40example.com?secret=ABC"
        ));
        assert!(url.contains("&issuer=Just%20Type%20Studio&"));
        assert!(!url.contains(' '));
    }
}

//! A purchase claim: what a buyer signs to bind a payment to their own key.
//!
//! The exit sells transit to a *key*, never to an address and never to a name a
//! caller typed. So a purchase carries a signature: the buyer answers the exit's
//! challenge with a BIP340 schnorr signature over the challenge's canonical bytes,
//! made with the key whose outer address the FIPS transport will observe. The exit
//! derives the address from the key, compares it with what it saw, and admits only
//! if they agree.
//!
//! That is the whole reason this exists. A caller that submits someone else's npub
//! is refused, because it cannot produce a signature that verifies under the key it
//! named. Requirement A, in one type.
//!
//! # The canonical bytes are a wire contract, not an implementation detail
//!
//! The exit serialises the challenge and hashes it; this module must produce the
//! same bytes or every signature is refused as malformed. The layout is pinned
//! here with the field widths spelled out, and the vectors in
//! `contracts/fips-exit-admission-vectors.json` from the physical-router repo are
//! the oracle: a change here that does not reproduce those hex strings is wrong,
//! and the test below fails loudly rather than letting the two sides drift.
//!
//! # Time is carried as Unix nanoseconds, big-endian
//!
//! Not RFC 3339, and not seconds. The exit writes `uint64` nanosecond counts so a
//! challenge cannot be replayed inside the same second, and so two sides agreeing
//! on a validity window agree on its exact edges.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The literal the exit prefixes every canonical encoding with, then a version byte.
///
/// A domain separator: a signature over these bytes can never be mistaken for a
/// signature over any other structure, even one with the same field widths.
const CANONICAL_PREFIX: &[u8] = b"tollgate-fips-exit/claim\x00";
/// Serialisation version, mixed in right after the prefix.
const CANONICAL_VERSION: u8 = 0x01;

/// What a claim is about, as the exit offered it. Signed, so neither side can
/// substitute cheaper terms after the fact.
///
/// Field names and shapes match the exit's Go struct as it serialises them, which
/// is what arrives in `X-TollGate-Challenge`. `PeerKey` and `ClaimNonce` are fixed
/// arrays there with no custom marshaller, so they arrive as arrays of numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    /// Which exit this is for, as 32 bytes.
    #[serde(rename = "ExitKey")]
    pub exit_key: [u8; 32],
    /// The single-use value that makes a claim non-replayable.
    #[serde(rename = "Nonce")]
    pub nonce: [u8; 16],
    /// When the exit issued it.
    #[serde(rename = "IssuedAt")]
    pub issued_at: String,
    /// When it stops being acceptable.
    #[serde(rename = "ExpiresAt")]
    pub expires_at: String,
    /// The quoted price in sats. Signed so the buyer and exit cannot disagree.
    #[serde(rename = "PriceSats")]
    pub price_sats: u64,
    /// What the price buys: `bytes` or `milliseconds`.
    #[serde(rename = "Mode")]
    pub mode: String,
}

impl Challenge {
    /// The bytes the buyer signs, in the exit's layout.
    ///
    /// Field order and widths are the contract. Every integer is big-endian; the
    /// two timestamps are nanoseconds since the Unix epoch as `u64`, not the
    /// RFC 3339 spelling they arrived in.
    ///
    /// Returns an error rather than guessing if a timestamp will not parse,
    /// because a silently zeroed timestamp produces a signature over different
    /// bytes than the exit expects, and the buyer would be told "malformed"
    /// instead of "your clock reading failed".
    pub fn canonical(&self) -> Result<Vec<u8>, ClaimError> {
        let issued = parse_unix_nanos(&self.issued_at)?;
        let expires = parse_unix_nanos(&self.expires_at)?;

        let mode = self.mode.as_bytes();
        let mode_len = u16::try_from(mode.len())
            .map_err(|_| ClaimError::Canonical("the billing mode is longer than 64 KiB"))?;

        let mut buf =
            Vec::with_capacity(CANONICAL_PREFIX.len() + 1 + 32 + 16 + 8 + 8 + 8 + 2 + mode.len());
        buf.extend_from_slice(CANONICAL_PREFIX);
        buf.push(CANONICAL_VERSION);
        buf.extend_from_slice(&self.exit_key);
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&issued.to_be_bytes());
        buf.extend_from_slice(&expires.to_be_bytes());
        buf.extend_from_slice(&self.price_sats.to_be_bytes());
        buf.extend_from_slice(&mode_len.to_be_bytes());
        buf.extend_from_slice(mode);
        Ok(buf)
    }

    /// The 32-byte message BIP340 signs: SHA-256 over [`Self::canonical`].
    ///
    /// Separated from `canonical` so the hash is pinned in exactly one place, and so
    /// a test can compare the hash against a vector without re-deriving it.
    pub fn sign_digest(&self) -> Result<[u8; 32], ClaimError> {
        Ok(Sha256::digest(self.canonical()?).into())
    }
}

/// A buyer's answer: the terms, who is buying, and proof they hold that key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// The terms being accepted, verbatim as offered.
    #[serde(rename = "Challenge")]
    pub challenge: Challenge,
    /// The buyer's key, 32 bytes of x-only public key as hex — the identity the
    /// exit derives an address from and compares with the tunnel it saw.
    #[serde(rename = "Buyer")]
    pub buyer: String,
    /// 128 characters of lower-case hex: a BIP340 signature over
    /// [`Challenge::sign_digest`], made by `buyer`.
    #[serde(rename = "Sig")]
    pub sig: String,
}

/// What can go wrong before anything is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// A timestamp on the challenge could not be read.
    Canonical(&'static str),
    /// The challenge, the body handed over, or the signature is the wrong shape.
    Shape(String),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Canonical(why) => write!(f, "the challenge cannot be canonicalised: {why}"),
            Self::Shape(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ClaimError {}

/// Read an RFC 3339 timestamp as nanoseconds since the Unix epoch.
///
/// The exit writes Go's `time.Time` default format, which carries a variable
/// number of fractional digits (trailing zeros trimmed) and either `Z` or a
/// numeric offset. RFC 3339 permits both, so both are handled, and the offset is
/// applied rather than dropped — a challenge from an exit in another timezone
/// would otherwise canonicalise to a different instant and every signature would
/// be refused.
fn parse_unix_nanos(text: &str) -> Result<u64, ClaimError> {
    // Split the offset off the end: `Z`, or `+HH:MM` / `-HH:MM`.
    let (body, offset_secs) = match text.strip_suffix('Z') {
        Some(rest) => (rest, 0i64),
        None => {
            // Find the last sign that is not the first character.
            // Scan from the end: the date's own hyphens must not be mistaken for
            // the offset's sign, so the *last* sign wins.
            let idx = text
                .char_indices()
                .rev()
                .find(|(i, c)| *i > 0 && (*c == '+' || *c == '-'))
                .map(|(i, _)| i)
                .ok_or(ClaimError::Canonical(
                    "the timestamp carries neither a Z nor a numeric offset",
                ))?;
            let (rest, off) = text.split_at(idx);
            (rest, parse_offset(off)?)
        }
    };

    let (date_part, time_part) = body.split_once('T').ok_or(ClaimError::Canonical(
        "the timestamp has no date/time separator",
    ))?;

    let mut d = date_part.split('-');
    let year: i64 = next_int(&mut d, "year")?;
    let month: i64 = next_int(&mut d, "month")?;
    let day: i64 = next_int(&mut d, "day")?;

    let (hms, frac) = match time_part.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (time_part, None),
    };
    let mut t = hms.split(':');
    let hour: i64 = next_int(&mut t, "hour")?;
    let minute: i64 = next_int(&mut t, "minute")?;
    let second: i64 = next_int(&mut t, "second")?;

    // Fractional part: pad or trim to exactly nine digits.
    let nanos: u64 = match frac {
        None => 0,
        Some(f) => {
            if !f.bytes().all(|b| b.is_ascii_digit()) || f.is_empty() {
                return Err(ClaimError::Canonical(
                    "the fractional seconds are not digits",
                ));
            }
            let mut digits = String::with_capacity(9);
            digits.push_str(&f[..f.len().min(9)]);
            while digits.len() < 9 {
                digits.push('0');
            }
            digits
                .parse()
                .map_err(|_| ClaimError::Canonical("the fractional seconds do not parse"))?
        }
    };

    let secs = civil_to_unix(year, month, day, hour, minute, second) - offset_secs;
    let secs = u64::try_from(secs)
        .map_err(|_| ClaimError::Canonical("the timestamp is before the Unix epoch"))?;
    secs.checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(nanos))
        .ok_or(ClaimError::Canonical(
            "the timestamp is too far in the future",
        ))
}

fn next_int<'a, I: Iterator<Item = &'a str>>(
    it: &mut I,
    what: &'static str,
) -> Result<i64, ClaimError> {
    it.next()
        .ok_or(ClaimError::Canonical(what))?
        .parse()
        .map_err(|_| ClaimError::Canonical(what))
}

/// Parse `+HH:MM` / `-HH:MM` into seconds east of UTC.
fn parse_offset(text: &str) -> Result<i64, ClaimError> {
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'+') => (1i64, &text[1..]),
        Some(b'-') => (-1i64, &text[1..]),
        _ => {
            return Err(ClaimError::Canonical(
                "the offset does not start with a sign",
            ))
        }
    };
    let (h, m) = rest
        .split_once(':')
        .ok_or(ClaimError::Canonical("the offset has no minutes"))?;
    let hours: i64 = h
        .parse()
        .map_err(|_| ClaimError::Canonical("the offset hours"))?;
    let mins: i64 = m
        .parse()
        .map_err(|_| ClaimError::Canonical("the offset minutes"))?;
    Ok(sign * (hours * 3600 + mins * 60))
}

/// Days from 1970-01-01 to `y-m-d`, by Howard Hinnant's civil-from-days algorithm.
///
/// Used instead of a date library because this crate cross-builds for Android and
/// the arithmetic is exact for every year a challenge can carry. The month and day
/// are shifted so a year starts in March, which puts the leap day at the end and
/// removes every special case from the day count.
fn civil_to_unix(y: i64, m: i64, d: i64, hh: i64, mm: i64, ss: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hh * 3_600 + mm * 60 + ss
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A challenge taken from the exit's own vector set, so a change here that
    /// breaks the wire contract fails against the published bytes rather than
    /// against a fixture written to match this implementation.
    fn vector_challenge() -> Challenge {
        Challenge {
            exit_key: hex32("505f234a81fe3af88625ebda259dbaec44c72b181e2f64735f8b8ec8d7cf7377"),
            nonce: hex16("00112233445566778899aabbccddeeff"),
            issued_at: "2027-01-15T08:00:00Z".into(),
            expires_at: "2027-01-15T08:10:00Z".into(),
            price_sats: 2,
            mode: "bytes".into(),
        }
    }

    fn hex32(s: &str) -> [u8; 32] {
        let v: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        out
    }

    fn hex16(s: &str) -> [u8; 16] {
        let v: Vec<u8> = (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect();
        let mut out = [0u8; 16];
        out.copy_from_slice(&v);
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_canonical_bytes_match_the_published_vector() {
        // From contracts/fips-exit-admission-vectors.json, vector `canonical-0`.
        // If this fails, the two sides disagree about what a buyer signs and every
        // purchase would be refused as malformed.
        let want = "746f6c6c676174652d666970732d657869742f636c61696d0001505f234a81fe3af88625ebda259dbaec44c72b181e2f64735f8b8ec8d7cf737700112233445566778899aabbccddeeff18fae27693b4000018fae302467d7000000000000000000200056279746573";
        let got = hex(&vector_challenge().canonical().expect("canonical"));
        assert_eq!(got, want, "the canonical encoding drifted from the exit's");
    }

    #[test]
    fn the_sign_digest_matches_the_published_vector() {
        let want = "e3d658141d09e0f43bd76b4f805832471cbc25bae1382be6942719182d5cad74";
        let got = hex(&vector_challenge().sign_digest().expect("digest"));
        assert_eq!(got, want, "the signed message drifted from the exit's");
    }

    #[test]
    fn a_numeric_offset_and_z_agree_on_the_same_instant() {
        let mut utc = vector_challenge();
        let mut stockholm = vector_challenge();
        stockholm.issued_at = "2027-01-15T09:00:00+01:00".into();
        stockholm.expires_at = "2027-01-15T09:10:00+01:00".into();
        utc.price_sats = 2;
        assert_eq!(
            utc.canonical().expect("utc"),
            stockholm.canonical().expect("offset"),
            "a challenge in another timezone must canonicalise to the same instant"
        );
    }

    #[test]
    fn fractional_seconds_are_padded_not_dropped() {
        // Go trims trailing zeros, so `.5` means half a second, not five nanoseconds.
        let mut c = vector_challenge();
        c.issued_at = "2027-01-15T08:00:00.5Z".into();
        let a = parse_unix_nanos(&c.issued_at).expect("parse .5");
        let b = parse_unix_nanos("2027-01-15T08:00:00.500000000Z").expect("parse .500000000");
        assert_eq!(
            a, b,
            "fractional seconds must be right-padded to nanoseconds"
        );
    }

    #[test]
    fn a_challenge_with_no_offset_is_refused_rather_than_assumed_utc() {
        let mut c = vector_challenge();
        c.issued_at = "2027-01-15T08:00:00".into();
        assert!(matches!(c.canonical(), Err(ClaimError::Canonical(_))));
    }

    #[test]
    fn the_exit_key_and_nonce_sit_at_their_pinned_offsets() {
        // Spot-check the layout rather than trusting the whole-string compare: a
        // shifted field with a compensating bug elsewhere would still be wrong.
        let bytes = vector_challenge().canonical().expect("canonical");
        assert_eq!(&bytes[..CANONICAL_PREFIX.len()], CANONICAL_PREFIX);
        assert_eq!(bytes[CANONICAL_PREFIX.len()], CANONICAL_VERSION);
        let key_at = CANONICAL_PREFIX.len() + 1;
        assert_eq!(&bytes[key_at..key_at + 32], &vector_challenge().exit_key);
        assert_eq!(&bytes[key_at + 32..key_at + 48], &vector_challenge().nonce);
    }

    #[test]
    fn the_mode_length_is_big_endian_and_trails_the_fixed_fields() {
        let bytes = vector_challenge().canonical().expect("canonical");
        let tail = &bytes[bytes.len() - 7..];
        assert_eq!(
            &tail[..2],
            &[0x00, 0x05],
            "a 5-byte mode is a 2-byte big-endian length"
        );
        assert_eq!(&tail[2..], b"bytes");
    }
}

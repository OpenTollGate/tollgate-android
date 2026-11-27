//! Protocol v2 on the wire: what the app sends a gateway, and how it reads back.
//!
//! The old core treated the gateway as an HTTP endpoint with a request/response
//! exchange (`POST /tollgate/v1/exchange`, a body carrying an `Announce` and a
//! `BootstrapToken`). Protocol v2 has no such endpoint. A gateway listens on raw
//! TCP ([`DEFAULT_PORT`]) and the conversation is a session: the payer sends an
//! `Announce`, the node answers with its own `Announce` and an `Offer`, and from
//! there the payer funds a channel and buys windows with signed `TopUp`s.
//!
//! This module owns only the read side of that session — framing, decoding, and
//! turning what came back into something the UI can show. Buying is not here:
//! it needs a wallet, and a wallet on a phone is its own piece of work.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use tollgate_protocol::{
    encode_frame, Announce, FrameReader, Message, Offer, ReasonCode, DEFAULT_PORT,
    PROTOCOL_VERSION,
};

/// What can go wrong reading a v2 stream.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The frame arrived whole but its body did not decode. The stream is still
    /// in sync — the length prefix was intact — so this is about one message.
    #[error("a frame did not decode: {0:?}")]
    Codec(tollgate_protocol::Error),
    /// We could not build our OWN frame. Nothing arrived from the peer, so this
    /// must never be reported as a peer fault: the bug would be ours.
    #[error("could not build our Announce: {0}")]
    Encode(String),
    /// We never obtained a socket: connect refused or timed out. Distinct from
    /// `Io` because only this arm justifies telling the user the peer is away.
    #[error("could not reach the gateway: {0}")]
    Connect(String),
    /// Nothing arrived in time, or the socket failed.
    #[error("talking to the gateway: {0}")]
    Io(String),
    /// The peer answered, in framing we understood, with a refusal. That is an
    /// answer, not a dead socket: a version mismatch is a permanent condition
    /// and must not be retried like an unreachable host.
    #[error("the gateway refused us: {0:?}")]
    Refused(RejectReason),
}

/// How badly a v2 attempt failed, in the terms a caller has to act on.
///
/// Carried on `DiscoverError` instead of a bare `io: bool` so that a new
/// `WireError` arm cannot silently default into "not a network problem".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// We never reached anything: connect failed or timed out.
    Connect,
    /// We reached the peer and the conversation broke.
    Io,
    Codec,
    Encode,
    Refused,
}

impl From<&WireError> for FailureKind {
    fn from(e: &WireError) -> Self {
        match e {
            WireError::Connect(_) => FailureKind::Connect,
            WireError::Io(_) => FailureKind::Io,
            WireError::Codec(_) => FailureKind::Codec,
            WireError::Encode(_) => FailureKind::Encode,
            WireError::Refused(_) => FailureKind::Refused,
        }
    }
}

/// An `Offer` as the app needs to show it.
///
/// Deliberately without a price. v2's `Offer` carries terms — mints, unit, the
/// window and rate limits — because delivery is one voucher per unit and the
/// money is the mint's, not the gateway's. A UI that wants to show "cost" has to
/// ask the mint what a voucher of that unit is worth; inventing a number here
/// would be a lie about someone else's price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferView {
    pub accepted_mints: Vec<String>,
    pub unit: String,
    pub min_window_ms: u64,
    pub max_window_ms: u64,
    pub from_payer_weight: u16,
    pub no_charge: bool,
    pub min_reserved_rate: u64,
    pub min_topup_gap_ms: u64,
}

impl From<&Offer> for OfferView {
    fn from(offer: &Offer) -> Self {
        Self {
            accepted_mints: offer.accepted_mints.clone(),
            unit: offer.unit.clone(),
            min_window_ms: offer.min_window_ms,
            max_window_ms: offer.max_window_ms,
            from_payer_weight: offer.from_payer_weight,
            no_charge: offer.no_charge,
            min_reserved_rate: offer.min_reserved_rate,
            min_topup_gap_ms: offer.min_topup_gap_ms,
        }
    }
}

/// Why a gateway refused something, in terms the app can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    MintNotAccepted,
    UnitNotAccepted,
    OutOfRange,
    FundingInvalid,
    /// The grant's signature, or its `cumulative`, did not hold up.
    InvalidGrant,
    RateExceedsCapacity,
    GrantExceedsChannel,
    VersionUnsupported,
    TooSoon,
    Other,
}

impl From<ReasonCode> for RejectReason {
    fn from(code: ReasonCode) -> Self {
        match code {
            ReasonCode::MintNotAccepted => Self::MintNotAccepted,
            ReasonCode::UnitNotAccepted => Self::UnitNotAccepted,
            ReasonCode::OutOfRange => Self::OutOfRange,
            ReasonCode::FundingInvalid => Self::FundingInvalid,
            ReasonCode::FromPayerWeightUnacceptable => Self::InvalidGrant,
            ReasonCode::GrantInvalid => Self::InvalidGrant,
            ReasonCode::RateExceedsCapacity => Self::RateExceedsCapacity,
            ReasonCode::GrantExceedsChannel => Self::GrantExceedsChannel,
            ReasonCode::VersionUnsupported => Self::VersionUnsupported,
            ReasonCode::TooSoon => Self::TooSoon,
            ReasonCode::Other => Self::Other,
        }
    }
}

/// Everything a gateway said in one readable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub pubkey_hex: String,
    pub unit: String,
    pub version: u8,
    pub offer: Option<OfferView>,
}

/// Decode every whole frame in `bytes`, stopping at a partial tail.
///
/// A tail that has not fully arrived is not an error: the caller reads again.
/// A frame that arrived whole and did not decode *is* one, because a stream that
/// silently drops messages would look like a gateway that said nothing.
pub fn read_messages(bytes: &[u8]) -> Result<Vec<Message>, WireError> {
    let mut reader = FrameReader::new();
    reader.push(bytes);
    let mut out = Vec::new();
    while let Some(next) = reader.next_message() {
        out.push(next.map_err(WireError::Codec)?);
    }
    Ok(out)
}

/// The gateway's `Announce` and `Offer`, if both were in the answer.
///
/// `Announce` is required — without it there is no identity to pay and no unit
/// to buy in, so an answer without one is not "a gateway that offers nothing",
/// it is a gateway that did not introduce itself.
pub fn detect_from(messages: &[Message]) -> Option<Detected> {
    let announce: &Announce = messages.iter().find_map(|m| match m {
        Message::Announce(a) => Some(a),
        _ => None,
    })?;
    let offer: Option<&Offer> = messages.iter().find_map(|m| match m {
        Message::Offer(o) => Some(o),
        _ => None,
    });
    Some(Detected {
        pubkey_hex: hex::encode(announce.pubkey.as_bytes()),
        unit: announce.unit.clone(),
        version: announce.version,
        offer: offer.map(OfferView::from),
    })
}

/// The first refusal in an answer, if any.
pub fn refusal_in(messages: &[Message]) -> Option<RejectReason> {
    messages.iter().find_map(|m| match m {
        Message::Reject(r) => Some(RejectReason::from(r.reason)),
        Message::Disconnect(d) => Some(RejectReason::from(d.reason)),
        Message::TopUpReject(t) => Some(RejectReason::from(t.reason)),
        _ => None,
    })
}

/// Our side of the introduction.
fn announce(secret: &secp256k1::SecretKey, unit: &str) -> Message {
    let public = secp256k1::PublicKey::from_secret_key(secp256k1::SECP256K1, secret);
    Message::Announce(Announce {
        version: PROTOCOL_VERSION,
        pubkey: tollgate_protocol::PubKey(public.serialize()),
        unit: unit.to_string(),
        capabilities: 0,
    })
}

/// Open a session, say who we are, and read the gateway's answer.
///
/// `endpoint` is `host:port`; without a port, [`DEFAULT_PORT`] is used. The read
/// stops as soon as an `Announce` and an `Offer` have arrived — waiting for the
/// socket to go quiet would hang on a gateway that leaves the session open,
/// which is the normal case.
pub async fn detect(
    endpoint: &str,
    secret: &secp256k1::SecretKey,
    unit: &str,
    timeout: Duration,
) -> Result<Detected, WireError> {
    let address = with_default_port(endpoint);
    // Both of these mean we never got a socket, which is the only honest
    // "unreachable" signal available to us.
    let mut stream = tokio::time::timeout(timeout, TcpStream::connect(&address))
        .await
        .map_err(|_| WireError::Connect(format!("connecting to {address}: timed out")))?
        .map_err(|e| WireError::Connect(format!("connecting to {address}: {e}")))?;

    let mut out = Vec::new();
    // Our own encode failing is our bug, never a peer fault — hence Encode, not Codec.
    encode_frame(&announce(secret, unit), &mut out).map_err(|e| WireError::Encode(format!("{e:?}")))?;
    tokio::time::timeout(timeout, stream.write_all(&out))
        .await
        .map_err(|_| WireError::Io("sending our Announce: timed out".into()))?
        .map_err(|e| WireError::Io(format!("sending our Announce: {e}")))?;

    let mut reader = FrameReader::new();
    let mut seen: Vec<Message> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let read = match tokio::time::timeout(timeout, stream.read(&mut buf)).await {
            Err(_) => {
                // A timeout is fatal only if we have nothing yet. Once the
                // gateway has introduced itself, a node that offers nothing and
                // then goes quiet has still told us who it is, and that is worth
                // more than an error.
                if seen.is_empty() {
                    return Err(WireError::Io(
                        "waiting for the gateway: timed out".into(),
                    ));
                }
                break;
            }
            // A reset is how a gateway that closes without a FIN arrives. Same
            // rule as a timeout: keep what we already decoded.
            Ok(Err(e)) => {
                if seen.is_empty() {
                    return Err(WireError::Io(format!("reading the gateway: {e}")));
                }
                break;
            }
            Ok(Ok(n)) => n,
        };
        if read == 0 {
            break;
        }
        reader.push(&buf[..read]);
        while let Some(next) = reader.next_message() {
            seen.push(next.map_err(WireError::Codec)?);
        }
        if seen
            .iter()
            .any(|m| matches!(m, Message::Announce(_)))
            && seen.iter().any(|m| matches!(m, Message::Offer(_)))
        {
            break;
        }
    }

    // A peer that answered with a refusal told us why it will not sell. Reporting
    // that as "closed without introducing itself" would be false about the socket
    // AND would make the app retry a permanent condition forever.
    if let Some(reason) = refusal_in(&seen) {
        return Err(WireError::Refused(reason));
    }
    detect_from(&seen).ok_or(WireError::Io(
        "the gateway closed without introducing itself".into(),
    ))
}

/// `host`/`host:port` → `host:4747` when no port was given.
///
/// IPv6 literals carry colons of their own, so a bracketed address is the only
/// thing that can safely have its last colon treated as a port separator.
fn with_default_port(endpoint: &str) -> String {
    // v1 gateways were addressed as URLs, so callers (and the app's stored
    // settings, and its seed list) still carry `http://`. v2 is raw TCP: a
    // scheme here would be parsed as part of the host and fail to resolve.
    let trimmed = endpoint
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let has_explicit_port = if let Some(rest) = trimmed.strip_prefix('[') {
        // `[::1]` or `[::1]:4747`
        rest.split_once(']')
            .map(|(_, after)| after.starts_with(':'))
            .unwrap_or(false)
    } else {
        trimmed.matches(':').count() == 1
    };
    if has_explicit_port {
        trimmed.to_string()
    } else {
        format!("{trimmed}:{DEFAULT_PORT}")
    }
}

#[cfg(test)]
mod address_tests {
    use super::with_default_port;

    #[test]
    fn a_bare_host_gets_the_default_port() {
        assert_eq!(with_default_port("10.0.2.2"), "10.0.2.2:4747");
        assert_eq!(with_default_port("gateway.local"), "gateway.local:4747");
    }

    #[test]
    fn an_explicit_port_is_left_alone() {
        assert_eq!(with_default_port("10.0.2.2:9000"), "10.0.2.2:9000");
        assert_eq!(with_default_port("[::1]:9000"), "[::1]:9000");
    }

    #[test]
    fn a_url_shaped_endpoint_is_reduced_to_host_and_port() {
        // v1 addressed gateways as URLs; v2 speaks raw TCP and would try to
        // resolve "http" as a host.
        assert_eq!(with_default_port("http://10.0.2.2:4747"), "10.0.2.2:4747");
        assert_eq!(with_default_port("http://192.168.8.1:4747/"), "192.168.8.1:4747");
        assert_eq!(with_default_port("http://10.0.2.2"), "10.0.2.2:4747");
        assert_eq!(with_default_port("  http://[fd00::1]:4747  "), "[fd00::1]:4747");
    }

    #[test]
    fn a_bracketed_ipv6_without_a_port_gets_one() {
        // The colons in the address must not be mistaken for a separator.
        assert_eq!(with_default_port("[fd00::1]"), "[fd00::1]:4747");
    }
}

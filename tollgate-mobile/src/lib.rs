//! Native Android TollGate client — Rust core.
//!
//! This crate is the Rust half of the native Android TollGate client (kanban
//! `t_e8a0c51f`). It replaces the JS captive-portal SPA
//! (`tollgate-captive-portal-site`, TollGate v1: NIP-01 kind 10021 over HTTP
//! :2121) with a fully-native app that speaks the **TollGate v2** CBOR protocol
//! implemented by `tollgate-rs`.
//!
//! Design (modelled on `fips-android` / Michael Malmi's native FIPS app):
//! - reuses the real wire types from [`tollgate_protocol`];
//! - speaks v2 over raw TCP to the gateway's control port ([`v2`]);
//! - exposes a single UniFFI object [`TollgateMobileNode`] to Kotlin, owning a
//!   tokio runtime so the Android UI thread never blocks on network I/O.
//!
//! Scope: this speaks the half of the protocol that needs no wallet — introduce,
//! be offered terms, read a refusal. Buying is a Spilman channel funded by a
//! Cashu wallet, and the phone has no wallet yet; `ARCHITECTURE.md` tracks it.
//!
//! What changed from the first cut: v2 has **no** `POST /tollgate/v1/exchange`.
//! The old core posted an `Announce` + `BootstrapToken` to an HTTP endpoint and
//! read back a `PriceSheet`; none of those types exist any more, and the Offer
//! that replaced them deliberately carries no price. Any UI showing a price per
//! second or per unit is now showing something the gateway never said.

uniffi::setup_scaffolding!("tollgate_mobile");

#[cfg(feature = "fips")]
mod jni;

mod v2;
#[cfg(test)]
mod v2_session_tests;
#[cfg(test)]
mod v2_tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use thiserror::Error;
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors surfaced across the FFI boundary.
#[derive(uniffi::Enum, Error, Debug)]
pub enum TollgateError {
    /// The socket failed, timed out, or the gateway never introduced itself.
    #[error("network error: {message}")]
    Network { message: String },
    /// A frame arrived whole and did not decode. The stream is still in sync,
    /// so this is about one message rather than a broken connection.
    #[error("protocol error: {message}")]
    Protocol { message: String },
    #[error("{message}")]
    Other { message: String },
}

impl From<v2::WireError> for TollgateError {
    fn from(e: v2::WireError) -> Self {
        match e {
            v2::WireError::Io(message) => Self::Network { message },
            v2::WireError::Codec(e) => Self::Protocol {
                message: format!("{e:?}"),
            },
        }
    }
}

impl From<anyhow::Error> for TollgateError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other {
            message: e.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Identity — port of tollgate-net config::Identity (load-or-generate keypair)
// ---------------------------------------------------------------------------

/// The node's signing identity: a secp256k1 keypair persisted to
/// `<data_dir>/identity.hex` (the secret key as 64 hex chars, 0600). Generated
/// on first use; stable thereafter. Mirrors `config::Identity::load_or_generate`.
pub(crate) struct Identity {
    pub public_key: PublicKey,
    secret_key: SecretKey,
}

impl Identity {
    /// Load the keypair from `<dir>/identity.hex`, or generate + persist a new
    /// one if absent. `dir` is the app's `filesDir` on Android.
    fn load_or_generate(dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(dir).context("creating data dir")?;
        let path = dir.join("identity.hex");
        let secp = Secp256k1::new();
        let (secret_key, public_key) = match std::fs::read_to_string(&path) {
            Ok(hex_str) => {
                let bytes = hex::decode(hex_str.trim()).context("identity.hex not valid hex")?;
                let sk = SecretKey::from_slice(&bytes).context("invalid secret key in identity")?;
                let pk = sk.public_key(&secp);
                (sk, pk)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let (sk, pk) = secp.generate_keypair(&mut secp256k1::rand::rngs::OsRng);
                // Persist (best-effort; 0600 to protect the secret).
                let _ = persist_secret(&path, &sk);
                (sk, pk)
            }
            Err(e) => return Err(e).context("reading identity.hex"),
        };
        Ok(Self {
            public_key,
            secret_key,
        })
    }

    fn pubkey_hex(&self) -> String {
        hex::encode(self.public_key.serialize())
    }
}

fn persist_secret(path: &Path, sk: &SecretKey) -> anyhow::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context("opening identity.hex for write")?;
    writeln!(f, "{}", hex::encode(sk.secret_bytes())).context("writing identity.hex")?;
    // Best-effort restrictive perms; Android filesDir is app-private anyway.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Protocol v2 client
// ---------------------------------------------------------------------------

/// The unit this client asks a gateway to denominate in. Bytes, because that is
/// what data delivery is measured in and what the exit meters.
const UNIT: &str = "bytes";

/// How long any single step of a handshake may take.
///
/// A gateway that accepts the TCP connection and then says nothing would
/// otherwise hang the UI thread's blocking call for as long as it felt like.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(8);

/// A gateway's terms, as the app needs to show them.
///
/// Deliberately without a price. Protocol v2's `Offer` carries mints, unit and
/// limits, but no money: delivery is one voucher per unit, so the price is set
/// by the mint the gateway accepts, not by the gateway. A UI that printed a
/// number here would be stating someone else's price as fact.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct OfferTerms {
    /// Mints whose vouchers this gateway accepts, most preferred first.
    pub accepted_mints: Vec<String>,
    pub unit: String,
    /// Shortest budget window it accepts, in milliseconds.
    pub min_window_ms: u64,
    /// Longest budget it can be kept for, in milliseconds.
    pub max_window_ms: u64,
    /// What one unit from us draws from the budget, where one unit to us draws
    /// one: `moved = to_us + from_us × weight`.
    pub from_payer_weight: u16,
    /// True when the gateway does not charge at all, so no channel is funded.
    pub no_charge: bool,
    pub min_reserved_rate: u64,
    /// Shortest gap it accepts between two purchases from one payer.
    pub min_topup_gap_ms: u64,
}

impl From<&v2::OfferView> for OfferTerms {
    fn from(o: &v2::OfferView) -> Self {
        Self {
            accepted_mints: o.accepted_mints.clone(),
            unit: o.unit.clone(),
            min_window_ms: o.min_window_ms,
            max_window_ms: o.max_window_ms,
            from_payer_weight: o.from_payer_weight,
            no_charge: o.no_charge,
            min_reserved_rate: o.min_reserved_rate,
            min_topup_gap_ms: o.min_topup_gap_ms,
        }
    }
}

/// What a gateway told us about itself.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    /// The gateway's compressed pubkey, 66 hex chars — who we would pay.
    pub pubkey_hex: String,
    /// The unit its offer is denominated in.
    pub unit: String,
    /// Protocol version it announced. A mismatch is not an error here: it is
    /// something to show, because it is the commonest reason a session fails.
    pub version: u8,
    /// Its terms, when it sent an Offer. Absent means it introduced itself but
    /// offered nothing to buy.
    pub offer: Option<OfferTerms>,
}

// ---------------------------------------------------------------------------
// UniFFI facade — the Kotlin-facing object
// ---------------------------------------------------------------------------

/// The native TollGate client. Owns a tokio runtime and a persisted identity.
///
/// Kotlin usage:
/// ```kotlin
/// val node = TollgateMobileNode(context.filesDir.absolutePath)
/// val gateway = node.detect("10.0.2.2:4747")   // or "[fd00::1]:4747"
/// // gateway.pubkeyHex, gateway.unit, gateway.offer?.acceptedMints ...
/// ```
///
/// What this object does **not** do yet: buy. A v2 purchase funds a Spilman
/// channel, which needs a Cashu wallet on the device and a funding blob from
/// it; neither exists in this app. `detect` is the half of the protocol that
/// needs no wallet, and it is the half a user needs in order to decide whether
/// to buy at all.
#[derive(uniffi::Object)]
pub struct TollgateMobileNode {
    runtime: Arc<Runtime>,
    identity: Identity,
    data_dir: PathBuf,
}

#[uniffi::export]
impl TollgateMobileNode {
    /// Load-or-generate the device identity under `data_dir` (Android
    /// `context.filesDir`).
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, TollgateError> {
        let dir = PathBuf::from(&data_dir);
        let identity = Identity::load_or_generate(&dir)?;
        let runtime = Arc::new(Runtime::new().context("building tokio runtime")?);
        Ok(Arc::new(Self {
            runtime,
            identity,
            data_dir: dir,
        }))
    }

    /// This device's compressed pubkey as 66 hex chars.
    pub fn pubkey_hex(&self) -> String {
        self.identity.pubkey_hex()
    }

    /// Where the identity (and future wallet state) is persisted.
    pub fn data_dir(&self) -> String {
        self.data_dir.to_string_lossy().into_owned()
    }

    /// Open a session with the gateway at `endpoint` (`host`, `host:port`, or a
    /// bracketed IPv6 literal) and learn its identity, unit and terms.
    ///
    /// Synchronous for the caller; the async work runs on the owned runtime so
    /// the Kotlin caller is the only thread blocked.
    pub fn detect(&self, endpoint: String) -> Result<Detected, TollgateError> {
        let detected = self
            .runtime
            .block_on(v2::detect(
                &endpoint,
                &self.identity.secret_key,
                UNIT,
                HANDSHAKE_TIMEOUT,
            ))
            .map_err(TollgateError::from)?;
        Ok(Detected {
            pubkey_hex: detected.pubkey_hex,
            unit: detected.unit,
            version: detected.version,
            offer: detected.offer.as_ref().map(OfferTerms::from),
        })
    }
}

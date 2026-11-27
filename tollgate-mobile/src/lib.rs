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
//! Cashu wallet, and the phone has no wallet yet; [`ARCHITECTURE.md`] at the repo
//! root records what that needs and which decisions are still open.
//!
//! [`ARCHITECTURE.md`]: ../ARCHITECTURE.md
//!
//! What changed from the first cut: v2 has **no** `POST /tollgate/v1/exchange`.
//! The old core posted an `Announce` + `BootstrapToken` to an HTTP endpoint and
//! read back a `PriceSheet`; none of those types exist any more, and the Offer
//! that replaced them deliberately carries no price. Any UI showing a price per
//! second or per unit is now showing something the gateway never said.

uniffi::setup_scaffolding!("tollgate_mobile");

#[cfg(feature = "fips")]
mod jni;

pub mod claim;
pub mod gateway;
pub mod purchase;
pub mod v1;
mod v2;
#[cfg(test)]
mod v2_session_tests;
#[cfg(test)]
mod v2_tests;
pub mod wallet;

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

/// Carry the *kind* of discovery failure through to the caller.
///
/// Flattening a `DiscoverError` to `Other` loses the one thing Kotlin acts on:
/// whether the peer is unreachable (retry, show "no gateway") or answered
/// wrongly (show the reason). `Network` is the variant the UI treats as "try
/// again", so an I/O failure has to arrive as `Network`, not as prose in `Other`.
fn discover_error(e: gateway::DiscoverError) -> TollgateError {
    match e {
        gateway::DiscoverError::V2 { message, io: true } => TollgateError::Network { message },
        other => TollgateError::Other {
            message: other.to_string(),
        },
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

    /// The 64-byte wallet seed, derived from the device's own secret key.
    ///
    /// Derived rather than stored a second time, so restoring `identity.hex` on a
    /// new phone restores the money with it, and there is no second secret to lose.
    /// The tag stops the same bytes standing in as anything else.
    /// This device's x-only public key as hex, for a harness that needs to know
    /// which address the exit will derive. The app shows `pubkey_hex` and has no
    /// use for this, so it is not exported.
    pub fn pubkey_hex_identity(&self) -> String {
        let (xonly, _) = self.secret_key.x_only_public_key(&Secp256k1::new());
        xonly
            .serialize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// This device's x-only public key, 32 bytes — the identity an exit derives
    /// an address from and admits.
    fn public_key_bytes(&self) -> [u8; 32] {
        let (xonly, _) = self.secret_key.x_only_public_key(&Secp256k1::new());
        xonly.serialize()
    }

    /// Sign a purchase claim's digest with the device key.
    ///
    /// This is the device's own identity — the key the exit derives an address
    /// from. Using it is the whole point: a claim signed by any other key would
    /// be refused, so nothing else can be substituted here.
    fn sign_claim(&self, digest: &[u8; 32]) -> String {
        let secp = Secp256k1::new();
        let keypair = secp256k1::Keypair::from_secret_key(&secp, &self.secret_key);
        let sig = secp.sign_schnorr(digest, &keypair);
        sig.serialize().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn wallet_seed(&self) -> [u8; 64] {
        use sha2::{Digest, Sha256};
        let secret = self.secret_key.secret_bytes();
        // Two domain-separated digests, so one 32-byte hash is never stretched into a
        // 64-byte seed and both halves get full-width input. The tag keeps these
        // bytes from doubling as anything else in the protocol.
        let mut first = Sha256::new();
        first.update(b"tollgate-mobile/wallet-seed/v1");
        first.update(secret);
        let first: [u8; 32] = first.finalize().into();

        let mut second = Sha256::new();
        second.update(b"tollgate-mobile/wallet-seed/v1/2");
        second.update(first);
        let second: [u8; 32] = second.finalize().into();

        let mut seed = [0u8; 64];
        seed[..32].copy_from_slice(&first);
        seed[32..].copy_from_slice(&second);
        seed
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
// Protocol v1 — the API a `tollgate-module-basic-go` router serves on :2121
// ---------------------------------------------------------------------------

/// One way to pay a v1 gateway: a price per step, in a named mint's vouchers.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PriceOptionV1 {
    /// `"cashu"` in every advertisement seen so far.
    pub asset: String,
    /// Price per `step_size` units of the metered resource, in `unit`.
    pub price_per_step: u64,
    /// The money unit — `"sat"` — not the metered one.
    pub unit: String,
    pub mint_url: String,
    /// Least a purchase may be, in steps.
    pub min_steps: u64,
}

/// A v1 gateway's advertisement: what it meters, how finely, and for how much.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct AdvertisementV1 {
    /// `"bytes"` or `"milliseconds"`.
    pub metric: String,
    /// What one `price_per_step` buys, in `metric` units.
    pub step_size: u64,
    /// TIP numbers the gateway claims to support.
    pub tips: Vec<String>,
    pub prices: Vec<PriceOptionV1>,
    /// The pubkey that signed the advertisement — the gateway's Nostr identity.
    pub pubkey: String,
    /// The MAC the gateway resolves for *this* device, from `GET /whoami`.
    ///
    /// Empty when the gateway cannot identify the caller. This is the whole of
    /// v1's identity, and it is the gateway's opinion, not ours: it is resolved
    /// from the connection, so the device that asks is the device that buys.
    pub device_mac: String,
}

impl From<v1::Advertisement> for AdvertisementV1 {
    fn from(a: v1::Advertisement) -> Self {
        Self {
            metric: a.metric,
            step_size: a.step_size,
            tips: a.tips,
            prices: a
                .prices
                .into_iter()
                .map(|p| PriceOptionV1 {
                    asset: p.asset,
                    price_per_step: p.price_per_step,
                    unit: p.unit,
                    mint_url: p.mint_url,
                    min_steps: p.min_steps,
                })
                .collect(),
            pubkey: a.pubkey,
            device_mac: String::new(),
        }
    }
}

/// A quote and the Lightning invoice that settles it.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct InvoiceV1 {
    /// Quote id; the status poll is keyed by it.
    pub quote: String,
    /// BOLT11 to pay. Empty only if the gateway granted access without one.
    pub invoice: String,
    pub mint_url: String,
    /// Sats to pay — the amount quoted, not the amount asked for; a gateway may
    /// adjust it for its mint's fees.
    pub amount_sat: u64,
    /// Unix seconds.
    pub expiry: u64,
    pub state: String,
}

impl From<v1::Invoice> for InvoiceV1 {
    fn from(i: v1::Invoice) -> Self {
        Self {
            quote: i.quote,
            invoice: i.invoice,
            mint_url: i.mint_url,
            amount_sat: i.amount_sat,
            expiry: i.expiry,
            state: i.state,
        }
    }
}

/// One poll of a quote.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct InvoiceStatusV1 {
    pub quote: String,
    /// `"UNPAID"`, `"PAID"`, `"ISSUED"`, `"EXPIRED"`…
    pub state: String,
    /// True once the gateway has actually opened the gate for this device.
    ///
    /// A `PAID` quote with this false is a paid purchase applied to nobody —
    /// the failure the Go module logs at ERROR and retries. Never read `state`
    /// alone before telling a customer they are online.
    pub access_granted: bool,
    pub allotment: u64,
    pub metric: String,
}

impl From<v1::InvoiceStatus> for InvoiceStatusV1 {
    fn from(s: v1::InvoiceStatus) -> Self {
        Self {
            quote: s.quote,
            state: s.state,
            access_granted: s.access_granted,
            allotment: s.allotment,
            metric: s.metric,
        }
    }
}

/// What the gateway thinks this device has left.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct BalanceV1 {
    pub session_active: bool,
    pub usage: u64,
    pub allotment: u64,
    pub remaining: u64,
    pub metric: String,
}

impl From<v1::Balance> for BalanceV1 {
    fn from(b: v1::Balance) -> Self {
        Self {
            session_active: b.session_active,
            usage: b.usage,
            allotment: b.allotment,
            remaining: b.remaining,
            metric: b.metric,
        }
    }
}

// ---------------------------------------------------------------------------
// The peer a v1-native app shows
// ---------------------------------------------------------------------------

/// A peer as the app shows it.
///
/// `protocol` is the spine of this record: the UI decides what to offer from it,
/// and it is the *only* thing that differs between a gateway that can take money
/// and one that cannot.
#[derive(uniffi::Record, Clone, Debug, PartialEq, Eq)]
pub struct PeerV1 {
    /// `"v1"` or `"v2"`. A v2 peer can be shown and cannot be bought.
    pub protocol: String,
    /// The endpoint that answered.
    pub endpoint: String,
    /// `"bytes"` / `"milliseconds"`.
    pub metric: String,
    /// What this gateway charges. **Empty on v2, always** — the protocol carries
    /// no price, and a number here would state the mint's business as ours.
    pub prices: Vec<PriceOptionV1>,
    /// The MAC the gateway resolved for this device. v1's identity is the
    /// network's opinion of who you are, so this is also who a purchase applies to.
    /// Absent means the gateway could not place this device, and a purchase will be
    /// refused — which is why `purchasable` checks it.
    pub device_mac: Option<String>,
    /// The peer's advertised signing key, when it published one.
    pub pubkey: Option<String>,
    /// Why nothing can be bought, when nothing can. Never empty for v2.
    pub unavailable: Option<String>,
    /// Whether a purchase is possible right now.
    pub purchasable: bool,
}

impl PeerV1 {
    fn from_view(v: &gateway::PeerView) -> Self {
        Self {
            protocol: v.protocol.as_str().to_string(),
            endpoint: v.endpoint.clone(),
            metric: v.metric.clone(),
            prices: v
                .prices
                .iter()
                .map(|p| PriceOptionV1 {
                    asset: "cashu".into(),
                    price_per_step: p.price_per_step,
                    unit: p.unit.clone(),
                    mint_url: p.mint_url.clone(),
                    min_steps: p.min_steps,
                })
                .collect(),
            device_mac: v.device_mac.clone(),
            pubkey: v.pubkey.clone(),
            unavailable: v.unavailable.clone(),
            purchasable: v.purchasable(),
        }
    }
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
    /// The ecash this device holds. Opened lazily by `new`, because it allocates a
    /// database and a `new` that can fail on a bad filesystem should say so once, at
    /// startup, rather than at the first purchase.
    wallet: Arc<wallet::Wallet>,
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
        let seed = identity.wallet_seed();
        let wallet = runtime
            .block_on(wallet::Wallet::open(dir.join("wallet.sqlite"), seed))
            .map_err(|e| TollgateError::Other {
                message: format!("opening the ecash wallet: {e}"),
            })?;
        Ok(Arc::new(Self {
            runtime,
            identity,
            data_dir: dir,
            wallet: Arc::new(wallet),
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

    /// Open a session with the gateway at `endpoint` and learn what it sells.
    ///
    /// v1 first, because that is what a deployed gateway serves and the only lane
    /// that can take a payment in this build. A v2 peer is still reported — behind
    /// the adapter, so it appears with no price and cannot be bought — because a
    /// user on a v2 network deserves to know the peer is there.
    ///
    /// Synchronous for the caller; the async work runs on the owned runtime so the
    /// Kotlin caller is the only thread blocked.
    pub fn detect(&self, endpoint: String) -> Result<PeerV1, TollgateError> {
        let gw = self
            .runtime
            .block_on(gateway::probe(&endpoint, HANDSHAKE_TIMEOUT))
            .map_err(discover_error)?;
        let view = self
            .runtime
            .block_on(gw.describe())
            .map_err(discover_error)?;
        Ok(PeerV1::from_view(&view))
    }

    /// Ask the gateway for a Lightning invoice for `amount_sat`, at a mint it
    /// advertises. Nothing is paid: the invoice goes to whatever wallet the user
    /// already has, and the vouchers it buys never touch this device.
    ///
    /// Refused on a v2 peer, with the reason — v2 has no invoice to give.
    pub fn request_invoice(
        &self,
        endpoint: String,
        amount_sat: u64,
        mint_url: String,
    ) -> Result<InvoiceV1, TollgateError> {
        let gw = Self::v1_gateway_impl(&self.runtime, &endpoint)?;
        self.runtime
            .block_on(purchase::quote(&gw, amount_sat, &mint_url))
            .map_err(|e| TollgateError::Other {
                message: e.message(),
            })
            .and_then(|phase| match phase {
                purchase::PurchasePhase::Quoted {
                    invoice,
                    amount_sat,
                    quote,
                } => Ok(InvoiceV1 {
                    quote,
                    invoice,
                    mint_url,
                    amount_sat,
                    expiry: 0,
                    state: "UNPAID".into(),
                }),
                other => Err(TollgateError::Other {
                    message: format!("the gateway did not quote: {other:?}"),
                }),
            })
    }

    /// Poll a quote until the gateway reports the gate open, or `timeout_ms`
    /// passes. Returns the grant on success.
    ///
    /// A paid quote whose access was not granted is deliberately not success: it is
    /// the case the Go module logs at ERROR, and the caller gets a "not yet" rather
    /// than access it does not have.
    pub fn await_grant(
        &self,
        endpoint: String,
        quote: String,
        timeout_ms: u64,
    ) -> Result<InvoiceStatusV1, TollgateError> {
        let gw = Self::v1_gateway_impl(&self.runtime, &endpoint)?;
        self.runtime
            .block_on(purchase::await_grant(
                &gw,
                &quote,
                Duration::from_millis(1_500),
                Duration::from_millis(timeout_ms),
            ))
            .map(|phase| match phase {
                purchase::PurchasePhase::Granted(grant) => InvoiceStatusV1 {
                    quote: quote.clone(),
                    state: grant.state,
                    access_granted: grant.access_granted,
                    allotment: grant.allotment,
                    metric: grant.metric,
                },
                purchase::PurchasePhase::Expired => InvoiceStatusV1 {
                    quote: quote.clone(),
                    state: "EXPIRED".into(),
                    access_granted: false,
                    allotment: 0,
                    metric: String::new(),
                },
                other => InvoiceStatusV1 {
                    quote: quote.clone(),
                    state: format!("{other:?}"),
                    access_granted: false,
                    allotment: 0,
                    metric: String::new(),
                },
            })
            .map_err(|e| TollgateError::Other {
                message: e.message(),
            })
    }

    /// What this device has left, on a v1 gateway.
    pub fn balance(&self, endpoint: String) -> Result<BalanceV1, TollgateError> {
        let gw = Self::v1_gateway_impl(&self.runtime, &endpoint)?;
        self.runtime
            .block_on(gw.balance())
            .map(BalanceV1::from)
            .map_err(|e| TollgateError::Other {
                message: e.to_string(),
            })
    }
    /// What this device is holding, money first.
    ///
    /// One entry per (mint, unit) that has a positive balance. A device short of
    /// money cannot buy from anyone, where one short of one issuer's vouchers is
    /// only short with that issuer, so `sat` sorts first.
    /// Buy v1 access by signing the exit's challenge with this device's key.
    ///
    /// This is the claim path, and it is the only path that can buy from an exit
    /// that binds a purchase to a peer. The invoice path cannot: it opens a quote
    /// at a mint and the exit then has no idea which tunnel paid.
    ///
    /// `endpoint` must be the address the exit will observe for this device. The
    /// exit derives an address from the key that signs and compares it with the
    /// tunnel it saw; if they differ it refuses, and it does so with the same
    /// status as a bad signature so the refusal cannot be used to probe other
    /// peers.
    pub fn buy_with_claim(
        &self,
        endpoint: String,
        ttl_seconds: u64,
        byte_budget: u64,
        observed_peer: Option<String>,
    ) -> Result<crate::v1::GrantV1, TollgateError> {
        let gw = Self::v1_gateway_impl(&self.runtime, &endpoint)?;
        let signed = self
            .runtime
            .block_on(gw.claim_and_buy(&self.identity, ttl_seconds, byte_budget, observed_peer))
            .map_err(|e| TollgateError::Other {
                message: e.to_string(),
            })?;
        Ok(signed)
    }

    /// Hex of this device's x-only public key, so a harness can compute the
    /// address the exit will derive for it.
    pub fn pubkey_bytes_for_test(&self) -> String {
        self.identity.pubkey_hex_identity()
    }

    pub fn wallet_balance(&self) -> Vec<HoldingV1> {
        let holdings = self.runtime.block_on(self.wallet.balance());
        holdings
            .into_iter()
            .map(|h| HoldingV1 {
                mint: h.mint,
                unit: h.unit,
                amount: h.amount,
            })
            .collect()
    }

    /// Take a Cashu token into this device's wallet.
    ///
    /// Receiving is a swap at the issuing mint, so a successful return means the
    /// paper was good and is now under keys only this device has. The `amount` in
    /// the answer is what the mint said it was worth — which is the number a
    /// purchase is actually priced against.
    pub fn wallet_receive(&self, token: String) -> Result<HoldingV1, TollgateError> {
        let holding = self
            .runtime
            .block_on(self.wallet.receive(&token))
            .map_err(|e| TollgateError::Other {
                message: e.to_string(),
            })?;
        Ok(HoldingV1 {
            mint: holding.mint,
            unit: holding.unit,
            amount: holding.amount,
        })
    }

    /// Buy access from `endpoint` by paying with paper this device already holds.
    ///
    /// This is the second v1 purchase lane, and the one that needs a wallet: the
    /// first lane (`request_invoice`) hands the buyer a BOLT11 invoice and never
    /// touches this device's ecash. Here the device spends `amount` from its own
    /// holdings at `mint`, posts the resulting token to the gateway, and reports
    /// what the gateway granted.
    ///
    /// `amount` is in `unit`'s units — `sat` for a sat-priced gateway. Spending
    /// more than is held is refused before anything leaves the device, with the
    /// amount held named in the error.
    pub fn buy_with_held_ecash(
        &self,
        endpoint: String,
        mint: String,
        unit: String,
        amount: u64,
    ) -> Result<InvoiceStatusV1, TollgateError> {
        let gw = Self::v1_gateway_impl(&self.runtime, &endpoint)?;

        let held = self.runtime.block_on(self.wallet.balance());
        let available = held
            .iter()
            .find(|h| h.mint == mint && h.unit == unit)
            .map(|h| h.amount)
            .unwrap_or(0);
        if available < amount {
            return Err(TollgateError::Other {
                message: format!(
                    "this device holds {available} {unit} from {mint}, and {amount} was asked for"
                ),
            });
        }

        let token = self
            .runtime
            .block_on(self.wallet.spend(&mint, &unit, amount))
            .map_err(|e| TollgateError::Other {
                message: e.to_string(),
            })?;

        // The token is handed over here. If the gateway refuses, the paper has
        // moved and cdk holds the proofs; it is not lost, but it is no longer
        // spendable until it is received back or the gateway is retried.
        let grant = self
            .runtime
            .block_on(gw.submit_token(&token))
            .map_err(|e| TollgateError::Other {
                message: format!("the gateway did not take the token: {e}"),
            })?;

        Ok(InvoiceStatusV1 {
            // A token purchase has no quote: the gateway answers with a grant, and
            // the receipt's identity is the token itself. Empty rather than invented,
            // so a UI that shows a quote id shows nothing rather than a lie.
            quote: String::new(),
            state: grant.state,
            access_granted: grant.access_granted,
            allotment: grant.allotment,
            metric: grant.metric,
        })
    }
}

/// One balance this device holds, from one issuer, in one unit.
///
/// `mint` is normalised (lower-case scheme, no trailing slash) so two spellings of
/// one issuer do not look like two balances.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HoldingV1 {
    /// The issuer's mint URL.
    pub mint: String,
    /// The unit the mint accounts in: `sat`, or a gateway's `bytes`.
    pub unit: String,
    /// How much of it this device holds.
    pub amount: u64,
}

/// Internals the FFI does not see. `#[uniffi::export]` exports every method in
/// its block, so anything returning a Rust-only type lives here.
impl TollgateMobileNode {
    /// A v1 client for `endpoint`, or a named refusal if the peer does not speak v1.
    ///
    /// One probe decides: a v2-only peer is refused here rather than at the invoice,
    /// so the UI never offers a purchase the protocol cannot complete.
    fn v1_gateway_impl(runtime: &Runtime, endpoint: &str) -> Result<v1::Gateway, TollgateError> {
        let gw = runtime
            .block_on(gateway::probe(endpoint, HANDSHAKE_TIMEOUT))
            .map_err(|e| TollgateError::Other {
                message: e.to_string(),
            })?;
        match gw {
            gateway::Gateway::V1(client) => Ok(client),
            // A v2-only peer is refused here rather than at the invoice, so the UI
            // never offers a purchase the protocol cannot complete.
            gateway::Gateway::V2 { view, .. } => Err(TollgateError::Other {
                message: view.unavailable.unwrap_or_else(|| {
                    "this peer speaks v2 only, and this build cannot buy over v2".into()
                }),
            }),
        }
    }
}

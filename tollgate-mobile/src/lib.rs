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
//! - ports the client logic that lives inside the `tollgate-net` binary's
//!   `client.rs` (`detect` / `pay` / `consume`) into a mobile-friendly shape;
//! - exposes a single UniFFI object [`TollgateMobileNode`] to Kotlin, owning a
//!   tokio runtime so the Android UI thread never blocks on network I/O.
//!
//! The bootstrap-token builder is the same Cashu stub `tollgate-net` ships
//! today (`build_test_token`): a syntactically valid token with a filler
//! signature, accepted by the test/fake mint. Real-wallet integration
//! (minting live tokens from a configured Cashu mint) is the same open item
//! upstream and is tracked in ARCHITECTURE.md.
//!
//! Android receive-side metering (the `meter_iface` / `meter_upstream` knobs
//! in `tollgate-net`, which read `/proc/net/arp` + nftables counters) is
//! Linux-router specific and omitted here — the mobile client acknowledges the
//! provider's own `delivered` count, the same fallback path `tollgate-net`
//! takes when no meter is configured.

uniffi::setup_scaffolding!("tollgate_mobile");

#[cfg(feature = "fips")]
mod jni;

pub mod wallet;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, anyhow};
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use thiserror::Error;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use tollgate_protocol::{
    Announce, BootstrapAck, BootstrapToken, MessageType, MeteringReport, PROTOCOL_VERSION,
    PriceSheet, PublicKey as TgPublicKey, Reject, decode_frames, encode_frame, frame, peek_type,
};

// ---------------------------------------------------------------------------
// UniFFI-exposed types
// ---------------------------------------------------------------------------

/// A price the peer charges, in scaled milli-units (matches `tollgate-core`).
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq, Eq)]
pub struct PriceView {
    pub per_second: i64,
    pub per_unit: i64,
}

impl PriceView {
    fn cost_scaled(&self, elapsed_ms: u64, units: u64) -> i64 {
        // Mirror tollgate_core::Price::cost_scaled: per-second accrues on
        // elapsed time, per-unit on delivered units. i64 so negative pricing
        // (provider pays us) is expressible.
        let by_time = (elapsed_ms as i64)
            .saturating_mul(self.per_second)
            .saturating_div(1000);
        let by_units = (units as i64).saturating_mul(self.per_unit);
        by_time.saturating_add(by_units)
    }
}

/// Outcome of a detection probe: who the peer is + what it charges.
#[derive(uniffi::Record, Clone, Debug)]
pub struct Detected {
    pub pubkey_hex: String,
    pub unit: String,
    pub version: u8,
    pub price: Option<PriceView>,
}

/// Outcome of a bootstrap payment.
#[derive(uniffi::Record, Clone, Debug)]
pub struct Paid {
    pub peer_pubkey_hex: String,
    pub accepted: bool,
    pub reason: Option<String>,
    pub price: Option<PriceView>,
}

/// One observation from the consume loop, surfaced to the UI each poll.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ConsumeEvent {
    /// 0 = the initial "start" event, then 1, 2, … per poll.
    pub poll: u32,
    pub peer_pubkey: String,
    pub price: PriceView,
    pub paid_scaled: u64,
    /// Signed remaining balance: `+` prepaid credit left, `−` provider owes us.
    pub remaining_scaled: i64,
    pub report: Option<ReportSummary>,
    /// The peer reported us balance-exhausted (cut off).
    pub cut_off: bool,
    /// We sent a top-up this poll.
    pub topped_up: bool,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct ReportSummary {
    pub elapsed_ms: u64,
    pub delivered: u64,
    pub received: u64,
}

/// A Cashu mint quote: the Lightning invoice to pay and the quote ID to
/// track its state.
#[derive(uniffi::Record, Clone, Debug)]
pub struct MintQuote {
    pub quote_id: String,
    pub invoice: String,
}

/// Errors surfaced across the FFI boundary. `reason` is a short human string.
#[derive(uniffi::Enum, Error, Debug)]
pub enum TollgateError {
    #[error("network error: {message}")]
    Network { message: String },
    #[error("protocol error: {message}")]
    Protocol { message: String },
    #[error("bootstrap rejected: {reason}")]
    Rejected { reason: String },
    #[error("{message}")]
    Other { message: String },
}

impl From<anyhow::Error> for TollgateError {
    fn from(e: anyhow::Error) -> Self {
        // Heuristic split: reqwest/HTTP failures → Network; the rest → Other.
        let s = e.to_string();
        let low = s.to_ascii_lowercase();
        if low.contains("error_for_status")
            || low.contains("posting to")
            || low.contains("connection")
            || low.contains("dns")
            || low.contains("timeout")
            || low.contains("connect")
        {
            TollgateError::Network { message: s }
        } else {
            TollgateError::Other { message: s }
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
// Client protocol — faithful port of tollgate-net/src/client.rs (detect/pay/run_consume)
// ---------------------------------------------------------------------------

/// Scale between sats and the node's internal milli-unit balance (matches
/// `tollgate-net::client::PRICING_SCALE`).
const PRICING_SCALE: u64 = 1000;
/// The unit this mobile client meters. Bytes (data delivered by the gateway).
const UNIT: &str = "bytes";

/// POST already-framed `request` to the peer's exchange endpoint, return the
/// decoded response message bodies.
async fn exchange(base_url: &str, request: Vec<u8>) -> anyhow::Result<Vec<Vec<u8>>> {
    let endpoint = format!(
        "{}/tollgate/v1/exchange",
        base_url.trim_end_matches('/')
    );
    let resp = reqwest::Client::new()
        .post(&endpoint)
        .header("content-type", "application/cbor")
        .body(request)
        .send()
        .await
        .with_context(|| format!("posting to {endpoint}"))?
        .error_for_status()
        .context("peer returned an error status")?;
    let bytes = resp.bytes().await.context("reading peer response")?;
    let frames = decode_frames(&bytes).map_err(|e| anyhow!("bad framing: {e:?}"))?;
    Ok(frames.into_iter().map(|f| f.to_vec()).collect())
}

/// Our own Announce, encoded.
fn our_announce(identity: &Identity) -> Vec<u8> {
    let pubkey = TgPublicKey::from_bytes(identity.public_key.serialize());
    Announce::new(PROTOCOL_VERSION, pubkey, UNIT, 0).encode()
}

fn find_announce(messages: &[Vec<u8>]) -> Option<Announce> {
    messages
        .iter()
        .filter(|m| matches!(peek_type(m), Some(MessageType::Announce)))
        .find_map(|m| Announce::decode(m).ok())
}

fn find_price_sheet(messages: &[Vec<u8>]) -> Option<PriceSheet> {
    messages
        .iter()
        .filter(|m| matches!(peek_type(m), Some(MessageType::PriceSheet)))
        .find_map(|m| PriceSheet::decode(m).ok())
}

fn find_metering_report(messages: &[Vec<u8>]) -> Option<MeteringReport> {
    messages
        .iter()
        .filter(|m| matches!(peek_type(m), Some(MessageType::MeteringReport)))
        .find_map(|m| MeteringReport::decode(m).ok())
}

fn balance_exhausted(messages: &[Vec<u8>]) -> bool {
    messages
        .iter()
        .filter(|m| matches!(peek_type(m), Some(MessageType::Reject)))
        .filter_map(|m| Reject::decode(m).ok())
        .any(|r| r.is_balance_exhausted())
}

fn price_from_sheet(sheet: &PriceSheet) -> PriceView {
    sheet
        .products
        .first()
        .and_then(|p| p.mints.first())
        .map(|m| PriceView {
            per_second: m.price_per_second,
            per_unit: m.price_per_unit,
        })
        .unwrap_or_default()
}

/// Send our Announce and learn the peer's identity + price.
pub(crate) async fn detect(base_url: &str, identity: &Identity) -> anyhow::Result<Detected> {
    let body =
        frame(&our_announce(identity)).map_err(|e| anyhow!("framing our Announce: {e:?}"))?;
    let messages = exchange(base_url, body).await?;
    let announce = find_announce(&messages).context("peer did not return an Announce")?;
    Ok(Detected {
        pubkey_hex: hex::encode(announce.public_key().as_bytes()),
        unit: announce.unit.clone(),
        version: announce.version,
        price: find_price_sheet(&messages).as_ref().map(price_from_sheet),
    })
}

/// Detect the peer and pay a bootstrap token of `amount_sat` drawn on `mint_url`.
pub(crate) async fn pay(
    base_url: &str,
    identity: &Identity,
    mint_url: &str,
    amount_sat: u64,
) -> anyhow::Result<Paid> {
    let token = build_bootstrap_token(mint_url, amount_sat).context("building bootstrap token")?;

    let mut body = Vec::new();
    let frame_err = |e| anyhow!("framing: {e:?}");
    encode_frame(&our_announce(identity), &mut body).map_err(frame_err)?;
    encode_frame(&BootstrapToken::new(token.into_bytes()).encode(), &mut body).map_err(frame_err)?;

    let messages = exchange(base_url, body).await?;
    let peer_pubkey_hex = find_announce(&messages)
        .map(|a| hex::encode(a.public_key().as_bytes()))
        .context("peer did not return an Announce")?;
    let ack = messages
        .iter()
        .filter(|m| matches!(peek_type(m), Some(MessageType::BootstrapAck)))
        .find_map(|m| BootstrapAck::decode(m).ok())
        .context("peer did not return a BootstrapAck")?;

    Ok(Paid {
        peer_pubkey_hex,
        accepted: ack.is_accepted(),
        reason: ack.reason,
        price: find_price_sheet(&messages).as_ref().map(price_from_sheet),
    })
}

/// Pay, then poll for MeteringReports, auto-topping-up before the balance runs
/// out. `on_event` fires once at start, then once per poll. Stops when
/// `max_polls` is reached (None = forever) or `stop` is set. Android skips the
/// Linux-only independent receive meter (`meter_iface`/`meter_upstream`) and
/// acknowledges the provider's own delivered count — the same fallback
/// `tollgate-net` uses with no meter configured.
pub(crate) async fn run_consume(
    base_url: &str,
    identity: &Identity,
    mint_url: &str,
    amount_sat: u64,
    topup_sat: u64,
    interval: Duration,
    max_polls: Option<u32>,
    stop: Arc<AtomicBool>,
    mut on_event: impl FnMut(ConsumeEvent),
) -> anyhow::Result<()> {
    let paid = pay(base_url, identity, mint_url, amount_sat).await?;
    if !paid.accepted {
        anyhow::bail!(
            "initial bootstrap rejected: {}",
            paid.reason.as_deref().unwrap_or("unknown")
        );
    }
    let price = paid.price.clone().unwrap_or_default();
    let peer_pubkey = paid.peer_pubkey_hex.clone();
    let topup_scaled = topup_sat.saturating_mul(PRICING_SCALE);
    let mut paid_scaled = amount_sat.saturating_mul(PRICING_SCALE);

    on_event(ConsumeEvent {
        poll: 0,
        peer_pubkey: peer_pubkey.clone(),
        price: price.clone(),
        paid_scaled,
        remaining_scaled: paid_scaled as i64,
        report: None,
        cut_off: false,
        topped_up: false,
    });

    let mut poll = 0u32;
    let mut acked_received: u64 = 0;
    while max_polls.is_none_or(|max| poll < max) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        poll += 1;
        tokio::time::sleep(interval).await;
        if stop.load(Ordering::Relaxed) {
            break;
        }

        // Re-announce to collect queued frames + echo our receive count back.
        let mut body = Vec::new();
        encode_frame(&our_announce(identity), &mut body)
            .map_err(|e| anyhow!("framing our Announce: {e:?}"))?;
        if acked_received > 0 {
            let ack = MeteringReport::new(0, 0, acked_received).encode();
            encode_frame(&ack, &mut body).map_err(|e| anyhow!("framing our MeteringReport: {e:?}"))?;
        }
        let messages = exchange(base_url, body).await?;
        let cut_off = balance_exhausted(&messages);
        if cut_off {
            acked_received = 0;
        }
        let report = find_metering_report(&messages);
        if let Some(rx) = report.as_ref().map(|r| r.delivered) {
            acked_received = rx;
        }
        let cost = match report.as_ref() {
            Some(r) => price.cost_scaled(r.elapsed_ms, r.delivered),
            None => 0,
        };

        let mut topped_up = false;
        if cut_off || (paid_scaled as i64).saturating_sub(cost) < topup_scaled as i64 {
            let top = pay(base_url, identity, mint_url, topup_sat).await?;
            if top.accepted {
                paid_scaled = if cut_off {
                    topup_scaled
                } else {
                    paid_scaled.saturating_add(topup_scaled)
                };
                topped_up = true;
            }
        }

        on_event(ConsumeEvent {
            poll,
            peer_pubkey: peer_pubkey.clone(),
            price: price.clone(),
            paid_scaled,
            remaining_scaled: (paid_scaled as i64).saturating_sub(cost),
            report: report.map(|r| ReportSummary {
                elapsed_ms: r.elapsed_ms,
                delivered: r.delivered,
                received: r.received,
            }),
            cut_off,
            topped_up,
        });
    }
    Ok(())
}

/// Build a syntactically valid cashuA token of `amount_sat` on `mint_url`.
///
/// Faithful copy of `tollgate-net::client::build_test_token`: the proof's
/// keyset/secret/signature are filler (the fake/test mint accepts any proof as
/// UNSPENT); what matters is that the token parses and embeds `mint_url`, so the
/// provider posts its NUT-07 check there. Real-wallet integration is pending
/// upstream (see ARCHITECTURE.md).
fn build_bootstrap_token(mint_url: &str, amount_sat: u64) -> anyhow::Result<String> {
    use std::str::FromStr;

    use cashu::Amount;
    use cashu::mint_url::MintUrl;
    use cashu::nuts::nut00::TokenV3;
    use cashu::nuts::{CurrencyUnit, Id, Proof, PublicKey as CashuPublicKey};
    use cashu::secret::Secret;

    let mint = MintUrl::from_str(mint_url).map_err(|e| anyhow!("bad mint url: {e}"))?;
    let keyset_id =
        Id::from_str("009a1f293253e41e").map_err(|e| anyhow!("keyset id: {e}"))?;
    // Filler unblinded-signature point = generator G (pubkey of scalar 1).
    let secp = Secp256k1::new();
    let one = SecretKey::from_slice(&{
        let mut b = [0u8; 32];
        b[31] = 1;
        b
    })
    .expect("scalar 1 is a valid secret key");
    let g = one.public_key(&secp);
    let c = CashuPublicKey::from_slice(&g.serialize()).map_err(|e| anyhow!("filler C: {e}"))?;

    let proof = Proof {
        amount: Amount::from(amount_sat),
        keyset_id,
        secret: Secret::generate(),
        c,
        witness: None,
        dleq: None,
        p2pk_e: None,
    };
    let token = TokenV3::new(mint, vec![proof], None, Some(CurrencyUnit::Sat))
        .map_err(|e| anyhow!("building token: {e}"))?;
    Ok(token.to_string())
}

// ---------------------------------------------------------------------------
// UniFFI facade — the Kotlin-facing object
// ---------------------------------------------------------------------------

/// The native TollGate client. Owns a tokio runtime + persisted identity.
///
/// Kotlin usage:
/// ```kotlin
/// val node = TollgateMobileNode(context.filesDir.absolutePath)
/// val detected = node.detect("http://192.168.8.1:4747")
/// node.startConsume(base = "...", mint = "https://mint.example", amountSat = 21,
///                   topupSat = 5, intervalMs = 5000, maxPolls = null)
/// while (running) {
///     node.pollEvent(5000)?.let { ev -> /* update Compose state */ }
/// }
/// node.stopConsume()
/// ```
#[derive(uniffi::Object)]
pub struct TollgateMobileNode {
    runtime: Arc<Runtime>,
    identity: Identity,
    data_dir: PathBuf,
    // Active consume loop handle + the channel it feeds.
    consume: Mutex<Option<ConsumeHandle>>,
    stop_flag: Arc<AtomicBool>,
}

struct ConsumeHandle {
    // Stored (not awaited) so the task is detached: it runs to completion on the
    // runtime and signals via the event channel. Kept so a future `abort()` on
    // stop could hard-cancel a stuck loop.
    #[allow(dead_code)]
    join: JoinHandle<anyhow::Result<()>>,
    events: std::sync::mpsc::Receiver<ConsumeEvent>,
}

#[uniffi::export]
impl TollgateMobileNode {
    /// Load-or-generate the device identity under `data_dir` (Android
    /// `context.filesDir`). Spawns a multi-thread tokio runtime so the consume
    /// loop's background task keeps progressing between FFI polls.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, TollgateError> {
        let dir = PathBuf::from(&data_dir);
        let identity = Identity::load_or_generate(&dir)?;
        let runtime = Arc::new(
            Runtime::new().context("building tokio runtime")?,
        );
        Ok(Arc::new(Self {
            runtime,
            identity,
            data_dir: dir,
            consume: Mutex::new(None),
            stop_flag: Arc::new(AtomicBool::new(false)),
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

    /// Probe `base_url` (e.g. `http://192.168.8.1:4747`) — learn the gateway's
    /// pubkey, unit, protocol version, and price. Synchronous on the Kotlin
    /// side; runs the async exchange on the owned runtime.
    pub fn detect(&self, base_url: String) -> Result<Detected, TollgateError> {
        self.runtime
            .block_on(detect(&base_url, &self.identity))
            .map_err(TollgateError::from)
    }

    /// Detect + pay a `amount_sat` bootstrap token drawn on `mint_url`.
    pub fn pay(
        &self,
        base_url: String,
        mint_url: String,
        amount_sat: u64,
    ) -> Result<Paid, TollgateError> {
        let paid = self
            .runtime
            .block_on(pay(&base_url, &self.identity, &mint_url, amount_sat))
            .map_err(TollgateError::from)?;
        if !paid.accepted {
            return Err(TollgateError::Rejected {
                reason: paid.reason.clone().unwrap_or_else(|| "unknown".into()),
            });
        }
        Ok(paid)
    }

    /// Start the stay-online loop in the background. Events arrive via
    /// [`poll_event`]. `max_polls` = null runs until [`stop_consume`].
    /// Only one consume loop at a time; calling again while one is running is
    /// an error.
    pub fn start_consume(
        self: &Arc<Self>,
        base_url: String,
        mint_url: String,
        amount_sat: u64,
        topup_sat: u64,
        interval_ms: u64,
        max_polls: Option<u32>,
    ) -> Result<(), TollgateError> {
        let mut guard = self.consume.lock().expect("consume lock poisoned");
        if guard.is_some() {
            return Err(TollgateError::Other {
                message: "a consume loop is already running; call stop_consume first".into(),
            });
        }
        self.stop_flag.store(false, Ordering::Relaxed);

        let (tx, rx) = std::sync::mpsc::channel::<ConsumeEvent>();
        let identity_pubkey = self.identity.public_key;
        let _ = identity_pubkey; // identity is not Clone; we pass the loop a fresh view below
        let stop = self.stop_flag.clone();

        // The loop needs the identity's keypair to build Announce each poll.
        // Identity isn't Clone (secrets), so reconstruct a lightweight handle:
        // we re-derive the public key from the stored secret each call inside
        // run_consume via &self.identity — but the spawned task must be 'static.
        // Solution: move a secret-key copy into the task (secp256k1::SecretKey
        // is Clone) and rebuild Identity there.
        let secret_bytes = self.identity.secret_key.secret_bytes();
        let data_dir = self.data_dir.clone();
        let base = base_url.clone();
        let runtime = self.runtime.clone();

        let join = runtime.spawn(async move {
            let secp = Secp256k1::new();
            let secret_key =
                SecretKey::from_slice(&secret_bytes).expect("secret key round-trips from storage");
            let public_key = secret_key.public_key(&secp);
            let identity = Identity {
                public_key,
                secret_key,
            };
            let _ = &data_dir; // kept for future wallet-state persistence
            run_consume(
                &base,
                &identity,
                &mint_url,
                amount_sat,
                topup_sat,
                Duration::from_millis(interval_ms.max(100)),
                max_polls,
                stop,
                |ev| {
                    let _ = tx.send(ev); // channel send; ignore error if UI dropped
                },
            )
            .await
        });

        *guard = Some(ConsumeHandle { join, events: rx });
        Ok(())
    }

    /// Fetch the next consume event, waiting up to `timeout_ms`. Null when the
    /// loop has finished (max_polls reached or stopped) and the queue is drained.
    pub fn poll_event(&self, timeout_ms: u64) -> Result<Option<ConsumeEvent>, TollgateError> {
        let guard = self.consume.lock().expect("consume lock poisoned");
        let Some(handle) = guard.as_ref() else {
            return Ok(None);
        };
        // Drop the lock before a blocking recv so start/stop aren't blocked.
        let rx = &handle.events;
        // Re-acquire-free read: clone isn't possible on Receiver; recv under lock
        // is fine because the sender never takes this lock.
        match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
            Ok(ev) => Ok(Some(ev)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    /// Signal the running consume loop to stop at its next poll boundary.
    pub fn stop_consume(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        // Detach: the task exits on its own. We don't await it here (would block
        // the Kotlin thread); poll_event returning None signals completion.
    }

    // -- Cashu wallet operations (real minting via the `cashu` crate) ---------

    /// Request a Lightning mint quote from a Cashu mint. Returns the quote ID
    /// and the bolt11 invoice the user must pay. After payment, call
    /// [`check_mint_quote`] until it returns `"PAID"`, then [`mint_tokens`].
    pub fn request_mint_quote(
        &self,
        mint_url: String,
        amount_sat: u64,
    ) -> Result<MintQuote, TollgateError> {
        let (quote_id, invoice) = self
            .runtime
            .block_on(wallet::request_quote(&mint_url, amount_sat))
            .map_err(TollgateError::from)?;
        Ok(MintQuote {
            quote_id,
            invoice,
        })
    }

    /// Check the payment state of a mint quote. Returns `"UNPAID"`, `"PAID"`,
    /// or `"ISSUED"`.
    pub fn check_mint_quote(
        &self,
        mint_url: String,
        quote_id: String,
    ) -> Result<String, TollgateError> {
        self.runtime
            .block_on(wallet::check_quote(&mint_url, &quote_id))
            .map_err(TollgateError::from)
    }

    /// Mint ecash tokens for a paid quote. Performs the full NUT-04 blind
    /// signature exchange and returns a `cashuA…` token string.
    pub fn mint_tokens(
        &self,
        mint_url: String,
        quote_id: String,
        amount_sat: u64,
    ) -> Result<String, TollgateError> {
        self.runtime
            .block_on(wallet::mint_tokens(&mint_url, &quote_id, amount_sat))
            .map_err(TollgateError::from)
    }

    /// Auto-mint ecash from a test mint that auto-settles invoices.
    ///
    /// Requests a quote, polls until PAID (test mints like testnut auto-pay
    /// within seconds), then mints tokens. Returns the `cashuA…` token string.
    ///
    /// Use this on app startup to pre-load the wallet before connecting to
    /// a tollgate.
    pub fn auto_mint(
        &self,
        mint_url: String,
        amount_sat: u64,
        max_wait_secs: u64,
    ) -> Result<String, TollgateError> {
        let (_quote_id, token) = self
            .runtime
            .block_on(wallet::auto_mint(&mint_url, amount_sat, max_wait_secs))
            .map_err(TollgateError::from)?;
        Ok(token)
    }

    /// Swap existing ecash tokens for new ones (NUT-03).
    ///
    /// Takes a cashuA token string and mint URL, sends proofs to mint for
    /// swap, returns a new cashuA token string with fresh proofs.
    pub fn swap_tokens(
        &self,
        mint_url: String,
        token_str: String,
    ) -> Result<String, TollgateError> {
        self.runtime
            .block_on(wallet::swap_tokens(&mint_url, &token_str))
            .map_err(TollgateError::from)
    }

    /// Receive a cashuA token from someone else.
    /// Returns the amount received in sats.
    pub fn receive_token(
        &self,
        mint_url: String,
        token_str: String,
    ) -> Result<u64, TollgateError> {
        self.runtime
            .block_on(wallet::receive_token(&mint_url, &token_str))
            .map_err(TollgateError::from)
    }

    /// Mint tokens and send a portion as a cashuA token.
    /// Returns (full_mint_token, send_token).
    pub fn mint_and_send(
        &self,
        mint_url: String,
        mint_amount: u64,
        send_amount: u64,
        max_wait_secs: u64,
    ) -> Result<(String, String), TollgateError> {
        self.runtime
            .block_on(wallet::mint_and_send(&mint_url, mint_amount, send_amount, max_wait_secs))
            .map_err(TollgateError::from)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "tollgate-lib-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // -- PriceView::cost_scaled -------------------------------------------------

    #[test]
    fn test_cost_scaled_per_unit_only() {
        let pv = PriceView {
            per_second: 0,
            per_unit: 3,
        };
        assert_eq!(pv.cost_scaled(0, 10), 30);
    }

    #[test]
    fn test_cost_scaled_per_second_only() {
        let pv = PriceView {
            per_second: 2,
            per_unit: 0,
        };
        assert_eq!(pv.cost_scaled(3000, 0), 6);
    }

    #[test]
    fn test_cost_scaled_both() {
        let pv = PriceView {
            per_second: 1,
            per_unit: 5,
        };
        assert_eq!(pv.cost_scaled(2000, 10), 2 + 50);
    }

    #[test]
    fn test_cost_scaled_zero() {
        let pv = PriceView::default();
        assert_eq!(pv.cost_scaled(9999, 9999), 0);
    }

    #[test]
    fn test_cost_scaled_saturation() {
        let pv = PriceView {
            per_second: i64::MAX / 2,
            per_unit: i64::MAX / 2,
        };
        // Should not panic — saturating math clamps to i64::MAX.
        let _ = pv.cost_scaled(u64::MAX, u64::MAX);
    }

    // -- TollgateError classification -------------------------------------------

    #[test]
    fn test_error_network() {
        let e = TollgateError::from(anyhow!("connection refused"));
        assert!(matches!(e, TollgateError::Network { .. }));
    }

    #[test]
    fn test_error_timeout() {
        let e = TollgateError::from(anyhow!("operation timeout exceeded"));
        assert!(matches!(e, TollgateError::Network { .. }));
    }

    #[test]
    fn test_error_other() {
        let e = TollgateError::from(anyhow!("bad mint url"));
        assert!(matches!(e, TollgateError::Other { .. }));
    }

    // -- Identity persistence ---------------------------------------------------

    #[test]
    fn test_identity_generates_and_persists() {
        let dir = tempdir();
        let id = Identity::load_or_generate(&dir).expect("load_or_generate should succeed");
        let pk = id.pubkey_hex();
        assert_eq!(pk.len(), 66, "compressed pubkey is 33 bytes = 66 hex chars");
        assert!(
            pk.chars().all(|c| c.is_ascii_hexdigit()),
            "pubkey must be hex"
        );
        let id_file = dir.join("identity.hex");
        assert!(id_file.exists(), "identity.hex must be persisted");
    }

    #[test]
    fn test_identity_loads_existing() {
        let dir = tempdir();
        let pk1 = Identity::load_or_generate(&dir)
            .expect("first load_or_generate")
            .pubkey_hex();
        let pk2 = Identity::load_or_generate(&dir)
            .expect("second load_or_generate")
            .pubkey_hex();
        assert_eq!(pk1, pk2, "identity must be stable across loads");
    }

    // -- PriceView::cost_scaled (task-specified tests) --------------------------

    #[test]
    fn test_price_view_cost_scaled_zero_price() {
        let pv = PriceView { per_second: 0, per_unit: 0 };
        assert_eq!(pv.cost_scaled(0, 0), 0);
        assert_eq!(pv.cost_scaled(5000, 100), 0);
        assert_eq!(pv.cost_scaled(u64::MAX, u64::MAX), 0);
    }

    #[test]
    fn test_price_view_cost_scaled_per_second_only() {
        // per_second=1000, per_unit=0, elapsed_ms=2000 → 2000*1000/1000 = 2000
        let pv = PriceView { per_second: 1000, per_unit: 0 };
        assert_eq!(pv.cost_scaled(2000, 0), 2000);
        assert_eq!(pv.cost_scaled(2000, 9999), 2000);
    }

    #[test]
    fn test_price_view_cost_scaled_per_unit_only() {
        // per_second=0, per_unit=3, units=1024 → 1024*3 = 3072
        let pv = PriceView { per_second: 0, per_unit: 3 };
        assert_eq!(pv.cost_scaled(0, 1024), 3072);
        assert_eq!(pv.cost_scaled(9999, 1024), 3072);
    }

    #[test]
    fn test_price_view_cost_scaled_both() {
        // per_second=2, per_unit=3, elapsed_ms=1000, units=100
        // → 100*3 + 1000*2/1000 = 300 + 2 = 302
        let pv = PriceView { per_second: 2, per_unit: 3 };
        assert_eq!(pv.cost_scaled(1000, 100), 302);
    }

    #[test]
    fn test_price_view_cost_scaled_overflow() {
        // Very large values should saturate, not panic.
        // per_second = i64::MAX, per_unit = i64::MAX, elapsed_ms = 2000, units = 1
        // by_time = (2000 as i64).saturating_mul(i64::MAX) → saturates to i64::MAX,
        //   then .saturating_div(1000) → i64::MAX/1000 ≈ 9.2e15
        // by_units = (1 as i64).saturating_mul(i64::MAX) → i64::MAX
        // saturating_add(9.2e15, i64::MAX) → i64::MAX
        let pv = PriceView { per_second: i64::MAX, per_unit: i64::MAX };
        let cost = pv.cost_scaled(2000, 1);
        assert_eq!(cost, i64::MAX, "saturating add should clamp to i64::MAX");
    }

    // -- build_bootstrap_token tests -------------------------------------------

    #[test]
    fn test_build_bootstrap_token_starts_with_cashuA() {
        let token = build_bootstrap_token("https://mint.example", 21)
            .expect("build_bootstrap_token should succeed");
        assert!(
            token.starts_with("cashuA"),
            "token should start with 'cashuA', got: {token}"
        );
    }

    #[test]
    fn test_build_bootstrap_token_decodes_to_correct_amount() {
        use base64::Engine;
        let token = build_bootstrap_token("https://mint.example", 21)
            .expect("build_bootstrap_token should succeed");
        let b64 = &token["cashuA".len()..];
        // Cashu's TokenV3 may use standard or URL-safe base64 with or without padding.
        // Try URL_SAFE_NO_PAD first, fall back to URL_SAFE (with padding).
        let json_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(b64))
            .expect("base64 decode");
        let json: serde_json::Value =
            serde_json::from_slice(&json_bytes).expect("json parse");
        let token_arr = json["token"].as_array().expect("token array");
        let proofs = token_arr[0]["proofs"].as_array().expect("proofs array");
        let amount = proofs[0]["amount"].as_u64().expect("amount field");
        assert_eq!(amount, 21, "decoded amount should be 21");
    }

    #[test]
    fn test_build_bootstrap_token_various_amounts() {
        use base64::Engine;
        for &amount in &[1u64, 21, 1000] {
            let token = build_bootstrap_token("https://mint.example", amount)
                .expect("build_bootstrap_token should succeed");
            let b64 = &token["cashuA".len()..];
            let json_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(b64)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(b64))
                .expect("base64 decode");
            let json: serde_json::Value =
                serde_json::from_slice(&json_bytes).expect("json parse");
            let token_arr = json["token"].as_array().expect("token array");
            let proofs = token_arr[0]["proofs"].as_array().expect("proofs array");
            let decoded = proofs[0]["amount"].as_u64().expect("amount field");
            assert_eq!(
                decoded, amount,
                "decoded amount should be {amount}, got {decoded}"
            );
        }
    }

    // -- TollgateError classification (task-specified tests) --------------------

    #[test]
    fn test_tollgate_error_network_classification() {
        let e = TollgateError::from(anyhow!("connection refused"));
        assert!(matches!(e, TollgateError::Network { .. }));
    }

    #[test]
    fn test_tollgate_error_other_classification() {
        let e = TollgateError::from(anyhow!("something went wrong"));
        assert!(matches!(e, TollgateError::Other { .. }));
    }
}

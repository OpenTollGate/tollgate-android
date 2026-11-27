//! One endpoint, two protocols, and a hard rule between them.
//!
//! Two things are called "v1" in this ecosystem and only one of them is on the
//! routers. `tollgate-module-basic-go` serves a JSON API on :2121 ([`crate::v1`])
//! and has never served `POST /tollgate/v1/exchange`; the CBOR exchange that the
//! app was first ported away from lived only in `tollgate-rs` and was deleted on
//! 2026-07-30. So:
//!
//! - **[`Protocol::V1`] is the native path.** It is what a deployed gateway
//!   speaks, it quotes a price, and a customer pays it with an ordinary
//!   Lightning wallet. No Cashu wallet is needed on the phone.
//! - **[`Protocol::V2`] is an adapter, and only that.** It exists so a device
//!   that can only speak v2 can still be shown *something* true about a peer.
//!   It cannot quote a price, cannot take a payment here, and does not pretend
//!   to: v2's `Offer` deliberately carries no money, and delivery is one voucher
//!   per unit issued by the mint.
//!
//! The rule this module enforces: **a v2 peer may never present as purchasable.**
//! Every operation that would move money returns [`Unsupported`] on v2, naming
//! the reason, rather than falling back to a v1 shape with an invented price.

use std::time::Duration;

use crate::v1;
use crate::v2;

/// Which protocol answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// The JSON API on :2121 — the native path.
    V1,
    /// The CBOR session on :4747 — adapter only.
    V2,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }

    /// True when this protocol can actually sell something in this build.
    ///
    /// v2 can only sell once a Spilman channel is funded from a Cashu wallet,
    /// and that wallet is not in this app. Reporting `false` here is what keeps
    /// a pay button from appearing over a peer that cannot be paid.
    pub fn can_take_payment(self) -> bool {
        matches!(self, Self::V1)
    }
}

/// One price, in the terms the customer pays it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceRow {
    /// What one `price_per_step` buys.
    pub step_size: u64,
    /// The metered unit — `"bytes"` / `"milliseconds"`.
    pub metric: String,
    pub price_per_step: u64,
    /// The money unit, `"sat"`.
    pub unit: String,
    /// The mint whose vouchers settle this price.
    pub mint_url: String,
    pub min_steps: u64,
}

/// A peer as the app needs to show it, whichever protocol answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerView {
    pub protocol: Protocol,
    /// The endpoint that was probed, as given.
    pub endpoint: String,
    /// The metered unit. `"bytes"` or `"milliseconds"`.
    pub metric: String,
    /// Prices, when the answering protocol has any. **Empty for v2, always** —
    /// and empty is the honest answer, not a missing one.
    pub prices: Vec<PriceRow>,
    /// The MAC the gateway resolved for this device (v1 only). v1's identity is
    /// the network's opinion of who you are, so this is also who a purchase
    /// would be applied to.
    pub device_mac: Option<String>,
    /// The peer's advertised signing key, when it published one.
    pub pubkey: Option<String>,
    /// Why there is nothing to buy, when there isn't. Never empty for v2.
    pub unavailable: Option<String>,
}

impl PeerView {
    /// Whether a customer can buy from this peer in this build.
    ///
    /// v1 additionally needs a resolved identity: its money routes refuse a
    /// client the gateway cannot place on the network, so a peer that advertises
    /// a price but cannot identify this device is **not** purchasable. Saying
    /// otherwise puts a pay button in front of a purchase that will be refused
    /// at the last step.
    pub fn purchasable(&self) -> bool {
        self.protocol.can_take_payment()
            && !self.prices.is_empty()
            && self.device_mac.is_some()
    }
}

/// An operation that the answering protocol cannot perform, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    pub protocol: Protocol,
    pub operation: &'static str,
    pub reason: String,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is not available over {}: {}",
            self.operation,
            self.protocol.as_str(),
            self.reason
        )
    }
}

impl std::error::Error for Unsupported {}

/// Something went wrong reaching the peer at all.
#[derive(Debug, thiserror::Error)]
pub enum DiscoverError {
    #[error("v1: {0}")]
    V1(String),
    /// A v2 attempt failed. `io` distinguishes a dead or silent peer (nothing
    /// arrived, the socket failed) from a peer that answered with something
    /// undecodable — the two are different problems and the caller reports
    /// them as different errors.
    #[error("v2: {message}")]
    V2 { message: String, io: bool },
}

/// An endpoint that answered, and the client bound to it.
#[derive(Debug, Clone)]
pub enum Gateway {
    V1(v1::Gateway),
    /// A v2 peer. No client is bound: v2's session needs a funded channel, and
    /// `detect` is a one-shot probe rather than a thing to keep.
    V2 {
        endpoint: String,
        view: PeerView,
    },
}

/// Probe `endpoint` and report what answered.
///
/// v1 is tried first, because it is the protocol a deployed gateway actually
/// serves and the one that can take a payment. Only if v1 does not answer as a
/// v1 gateway is v2 tried — a v2 gateway speaks raw TCP and will not answer
/// HTTP at all, so the two do not collide in practice, but the order still
/// matters for which error a caller sees.
pub async fn probe(
    endpoint: &str,
    timeout: Duration,
) -> Result<Gateway, DiscoverError> {
    if let Ok(gw) = v1::Gateway::new(endpoint, timeout) {
        match gw.detect().await {
            Ok(_) => return Ok(Gateway::V1(gw)),
            Err(e) => {
                // Named, not swallowed. v1 is the only lane that can take a payment,
                // so when it fails its reason is what a caller needs; the v2 error
                // describes the second attempt, not why the first one failed.
                eprintln!("probe {endpoint}: v1 said {e}");
            }
        }
        // Not a v1 gateway. Fall through to v2 rather than failing here: a v2
        // peer is a legitimate reason for v1's probe to fail, and which error a
        // caller sees should describe the endpoint, not the first attempt.
    }

    // v2 speaks raw TCP; `detect` needs our key to announce with.
    let secret = probe_secret();
    match v2::detect(endpoint, &secret, "bytes", timeout).await {
        Ok(detected) => {
            let units = detected.unit.clone();
            let view = PeerView {
                protocol: Protocol::V2,
                endpoint: endpoint.to_string(),
                metric: units,
                // Deliberately empty. v2's Offer carries no price and inventing
                // one would state the mint's business as the gateway's.
                prices: Vec::new(),
                device_mac: None,
                pubkey: Some(detected.pubkey_hex.clone()),
                unavailable: Some(
                    "v2 peers carry no price: delivery is one voucher per unit, \
                     issued by the peer's mint, and this build has no wallet to \
                     fund the channel a purchase needs."
                        .to_string(),
                ),
            };
            Ok(Gateway::V2 {
                endpoint: endpoint.to_string(),
                view,
            })
        }
        // `io` carries the distinction the caller needs: a dead or silent peer
        // is a network failure, a peer that answered nonsense is not.
        Err(e) => Err(DiscoverError::V2 {
            message: e.to_string(),
            io: matches!(e, v2::WireError::Io(_)),
        }),
    }
}

/// The key to announce with during a v2 probe.
///
/// A probe is not a purchase, so a throwaway key is correct here: nothing is
/// signed that anyone would honour, and no identity should be published by a
/// mere reachability check.
fn probe_secret() -> secp256k1::SecretKey {
    secp256k1::SecretKey::from_slice(&[0x42u8; 32]).expect("a valid scalar")
}

impl Gateway {
    /// The unified view of this peer.
    pub async fn describe(&self) -> Result<PeerView, DiscoverError> {
        match self {
            Self::V1(gw) => {
                let ad = gw
                    .detect()
                    .await
                    .map_err(|e| DiscoverError::V1(e.to_string()))?;
                let device_mac = gw.whoami().await.unwrap_or_default();
                let prices = ad
                    .prices
                    .iter()
                    .map(|p| PriceRow {
                        step_size: ad.step_size,
                        metric: ad.metric.clone(),
                        price_per_step: p.price_per_step,
                        unit: p.unit.clone(),
                        mint_url: p.mint_url.clone(),
                        min_steps: p.min_steps,
                    })
                    .collect();
                Ok(PeerView {
                    protocol: Protocol::V1,
                    endpoint: gw.base().to_string(),
                    metric: ad.metric,
                    prices,
                    device_mac: (!device_mac.is_empty()).then_some(device_mac),
                    pubkey: (!ad.pubkey.is_empty()).then_some(ad.pubkey),
                    unavailable: None,
                })
            }
            Self::V2 { view, .. } => Ok(view.clone()),
        }
    }

    pub fn protocol(&self) -> Protocol {
        match self {
            Self::V1(_) => Protocol::V1,
            Self::V2 { .. } => Protocol::V2,
        }
    }

    /// Ask for a quote and its invoice. v1 only.
    pub async fn request_invoice(
        &self,
        amount_sat: u64,
        mint_url: &str,
    ) -> Result<v1::Invoice, Unsupported> {
        match self {
            Self::V1(gw) => gw.request_invoice(amount_sat, mint_url).await.map_err(|e| {
                Unsupported {
                    protocol: Protocol::V1,
                    operation: "requesting an invoice",
                    reason: e.to_string(),
                }
            }),
            Self::V2 { .. } => Err(self.v2_no_payment("requesting an invoice")),
        }
    }

    /// Poll a quote. v1 only.
    pub async fn invoice_status(&self, quote: &str) -> Result<v1::InvoiceStatus, Unsupported> {
        match self {
            Self::V1(gw) => gw.invoice_status(quote).await.map_err(|e| Unsupported {
                protocol: Protocol::V1,
                operation: "polling a quote",
                reason: e.to_string(),
            }),
            Self::V2 { .. } => Err(self.v2_no_payment("polling a quote")),
        }
    }

    /// What this device has left. v1 only — v2 keeps a budget per key, and
    /// reading it needs an established session, not a probe.
    pub async fn balance(&self) -> Result<v1::Balance, Unsupported> {
        match self {
            Self::V1(gw) => gw.balance().await.map_err(|e| Unsupported {
                protocol: Protocol::V1,
                operation: "reading a balance",
                reason: e.to_string(),
            }),
            Self::V2 { .. } => Err(self.v2_no_payment("reading a balance")),
        }
    }

    fn v2_no_payment(&self, operation: &'static str) -> Unsupported {
        Unsupported {
            protocol: Protocol::V2,
            operation,
            reason: "a v2 purchase funds a Spilman channel from a Cashu wallet, \
                     and this build has no wallet on the device. The peer carries \
                     terms but no price."
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_peer() -> Gateway {
        Gateway::V2 {
            endpoint: "127.0.0.1:4747".into(),
            view: PeerView {
                protocol: Protocol::V2,
                endpoint: "127.0.0.1:4747".into(),
                metric: "bytes".into(),
                prices: Vec::new(),
                device_mac: None,
                pubkey: Some("02aa".into()),
                unavailable: Some("no price on the wire".into()),
            },
        }
    }

    fn v1_view(prices: Vec<PriceRow>, mac: Option<String>) -> PeerView {
        PeerView {
            protocol: Protocol::V1,
            endpoint: "http://gw:2121".into(),
            metric: "bytes".into(),
            prices,
            device_mac: mac,
            pubkey: Some("aa".into()),
            unavailable: None,
        }
    }

    fn one_price() -> Vec<PriceRow> {
        vec![PriceRow {
            step_size: 1_048_576,
            metric: "bytes".into(),
            price_per_step: 2,
            unit: "sat".into(),
            mint_url: "https://mint.example/".into(),
            min_steps: 1,
        }]
    }

    #[test]
    fn a_v1_peer_with_a_price_and_an_identity_is_purchasable() {
        let view = v1_view(one_price(), Some("02:00:00:00:00:20".into()));
        assert!(view.purchasable());
    }

    #[test]
    fn a_v1_peer_that_cannot_identify_us_is_not_purchasable() {
        // The gateway's money routes refuse a client it cannot resolve, so a
        // price alone is not enough: a pay button here would fail at the last
        // step, after the customer had committed to paying.
        let view = v1_view(one_price(), None);
        assert!(!view.purchasable(), "a price without an identity cannot be bought");
    }

    #[test]
    fn a_v1_peer_with_no_price_is_not_purchasable() {
        let view = v1_view(Vec::new(), Some("02:00:00:00:00:20".into()));
        assert!(!view.purchasable());
    }

    #[tokio::test]
    async fn a_v2_peer_is_never_purchasable() {
        // The whole point of the adapter: a v2 peer can be shown, and cannot be
        // bought. If this ever returns true the UI grows a pay button over a
        // peer that cannot be paid.
        let gw = v2_peer();
        let view = gw.describe().await.expect("a v2 view");
        assert_eq!(view.protocol, Protocol::V2);
        assert!(view.prices.is_empty(), "v2 carries no price");
        assert!(!view.purchasable());
        assert!(view.unavailable.is_some(), "and it says why");
    }

    #[tokio::test]
    async fn every_money_operation_on_v2_names_the_reason() {
        let gw = v2_peer();
        let err = gw.request_invoice(21, "https://mint").await.unwrap_err();
        assert_eq!(err.protocol, Protocol::V2);
        assert_eq!(err.operation, "requesting an invoice");
        assert!(err.reason.contains("no wallet"), "got: {}", err.reason);
        assert!(err.to_string().contains("not available over v2"));

        assert!(gw.invoice_status("quote").await.is_err());
        assert!(gw.balance().await.is_err());
    }

    #[test]
    fn only_v1_can_take_a_payment_in_this_build() {
        assert!(Protocol::V1.can_take_payment());
        assert!(!Protocol::V2.can_take_payment());
    }

    #[test]
    fn a_probe_does_not_publish_a_real_identity() {
        // A reachability check must not mint a lasting key or leak one; the
        // scalar is fixed and throwaway precisely so nothing is enrolled.
        let a = probe_secret();
        let b = probe_secret();
        assert_eq!(a.secret_bytes(), b.secret_bytes());
    }
}

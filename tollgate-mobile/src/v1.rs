//! Protocol v1: the HTTP API a `tollgate-module-basic-go` gateway serves on
//! :2121.
//!
//! This is a different protocol from [`crate::v2`], not an older spelling of it.
//! v1 is request/response JSON for a captive portal; v2 is a stateful CBOR
//! session between two peers. The shapes below were read off the Go module's
//! handlers, not guessed:
//!
//! | Route | What it answers |
//! |---|---|
//! | `GET /` | a JSON kind-10021 Nostr event: `metric`, `step_size`, one `price_per_step` tag per mint |
//! | `GET /whoami` | `mac=aa:bb:…`, plain text |
//! | `POST /ln-invoice` | `{amount, mint_url}` → quote + BOLT11 invoice |
//! | `GET /ln-invoice?quote=` | that quote's state, and whether access was granted |
//! | `POST /` | the Cashu token itself as the body (`text/plain`) |
//! | `GET /balance` | `{session_active, usage, allotment, remaining}` |
//! | `GET /usage` | `used/total` as plain text |
//!
//! **Identity is the socket.** Every route that moves money resolves the
//! caller's MAC from the connection (`clientIdentity`) and *ignores* any `mac`
//! the caller sends. That is why this client must reach the gateway from the
//! device that wants the access, and why there is no "pay for someone else"
//! here: there is nothing to name.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Errors from the v1 HTTP surface.
#[derive(Debug, thiserror::Error)]
pub enum V1Error {
    #[error("asking the gateway: {0}")]
    Transport(String),
    /// A well-formed HTTP answer whose body was not what the route promises.
    #[error("the gateway's answer did not parse: {0}")]
    Shape(String),
    /// The gateway answered with a refusal it wants the customer to read.
    /// `code` is its machine-readable reason when it sent one.
    #[error("{message}")]
    Refused {
        code: Option<String>,
        message: String,
        /// Seconds to wait before retrying, when the gateway said.
        retry_after: Option<u64>,
    },
}

/// One way to pay this gateway: a price per step, in a mint's vouchers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceOption {
    /// `"cashu"` in every advertisement seen so far.
    pub asset: String,
    /// Price per `step_size` units, in `unit`.
    pub price_per_step: u64,
    /// `"sat"` — the money unit, not the metered unit.
    pub unit: String,
    pub mint_url: String,
    /// Least a purchase may be, in steps.
    pub min_steps: u64,
}

/// What a gateway advertises: what it meters, how finely, and at what price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advertisement {
    /// `"bytes"` or `"milliseconds"`.
    pub metric: String,
    /// The metered unit each `price_per_step` buys.
    pub step_size: u64,
    pub tips: Vec<String>,
    pub prices: Vec<PriceOption>,
    /// The gateway's Nostr pubkey — the identity that signed the advertisement.
    pub pubkey: String,
}

/// A quote and the invoice that settles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invoice {
    /// Quote id; the status poll is keyed by it.
    pub quote: String,
    /// BOLT11. Empty when the gateway granted access without one.
    pub invoice: String,
    pub mint_url: String,
    pub amount_sat: u64,
    /// Unix seconds, when the mint says the invoice expires.
    pub expiry: u64,
    pub state: String,
}

/// One poll of a quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceStatus {
    pub quote: String,
    /// `"UNPAID"`, `"PAID"`, `"ISSUED"`, `"EXPIRED"`…
    pub state: String,
    /// True once the gateway has actually opened the gate for this device.
    /// A `PAID` quote with this false is the case the Go module logs at ERROR:
    /// the money arrived and the access did not. Never read `state` alone.
    pub access_granted: bool,
    pub allotment: u64,
    /// `"bytes"` / `"milliseconds"`, echoing the advertisement.
    pub metric: String,
}

/// What the gateway thinks this device has left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Balance {
    pub session_active: bool,
    pub usage: u64,
    pub allotment: u64,
    pub remaining: u64,
    pub metric: String,
}

/// The gateway's answer to a Cashu token posted to `POST /`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub access_granted: bool,
    pub allotment: u64,
    pub metric: String,
    pub state: String,
}

// ---------------------------------------------------------------------------
// Wire shapes — private, so the field names of someone else's JSON stay in one
// place and a rename in the Go module breaks one struct instead of the client.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default)]
    pubkey: String,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

#[derive(Serialize)]
struct RawInvoiceRequest<'a> {
    amount: u64,
    mint_url: &'a str,
}

#[derive(Deserialize)]
struct RawInvoice {
    #[serde(default)]
    status: i64,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    invoice: String,
    #[serde(default)]
    mint_url: String,
    #[serde(default)]
    amount: u64,
    #[serde(default)]
    expiry: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    retry_after: u64,
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(default)]
    status: i64,
    #[serde(default)]
    quote: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    access_granted: bool,
    #[serde(default)]
    allotment: u64,
    #[serde(default)]
    metric: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
struct RawBalance {
    #[serde(default)]
    session_active: bool,
    #[serde(default)]
    usage: u64,
    #[serde(default)]
    allotment: u64,
    #[serde(default)]
    remaining: u64,
    #[serde(default)]
    metric: String,
}

// ---------------------------------------------------------------------------
// Parsing, kept separate from I/O so it can be tested without a socket.
// ---------------------------------------------------------------------------

/// Parse the advertisement a gateway serves at `GET /`.
///
/// The tags are the protocol: `["metric", "bytes"]`, `["step_size", "1048576"]`,
/// and one `["price_per_step", "cashu", "<price>", "<unit>", "<mint>", "<min>"]`
/// per accepted mint. A tag that does not have the arity or the numbers it
/// claims is dropped rather than defaulted — a price invented from a malformed
/// tag is worse than no price shown.
pub fn parse_advertisement(body: &str) -> Result<Advertisement, V1Error> {
    let event: RawEvent = serde_json::from_str(body)
        .map_err(|e| V1Error::Shape(format!("advertisement is not a nostr event: {e}")))?;

    let mut metric = String::new();
    let mut step_size = 0u64;
    let mut tips: Vec<String> = Vec::new();
    let mut prices: Vec<PriceOption> = Vec::new();

    for tag in &event.tags {
        match tag.first().map(String::as_str) {
            Some("metric") if tag.len() >= 2 => metric = tag[1].clone(),
            Some("step_size") if tag.len() >= 2 => {
                step_size = tag[1].parse().unwrap_or(0);
            }
            Some("tips") => tips.extend(tag.iter().skip(1).cloned()),
            Some("price_per_step") if tag.len() >= 5 => {
                let Some(price_per_step) = tag[2].parse::<u64>().ok() else {
                    continue;
                };
                prices.push(PriceOption {
                    asset: tag[1].clone(),
                    price_per_step,
                    unit: tag[3].clone(),
                    mint_url: tag[4].clone(),
                    min_steps: tag.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
            _ => {}
        }
    }

    if metric.is_empty() {
        return Err(V1Error::Shape(
            "advertisement carries no metric tag; not a TollGate gateway".into(),
        ));
    }

    Ok(Advertisement {
        metric,
        step_size,
        tips,
        prices,
        pubkey: event.pubkey,
    })
}

/// Parse the body of `GET /whoami`, which is `mac=aa:bb:…` in plain text.
///
/// An empty value is not an error here: the gateway answers `mac=` for a client
/// it cannot resolve, and that is worth surfacing as "no identity" rather than
/// as a broken response.
pub fn parse_whoami(body: &str) -> String {
    body.trim()
        .strip_prefix("mac=")
        .unwrap_or(body.trim())
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

/// A v1 gateway, addressed as `http://host:2121`.
#[derive(Debug, Clone)]
pub struct Gateway {
    base: String,
    http: reqwest::Client,
}

impl Gateway {
    /// `base` may carry a scheme and a trailing slash or neither; both are
    /// normalised, because a stored setting outlives any one build's spelling.
    pub fn new(base: &str, timeout: Duration) -> Result<Self, V1Error> {
        let trimmed = base.trim().trim_end_matches('/');
        let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{trimmed}")
        };
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| V1Error::Transport(format!("building an http client: {e}")))?;
        Ok(Self {
            base: with_scheme,
            http,
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// `GET /` — who this gateway is and what it charges.
    pub async fn detect(&self) -> Result<Advertisement, V1Error> {
        let resp = self
            .http
            .get(format!("{}/", self.base))
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("GET {}: {e}", self.base)))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading the advertisement: {e}")))?;
        if !status.is_success() {
            return Err(V1Error::Refused {
                code: None,
                message: format!("the gateway answered {status} to GET /"),
                retry_after: None,
            });
        }
        parse_advertisement(&body)
    }

    /// `GET /whoami` — the MAC the gateway resolved for this connection.
    pub async fn whoami(&self) -> Result<String, V1Error> {
        let body = self
            .http
            .get(format!("{}/whoami", self.base))
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("GET /whoami: {e}")))?
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading /whoami: {e}")))?;
        Ok(parse_whoami(&body))
    }

    /// `POST /ln-invoice` — ask the gateway to quote `amount_sat` and have its
    /// mint issue an invoice for it.
    ///
    /// The invoice is not the gateway's: the merchant opens a NUT-04 quote at
    /// `mint_url`, so paying it mints the vouchers the gateway will then hold.
    /// A client that pays it needs no wallet of its own, which is why this is
    /// the path the browser portal uses.
    pub async fn request_invoice(
        &self,
        amount_sat: u64,
        mint_url: &str,
    ) -> Result<Invoice, V1Error> {
        let resp = self
            .http
            .post(format!("{}/ln-invoice", self.base))
            .json(&RawInvoiceRequest {
                amount: amount_sat,
                mint_url,
            })
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("POST /ln-invoice: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading the quote: {e}")))?;
        let raw: RawInvoice = serde_json::from_str(&body).map_err(|e| {
            V1Error::Shape(format!("invoice answer (HTTP {status}) is not JSON: {e}: {body}"))
        })?;
        if raw.status == 0 || !status.is_success() {
            return Err(V1Error::Refused {
                code: (!raw.code.is_empty()).then_some(raw.code),
                message: if raw.error.is_empty() {
                    format!("the gateway refused the quote (HTTP {status})")
                } else {
                    raw.error
                },
                retry_after: (raw.retry_after > 0).then_some(raw.retry_after),
            });
        }
        Ok(Invoice {
            quote: raw.quote,
            invoice: raw.invoice,
            mint_url: raw.mint_url,
            amount_sat: raw.amount,
            expiry: raw.expiry,
            state: raw.state,
        })
    }

    /// `GET /ln-invoice?quote=` — has it been paid, and has the gate opened.
    pub async fn invoice_status(&self, quote: &str) -> Result<InvoiceStatus, V1Error> {
        let resp = self
            .http
            .get(format!("{}/ln-invoice", self.base))
            .query(&[("quote", quote)])
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("GET /ln-invoice: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading the quote status: {e}")))?;
        let raw: RawStatus = serde_json::from_str(&body).map_err(|e| {
            V1Error::Shape(format!("quote status (HTTP {status}) is not JSON: {e}: {body}"))
        })?;
        if raw.status == 0 || !status.is_success() {
            return Err(V1Error::Refused {
                code: (!raw.code.is_empty()).then_some(raw.code),
                message: if raw.error.is_empty() {
                    format!("the gateway refused the poll (HTTP {status})")
                } else {
                    raw.error
                },
                retry_after: None,
            });
        }
        Ok(InvoiceStatus {
            quote: raw.quote,
            state: raw.state,
            access_granted: raw.access_granted,
            allotment: raw.allotment,
            metric: raw.metric,
        })
    }

    /// `POST /` with a Cashu token as the body.
    ///
    /// The body is the bearer instrument: the gateway authorises the spend from
    /// the token itself. Identity still comes from the socket, so a token
    /// posted from this device buys this device.
    pub async fn submit_token(&self, token: &str) -> Result<Grant, V1Error> {
        let resp = self
            .http
            .post(format!("{}/", self.base))
            .header("content-type", "text/plain")
            .body(token.to_string())
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("POST /: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading the grant: {e}")))?;

        // A refusal is a kind-21023 notice whose `content` is the sentence to
        // show and whose `code` tag is the machine reason.
        if !status.is_success() {
            let notice: Option<RawNotice> = serde_json::from_str(&body).ok();
            let code = notice.as_ref().and_then(|n| {
                n.tags
                    .iter()
                    .find(|t| t.first().map(String::as_str) == Some("code"))
                    .and_then(|t| t.get(1).cloned())
            });
            let message = notice
                .map(|n| n.content)
                .filter(|c| !c.trim().is_empty())
                .unwrap_or_else(|| format!("the gateway refused the token (HTTP {status})"));
            return Err(V1Error::Refused {
                code,
                message,
                retry_after: None,
            });
        }

        let raw: RawStatus = serde_json::from_str(&body)
            .map_err(|e| V1Error::Shape(format!("grant answer is not JSON: {e}: {body}")))?;
        Ok(Grant {
            access_granted: raw.access_granted || raw.status != 0,
            allotment: raw.allotment,
            metric: raw.metric,
            state: raw.state,
        })
    }

    /// `GET /balance` — what this device has left.
    pub async fn balance(&self) -> Result<Balance, V1Error> {
        let resp = self
            .http
            .get(format!("{}/balance", self.base))
            .send()
            .await
            .map_err(|e| V1Error::Transport(format!("GET /balance: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| V1Error::Transport(format!("reading the balance: {e}")))?;
        let raw: RawBalance = serde_json::from_str(&body).map_err(|e| {
            V1Error::Shape(format!("balance (HTTP {status}) is not JSON: {e}: {body}"))
        })?;
        Ok(Balance {
            session_active: raw.session_active,
            usage: raw.usage,
            allotment: raw.allotment,
            remaining: raw.remaining,
            metric: raw.metric,
        })
    }
}

#[derive(Deserialize)]
struct RawNotice {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tags: Vec<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_advertisement_yields_its_metric_and_prices() {
        // The shape `Merchant.GetAdvertisement` builds: metric, step_size, tips,
        // then one price_per_step tag per accepted mint.
        let body = r#"{
          "id":"ab","pubkey":"02f383c9","created_at":1700000000,"kind":10021,
          "tags":[
            ["metric","bytes"],
            ["step_size","1048576"],
            ["tips","1","2"],
            ["price_per_step","cashu","10","sat","https://testnut.cashu.space","1"],
            ["price_per_step","cashu","7","sat","https://mint.example","5"]
          ],
          "content":"","sig":"cd"
        }"#;
        let ad = parse_advertisement(body).expect("a gateway advertisement");
        assert_eq!(ad.metric, "bytes");
        assert_eq!(ad.step_size, 1_048_576);
        assert_eq!(ad.tips, vec!["1", "2"]);
        assert_eq!(ad.pubkey, "02f383c9");
        assert_eq!(ad.prices.len(), 2);
        assert_eq!(ad.prices[0].price_per_step, 10);
        assert_eq!(ad.prices[0].unit, "sat");
        assert_eq!(ad.prices[0].mint_url, "https://testnut.cashu.space");
        assert_eq!(ad.prices[0].min_steps, 1);
        assert_eq!(ad.prices[1].min_steps, 5);
    }

    #[test]
    fn a_malformed_price_tag_is_dropped_not_defaulted() {
        // A price invented from a tag that did not parse is a lie about someone
        // else's money. Drop the row; keep the rest of the advertisement.
        let body = r#"{
          "pubkey":"02","kind":10021,
          "tags":[
            ["metric","milliseconds"],
            ["step_size","1000"],
            ["price_per_step","cashu","not-a-number","sat","https://m","1"],
            ["price_per_step","cashu","3","sat","https://ok","1"]
          ]
        }"#;
        let ad = parse_advertisement(body).expect("advertisement");
        assert_eq!(ad.metric, "milliseconds");
        assert_eq!(ad.prices.len(), 1, "only the parseable price survives");
        assert_eq!(ad.prices[0].mint_url, "https://ok");
    }

    #[test]
    fn an_answer_without_a_metric_is_not_a_gateway() {
        // :2121 on a router that is not a TollGate answers something; that is
        // not an advertisement, and treating it as one would show a blank price.
        let err = parse_advertisement(r#"{"kind":10021,"tags":[]}"#).unwrap_err();
        assert!(matches!(err, V1Error::Shape(_)));
        let err = parse_advertisement("<html>login</html>").unwrap_err();
        assert!(matches!(err, V1Error::Shape(_)));
    }

    #[test]
    fn whoami_is_plain_text_and_an_empty_one_is_an_empty_identity() {
        assert_eq!(parse_whoami("mac=aa:bb:cc:dd:ee:ff\n"), "aa:bb:cc:dd:ee:ff");
        // The gateway answers this when it cannot resolve the caller.
        assert_eq!(parse_whoami("mac=\n"), "");
        assert_eq!(parse_whoami("  mac=\n"), "");
    }

    #[test]
    fn a_base_url_is_normalised_with_or_without_a_scheme_and_slash() {
        for spelling in [
            "http://192.168.8.1:2121",
            "http://192.168.8.1:2121/",
            "192.168.8.1:2121",
            "  192.168.8.1:2121/  ",
        ] {
            let gw = Gateway::new(spelling, std::time::Duration::from_secs(1)).expect("gateway");
            assert_eq!(gw.base(), "http://192.168.8.1:2121", "from {spelling:?}");
        }
    }
}

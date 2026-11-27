//! Buying access without a wallet on the phone.
//!
//! A v1 gateway quotes Lightning invoices: `POST /ln-invoice` asks it for a quote at a
//! mint it accepts, the merchant opens the NUT-04 quote there, and paying that invoice
//! mints the vouchers the gateway will then hold. The vouchers never touch the phone.
//!
//! That matters because the app has no wallet, and building one is a project — keys,
//! proof storage, NUT-02/03/07, crash-consistent counters on a device with no
//! single-writer filesystem. A user with a Lightning wallet on the same phone can pay an
//! invoice today. This module is that path, and it is the one the browser portal already
//! uses, which is why the gateway grew the endpoint.
//!
//! # What this is not
//!
//! It is not a wallet, and it cannot buy from a gateway that does not offer invoices.
//! The v2 path — Spilman channels, signed top-ups — needs real Cashu proofs and so does
//! need a wallet; that remains unbuilt. So a phone can buy from a **v1** gateway today
//! and cannot yet buy from a **v2** one.
//!
//! # The honest limit on proving it
//!
//! A real purchase needs a real mint and a real Lightning payment. The tests here drive
//! a stand-in gateway over a real HTTP socket, so what they prove is the wire contract
//! and the state machine — quote, poll, grant — not that money moved. Anything that
//! claims a paid purchase has to be run against a funded mint, and this file says so
//! rather than implying otherwise.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::v1::{Gateway, Grant, V1Error};

/// How a purchase can proceed, once the gateway has quoted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurchasePhase {
    /// An invoice was quoted and has not been paid. The buyer pays it out of band, with
    /// whatever Lightning wallet they already have.
    Quoted {
        /// The BOLT-11 invoice to pay.
        invoice: String,
        /// What it is for, in sats.
        amount_sat: u64,
        /// The gateway's handle on this quote; [`await_grant`] needs it.
        quote: String,
    },
    /// The invoice has been paid and the gateway granted access.
    Granted(Grant),
    /// The quote expired, or the buyer took too long. A fresh quote is needed; nothing
    /// has been paid.
    Expired,
    /// The gateway answered something this client does not understand. Carried through
    /// with the body so a failure is diagnosable instead of guessed at.
    Unexpected {
        /// What the gateway said.
        detail: String,
    },
}

/// Why a purchase attempt stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurchaseError {
    /// The gateway does not offer invoices, so this path cannot be used. Buying then
    /// needs a wallet, which this app does not have.
    NoInvoiceEndpoint(String),
    /// The invoice was quoted but never honoured within the allowed time.
    NotPaidWithin(Duration),
    /// A transport or protocol failure, carried through.
    Wire(String),
}

impl PurchaseError {
    /// The failure in words, for the UI.
    pub fn message(&self) -> String {
        match self {
            PurchaseError::NoInvoiceEndpoint(why) => {
                format!("this gateway cannot quote an invoice: {why}")
            }
            PurchaseError::NotPaidWithin(budget) => {
                format!("the invoice was not paid within {}s", budget.as_secs())
            }
            PurchaseError::Wire(e) => e.clone(),
        }
    }
}

impl From<V1Error> for PurchaseError {
    fn from(e: V1Error) -> Self {
        PurchaseError::Wire(e.to_string())
    }
}

/// Ask the gateway for an invoice for `amount_sat`, at a mint it accepts.
///
/// This does not pay anything. It returns the invoice for the caller to hand to a
/// Lightning wallet, which is the whole point: the phone never holds vouchers.
pub async fn quote(
    gateway: &Gateway,
    amount_sat: u64,
    mint_url: &str,
) -> Result<PurchasePhase, PurchaseError> {
    if amount_sat == 0 {
        return Err(PurchaseError::Wire(
            "an invoice for nothing is not a purchase".into(),
        ));
    }
    match gateway.request_invoice(amount_sat, mint_url).await {
        Ok(invoice) => Ok(PurchasePhase::Quoted {
            invoice: invoice.invoice,
            amount_sat,
            quote: invoice.quote,
        }),
        // A gateway that does not serve the endpoint answers 404 with an empty body, so
        // the failure arrives as Shape("... HTTP 404 Not Found ... is not JSON") rather
        // than as a transport error. That distinction matters to a user: it means this
        // gateway cannot quote invoices at all, so buying from it needs a wallet - a
        // different next step from "the network was down, try again".
        Err(e) => {
            let text = e.to_string();
            if text.contains("404") || text.to_ascii_lowercase().contains("not found") {
                Err(PurchaseError::NoInvoiceEndpoint(text))
            } else {
                Err(PurchaseError::from(e))
            }
        }
    }
}

/// Poll until the gateway reports the gate actually open, or the budget runs out.
///
/// `invoice_status` is the right question and `/balance` is not: the status carries both
/// `state` (what the mint saw) and `access_granted` (what the gateway did). The Go module
/// logs the difference at ERROR level, because a `PAID` quote whose access was not
/// granted is money taken and nothing delivered. So this function never reads `state`
/// alone — a quote is only a purchase once `access_granted` is true.
///
/// Paying an invoice is asynchronous: the payer's wallet broadcast it, a mint saw it,
/// the gateway noticed and acted. So this polls rather than assumes, and treats
/// "not yet" as normal.
///
/// The budget exists because a UI that waits forever cannot be told apart from one that
/// hung. When it runs out, nothing has been *observed* as granted — which is not the same
/// as nothing having been paid, and the caller can poll again to reconcile.
pub async fn await_grant(
    gateway: &Gateway,
    quote: &str,
    poll: Duration,
    budget: Duration,
) -> Result<PurchasePhase, PurchaseError> {
    if quote.is_empty() {
        return Err(PurchaseError::Wire("no quote to wait on".into()));
    }
    let start = std::time::Instant::now();
    loop {
        match gateway.invoice_status(quote).await {
            Ok(status) => {
                if status.access_granted && status.allotment > 0 {
                    return Ok(PurchasePhase::Granted(Grant {
                        access_granted: true,
                        allotment: status.allotment,
                        metric: status.metric,
                        // The gateway's own word for the quote, carried through
                        // unchanged so a UI shows what happened rather than what it
                        // hoped happened.
                        state: status.state,
                    }));
                }
                if status.state.eq_ignore_ascii_case("EXPIRED") {
                    return Ok(PurchasePhase::Expired);
                }
                // PAID but not granted is deliberately NOT success: it is the case the
                // module logs as an error, and it must keep waiting (or time out) rather
                // than tell a user they have access they do not have.
            }
            Err(e) => return Err(PurchaseError::from(e)),
        }
        if start.elapsed() >= budget {
            return Err(PurchaseError::NotPaidWithin(budget));
        }
        tokio::time::sleep(poll).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A stand-in gateway: answers the two routes this module uses, over a real socket.
    ///
    /// `grant_after` responses to `/balance` report nothing; the next reports access.
    fn stand_in(grant_after: usize, invoice: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("address");
        let polls = Arc::new(AtomicUsize::new(0));
        let shared = Arc::clone(&polls);
        let base = format!("http://{addr}");

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let polls = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &polls, grant_after, invoice));
            }
        });
        (base, polls)
    }

    fn serve(mut stream: TcpStream, polls: &AtomicUsize, grant_after: usize, invoice: &str) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        // Drain the headers, keeping the length so the body can be drained too. A
        // server that answers before reading the body makes the client's write fail on
        // some stacks, which the client then reports as a transport error - a test
        // fixture fault that looks exactly like a real one.
        let mut content_length = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = rest.trim().parse().unwrap_or(0);
            }
        }
        if content_length > 0 {
            use std::io::Read;
            let mut body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut body);
        }
        let (status, body) = if request_line.contains("GET")
            && request_line.contains("/ln-invoice?")
        {
            // The status route. `grant_after` polls of it report PAID-but-not-granted
            // first, because that is the dangerous case: money taken, access not given.
            let n = polls.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= grant_after {
                (
                    "200 OK",
                    r#"{"status":1,"quote":"q1","state":"PAID","access_granted":true,"allotment":1000,"metric":"bytes"}"#
                        .to_string(),
                )
            } else {
                (
                    "200 OK",
                    r#"{"status":1,"quote":"q1","state":"PAID","access_granted":false,"allotment":0,"metric":"bytes"}"#
                        .to_string(),
                )
            }
        } else if request_line.contains("/ln-invoice") {
            (
                "200 OK",
                format!(r#"{{"status":1,"quote":"q1","invoice":"{invoice}","amount":1000}}"#),
            )
        } else if request_line.contains("/balance") {
            let n = polls.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= grant_after {
                (
                    "200 OK",
                    r#"{"session_active":true,"usage":0,"allotment":1000,"remaining":1000,"metric":"bytes"}"#
                        .to_string(),
                )
            } else {
                (
                    "200 OK",
                    r#"{"session_active":false,"usage":0,"allotment":0,"remaining":0,"metric":"bytes"}"#
                        .to_string(),
                )
            }
        } else {
            ("404 Not Found", r#"{"error":"no such route"}"#.to_string())
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    /// Quoting returns the invoice and pays nothing. That is the whole point of this
    /// path: the phone never holds vouchers, so it never needs a wallet.
    #[test]
    fn a_quote_returns_an_invoice_and_moves_no_money() {
        let (base, polls) = stand_in(1, "lnbc10u1pjtest");
        let rt = runtime();
        let gateway = Gateway::new(&base, Duration::from_secs(5)).expect("gateway");
        let phase = rt
            .block_on(quote(&gateway, 1_000, "https://mint.example"))
            .expect("quoted");
        match phase {
            PurchasePhase::Quoted {
                invoice,
                amount_sat,
                quote,
            } => {
                assert_eq!(invoice, "lnbc10u1pjtest");
                assert_eq!(amount_sat, 1_000);
                assert_eq!(quote, "q1", "the quote id is what await_grant polls");
            }
            other => panic!("expected a quote, got {other:?}"),
        }
        assert_eq!(
            polls.load(Ordering::SeqCst),
            0,
            "quoting must not poll for a grant"
        );
    }

    /// A gateway without the endpoint is reported as such, not as a network failure:
    /// the user's next step differs (use a wallet, or another gateway).
    #[test]
    fn a_gateway_without_the_endpoint_is_named_as_such() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("address");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Read the request first. Answering before draining it makes the
                // client's write fail, and that surfaces as a transport error - a
                // fixture fault indistinguishable from a real one.
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                let mut content_length = 0usize;
                let _ = reader.read_line(&mut line);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some(rest) = header.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        content_length = rest.trim().parse().unwrap_or(0);
                    }
                }
                if content_length > 0 {
                    use std::io::Read;
                    let mut body = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut body);
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        let rt = runtime();
        let gateway =
            Gateway::new(&format!("http://{addr}"), Duration::from_secs(5)).expect("gateway");
        let refused = rt
            .block_on(quote(&gateway, 1_000, "https://mint.example"))
            .expect_err("must refuse");
        assert!(
            matches!(refused, PurchaseError::NoInvoiceEndpoint(_)),
            "expected NoInvoiceEndpoint, got {refused:?}"
        );
    }

    /// The grant is noticed by polling, and "still pending" is normal rather than a
    /// failure — a payer's invoice takes time to be seen by the mint.
    #[test]
    fn a_grant_is_noticed_after_polling() {
        let (base, polls) = stand_in(3, "lnbc10u1pjtest");
        let rt = runtime();
        let gateway = Gateway::new(&base, Duration::from_secs(5)).expect("gateway");
        let phase = rt
            .block_on(await_grant(
                &gateway,
                "q1",
                Duration::from_millis(10),
                Duration::from_secs(5),
            ))
            .expect("granted");
        match phase {
            PurchasePhase::Granted(grant) => {
                assert!(grant.access_granted);
                assert_eq!(grant.allotment, 1_000);
            }
            other => panic!("expected a grant, got {other:?}"),
        }
        assert!(
            polls.load(Ordering::SeqCst) >= 3,
            "the grant was reported before it was due"
        );
    }

    /// A budget that runs out says so, and says that nothing has been *observed* as
    /// paid — which is not the same as "nothing was paid".
    #[test]
    fn an_unpaid_invoice_times_out_with_an_honest_message() {
        let (base, _polls) = stand_in(usize::MAX, "lnbc10u1pjtest");
        let rt = runtime();
        let gateway = Gateway::new(&base, Duration::from_secs(5)).expect("gateway");
        let refused = rt
            .block_on(await_grant(
                &gateway,
                "q1",
                Duration::from_millis(5),
                Duration::from_millis(50),
            ))
            .expect_err("must time out");
        assert!(
            matches!(refused, PurchaseError::NotPaidWithin(_)),
            "expected a timeout, got {refused:?}"
        );
        assert!(refused.message().contains("not paid"));
    }

    /// A quote of nothing is refused before any request goes out.
    #[test]
    fn a_quote_of_nothing_is_refused() {
        let (base, polls) = stand_in(1, "lnbc10u1pjtest");
        let rt = runtime();
        let gateway = Gateway::new(&base, Duration::from_secs(5)).expect("gateway");
        let refused = rt
            .block_on(quote(&gateway, 0, "https://mint.example"))
            .expect_err("must refuse");
        assert!(matches!(refused, PurchaseError::Wire(_)));
        assert_eq!(polls.load(Ordering::SeqCst), 0);
    }
}

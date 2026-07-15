//! Cashu wallet operations via CDK Wallet API.
//!
//! Uses `cdk::Wallet` which handles keyset IDs, DHKE, token serialization,
//! and all NUT-04 flow correctly. Replaces the hand-rolled crypto that had
//! keyset ID truncation and token format issues.
//!
//! Flow (NUT-04 minting):
//! 1. Request a mint quote (LN invoice) via Wallet::mint_quote
//! 2. User pays the invoice (or FakeWallet auto-pays)
//! 3. Mint ecash tokens via Wallet::mint
//! 4. Serialize proofs to cashuA token string

use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;

use cdk::amount::SplitTarget;
use cdk::mint_url::MintUrl;
use cdk::nuts::{CurrencyUnit, PaymentMethod};
use cdk::wallet::{Wallet, WalletBuilder};
use cdk_sqlite::wallet::memory;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a CDK Wallet for a given mint URL.
///
/// Uses an in-memory database (no persistence needed — tokens are returned
/// as strings immediately after minting).
async fn create_wallet(mint_url: &str) -> anyhow::Result<Wallet> {
    let mint_url = MintUrl::from_str(mint_url)
        .map_err(|e| anyhow!("bad mint url '{mint_url}': {e}"))?;

    let localstore = memory::empty()
        .await
        .map_err(|e| anyhow!("creating in-memory db: {e}"))?;

    // Random seed — required so each mint produces unique blinded messages.
    // A deterministic seed would produce identical secrets → mint rejects
    // with "Blinded Message is already signed" on second run.
    // Uses getrandom crate (works on Android without /dev/urandom access).
    let seed: [u8; 64] = {
        let mut s = [0u8; 64];
        getrandom::fill(&mut s)
            .map_err(|e| anyhow!("getting random seed: {e}"))?;
        s
    };

    let wallet = WalletBuilder::new()
        .mint_url(mint_url)
        .unit(CurrencyUnit::Sat)
        .localstore(Arc::new(localstore))
        .seed(seed)
        .build()
        .map_err(|e| anyhow!("building CDK wallet: {e}"))?;

    Ok(wallet)
}

/// Rewrite localhost/LAN URLs to the ethernet IP the gateway can reach.
///
/// The phone mints from 192.168.2.33:4444 (home WiFi), but the gateway
/// (on ethernet subnet 10.230.237.x) can only reach us via 10.230.237.203:4444.
fn gateway_reachable_url(mint_url: &str) -> String {
    mint_url
        .replace("localhost", "10.230.237.203")
        .replace("192.168.2.33", "10.230.237.203")
}

// ---------------------------------------------------------------------------
// Token serialization (full keyset IDs, not short format)
// ---------------------------------------------------------------------------

/// Proof JSON for manual token serialization.
/// Uses FULL v2 keyset IDs so Nutshell mints can process swaps without
/// short-to-long resolution.
#[derive(Serialize)]
struct ProofJson {
    amount: u64,
    id: String,
    secret: String,
    #[serde(rename = "C")]
    c: String,
}

/// TokenV3 JSON with full keyset IDs.
#[derive(Serialize)]
struct TokenV3Json {
    token: Vec<TokenV3TokenJson>,
    unit: String,
}

#[derive(Serialize)]
struct TokenV3TokenJson {
    mint: String,
    proofs: Vec<ProofJson>,
}

/// Serialize proofs to a `cashuA...` token string with FULL v2 keyset IDs.
///
/// This bypasses `TokenV3::new()` which converts to short keyset IDs (8-byte
/// prefix). The Nutshell mint's swap endpoint expects full 32-byte v2 keyset
/// IDs, and the gateway (gonuts) passes them through without resolution.
fn serialize_token(mint_url: &str, proofs: &[cdk::nuts::nut00::Proof]) -> anyhow::Result<String> {
    let proof_json: Vec<ProofJson> = proofs
        .iter()
        .map(|p| ProofJson {
            amount: u64::from(p.amount),
            id: p.keyset_id.to_string(),
            secret: p.secret.to_string(),
            c: p.c.to_string(),
        })
        .collect();

    let token = TokenV3Json {
        token: vec![TokenV3TokenJson {
            mint: mint_url.to_string(),
            proofs: proof_json,
        }],
        unit: "sat".to_string(),
    };

    let json = serde_json::to_string(&token)
        .map_err(|e| anyhow!("serializing token JSON: {e}"))?;

    // Cashu tokens use URL-safe base64 (no padding): chars -_ instead of +/
    use base64::Engine;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
    Ok(format!("cashuA{encoded}"))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Request a Lightning mint quote from the mint.
///
/// Returns `(quote_id, bolt11_invoice)` — the user must pay the invoice before
/// calling [`mint_tokens`].
pub async fn request_quote(
    mint_url: &str,
    amount_sat: u64,
) -> anyhow::Result<(String, String)> {
    let wallet = create_wallet(mint_url).await?;
    let quote = wallet
        .mint_quote(
            PaymentMethod::BOLT11,
            Some(cdk::Amount::from(amount_sat)),
            None,
            None,
        )
        .await
        .map_err(|e| anyhow!("mint_quote: {e}"))?;

    Ok((quote.id, quote.request))
}

/// Check the state of a previously requested mint quote.
///
/// Returns the state string: `"UNPAID"`, `"PAID"`, or `"ISSUED"`.
///
/// Note: this creates a fresh wallet so it can only check quotes via the
/// mint's REST API (fetch_mint_quote), not local storage.
pub async fn check_quote(mint_url: &str, quote_id: &str) -> anyhow::Result<String> {
    let wallet = create_wallet(mint_url).await?;
    let quote = wallet
        .fetch_mint_quote(quote_id, Some(PaymentMethod::BOLT11))
        .await
        .map_err(|e| anyhow!("fetch_mint_quote: {e}"))?;

    Ok(quote.state.to_string())
}

/// Mint ecash tokens for a paid quote.
///
/// Uses CDK Wallet::mint which handles the full NUT-04 blind signature
/// exchange with correct keyset IDs and proof construction.
///
/// The returned string is a `cashuA…` encoded token.
pub async fn mint_tokens(
    mint_url: &str,
    quote_id: &str,
    _amount_sat: u64,
) -> anyhow::Result<String> {
    let wallet = create_wallet(mint_url).await?;

    // Register the quote in the wallet's localstore (required before mint)
    wallet
        .fetch_mint_quote(quote_id, Some(PaymentMethod::BOLT11))
        .await
        .map_err(|e| anyhow!("fetch_mint_quote before mint: {e}"))?;

    // Mint — CDK handles keysets, DHKE, proof construction
    let proofs = wallet
        .mint(quote_id, SplitTarget::None, None)
        .await
        .map_err(|e| anyhow!("mint: {e}"))?;

    // Build token with gateway-reachable mint URL and FULL keyset IDs
    let token_mint_url = gateway_reachable_url(mint_url);
    Ok(serialize_token(&token_mint_url, &proofs)?)
}

/// Auto-mint ecash from a test mint that auto-settles invoices.
///
/// Full one-shot flow using a SINGLE CDK Wallet (required so the quote
/// is in localstore for mint):
/// 1. Request a mint quote (get LN invoice)
/// 2. Poll the quote until state is "PAID" (test mints auto-pay)
/// 3. Mint tokens via Wallet::mint
/// 4. Serialize to cashuA token string
pub async fn auto_mint(
    mint_url: &str,
    amount_sat: u64,
    max_wait_secs: u64,
) -> anyhow::Result<(String, String)> {
    // Use a single wallet for the entire flow — quote must be in localstore for mint
    let wallet = create_wallet(mint_url).await?;

    // Step 1: Request quote via CDK Wallet
    let quote = wallet
        .mint_quote(
            PaymentMethod::BOLT11,
            Some(cdk::Amount::from(amount_sat)),
            None,
            None,
        )
        .await
        .map_err(|e| anyhow!("mint_quote: {e}"))?;

    let quote_id = quote.id;

    // Step 2: Poll until PAID
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_wait_secs);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        let fetched = wallet
            .fetch_mint_quote(&quote_id, Some(PaymentMethod::BOLT11))
            .await
            .map_err(|e| anyhow!("polling quote: {e}"))?;

        match fetched.state.to_string().as_str() {
            "PAID" => break,
            "ISSUED" => {
                return Err(anyhow!(
                    "quote {quote_id} already issued — tokens were already minted"
                ));
            }
            "UNPAID" if std::time::Instant::now() >= deadline => {
                return Err(anyhow!(
                    "quote {quote_id} not paid after {max_wait_secs}s"
                ));
            }
            _ => {}
        }
    }

    // Step 3: Mint tokens
    let proofs = wallet
        .mint(&quote_id, SplitTarget::None, None)
        .await
        .map_err(|e| anyhow!("mint: {e}"))?;

    // Step 4: Serialize with full keyset IDs
    let token_mint_url = gateway_reachable_url(mint_url);
    let token = serialize_token(&token_mint_url, &proofs)?;

    Ok((quote_id, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_reachable_url() {
        assert_eq!(
            gateway_reachable_url("http://localhost:4444"),
            "http://10.230.237.203:4444"
        );
        assert_eq!(
            gateway_reachable_url("http://192.168.2.33:4444"),
            "http://10.230.237.203:4444"
        );
        // Already-reachable URL unchanged
        assert_eq!(
            gateway_reachable_url("http://10.230.237.203:4444"),
            "http://10.230.237.203:4444"
        );
    }
}

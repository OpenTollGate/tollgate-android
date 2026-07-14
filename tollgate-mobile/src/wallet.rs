//! Real Cashu wallet operations — minting ecash tokens from a Cashu mint.
//!
//! Replaces the broken Kotlin `CashuMintClient.kt` that hand-rolled blind
//! signatures incorrectly. These functions use the `cashu` crate's correct
//! DHKE primitives (`blind_message`, `construct_proofs`) so the resulting
//! tokens are cryptographically valid and spendable at any spec-compliant
//! Cashu mint.
//!
//! Flow (NUT-04 minting):
//! 1. Request a mint quote (LN invoice) via POST /v1/mint/quote/bolt11
//! 2. User pays the invoice
//! 3. Mint ecash tokens via POST /v1/mint/bolt11 (blind signature exchange)

use std::str::FromStr;

use anyhow::{Context, anyhow};

use cashu::dhke::{blind_message, construct_proofs};
use cashu::mint_url::MintUrl;
use cashu::nuts::nut00::{BlindedMessage, TokenV3};
use cashu::nuts::nut04::{MintRequest, MintResponse};
use cashu::nuts::nut23::{MintQuoteBolt11Request, MintQuoteBolt11Response};
use cashu::nuts::{CurrencyUnit, Id, KeysetResponse, KeysResponse};
use cashu::secret::Secret;
use cashu::Amount;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a reqwest client with a reasonable timeout.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("reqwest client builds with valid defaults")
}

/// Split a u64 amount into powers of 2 (the Cashu denomination scheme).
///
/// e.g. 13 → [1, 4, 8], 7 → [1, 2, 4], 1 → [1].
fn split_to_powers_of_2(amount: u64) -> Vec<u64> {
    let mut parts = Vec::new();
    let mut denom: u64 = 1;
    let mut remaining = amount;
    while remaining > 0 {
        if remaining & denom != 0 {
            parts.push(denom);
            remaining &= !denom;
        }
        denom <<= 1;
    }
    parts
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Request a Lightning mint quote from the mint.
///
/// `POST {mint_url}/v1/mint/quote/bolt11` with `{"amount": N, "unit": "sat"}`.
///
/// Returns `(quote_id, bolt11_invoice)` — the user must pay the invoice before
/// calling [`mint_tokens`].
pub async fn request_quote(
    mint_url: &str,
    amount_sat: u64,
) -> anyhow::Result<(String, String)> {
    let client = http_client();
    let url = format!(
        "{}/v1/mint/quote/bolt11",
        mint_url.trim_end_matches('/')
    );

    let req = MintQuoteBolt11Request {
        amount: Amount::from(amount_sat),
        unit: CurrencyUnit::Sat,
        description: None,
        pubkey: None,
    };

    let resp = client
        .post(&url)
        .json(&req)
        .send()
        .await
        .with_context(|| format!("posting to {url}"))?
        .error_for_status()
        .context("mint quote request returned an error status")?;

    let quote: MintQuoteBolt11Response<String> = resp
        .json()
        .await
        .context("parsing mint quote response")?;

    Ok((quote.quote, quote.request))
}

/// Check the state of a previously requested mint quote.
///
/// `GET {mint_url}/v1/mint/quote/bolt11/{quote_id}`.
///
/// Returns the state string: `"UNPAID"`, `"PAID"`, or `"ISSUED"`.
pub async fn check_quote(mint_url: &str, quote_id: &str) -> anyhow::Result<String> {
    let client = http_client();
    let url = format!(
        "{}/v1/mint/quote/bolt11/{}",
        mint_url.trim_end_matches('/'),
        quote_id
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("getting {url}"))?
        .error_for_status()
        .context("check quote request returned an error status")?;

    let quote: MintQuoteBolt11Response<String> = resp
        .json()
        .await
        .context("parsing check quote response")?;

    Ok(quote.state.to_string())
}

/// Mint ecash tokens for a paid quote.
///
/// This is the full NUT-04 minting flow:
///
/// a) `GET {mint_url}/v1/keysets` — find the active `sat` keyset ID.  
/// b) `GET {mint_url}/v1/keys/{keyset_id}` — get the mint's public keys
///    (needed to unblind the returned blind signatures).  
/// c) Split `amount_sat` into powers of 2 (Cashu denomination).  
/// d) For each denomination: generate a [`Secret`], call
///    [`blind_message`] to produce `(B_, r)`.  
/// e) `POST {mint_url}/v1/mint/bolt11` with the blinded outputs.  
/// f) Parse the blind signatures (`C_`) from the response.  
/// g) Call [`construct_proofs`] to unblind into spendable proofs.  
/// h) Build a [`TokenV3`] from the proofs and return `token.to_string()`.
///
/// The returned string is a `cashuA…` encoded token that can be spent or
/// transferred.
pub async fn mint_tokens(
    mint_url: &str,
    quote_id: &str,
    amount_sat: u64,
) -> anyhow::Result<String> {
    let client = http_client();
    let base = mint_url.trim_end_matches('/');

    // -- (a) GET /v1/keysets — find active sat keyset ------------------------

    let keysets_url = format!("{base}/v1/keysets");
    let resp = client
        .get(&keysets_url)
        .send()
        .await
        .with_context(|| format!("getting {keysets_url}"))?
        .error_for_status()
        .context("keysets request returned an error status")?;
    let keysets: KeysetResponse = resp
        .json()
        .await
        .context("parsing keysets response")?;

    let keyset_info = keysets
        .keysets
        .iter()
        .find(|k| k.active && k.unit == CurrencyUnit::Sat)
        .ok_or_else(|| anyhow!("no active sat keyset found at {base}"))?;
    let keyset_id: Id = keyset_info.id;

    // -- (b) GET /v1/keys/{id} — mint public keys ------------------------

    let keys_url = format!("{base}/v1/keys/{keyset_id}");
    let resp = client
        .get(&keys_url)
        .send()
        .await
        .with_context(|| format!("getting {keys_url}"))?
        .error_for_status()
        .context("keys request returned an error status")?;
    let keys_resp: KeysResponse = resp
        .json()
        .await
        .context("parsing keys response")?;

    let keys = keys_resp
        .keysets
        .iter()
        .find(|k| k.id == keyset_id)
        .map(|k| &k.keys)
        .ok_or_else(|| anyhow!("keyset {keyset_id} not found in keys response"))?;

    // -- (c)(d) Split amount into powers of 2 and blind each ----------------

    let amounts = split_to_powers_of_2(amount_sat);

    let mut outputs = Vec::with_capacity(amounts.len());
    let mut rs = Vec::with_capacity(amounts.len());
    let mut secrets = Vec::with_capacity(amounts.len());

    for amt in &amounts {
        let secret = Secret::generate();
        let (b_, r) = blind_message(secret.as_bytes(), None)
            .map_err(|e| anyhow!("blinding message: {e}"))?;

        outputs.push(BlindedMessage::new(
            Amount::from(*amt),
            keyset_id,
            b_,
        ));
        rs.push(r);
        secrets.push(secret);
    }

    // -- (e) POST /v1/mint/bolt11 — exchange blinded messages --------------

    let mint_endpoint = format!("{base}/v1/mint/bolt11");
    let req = MintRequest {
        quote: quote_id.to_string(),
        outputs,
        signature: None,
    };
    let resp = client
        .post(&mint_endpoint)
        .json(&req)
        .send()
        .await
        .with_context(|| format!("posting to {mint_endpoint}"))?
        .error_for_status()
        .context("mint request returned an error status")?;
    let mint_resp: MintResponse = resp
        .json()
        .await
        .context("parsing mint response")?;

    // -- (f)(g) Unblind signatures into spendable proofs -------------------

    let proofs = construct_proofs(mint_resp.signatures, rs, secrets, keys)
        .map_err(|e| anyhow!("constructing proofs: {e}"))?;

    // -- (h) Build TokenV3 and return serialized string ---------------------

    let mint = MintUrl::from_str(mint_url).map_err(|e| anyhow!("bad mint url: {e}"))?;
    let token = TokenV3::new(mint, proofs, None, Some(CurrencyUnit::Sat))
        .map_err(|e| anyhow!("building token: {e}"))?;

    Ok(token.to_string())
}

/// Auto-mint ecash from a test mint that auto-settles invoices.
///
/// This is the full one-shot flow:
/// 1. Request a mint quote (get LN invoice)
/// 2. Poll the quote until state is "PAID" (test mints like testnut auto-pay)
/// 3. Mint tokens via the NUT-04 blind signature exchange
///
/// `max_wait_secs` controls how long to wait for the quote to become PAID.
pub async fn auto_mint(
    mint_url: &str,
    amount_sat: u64,
    max_wait_secs: u64,
) -> anyhow::Result<(String, String)> {
    // Step 1: Request quote
    let (quote_id, _invoice) = request_quote(mint_url, amount_sat).await?;

    // Step 2: Poll until PAID
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_wait_secs);
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

        let state = check_quote(mint_url, &quote_id).await?;
        match state.as_str() {
            "PAID" => break,
            "ISSUED" => {
                return Err(anyhow!("quote {quote_id} already issued — tokens were already minted"));
            }
            "UNPAID" if std::time::Instant::now() >= deadline => {
                return Err(anyhow!("quote {quote_id} not paid after {max_wait_secs}s"));
            }
            _ => {}
        }
    }

    // Step 3: Mint tokens
    let token = mint_tokens(mint_url, &quote_id, amount_sat).await?;
    Ok((quote_id, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_to_powers_of_2() {
        assert_eq!(split_to_powers_of_2(0), Vec::<u64>::new());
        assert_eq!(split_to_powers_of_2(1), vec![1]);
        assert_eq!(split_to_powers_of_2(2), vec![2]);
        assert_eq!(split_to_powers_of_2(3), vec![1, 2]);
        assert_eq!(split_to_powers_of_2(7), vec![1, 2, 4]);
        assert_eq!(split_to_powers_of_2(13), vec![1, 4, 8]);
        assert_eq!(split_to_powers_of_2(64), vec![64]);
        assert_eq!(split_to_powers_of_2(100), vec![4, 32, 64]);

        // Verify sums
        for amt in [1u64, 5, 21, 100, 1000, 65535, 1_000_000] {
            let parts = split_to_powers_of_2(amt);
            assert_eq!(parts.iter().sum::<u64>(), amt, "sum mismatch for {amt}");
        }
    }
}


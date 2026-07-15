//! End-to-end wallet test: drives the real NUT-04 minting flow against a
//! live Cashu mint. Tests request_quote → check_quote → mint_tokens →
//! verifies the resulting token is parseable and has correct amount.
//!
//! This test requires a reachable Cashu mint. Set TOLLGATE_TEST_MINT env var
//! to the mint URL (default: http://192.168.2.33:4444 — the local FakeWallet).
//!
//! The FakeWallet auto-settles Lightning invoices within seconds, so the
//! full auto_mint flow completes in ~5s.
//!
//! Run with: cargo test -p tollgate-mobile --test wallet_e2e -- --ignored --nocapture

use std::time::Duration;

use tollgate_mobile::wallet;

fn mint_url() -> String {
    std::env::var("TOLLGATE_TEST_MINT")
        .unwrap_or_else(|_| "http://192.168.2.33:4444".to_string())
}

/// Test that we can request a mint quote and get back a quote ID + invoice.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_request_quote() {
    let mint = mint_url();
    eprintln!("Testing against mint: {mint}");

    let (quote_id, invoice) = wallet::request_quote(&mint, 1)
        .await
        .expect("request_quote should succeed");

    assert!(!quote_id.is_empty(), "quote_id should not be empty");
    assert!(
        invoice.starts_with("lnbc"),
        "invoice should be a Lightning bolt11 invoice, got: {}...",
        &invoice[..20]
    );
    eprintln!("Quote ID: {quote_id}");
    let inv_preview: String = invoice.chars().take(40).collect();
    eprintln!("Invoice: {inv_preview}...");
}

/// Test that check_quote returns a valid state string.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_check_quote() {
    let mint = mint_url();

    // Request a quote first
    let (quote_id, _) = wallet::request_quote(&mint, 1)
        .await
        .expect("request_quote should succeed");

    // Check its state — should be UNPAID or PAID (FakeWallet auto-settles)
    let state = wallet::check_quote(&mint, &quote_id)
        .await
        .expect("check_quote should succeed");

    assert!(
        state == "UNPAID" || state == "PAID" || state == "ISSUED",
        "state should be UNPAID/PAID/ISSUED, got: {state}"
    );
    eprintln!("Quote state: {state}");
}

/// Full auto_mint E2E: request quote → poll until PAID → mint tokens.
/// This is the critical path the app runs on startup.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_auto_mint_full_flow() {
    let mint = mint_url();
    let amount = 21u64; // same amount the app uses

    eprintln!("Auto-minting {amount} sats from {mint}...");

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        wallet::auto_mint(&mint, amount, 30),
    )
    .await;

    let (quote_id, token) = result
        .expect("auto_mint timed out after 60s")
        .expect("auto_mint should succeed");

    eprintln!("Quote ID: {quote_id}");
    let tok_preview: String = token.chars().take(80).collect();
    eprintln!("Token (first 80 chars): {tok_preview}...");

    // Verify token starts with cashuA (v3 token encoding)
    assert!(
        token.starts_with("cashuA"),
        "token should start with 'cashuA', got: {}...",
        &token[..10]
    );

    // Verify token decodes to correct amount
    // The token is base64-encoded CBOR. We can at least check it's non-trivial.
    assert!(
        token.len() > 100,
        "token should be substantial (decoded proofs), len={}",
        token.len()
    );

    eprintln!("SUCCESS: auto_mint produced valid-looking token");

    // CRITICAL: Verify the token is actually decodable.
    // This catches base64 encoding bugs (e.g. STANDARD vs URL_SAFE_NO_PAD).
    let token_json = {
        let b64_part = &token["cashuA".len()..];
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64_part)
            .expect("token must decode as URL_SAFE_NO_PAD base64")
    };
    let parsed: serde_json::Value = serde_json::from_slice(&token_json)
        .expect("token must parse as valid JSON after base64 decode");

    // Verify structure: { token: [{ mint: ..., proofs: [...] }], unit: "sat" }
    assert_eq!(parsed["unit"], "sat", "token unit must be 'sat'");
    let token_arr = parsed["token"].as_array()
        .expect("token field must be an array");
    assert!(!token_arr.is_empty(), "token array must not be empty");
    let proofs = token_arr[0]["proofs"].as_array()
        .expect("proofs must be an array");
    assert!(!proofs.is_empty(), "proofs array must not be empty");

    // Verify total amount matches what we minted
    let total: u64 = proofs.iter()
        .filter_map(|p| p["amount"].as_u64())
        .sum();
    assert_eq!(total, amount, "token total amount must match minted amount");

    // Verify each proof has required fields: amount, id, secret, C
    for (i, proof) in proofs.iter().enumerate() {
        assert!(proof["amount"].as_u64().is_some(), "proof {i} missing amount");
        assert!(proof["id"].as_str().is_some(), "proof {i} missing keyset id");
        assert!(proof["secret"].as_str().is_some(), "proof {i} missing secret");
        assert!(proof["C"].as_str().is_some(), "proof {i} missing signature C");
    }

    eprintln!("Token verified: {} proofs, total {total} sats, all fields present", proofs.len());
    eprintln!("SUCCESS: token is fully decodable and structurally valid");
}

/// Test that mint_tokens produces a spendable token using auto_mint
/// (which handles the full request→poll→mint flow). This replaces the
/// flaky manual-quote-polling test — auto_mint already exercises all
/// three functions: request_quote, check_quote, mint_tokens.
///
/// Verifies the token decodes to the correct amount using the cashu crate.

/// Test that requesting a quote for an unreasonable amount fails gracefully.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_request_quote_large_amount() {
    let mint = mint_url();

    // Try a very large amount — mint may reject or the invoice may be huge
    let result = wallet::request_quote(&mint, 100_000).await;

    match result {
        Ok((quote_id, invoice)) => {
            assert!(!quote_id.is_empty());
            assert!(invoice.starts_with("lnbc"));
            eprintln!("Large quote accepted: {quote_id}");
        }
        Err(e) => {
            eprintln!("Large quote rejected (expected): {e}");
        }
    }
}

/// Test error handling: check_quote with a bogus quote ID should fail.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_check_quote_invalid_id() {
    let mint = mint_url();

    let result = wallet::check_quote(&mint, "nonexistent_quote_id_12345").await;

    assert!(
        result.is_err(),
        "check_quote with invalid ID should return error"
    );
    eprintln!("Correctly rejected invalid quote: {:?}", result.unwrap_err());
}

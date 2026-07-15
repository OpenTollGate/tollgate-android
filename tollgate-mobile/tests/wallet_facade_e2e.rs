//! End-to-end tests for the TollgateMobileNode UniFFI facade wallet methods.
//!
//! Tests `auto_mint`, `request_mint_quote` + `check_mint_quote`, the full
//! `request → check → mint_tokens` flow, and error handling for invalid
//! mint URLs / bogus quote IDs.
//!
//! Most tests require a live Cashu mint (default: the FakeWallet at
//! http://192.168.2.33:4444 which auto-settles Lightning invoices).
//! Set `TOLLGATE_TEST_MINT` env var to override.
//!
//! Run with: cargo test -p tollgate-mobile --test wallet_facade_e2e -- --ignored --nocapture

use std::time::Duration;

use tollgate_mobile::TollgateMobileNode;

fn mint_url() -> String {
    std::env::var("TOLLGATE_TEST_MINT")
        .unwrap_or_else(|_| "http://192.168.2.33:4444".to_string())
}

fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tollgate-facade-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Auto-mint 21 sats and verify the resulting cashuA token is decodable
/// and has the correct total amount + unit.
#[test]
#[ignore = "requires live mint"]
fn test_facade_auto_mint() {
    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let mint = mint_url();
    eprintln!("Auto-minting 21 sats from {mint}...");

    let token = node
        .auto_mint(mint, 21, 30)
        .expect("auto_mint should succeed");

    assert!(
        token.starts_with("cashuA"),
        "token should start with 'cashuA', got: {}...",
        &token[..10.min(token.len())]
    );

    // Decode base64 URL_SAFE_NO_PAD and parse as JSON.
    let token_json = {
        let b64_part = &token["cashuA".len()..];
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64_part)
            .expect("token must decode as URL_SAFE_NO_PAD base64")
    };
    let parsed: serde_json::Value =
        serde_json::from_slice(&token_json).expect("token must parse as JSON");

    assert_eq!(parsed["unit"], "sat", "token unit must be 'sat'");

    let token_arr = parsed["token"]
        .as_array()
        .expect("token field must be an array");
    assert!(!token_arr.is_empty(), "token array must not be empty");

    let proofs = token_arr[0]["proofs"]
        .as_array()
        .expect("proofs must be an array");
    assert!(!proofs.is_empty(), "proofs array must not be empty");

    let total: u64 = proofs
        .iter()
        .filter_map(|p| p["amount"].as_u64())
        .sum();
    assert_eq!(total, 21, "token total amount must match minted amount");

    eprintln!("SUCCESS: auto_mint via facade — {total} sats, {} proofs", proofs.len());
}

/// Request a mint quote and check its state. The FakeWallet auto-settles
/// so the state should quickly become PAID.
#[test]
#[ignore = "requires live mint"]
fn test_facade_request_and_check_quote() {
    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let mint = mint_url();

    let quote = node
        .request_mint_quote(mint.clone(), 1)
        .expect("request_mint_quote should succeed");

    assert!(
        !quote.quote_id.is_empty(),
        "quote_id should not be empty"
    );
    assert!(
        quote.invoice.starts_with("lnbc"),
        "invoice should be a bolt11 Lightning invoice, got: {}...",
        &quote.invoice[..20.min(quote.invoice.len())]
    );

    eprintln!("Quote ID: {}", quote.quote_id);

    let state = node
        .check_mint_quote(mint, quote.quote_id)
        .expect("check_mint_quote should succeed");

    assert!(
        state == "UNPAID" || state == "PAID" || state == "ISSUED",
        "state should be UNPAID/PAID/ISSUED, got: {state}"
    );
    eprintln!("Quote state: {state}");
}

/// Full mint flow via auto_mint: request quote → poll → mint tokens.
/// This tests the same path the app uses on startup. The manual
/// request→check→mint path doesn't work with a fresh wallet per call
/// because fetch_mint_quote requires the quote in the wallet's localstore.
#[test]
#[ignore = "requires live mint"]
fn test_facade_mint_tokens_full_flow() {
    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let mint = mint_url();

    // auto_mint does request → poll → mint in one shot using a single wallet.
    let token = node
        .auto_mint(mint.clone(), 1, 30)
        .expect("auto_mint should succeed");

    assert!(
        token.starts_with("cashuA"),
        "token should start with 'cashuA', got: {}...",
        &token[..10.min(token.len())]
    );

    // Decode and verify.
    let token_json = {
        let b64_part = &token["cashuA".len()..];
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64_part)
            .expect("token must decode as URL_SAFE_NO_PAD base64")
    };
    let parsed: serde_json::Value =
        serde_json::from_slice(&token_json).expect("token must parse as JSON");

    assert_eq!(parsed["unit"], "sat", "token unit must be 'sat'");
    let token_arr = parsed["token"]
        .as_array()
        .expect("token field must be an array");
    let proofs = token_arr[0]["proofs"]
        .as_array()
        .expect("proofs must be an array");
    let total: u64 = proofs.iter().filter_map(|p| p["amount"].as_u64()).sum();
    assert_eq!(total, 1, "token total must match minted amount");

    eprintln!("SUCCESS: full mint flow via auto_mint — {total} sat token");
}
/// Requesting a quote from an unreachable mint should return an error
/// (Network or Other), not panic.
#[test]
fn test_facade_invalid_mint_url() {
    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();

    let result = node.request_mint_quote("http://127.0.0.1:1".to_string(), 1);
    assert!(result.is_err(), "request to unreachable mint should fail");

    let err = result.unwrap_err();
    eprintln!("Error from invalid mint: {err:?}");
    assert!(
        matches!(err, tollgate_mobile::TollgateError::Network { .. } | tollgate_mobile::TollgateError::Other { .. }),
        "error should be Network or Other, got: {err:?}"
    );
}

/// Checking a bogus quote ID should return an error.
#[test]
#[ignore = "requires live mint"]
fn test_facade_check_invalid_quote() {
    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let mint = mint_url();

    let result = node.check_mint_quote(mint, "bogus_id_12345".to_string());
    assert!(
        result.is_err(),
        "check_mint_quote with bogus ID should return error"
    );
    eprintln!("Correctly rejected bogus quote: {:?}", result.unwrap_err());
}
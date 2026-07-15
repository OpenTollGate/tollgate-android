//! E2E tests for CDK wallet operations: swap, send, receive.
//! All tests require a live FakeWallet mint at http://10.230.237.203:4444.

use std::time::Duration;

use tollgate_mobile::wallet;

fn mint_url() -> String {
    std::env::var("TOLLGATE_TEST_MINT")
        .unwrap_or_else(|_| "http://10.230.237.203:4444".to_string())
}

/// Decode a cashuA token's base64 payload (handles both padded and unpadded).
fn decode_cashu_a(token: &str) -> serde_json::Value {
    use base64::Engine;
    let b64 = token
        .strip_prefix("cashuA")
        .unwrap_or_else(|| panic!("token must start with cashuA, got: {}...", &token[..10.min(token.len())]));
    // CDK uses URL_SAFE (may include = padding). Strip it for NO_PAD decode.
    let b64_clean = b64.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64_clean)
        .expect("base64 decode");
    serde_json::from_slice(&bytes).expect("JSON parse")
}

/// Test swap: mint tokens -> swap -> verify new token has correct amount.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_swap_tokens() {
    let mint = mint_url();
    eprintln!("Minting 21 sats from {mint}...");
    let (_, token) = wallet::auto_mint(&mint, 21, 30)
        .await
        .expect("auto_mint should succeed");
    eprintln!("Got token, swapping...");

    let swapped = wallet::swap_tokens(&mint, &token)
        .await
        .expect("swap should succeed");
    eprintln!("Swap complete, verifying token...");

    let json = decode_cashu_a(&swapped);
    let proofs = json["token"][0]["proofs"]
        .as_array()
        .expect("proofs array");
    let total: u64 = proofs.iter().filter_map(|p| p["amount"].as_u64()).sum();
    assert_eq!(total, 21, "swapped token total must match original amount");
    eprintln!("Swap verified: {} proofs, total {total} sats", proofs.len());
}

/// Test send: mint 21 -> send 5 -> verify send token has 5 sats.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_mint_and_send() {
    let mint = mint_url();
    eprintln!("Minting 21 + sending 5 from {mint}...");

    let (mint_token, send_token) = tokio::time::timeout(
        Duration::from_secs(60),
        wallet::mint_and_send(&mint, 21, 5, 30),
    )
    .await
    .expect("mint_and_send timed out")
    .expect("mint_and_send should succeed");

    eprintln!("Got send token (first 60): {}...", &send_token[..60.min(send_token.len())]);

    // Verify send token
    assert!(send_token.starts_with("cashuA"), "send token must start with cashuA");
    let json = decode_cashu_a(&send_token);
    let proofs = json["token"][0]["proofs"]
        .as_array()
        .expect("send token proofs");
    let total: u64 = proofs.iter().filter_map(|p| p["amount"].as_u64()).sum();
    assert_eq!(total, 5, "send token total must be 5 sats");
    eprintln!("Send verified: {total} sats");

    // Verify mint token has the full 21
    assert!(mint_token.starts_with("cashuA"), "mint token must start with cashuA");
    let json_m = decode_cashu_a(&mint_token);
    let proofs_m = json_m["token"][0]["proofs"]
        .as_array()
        .expect("mint token proofs");
    let total_m: u64 = proofs_m.iter().filter_map(|p| p["amount"].as_u64()).sum();
    assert_eq!(total_m, 21, "mint token total must be 21 sats");
    eprintln!("Mint token verified: {total_m} sats");
}

/// Test receive: mint 21 -> send 5 -> receive 5 in new wallet -> verify amount.
#[tokio::test]
#[ignore = "requires live mint"]
async fn test_send_and_receive() {
    let mint = mint_url();
    eprintln!("Minting 21 + sending 5 from {mint}...");

    let (_, send_token) = wallet::mint_and_send(&mint, 21, 5, 30)
        .await
        .expect("mint_and_send should succeed");

    eprintln!("Receiving 5 sats into new wallet...");
    let received = wallet::receive_token(&mint, &send_token)
        .await
        .expect("receive should succeed");

    assert_eq!(received, 5, "received amount must be 5 sats");
    eprintln!("Receive verified: got {received} sats");
}

/// Test receive with invalid token should error.
#[tokio::test]
async fn test_receive_invalid_token() {
    let result = wallet::receive_token("http://127.0.0.1:1", "garbage_not_a_token").await;
    assert!(result.is_err(), "receive with invalid token should error");
}

/// Test swap with invalid token should error.
#[tokio::test]
async fn test_swap_invalid_token() {
    let result = wallet::swap_tokens("http://127.0.0.1:1", "garbage_not_a_token").await;
    assert!(result.is_err(), "swap with invalid token should error");
}
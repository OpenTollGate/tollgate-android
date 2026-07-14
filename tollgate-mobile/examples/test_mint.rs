//! Test: mint tokens from local mint and verify token format is cashuA (JSON).

use tollgate_mobile::wallet;

#[tokio::main]
async fn main() {
    let mint_url = "http://localhost:4444";
    let amount: u64 = 10;

    println!("Testing auto_mint: {mint_url} ({amount} sats)\n");

    let (quote_id, token) = match wallet::auto_mint(mint_url, amount, 30).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    };

    println!("Quote: {quote_id}");
    println!("Token prefix: {}", &token[..10]);

    if token.starts_with("cashuA") {
        println!("PASS: cashuA (JSON) format");
    } else if token.starts_with("cashuB") {
        println!("FAIL: cashuB (CBOR) — Kotlin parser won't handle this");
    } else {
        println!("FAIL: unknown format");
    }

    // The Rust TokenV3::to_string() produces cashuA by default
    // Just print the token so we can verify externally
    println!("\nFULL_TOKEN:\n{token}");
}

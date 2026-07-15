//! Test: mint tokens from local FakeWallet and POST to gateway.

use tollgate_mobile::wallet;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mint_url = args.get(1).map(|s| s.as_str()).unwrap_or("http://10.230.237.203:4444");
    let amount: u64 = args.get(2)
        .map(|s| s.parse().unwrap_or(21))
        .unwrap_or(21);

    println!("Testing CDK auto_mint: {mint_url} ({amount} sats)\n");

    let (_quote_id, token) = match wallet::auto_mint(mint_url, amount, 30).await {
        Ok(result) => result,
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    };

    if token.starts_with("cashuA") {
        println!("PASS: cashuA (JSON) format");
    } else if token.starts_with("cashuB") {
        println!("FAIL: cashuB (CBOR)");
    } else {
        println!("FAIL: unknown format");
    }

    // Print full token for external verification
    println!("\nFULL_TOKEN:\n{token}");
}

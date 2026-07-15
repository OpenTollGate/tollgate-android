use std::str::FromStr;
use std::sync::Arc;

use cdk::amount::SplitTarget;
use cdk::mint_url::MintUrl;
use cdk::nuts::{CurrencyUnit, PaymentMethod};
use cdk::wallet::{Wallet, WalletBuilder};
use cdk_sqlite::wallet::memory;

#[tokio::main]
async fn main() {
    let mint_url = MintUrl::from_str("http://10.230.237.203:4444").unwrap();
    let localstore = memory::empty().await.unwrap();
    let wallet = WalletBuilder::new()
        .mint_url(mint_url)
        .unit(CurrencyUnit::Sat)
        .localstore(Arc::new(localstore))
        .seed([0u8; 64])
        .build()
        .unwrap();

    // Step 1: Create quote
    let quote = wallet
        .mint_quote(PaymentMethod::BOLT11, Some(cdk::Amount::from(21u64)), None, None)
        .await
        .unwrap();
    println!("Quote ID: {}", quote.id);
    println!("Initial state: {:?}", quote.state);

    // Wait for auto-payment
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Step 2: Fetch quote state from mint
    let fetched = wallet
        .fetch_mint_quote(&quote.id, Some(PaymentMethod::BOLT11))
        .await;
    match fetched {
        Ok(q) => {
            println!("Fetched state: {:?} (to_string: {})", q.state, q.state);

            if q.state.to_string() == "PAID" {
                println!("Attempting mint...");
                let proofs = wallet.mint(&quote.id, SplitTarget::None, None).await;
                match proofs {
                    Ok(p) => {
                        println!("Minted {} proofs!", p.len());
                        for proof in &p {
                            println!("  amount={} keyset={}", proof.amount, proof.keyset_id);
                        }
                    }
                    Err(e) => println!("Mint error: {e}"),
                }
            }
        }
        Err(e) => println!("Fetch error: {e}"),
    }
}

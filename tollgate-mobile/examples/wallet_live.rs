//! Live proof: mint sats, hold them, spend them, receive them back.
//!
//! Against a **real** `mintd` on :3338 — real keysets, real blind signatures, a real
//! spent-proof set. Nothing here is a stub.
//!
//! Run:
//!   TOLLGATE_MINT=http://127.0.0.1:3338 \
//!     cargo run -p tollgate-mobile --example wallet_live
//!
//! What it proves: the wallet mints ecash, holds it, spends an exact amount, and can
//! receive that token back. What it does not prove: a purchase from a gateway priced
//! in another unit.

use std::time::Duration;

use tollgate_mobile::wallet::Wallet;

#[tokio::main]
async fn main() {
    let mint = std::env::var("TOLLGATE_MINT").unwrap_or_else(|_| "http://127.0.0.1:3338".into());
    let mint = mint.trim_end_matches('/').to_string();
    let dir = std::env::temp_dir().join(format!("wallet-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let db = dir.join("wallet.sqlite");

    let mut failures = 0u32;
    println!("=== mint at {mint} ===");

    let wallet = match Wallet::open(&db, [0x11u8; 64]).await {
        Ok(w) => w,
        Err(e) => {
            println!("FAIL: could not open the wallet: {e}");
            std::process::exit(1);
        }
    };
    println!("wallet   {}", wallet.path().display());

    // ---- mint some sats ---------------------------------------------------
    let amount = 64u64;
    match wallet.mint(&mint, "sat", amount).await {
        Ok(req) => {
            println!("quote    {}", req.quote);
            let inv = &req.invoice;
            println!("invoice  {}…", &inv[..inv.len().min(40)]);

            match wallet
                .mint_settle(&mint, "sat", &req.quote, Duration::from_secs(20))
                .await
            {
                Ok(minted) => {
                    println!("minted   {minted} sat");
                    if minted < amount {
                        println!("FAIL: minted {minted}, asked for {amount}");
                        failures += 1;
                    }
                }
                Err(e) => {
                    println!("FAIL: settling the quote: {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            println!("FAIL: asking for a quote: {e}");
            std::process::exit(1);
        }
    }

    // ---- the balance reflects it -----------------------------------------
    let held = wallet.balance().await;
    println!("holdings {held:?}");
    let sat = held
        .iter()
        .find(|h| h.unit == "sat")
        .map(|h| h.amount)
        .unwrap_or(0);
    if sat < amount {
        println!("FAIL: expected at least {amount} sat on hand, saw {sat}");
        failures += 1;
    }

    // ---- spend exactly, and take it back ---------------------------------
    let spend = 32u64;
    match wallet.spend(&mint, "sat", spend).await {
        Ok(token) => {
            println!("spent    {spend} sat");
            println!("token    {}…", &token[..token.len().min(40)]);

            match wallet.receive(&token).await {
                Ok(back) => {
                    println!("receive  {} {} from {}", back.amount, back.unit, back.mint);
                    if back.mint != mint {
                        println!("FAIL: the token named {}, not {mint}", back.mint);
                        failures += 1;
                    }
                    if back.amount != spend {
                        println!("FAIL: received {} back, spent {spend}", back.amount);
                        failures += 1;
                    }
                }
                Err(e) => {
                    println!("FAIL: a token we just made cannot be received: {e}");
                    failures += 1;
                }
            }
        }
        Err(e) => {
            println!("FAIL: spending {spend} sat: {e}");
            failures += 1;
        }
    }

    // ---- overspending is refused ------------------------------------------
    match wallet.spend(&mint, "sat", 10_000_000).await {
        Err(e) => println!("refused  overspend, as designed: {e}"),
        Ok(_) => {
            println!("FAIL: spent far more than the balance held");
            failures += 1;
        }
    }

    // ---- an issuer we hold nothing at --------------------------------------
    match wallet.spend("https://mint.example", "sat", 1).await {
        Err(e) => println!("refused  a mint we hold nothing at: {e}"),
        Ok(_) => {
            println!("FAIL: spent at a mint the wallet never minted with");
            failures += 1;
        }
    }

    // ---- spending nothing --------------------------------------------------
    match wallet.spend(&mint, "sat", 0).await {
        Err(e) => println!("refused  zero, as designed: {e}"),
        Ok(_) => {
            println!("FAIL: spending nothing succeeded");
            failures += 1;
        }
    }

    let _ = std::fs::remove_dir_all(&dir);

    if failures == 0 {
        println!("\nPASS — mint, hold, spend, receive, and refuse");
    } else {
        println!("\n{failures} FAILURE(S)");
        std::process::exit(1);
    }
}

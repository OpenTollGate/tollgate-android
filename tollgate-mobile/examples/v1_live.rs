//! Drive a live v1 gateway with the real [`v1::Gateway`].
//!
//! This is not a mock. It talks HTTP to whatever `--base` names, and the point
//! of it is to prove the client's shapes against a running
//! `tollgate-module-basic-go`, whose replies it cannot influence.
//!
//! ```sh
//! cargo run -p tollgate-mobile --example v1_live -- http://127.0.0.1:2121 \
//!     http://127.0.0.1:35213
//! ```
//!
//! Arguments: `--base` (gateway, :2121), `--mint` (a mint it advertises),
//! `--amount` (sats to quote). Nothing is paid: the invoice is requested and
//! polled, and the gate opening is the *gateway's* business (it opens one when
//! its mint says the quote is issued).

use std::time::Duration;

use tollgate_mobile::v1::Gateway;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| "http://127.0.0.1:2121".into());
    let mint = args.next().unwrap_or_else(|| "http://127.0.0.1:35213".into());
    let amount: u64 = args
        .next()
        .unwrap_or_else(|| "100".into())
        .parse()
        .unwrap_or(100);

    let gw = Gateway::new(&base, Duration::from_secs(20))?;
    println!("gateway {}", gw.base());

    // 1. Who is this, and does it identify us?
    let mac = gw.whoami().await?;
    println!("whoami: {:?}", mac);

    // 2. What does it charge, and in what?
    let ad = gw.detect().await?;
    println!(
        "advertised: metric={} step_size={} pubkey={} tips={:?}",
        ad.metric, ad.step_size, ad.pubkey, ad.tips
    );
    for p in &ad.prices {
        println!(
            "  price: {} {} per {} {} from {} (min {} steps)",
            p.price_per_step, p.unit, ad.step_size, ad.metric, p.mint_url, p.min_steps
        );
    }

    // A price the gateway did not advertise is not ours to assume.
    let quoted_mint = match ad.prices.iter().find(|p| {
        p.mint_url.trim_end_matches('/') == mint.trim_end_matches('/')
    }) {
        Some(p) => p.mint_url.clone(),
        None => {
            println!(
                "mint {mint} is not among the {} advertised; not quoting against it",
                ad.prices.len()
            );
            return Ok(());
        }
    };

    // 3. Ask for a quote and an invoice.
    let invoice = gw.request_invoice(amount, &quoted_mint).await?;
    println!(
        "quote {} for {} sat, state={}, expiry={}",
        invoice.quote, invoice.amount_sat, invoice.state, invoice.expiry
    );
    println!("bolt11 {}", invoice.invoice);

    // 4. Poll it. A real payment would move this to PAID/ISSUED on the mint's
    //    word; here we only observe, and stop after a bound.
    for attempt in 1..=10 {
        let status = gw.invoice_status(&invoice.quote).await?;
        println!(
            "poll {attempt}: state={} access_granted={} allotment={}",
            status.state, status.access_granted, status.allotment
        );
        if status.access_granted {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }

    // 5. What does it think we have?
    let balance = gw.balance().await?;
    println!(
        "balance: active={} usage={} allotment={} remaining={} ({})",
        balance.session_active,
        balance.usage,
        balance.allotment,
        balance.remaining,
        if balance.metric.is_empty() {
            &ad.metric
        } else {
            &balance.metric
        }
    );

    Ok(())
}

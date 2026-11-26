//! Prove the adapter picks the right protocol and refuses to sell on v2.
//!
//! ```sh
//! cargo run -p tollgate-mobile --example adapter_live -- <v1-base> <v2-endpoint>
//! ```
//!
//! v1 is expected to answer with a price and be purchasable. v2 is expected to
//! answer with terms, no price, and every money operation refused *with a
//! reason* — not silently absent.

use std::time::Duration;

use tollgate_mobile::gateway::{probe, Protocol};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let v1 = args.next().unwrap_or_else(|| "http://127.0.0.1:2121".into());
    let v2 = args.next().unwrap_or_else(|| "127.0.0.1:4747".into());

    for endpoint in [&v1, &v2] {
        println!("=== probing {endpoint} ===");
        match probe(endpoint, Duration::from_secs(6)).await {
            Ok(gw) => {
                let view = gw.describe().await?;
                println!("  protocol:        {}", view.protocol.as_str());
                println!("  metric:          {}", view.metric);
                println!("  device_mac:      {:?}", view.device_mac);
                println!("  pubkey:          {:?}", view.pubkey);
                println!("  purchasable:     {}", view.purchasable());
                println!("  prices ({}):", view.prices.len());
                for p in &view.prices {
                    println!(
                        "    {} {} per {} {} from {} (min {} steps)",
                        p.price_per_step,
                        p.unit,
                        p.step_size,
                        p.metric,
                        p.mint_url,
                        p.min_steps
                    );
                }
                if let Some(why) = &view.unavailable {
                    println!("  unavailable:     {why}");
                }

                // The adapter's rule, exercised rather than asserted.
                match gw.request_invoice(21, "http://127.0.0.1:38103").await {
                    Ok(inv) => println!("  invoice:         {} ({})", inv.quote, inv.state),
                    Err(e) => println!("  invoice refused: {e}"),
                }
                assert_eq!(
                    view.protocol == Protocol::V2,
                    !view.purchasable(),
                    "a v2 peer must never be purchasable"
                );
            }
            Err(e) => println!("  no answer: {e}"),
        }
    }
    Ok(())
}

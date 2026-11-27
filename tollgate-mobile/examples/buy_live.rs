//! Buy from a live exit, over the wire, by signing its challenge.
//!
//! This is the whole purchase: fetch the terms, take the challenge from
//! `X-TollGate-Challenge`, sign its canonical bytes with a key, and POST the
//! claim. Nothing here is simulated — the exit verifies the signature with real
//! schnorr arithmetic, derives an address from the buyer key, and either admits
//! the tunnel or refuses.
//!
//! Usage: `cargo run -p tollgate-mobile --example buy_live -- <endpoint> [bind]`
//!
//! `bind` is the address the exit should observe for this caller. On a real exit
//! that comes from the FIPS transport and is not something a buyer supplies; on a
//! loopback harness it is passed with the exit's own test-only `?bind=` hook, so
//! the harness cannot silently pretend to have a tunnel.

use std::time::Duration;

use secp256k1::{Keypair, Secp256k1, SecretKey};
use tollgate_mobile::claim::{Challenge, Claim};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:2121".into());
    let bind = args.next();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // ---- terms, and the challenge beside them ------------------------------
    let resp = http.get(format!("{endpoint}/")).send().await?;
    let status = resp.status();
    if !status.is_success() {
        println!("FAIL: GET / answered {status}");
        return Ok(());
    }
    let challenge_b64 = resp
        .headers()
        .get("X-TollGate-Challenge")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let advert = resp.text().await?;

    let Some(b64) = challenge_b64 else {
        println!("FAIL: no X-TollGate-Challenge header, so there is nothing to sign");
        return Ok(());
    };
    let challenge_bytes = base64_decode(&b64)?;
    let challenge: Challenge = serde_json::from_slice(&challenge_bytes)?;
    println!(
        "terms    exit={} price={} sat mode={} expires={}",
        hex(&challenge.exit_key[..8]),
        challenge.price_sats,
        challenge.mode,
        challenge.expires_at
    );
    println!("digest   {}", hex(&challenge.sign_digest()?));

    // ---- a buyer key, signing the challenge --------------------------------
    //
    // A fixed secret so the run is reproducible. On a phone this is the device
    // identity the exit will derive an address from; here it only needs to be a
    // key that can sign.
    let secp = Secp256k1::signing_only();
    let secret = SecretKey::from_slice(&[0x42u8; 32])?;
    let keypair = Keypair::from_secret_key(&secp, &secret);
    let (xonly, _parity) = keypair.x_only_public_key();

    let digest = challenge.sign_digest()?;
    // `sign_schnorr` takes the 32-byte message directly in this secp256k1, and
    // hashes it to a BIP340 nonce internally - which is what the exit verifies
    // against, so passing the raw digest is correct here.
    let sig = secp.sign_schnorr(&digest, &keypair);

    let claim = Claim {
        challenge,
        buyer: xonly.serialize(),
        sig: hex(&sig.serialize()),
    };
    println!("buyer    {}", hex(&claim.buyer));

    // ---- the purchase ------------------------------------------------------
    let body = serde_json::json!({
        "claim": claim,
        "ttl_seconds": 300,
        "byte_budget": 60000,
    });

    println!("sending  {}", body);
    let url = match &bind {
        Some(b) => format!("{endpoint}/?bind={b}"),
        None => format!("{endpoint}/"),
    };
    let resp = http
        .post(&url)
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    println!("POST /   {status}");
    println!("body     {text}");

    if status.is_success() {
        // The grant is a kind-1022 event, which is the module's own purchase
        // contract: the app classifies on `kind` and reads `allotment`, `metric`
        // and `device-identifier`. There is no `access_granted` field here - that
        // belongs to the invoice flow, where the gate opens when a quote is paid.
        // Asserting the wrong shape would have reported a successful purchase as
        // a failure.
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                let kind = v.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
                let allotment = v.get("allotment").and_then(|a| a.as_u64()).unwrap_or(0);
                let metric = v.get("metric").and_then(|m| m.as_str()).unwrap_or("");
                let device = v
                    .get("device-identifier")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");
                if kind == 1022 && allotment > 0 && !device.is_empty() {
                    println!("PASS: the exit granted {allotment} {metric} to {device}");
                    if device == hex(&claim.buyer[..]).as_str() {
                        println!("NOTE: device-identifier echoed the key, not an address");
                    }
                } else {
                    println!(
                        "FAIL: kind={kind} allotment={allotment} metric={metric:?} device={device:?} \
                         - not a grant the app could read"
                    );
                }
            }
            Err(e) => println!("FAIL: the reply is not JSON: {e}"),
        }
    } else {
        println!("REFUSED (not a defect unless the reason is wrong)");
    }

    // The advert is echoed only so the run shows what was on offer; it is not
    // parsed here because `v1` already covers that.
    println!("advert   {} bytes", advert.len());
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Minimal base64 for the `X-TollGate-Challenge` header, so the example needs no
/// extra dependency beyond what the crate already carries.
fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = TABLE
            .iter()
            .position(|&t| t == c)
            .ok_or_else(|| format!("not base64: {}", c as char))? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

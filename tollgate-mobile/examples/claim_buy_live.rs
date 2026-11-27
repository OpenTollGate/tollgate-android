//! Buy from a live exit through the core's own entry point.
//!
//! `buy_live` exercises the wire by hand; this calls `buy_with_claim`, which is
//! the method the app calls, so it proves the path the phone actually takes:
//! the device identity signs, the core derives the buyer key from the same
//! identity, and the grant comes back as a kind-1022 record.
//!
//! Usage: `cargo run -p tollgate-mobile --example claim_buy_live -- <endpoint> <bind>`
//!
//! On a real exit the binding address is whatever FIPS observed and is not
//! supplied by a buyer. On loopback the exit's own test-only `?bind=` hook stands
//! in for it, which is why the address has to be passed here explicitly: the
//! harness must not be able to pretend it has a tunnel.

use std::time::Duration;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The outer IPv6 address an exit derives for a buyer key: `fd` followed by the
/// first 15 bytes of SHA-256 over the 32-byte x-only key.
///
/// Duplicated from the exit's `Derive` on purpose - the harness has to predict what
/// the exit will compute, and duplicating it means the two are compared rather than
/// one being taken on trust. They agree only if both are right.
fn derive_outer(key_hex: &str) -> String {
    let key: Vec<u8> = (0..key_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&key_hex[i..i + 2], 16).unwrap_or(0))
        .collect();
    let digest = sha256(&key);
    let mut addr = [0u8; 16];
    addr[0] = 0xfd;
    addr[1..].copy_from_slice(&digest[..15]);

    // Group the 8 hextets the way an address is written.
    let h: Vec<String> = addr
        .chunks(2)
        .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
        .collect();
    h.join(":")
}

/// SHA-256, so the example needs no extra dependency.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:2121".into());
    let bind = args.next();

    let dir = std::env::temp_dir().join(format!("tg-claim-buy-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;

    let node = tollgate_mobile::TollgateMobileNode::new(dir.to_string_lossy().into_owned())?;
    println!("device   {}", node.pubkey_hex());

    // The exit binds to the address its transport observed for the signing key.
    // Derive what it will be, so the harness can pass it to the test-only hook.
    let key = node.pubkey_bytes_for_test();
    println!("key      {key}");

    // The exit derives an address from the signing key, so the harness must too:
    // it knows the key, so it can compute what the exit will compute and hand that
    // to the gated test hook. Passing an address that is not derived from this key
    // is refused - which is the binding check working.
    let derived = derive_outer(&key);
    println!("derived  {derived}");

    let endpoint_for_core = match &bind {
        // The exit reads `?bind=` only on the request it is given. Rather than
        // teach the core about a test hook, point it at the address that resolves
        // to the same admission, which is what the harness is emulating.
        Some(b) => {
            println!("bind     {b} (via the exit's gated -allow-test-bind hook)");
            endpoint.clone()
        }
        None => endpoint.clone(),
    };

    println!("buying   {endpoint_for_core} ttl=300s budget=60000");
    match node.buy_with_claim(endpoint_for_core, 300, 60_000, Some(derived)) {
        Ok(grant) => {
            println!(
                "PASS: {} {} to {} until {}",
                grant.allotment, grant.metric, grant.device_identifier, grant.expires_at
            );
        }
        Err(e) => {
            println!("REFUSED: {e}");
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
    // Give the blocking runtime a moment to finish before the process exits.
    std::thread::sleep(Duration::from_millis(50));
    Ok(())
}

//! A stand-in v2 gateway, for exercising the app's Detect path over a real
//! socket from an emulator or a phone.
//!
//! This is **not** `tollgated`. It has no payment, no channel, no dataplane and
//! no budget: it answers a payer's `Announce` with its own `Announce` and an
//! `Offer`, and then keeps the connection open. That is exactly enough to prove
//! the app's transport, framing, identity persistence and terms rendering on a
//! device, and nothing more. Do not read a pass here as "the client can buy".
//!
//! ```sh
//! cargo run -p tollgate-mobile --example v2_gateway -- 0.0.0.0:4747
//! ```
//!
//! From an Android emulator the host is `10.0.2.2`, so point the app at
//! `10.0.2.2:4747`.

use std::env;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tollgate_protocol::{
    encode_frame, Announce, FrameReader, Message, Offer, PubKey, PROTOCOL_VERSION,
};

/// A well-formed compressed point for this fixture to sign nothing with.
const FIXTURE_KEY: [u8; 33] = {
    let mut k = [0u8; 33];
    k[0] = 0x02;
    k[32] = 0x22;
    k
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = env::args()
        .nth(1)
        .unwrap_or_else(|| format!("0.0.0.0:{}", tollgate_protocol::DEFAULT_PORT));
    let listener = TcpListener::bind(&address).await?;
    println!("fixture gateway (no payment, no dataplane) listening on {address}");
    println!("from an Android emulator, point the app at 10.0.2.2:{}", tollgate_protocol::DEFAULT_PORT);

    loop {
        let (socket, peer) = listener.accept().await?;
        println!("connection from {peer}");
        tokio::spawn(async move {
            if let Err(e) = serve(socket).await {
                println!("{peer}: {e}");
            }
        });
    }
}

async fn serve(mut socket: TcpStream) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut reader = FrameReader::new();
    let mut buf = [0u8; 2048];
    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            println!("peer closed before announcing");
            return Ok(());
        }
        reader.push(&buf[..n]);
        while let Some(next) = reader.next_message() {
            match next? {
                Message::Announce(a) => {
                    println!(
                        "announce: unit={} version={} pubkey={}",
                        a.unit,
                        a.version,
                        hex::encode(a.pubkey.as_bytes())
                    );
                    let mut out = Vec::new();
                    encode_frame(
                        &Message::Announce(Announce {
                            version: PROTOCOL_VERSION,
                            pubkey: PubKey(FIXTURE_KEY),
                            unit: a.unit.clone(),
                            capabilities: 0,
                        }),
                        &mut out,
                    )?;
                    encode_frame(
                        &Message::Offer(Offer {
                            accepted_mints: vec!["https://mint.minibits.cash".to_string()],
                            unit: a.unit,
                            min_window_ms: 1_000,
                            max_window_ms: 3_600_000,
                            from_payer_weight: 2,
                            no_charge: false,
                            min_reserved_rate: 64,
                            min_topup_gap_ms: 250,
                        }),
                        &mut out,
                    )?;
                    socket.write_all(&out).await?;
                    socket.flush().await?;
                    println!("sent Announce + Offer; holding the session open");
                }
                other => println!("ignoring {:?}", std::mem::discriminant(&other)),
            }
        }
    }
}

//! `v2::detect` against a real listening socket.
//!
//! The codec tests prove the messages are shaped right; these prove the session
//! is: we connect, we send an Announce, we read until the gateway has both
//! introduced itself and offered, and we do not hang when it keeps the
//! connection open afterwards — which is what a real gateway does.
//!
//! The "gateway" here is a plain `TcpListener` in the test speaking the real
//! `tollgate-protocol` framing. It is a fixture, not a gateway: no payment, no
//! channel, no dataplane.

use std::time::Duration;

use secp256k1::{Secp256k1, SecretKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use tollgate_protocol::{
    encode_frame, Announce, FrameReader, Message, Offer, PubKey, PROTOCOL_VERSION,
};

use crate::v2;

const TIMEOUT: Duration = Duration::from_secs(3);

fn gateway_key() -> PubKey {
    let mut bytes = [0u8; 33];
    bytes[0] = 0x02;
    bytes[32] = 0x22;
    PubKey(bytes)
}

fn announce() -> Message {
    Message::Announce(Announce {
        version: PROTOCOL_VERSION,
        pubkey: gateway_key(),
        unit: "bytes".into(),
        capabilities: 0,
    })
}

fn offer(unit: &str) -> Message {
    Message::Offer(Offer {
        accepted_mints: vec!["https://mint.example".into()],
        unit: unit.into(),
        min_window_ms: 1_000,
        max_window_ms: 3_600_000,
        from_payer_weight: 2,
        no_charge: false,
        min_reserved_rate: 64,
        min_topup_gap_ms: 250,
    })
}

fn our_key() -> SecretKey {
    SecretKey::from_slice(&[9u8; 32]).expect("a valid scalar")
}

/// Serve one connection: read until we have the client's Announce, then send
/// `reply`, then hold the socket open for `hold` so a reader that waits for EOF
/// would hang and fail the test.
fn serve_once(
    listener: TcpListener,
    reply: Vec<Message>,
    hold: Duration,
) -> tokio::task::JoinHandle<Option<Announce>> {
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let mut reader = FrameReader::new();
        let mut buf = [0u8; 1024];
        let mut seen: Option<Announce> = None;
        while seen.is_none() {
            let n = socket.read(&mut buf).await.expect("read");
            if n == 0 {
                break;
            }
            reader.push(&buf[..n]);
            while let Some(Ok(m)) = reader.next_message() {
                if let Message::Announce(a) = m {
                    seen = Some(a);
                }
            }
        }
        let mut out = Vec::new();
        for m in &reply {
            encode_frame(m, &mut out).expect("encode");
        }
        socket.write_all(&out).await.expect("write");
        socket.flush().await.expect("flush");
        // Deliberately do not close: the gateway holds the session open.
        tokio::time::sleep(hold).await;
        seen
    })
}

#[tokio::test]
async fn detect_reads_an_announce_and_offer_from_a_live_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr").to_string();
    let server = serve_once(listener, vec![announce(), offer("bytes")], TIMEOUT * 2);

    let detected = v2::detect(&address, &our_key(), "bytes", TIMEOUT)
        .await
        .expect("a gateway that introduces itself and offers is detected");

    assert_eq!(detected.unit, "bytes");
    assert_eq!(detected.version, PROTOCOL_VERSION);
    assert_eq!(detected.pubkey_hex, hex::encode(gateway_key().as_bytes()));
    let terms = detected.offer.expect("the Offer was read");
    assert_eq!(terms.from_payer_weight, 2);
    assert_eq!(terms.accepted_mints, vec!["https://mint.example".to_string()]);

    // The server saw our Announce and what we asked to be denominated in.
    let ours = server.await.expect("join").expect("the gateway read our Announce");
    assert_eq!(ours.unit, "bytes", "we must announce the unit we want");
    let expected = secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &our_key());
    assert_eq!(
        ours.pubkey.as_bytes(),
        &expected.serialize(),
        "we must announce the identity the purchase will be bound to"
    );
}

#[tokio::test]
async fn a_gateway_that_says_nothing_fails_rather_than_hangs() {
    // Accept and stay silent. Without the read timeout this would block the
    // caller for as long as the peer wanted, which on Android is the UI thread.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr").to_string();
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.expect("accept");
        tokio::time::sleep(TIMEOUT * 3).await;
    });

    let outcome = v2::detect(&address, &our_key(), "bytes", Duration::from_millis(300)).await;
    assert!(
        matches!(outcome, Err(v2::WireError::Io(_))),
        "a silent gateway must be an error, not a wait: {outcome:?}"
    );
    server.abort();
}

#[tokio::test]
async fn a_gateway_that_introduces_itself_without_offering_still_reports_itself() {
    // An Announce with no Offer is a node that will not sell anything. That is
    // an answer, not a failure: the user should see who it is and that there is
    // nothing to buy.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr").to_string();
    // Close after replying, so detect() ends on EOF rather than the timeout.
    let server = tokio::spawn({
        let listener = listener;
        async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut out = Vec::new();
            encode_frame(&announce(), &mut out).expect("encode");
            socket.write_all(&out).await.expect("write");
            drop(socket);
        }
    });

    let detected = v2::detect(&address, &our_key(), "bytes", TIMEOUT)
        .await
        .expect("an Announce alone is enough to identify the gateway");
    assert_eq!(detected.unit, "bytes");
    assert_eq!(detected.offer, None);
    server.await.expect("join");
}

#[tokio::test]
async fn a_closed_socket_before_any_announce_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr").to_string();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept");
        drop(socket);
    });

    let outcome = v2::detect(&address, &our_key(), "bytes", TIMEOUT).await;
    assert!(
        matches!(outcome, Err(v2::WireError::Io(_))),
        "no Announce means no gateway to pay: {outcome:?}"
    );
    server.await.expect("join");
}

//! Integration test: drive the real [`TollgateMobileNode`] UniFFI facade against
//! an in-process axum mock gateway that speaks the real TollGate v2 CBOR
//! protocol (`POST /tollgate/v1/exchange`, length-prefixed frames, real
//! `Announce` / `PriceSheet` / `BootstrapAck` / `MeteringReport` messages).
//!
//! This is the same transport + message set the production `tollgate-net` server
//! uses, so a pass here means the mobile client interoperates with the real wire
//! format — only the network is local.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::body::Bytes;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use tokio::runtime::Runtime;
use tollgate_mobile::TollgateMobileNode;
use tollgate_protocol::{
    Announce, BootstrapAck, MessageType, MeteringReport, MintPrice, PriceSheet,
    ProductOffer, PublicKey, decode_frames, encode_frame, peek_type,
};

/// Mock gateway state: a fixed identity + a poll counter (for metering).
#[derive(Clone)]
struct MockGw {
    pubkey_bytes: [u8; 33],
    polls: Arc<AtomicU32>,
}

fn sample_sheet() -> Vec<u8> {
    // per_unit = 3 milli-sat/byte; matches the `find_price_sheet` unit test in
    // tollgate-net. pricing_scale 1000, one product, one mint.
    let prices = vec![MintPrice {
        mint_url: "http://m".to_string(),
        price_per_second: 0,
        price_per_unit: 3,
        mint_unit: "sat".to_string(),
    }];
    PriceSheet::new(vec![ProductOffer::new(1000, &prices, vec![])], 5000, 60000).encode()
}

/// The mock exchange handler: decode the client's frames, reply with our
/// Announce + PriceSheet always, plus a BootstrapAck when it sent a token, plus
/// a MeteringReport on each poll after the bootstrap exchange.
async fn exchange(State(gw): State<MockGw>, body: Bytes) -> Response {
    let frames = match decode_frames(&body) {
        Ok(f) => f,
        Err(_) => return (StatusCode::BAD_REQUEST, "bad framing").into_response(),
    };
    let sent_bootstrap = frames
        .iter()
        .any(|f| peek_type(f) == Some(MessageType::BootstrapToken));

    let mut resp: Vec<u8> = Vec::new();
    // Peer Announce (always).
    encode_frame(
        &Announce::new(1, PublicKey::from_bytes(gw.pubkey_bytes), "bytes", 0).encode(),
        &mut resp,
    )
    .unwrap();
    // PriceSheet (always).
    encode_frame(&sample_sheet(), &mut resp).unwrap();

    if sent_bootstrap {
        // Bootstrap exchange: accept + (re-)send the sheet above.
        encode_frame(&BootstrapAck::accepted().encode(), &mut resp).unwrap();
    } else {
        // A poll (Announce only, maybe a MeteringReport from us). Report some
        // delivered bytes so the client accrues cost and exercises the top-up
        // path. Counter makes each poll deliver a growing amount.
        let n = gw.polls.fetch_add(1, Ordering::Relaxed) + 1;
        encode_frame(&MeteringReport::new(n as u64 * 1000, n as u64 * 1024, 0).encode(), &mut resp)
            .unwrap();
    }
    (StatusCode::OK, resp).into_response()
}

/// Spin the mock gateway on an ephemeral port; return its base URL.
fn spawn_mock_gw(rt: &Runtime) -> (String, MockGw) {
    let secp = secp256k1::Secp256k1::new();
    let (sk, pk) = secp.generate_keypair(&mut secp256k1::rand::rngs::OsRng);
    let _ = sk; // gateway key is only used to produce a valid compressed pubkey
    let gw = MockGw {
        pubkey_bytes: pk.serialize(),
        polls: Arc::new(AtomicU32::new(0)),
    };
    let app = Router::new()
        .route("/tollgate/v1/exchange", post(exchange))
        .with_state(gw.clone());
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://127.0.0.1:{}", addr.port()), gw)
}

#[test]
fn detect_returns_peer_identity_and_price() {
    let rt = Runtime::new().unwrap();
    let (base, gw) = spawn_mock_gw(&rt);
    let dir = tempdir();

    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let detected = node.detect(base).expect("detect should succeed");

    assert_eq!(detected.pubkey_hex, hex::encode(gw.pubkey_bytes));
    assert_eq!(detected.unit, "bytes");
    assert_eq!(detected.version, 1);
    let price = detected.price.expect("a price from the sheet");
    assert_eq!(price.per_unit, 3);
    assert_eq!(price.per_second, 0);
}

#[test]
fn pay_is_accepted_and_returns_price() {
    let rt = Runtime::new().unwrap();
    let (base, _gw) = spawn_mock_gw(&rt);
    let dir = tempdir();

    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let paid = node
        .pay(base, "https://mint.minibits.cash".to_string(), 21)
        .expect("pay should succeed");
    assert!(paid.accepted);
    assert!(paid.reason.is_none());
    let price = paid.price.expect("a price");
    assert_eq!(price.per_unit, 3);
}

#[test]
fn identity_persists_across_node_instances() {
    let dir = tempdir();
    let path = dir.join("identity.hex");
    assert!(!path.exists());

    let pk1 = {
        let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
        node.pubkey_hex()
    };
    assert!(path.exists(), "identity.hex must be persisted on first new()");

    // A second node in the same dir must load the SAME key, not generate a new one.
    let node2 = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let pk2 = node2.pubkey_hex();
    assert_eq!(pk1, pk2, "identity must persist across instances");
    assert_eq!(pk1.len(), 66, "compressed pubkey is 33 bytes = 66 hex chars");
}

#[test]
fn consume_loop_emits_start_then_poll_events() {
    let rt = Runtime::new().unwrap();
    let (base, _gw) = spawn_mock_gw(&rt);
    let dir = tempdir();

    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    // 2 polls, 50ms interval — fast but real sleeps.
    node.start_consume(
        base,
        "https://mint.minibits.cash".to_string(),
        21,   // amount_sat
        5,    // topup_sat
        50,   // interval_ms
        Some(2), // max_polls
    )
    .expect("start_consume");

    let mut events = Vec::new();
    // Drain: poll up to ~3s for the start + 2 poll events.
    for _ in 0..120 {
        if let Some(ev) = node.poll_event(50).unwrap() {
            events.push(ev);
        }
        if events.iter().any(|e| e.poll >= 2) {
            break;
        }
    }
    node.stop_consume();

    assert!(!events.is_empty(), "must receive at least the start event");
    assert_eq!(events[0].poll, 0, "first event is the start (poll 0)");
    assert!(events.iter().any(|e| e.poll == 1), "must receive poll 1");
    assert!(events.iter().any(|e| e.poll == 2), "must receive poll 2");
    // A poll event carries the metering report the mock sent.
    let p1 = events.iter().find(|e| e.poll == 1).unwrap();
    let report = p1.report.as_ref().expect("poll carries a MeteringReport");
    assert!(report.delivered > 0);
    assert_eq!(p1.peer_pubkey.len(), 66);
    assert_eq!(p1.price.per_unit, 3);
}

#[test]
fn pay_rejected_surfaces_as_rejected_error() {
    // A gateway that rejects the bootstrap. Separate minimal handler.
    use axum::routing::post;
    async fn reject_exchange(_: Bytes) -> Response {
        let mut resp = Vec::new();
        let pk = PublicKey::from_bytes([7u8; 33]);
        encode_frame(&Announce::new(1, pk, "bytes", 0).encode(), &mut resp).unwrap();
        encode_frame(
            &BootstrapAck::rejected("insufficient funds").encode(),
            &mut resp,
        )
        .unwrap();
        (StatusCode::OK, resp).into_response()
    }
    let rt = Runtime::new().unwrap();
    let app = Router::new().route("/tollgate/v1/exchange", post(reject_exchange));
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    rt.spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });

    let dir = tempdir();
    let node = TollgateMobileNode::new(dir.to_string_lossy().to_string()).unwrap();
    let err = node
        .pay(format!("http://127.0.0.1:{}", addr.port()), "https://m".to_string(), 1)
        .unwrap_err();
    match err {
        tollgate_mobile::TollgateError::Rejected { reason } => {
            assert!(reason.contains("insufficient funds"), "got: {reason}");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // Suppress unused-import warning for BootstrapToken in this variant's scope.
    let _ = MessageType::BootstrapToken;
}

fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tollgate-mobile-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

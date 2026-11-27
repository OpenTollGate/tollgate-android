//! Integration test: drive the real [`TollgateMobileNode`] facade against a
//! plain TCP socket speaking the real TollGate v2 CBOR framing.
//!
//! Protocol v2 has no HTTP endpoint. A gateway listens on raw TCP (4747 by
//! default) and the conversation is a session: the payer sends an `Announce`,
//! the gateway answers with its own `Announce` and an `Offer`. These tests use
//! the real `tollgate-protocol` codec for both directions, so a pass means the
//! facade interoperates with the real wire format — only the socket is local.
//!
//! What this does *not* prove: no payment, no Spilman channel, no dataplane, and
//! no real `tollgated` on the other end. It proves the read side of the session.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

use tollgate_mobile::TollgateMobileNode;
use tollgate_protocol::{
    encode_frame, Announce, FrameReader, Message, Offer, PubKey, PROTOCOL_VERSION,
};

/// The gateway's identity in these tests: a well-formed compressed point, not a
/// key anything signs with.
const GATEWAY_KEY: [u8; 33] = {
    let mut k = [0u8; 33];
    k[0] = 0x02;
    k[32] = 0x22;
    k
};

fn announce(unit: &str) -> Message {
    Message::Announce(Announce {
        version: PROTOCOL_VERSION,
        pubkey: PubKey(GATEWAY_KEY),
        unit: unit.to_string(),
        capabilities: 0,
    })
}

fn offer(unit: &str) -> Message {
    Message::Offer(Offer {
        accepted_mints: vec!["https://mint.example".to_string()],
        unit: unit.to_string(),
        min_window_ms: 1_000,
        max_window_ms: 3_600_000,
        from_payer_weight: 2,
        no_charge: false,
        min_reserved_rate: 64,
        min_topup_gap_ms: 250,
    })
}

/// Read one connection until the peer's `Announce` arrives.
async fn read_announce(socket: &mut TcpStream) -> Option<Announce> {
    let mut reader = FrameReader::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        reader.push(&buf[..n]);
        while let Some(Ok(m)) = reader.next_message() {
            if let Message::Announce(a) = m {
                return Some(a);
            }
        }
    }
}

/// Start a gateway that answers every connection with `reply`, holding the
/// socket open afterwards — which is what a real gateway does, and what a reader
/// that waited for EOF would hang on.
fn spawn_gateway(rt: &Runtime, reply: Vec<Message>) -> u16 {
    let listener = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let reply = reply.clone();
            tokio::spawn(async move {
                // A connection that never announces is not interesting here.
                let _ = read_announce(&mut socket).await;
                let mut out = Vec::new();
                for m in &reply {
                    encode_frame(m, &mut out).expect("encode");
                }
                let _ = socket.write_all(&out).await;
                let _ = socket.flush().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            });
        }
    });
    port
}

/// Start a gateway that accepts, reads our Announce, and answers with bytes that
/// carry no valid framing. It has *answered* — this is a protocol fault, not a
/// silent peer — so the facade must not call it unreachable.
fn spawn_garbage_gateway(rt: &Runtime) -> u16 {
    let listener = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            // Consume the payer's Announce so this is a real request/response, then
            // send ONE complete frame whose body cannot decode. The length prefix is
            // a little-endian u16 (frame.rs), so `03 00` declares a 3-byte body: the
            // reader sees a whole frame and the DECODER rejects it. (A huge prefix
            // would instead make the reader wait and time out, which is the wrong
            // fault for this test.)
            let mut junk = [0u8; 64];
            let _ = socket.read(&mut junk).await;
            let _ = socket.write_all(&[0x03, 0x00, 0xff, 0xff, 0xff]).await;
            let _ = socket.flush().await;
            drop(socket);
        }
    });
    port
}

/// Start a gateway that accepts and closes without introducing itself.
fn spawn_silent_gateway(rt: &Runtime) -> u16 {
    let listener = rt
        .block_on(TcpListener::bind("127.0.0.1:0"))
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    rt.spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            drop(socket);
        }
    });
    port
}

fn node() -> (Arc<TollgateMobileNode>, std::path::PathBuf) {
    let dir = tempdir();
    let node =
        TollgateMobileNode::new(dir.to_string_lossy().to_string()).expect("node construction");
    (node, dir)
}

#[test]
fn detect_reads_identity_and_terms_over_a_real_socket() {
    let rt = Runtime::new().unwrap();
    let port = spawn_gateway(&rt, vec![announce("bytes"), offer("bytes")]);
    let (node, _dir) = node();

    let detected = node
        .detect(format!("127.0.0.1:{port}"))
        .expect("detect should succeed");

    assert_eq!(detected.protocol, "v2");
    assert_eq!(detected.pubkey.as_deref(), Some(hex::encode(GATEWAY_KEY).as_str()));
    assert_eq!(detected.metric, "bytes");
    // v2 carries no price on the wire, and this build has no wallet to fund the
    // channel a v2 purchase needs, so the facade must say so rather than invent
    // a number: an empty `prices` with a stated reason is the honest answer.
    assert!(detected.prices.is_empty());
    assert!(!detected.purchasable);
    assert!(
        detected.unavailable.is_some(),
        "a v2 peer must state why it cannot be bought from"
    );
}

#[test]
fn a_v1_shaped_url_still_reaches_a_v2_gateway() {
    // Stored settings and the seed list predate the port and still carry a
    // scheme. v2 is raw TCP, so the facade must reduce the URL to host:port
    // rather than hand "http" to the resolver.
    let rt = Runtime::new().unwrap();
    let port = spawn_gateway(&rt, vec![announce("bytes"), offer("bytes")]);
    let (node, _dir) = node();

    let detected = node
        .detect(format!("http://127.0.0.1:{port}/"))
        .expect("a URL-shaped endpoint is accepted");
    assert_eq!(detected.protocol, "v2");
    assert_eq!(detected.metric, "bytes");
}

#[test]
fn a_gateway_that_introduces_itself_without_offering_reports_no_terms() {
    // An Announce with no Offer is a node that will not sell anything: an
    // answer, not a failure. The user should still see who it is.
    let rt = Runtime::new().unwrap();
    let port = spawn_gateway(&rt, vec![announce("bytes")]);
    let (node, _dir) = node();

    let detected = node.detect(format!("127.0.0.1:{port}")).expect("detect");
    assert_eq!(detected.protocol, "v2");
    assert_eq!(detected.metric, "bytes");
    // No Offer means nothing to sell: prices stay empty and the facade says why.
    assert!(detected.prices.is_empty());
    assert!(!detected.purchasable);
}

#[test]
fn a_gateway_that_says_nothing_is_a_network_error() {
    let rt = Runtime::new().unwrap();
    let port = spawn_silent_gateway(&rt);
    let (node, _dir) = node();

    let err = node.detect(format!("127.0.0.1:{port}")).unwrap_err();
    match err {
        tollgate_mobile::TollgateError::Network { .. } => {}
        other => panic!("expected Network, got {other:?}"),
    }
}

#[test]
fn nothing_listening_is_a_network_error() {
    // A closed port is the one case where "the peer is away" is unambiguously true.
    let rt = Runtime::new().unwrap();
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let (node, _dir) = node();

    let err = node.detect(format!("127.0.0.1:{port}")).unwrap_err();
    assert!(
        matches!(err, tollgate_mobile::TollgateError::Network { .. }),
        "a refused connection must be Network, got {err:?}"
    );
}

/// The mapping must not be "everything is Network". A peer that accepts, reads our
/// Announce and then hangs up without saying anything is silent — but a peer that
/// answers in bytes we cannot parse has ANSWERED, and retrying it forever is wrong.
/// This test exists to fail if `discover_error` is ever collapsed to one arm.
#[test]
fn a_gateway_that_answers_nonsense_is_not_a_network_error() {
    let rt = Runtime::new().unwrap();
    let port = spawn_garbage_gateway(&rt);
    let (node, _dir) = node();

    let err = node.detect(format!("127.0.0.1:{port}")).unwrap_err();
    let is_network = matches!(err, tollgate_mobile::TollgateError::Network { .. });
    assert!(
        !is_network,
        "a peer that answered with undecodable bytes must not be reported as \
         unreachable (it would be retried forever): {err:?}"
    );
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

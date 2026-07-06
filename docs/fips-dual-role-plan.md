# TollGate Android — Master Task Plan

> Phone as customer AND vendor. FIPS is the valve. VPS1 is the exit.
> Routers run TollGate + FIPS. Date: 2026-07-06 (rev 4: testing findings + embedder contract)

## Architecture (Reference)

```
VPS1 (66.92.204.38) — FIPS exit node, Cashu-gated nftables (THE VALVE)
  ▲
  │ FIPS mesh (Noise XK, UDP :2121 / TCP :8443)
  │
PHONE — regular FIPS node (like Myco, NOT an exit node)
  │ ┌──────────────────────────────────────┐
  │ │ Rust Core (libfips.so from ble-v2)    │
  │ │   ├── Node::new(config)               │
  │ │   ├── Node::enable_app_owned_tun()    │
  │ │   │     → (tokio_tx, std_rx)          │
  │ │   ├── control_read_handle()           │
  │ │   │     → peer_views() for UI         │
  │ │   ├── Node::start() → run_rx_loop()   │
  │ │   ├── AndroidBleBridge (BLE mesh)     │
  │ │   ├── Custom Android UDP transport    │
  │ │   │     (phone→VPS1 direct exit)      │
  │ │   ├── Noise XK handshake              │
  │ │   ├── CDK Cashu wallet                │
  │ │   └── Tollgate server (vendor mode)   │
  │ ├──────────────────────────────────────┤
  │ │ VpnService (owns TUN fd)              │
  │ │   ├── Read: fd → push tokio_tx        │
  │ │   └── Write: pull std_rx → write fd   │
  │ ├──────────────────────────────────────┤
  │ │ Kotlin UI (Compose)                   │
  │ │   ├── Connect/Disconnect              │
  │ │   ├── Peer list (PeerView)            │
  │ │   ├── Wallet (CDK balance/pay)        │
  │ │   ├── Vendor dashboard                │
  │ │   └── BLE scan results (AdvertView)   │
  │ └──────────────────────────────────────┘
  │
  ├── Customer: pays VPS1 → valve opens → internet flows
  │
  └── Vendor: customer pays phone → phone relays through FIPS → VPS1
       Phone controls WHO gets relayed (no firewall needed — FIPS routing
       is the valve). Not paying = not relayed = no internet.

TOLLGATE ROUTERS (OpenWRT)
  └── Has WAN → accepts Cashu → opens access for paid clients
      Reseller mode OFF for now (only needed when router has no WAN)
```

## Key Decision: ble-v2 Branch (NOT v0.4.0)

**Branch:** `ble-v2` on `github.com/jmcorgan/fips`
**Commit:** `56062094d604317a885e696f979c425518516cc1` (2026-06-30)
**Base:** v0.4.0 tag + 11 additive commits by Origami74

Why ble-v2 changes everything:
- **Cross-compiles clean** to `aarch64-linux-android` (v0.4.0 had 15 errors)
- `enable_app_owned_tun()` — TUN fd problem already SOLVED upstream
- `AndroidBleBridge` + `AndroidRadio` trait — JNI byte-bridge, no JNI on hot path
- `PeerView` + `peer_views()` — lock-free peer list for UI
- Protocol-identical to v0.4.0 (wire format, handshake, routing unchanged)
- **myco-core** by Origami74 — WORKING Android embedder, fork it
- Amperstrand endorsement: "android basics will be upstream soon, fine to rely on"

**Pointer doc:** `fips-exit-e2e/docs/ANDROID-LLM-POINTERS.md` (308 lines, full API signatures + line numbers)
**Full handover:** `fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md` (all stakeholder input consolidated)

## VPS1 Verified State (as of 2026-07-06)

- **FIPS binary on VPS1:** 0.5.0-dev (built July 5). Pin file says v0.4.0. Dashboard says 0.3.0-dev.
  → **Version drift is REAL. See H1.**
- **nftables table:** `inet fips-exit` (NOT `ip`). MASQUERADE conditional on `paid_peers` set.
- **paid_peers set:** Currently EMPTY. Cashu payment adds peer IP with 1h timeout. By design (EXIT-6).
- **WireGuard tunnel:** UP. 15s handshake. 515KB rx / 334KB tx bidirectional.
- **IP forwarding:** Enabled. Internet reachable from VPS1.
- **FIPS daemon:** Active (running since July 5).
- **Dashboard:** Static HTML, not live. Accurate when written but doesn't auto-update.
- **SMOKE-1 test:** 5 pytest tests. High quality code. Defaults to wrong IP (23.182.128.51).
- **Crons:** Daily smoke at 06:00 + health monitoring every 15min (created today, haven't run yet).

## Dependency Graph

```
A1 (fix discovery) ──────────────────────────────────────────
  │
  ├── A2 (CDK wallet) ── A3 (auto-topup) ── A4 (pay wiring)
  │       │                                      ▲
  │       └──────────────────────────────────────┘
  │
B1 (clone ble-v2, verify build) ── B2 (fork myco-core JNI layer)
  │                                   │
  ├── B3 (Android UDP transport) ◄────┘  (HARDEST TASK)
  │   │
  │   ├── B4 (VpnService + enable_app_owned_tun + 3 gotchas)
  │   │   │
  │   │   └── B5 (lifecycle + reconnect)
  │   │         │
  │   │         ├── C1 (tollgate server on phone)
  │   │         │   │
  │   │         │   └── C2 (hotspot relay) ── C3 (vendor UI)
  │   │         │
  │   │         └── F2 (FIPS mesh discovery) ── F3 (auto-connect)
  │   │
  └── B6 (BLE transport — optional for mesh peers)

D1 (flash routers) ── D2 (install tollgate-wrt)     (independent)
D3 (FIPS OpenWRT pkg) ── D4 (router FIPS config)    (independent)

E1 (VPS1 nostr fix)                                   (independent)
E2 (VPS1 paygate verify)                              (independent)

F1 (WiFi SSID scan)                                   (independent)

H1 (version drift fix) ── H2 (fix SMOKE-1 IP)        (independent)
H3 (CI pipeline) ── H4 (DQ05 KVM exit-node tests)    (independent)
H5 (Android test env setup)                           (independent)
```

---

## WORKSTREAM A: App Customer Flow (CRITICAL PATH)

### A1: Fix Discovery — Phone Can't Reach Gateway

**Problem:** Phone shows 0/9 peers. All probes fail including Nostr-discovered URLs.
**Scope:**
- Diagnose: is phone on same network as T470? AP isolation?
- Try binding gateway to `0.0.0.0` instead of specific IP
- Verify Nostr discovery returns URLs (events published, d-tag = `tollgate-gateway`)
- Test manual gateway URL entry from phone
**Dependencies:** None
**Acceptance:** Phone Discover shows ≥1 reachable peer. Screenshot evidence.
**Effort:** 1 session (may be a 5-min fix or a network config issue)
**Machine:** T470 + phone

---

### A2: CDK Cashu Wallet Integration

**Problem:** App uses bootstrap-token stub (filler crypto). Needs real Cashu operations.
**Scope:**
- Add `cdk` crate (not just `cashu` types) to tollgate-mobile Cargo.toml
- Implement wallet struct with: `check_balance()`, `mint_tokens()`, `melt_tokens()`, `check_spent()`
- Default mint: `https://testnut.cashu.space`
- Proof storage: SQLite or flatfile in app-private storage
- Expose via UniFFI: `wallet_balance() -> u64`, `wallet_topup(amount_sat)`, `wallet_pay(amount_sat) -> String`
**Dependencies:** None (parallel with B-track)
**Acceptance:** `cargo test` with real testnut mint. Wallet can mint 1000 sats, check balance, melt 100 sats.
**Effort:** 2 sessions
**Repo:** tollgate-android (Rust core)
**Build:** DQ05

---

### A3: Auto-Topup Logic

**Problem:** No automatic balance management during consume loop.
**Scope:**
- Monitor wallet balance during `start_consume()` loop
- When balance < renewal_threshold, auto-mint new tokens from testnut
- For testnet: free minting (no Lightning invoice needed)
- Surface topup events via `poll_event()`
- Configurable threshold and auto-topup toggle
**Dependencies:** A2 (CDK wallet)
**Acceptance:** Session stays alive for 10+ minutes with auto-topup. Balance never hits zero.
**Effort:** 1 session
**Repo:** tollgate-android (Rust core)

---

### A4: Payment Flow Wiring

**Problem:** `pay()` uses `build_bootstrap_token()` (filler). Needs real CDK tokens.
**Scope:**
- Replace `build_bootstrap_token()` call in `pay()` with `wallet.melt_tokens(amount)`
- Gateway receives real Cashu token, validates against mint, starts session
- Wire WalletScreen balance display to live CDK wallet
- Wire PayScreen to show real balance + price from gateway
- Test full cycle: detect → pay (real token) → consume → session active
**Dependencies:** A2, A3
**Acceptance:** Phone pays T470 gateway with real testnut Cashu token. Session starts. Traffic flows. Screenshot + logcat evidence.
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Rust + Kotlin)

---

## WORKSTREAM B: FIPS Integration (ble-v2 TRACK)

### B1: Clone ble-v2, Verify Cross-Compilation

**Problem:** Need FIPS `.so` for Android. ble-v2 should cross-compile clean.
**Scope:**
- Clone jmcorgan/fips, checkout ble-v2 branch (commit 56062094)
- Configure `.cargo/config.toml` with NDK linker paths (see pointer doc §BUILD)
- Cross-compile: `cargo build --target aarch64-linux-android --release`
- Copy `libfips.so` to tollgate-android `jniLibs/arm64-v8a/`
- If errors: they should be minor (ble-v2 is additive over v0.4.0)
**Cross-compilation watch-outs (from Origami74):**
- `ring` crate needs NDK C compiler (C/assembly crypto) — set CC env var
- `nostr-sdk` may need `rustls-tls` instead of `native-tls` for Android
- `tun` crate compiles but is UNUSED at runtime (app-owned TUN path skips it)
- `build.rs` auto-detects Android — no `--features` needed
**Dependencies:** None
**Acceptance:** `cargo build --target aarch64-linux-android --release` exits 0. `libfips.so` produced.
**Effort:** 0.5 sessions (was 2-3 with v0.4.0)
**Repo:** fork of jmcorgan/fips (ble-v2 branch)
**Build:** DQ05
**Pointer:** ANDROID-LLM-POINTERS.md §BUILD INSTRUCTIONS

---

### B2: Fork myco-core JNI Embedder

**Problem:** Need the JNI layer that bridges Kotlin ↔ Rust FIPS core.
**Scope:**
- Contact Origami74 (Arjen, Signal @1624e1bb-...) for myco-core access
- Fork myco-core, extract:
  - `Java_..._NativeCore_*` JNI exports
  - `AndroidRadio` trait impl via JNI `call_method` on Kotlin `BleRadio`
  - Kotlin BLE radio (scan, advertise, L2CAP listen/connect, socket read/write)
  - VpnService ↔ FIPS TUN channel glue (uses `enable_app_owned_tun()`)
- Adapt to TollGate namespace + UI
- Key pattern: byte hot path NEVER calls JNI — uses channel bridge:
  - Inbound: Kotlin calls `bridge.deliver_recv(ch_id, data)` (non-blocking push)
  - Outbound: Kotlin calls `bridge.next_send(ch_id, timeout)` (blocking pull)
**Dependencies:** B1 (FIPS compiles)
**Acceptance:** JNI layer compiles. Kotlin can instantiate FIPS Node, call `enable_app_owned_tun()`, receive channel pair.
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Kotlin JNI layer)
**Reference:** myco-core by Origami74
**Pointer:** ANDROID-LLM-POINTERS.md §THE EMBEDDER PATTERN

---

### B3: Android UDP Transport (HARDEST TASK)

**Problem:** ble-v2 gates out UDP/TCP system transports on Android. For direct phone→VPS1 exit, need custom transport.
**Scope:**
- Write Android UDP transport mirroring AndroidBleBridge byte-bridge pattern:
  - Kotlin owns `DatagramSocket` (connects to 66.92.204.38:2121)
  - Rust↔Kotlin exchange via channels (same pattern as BLE)
  - `deliver_recv(ch_id, data)` for inbound packets
  - `next_send(ch_id, timeout)` for outbound packets
- Register transport with FIPS Node before `start()`
- Must handle: Noise XK handshake over UDP, session establishment, retransmit
- Test against VPS1: phone sends Noise XK init → VPS1 responds → session active
**Dependencies:** B1 (FIPS compiles), B2 (JNI pattern established)
**Acceptance:** Phone connects to VPS1 (66.92.204.38:2121) via custom UDP transport. Logcat shows "Session established (initiator, XK)".
**Effort:** 2-3 sessions (this is the single hardest task in the plan)
**Repo:** tollgate-android (Rust transport + Kotlin socket)
**Reference:** AndroidBleBridge pattern from ble-v2 `src/transport/ble/android_io.rs`
**Pointer:** ANDROID-LLM-POINTERS.md §ANDROID TRANSPORT GAP

---

### B4: VpnService + enable_app_owned_tun Integration

**Problem:** FIPS needs TUN fd. Android requires VpnService API.
**Scope:**
- Create `FipsVpnService.kt` extending Android `VpnService`
- Request VPN consent (system dialog)
- Call `Builder.establish()` to get TUN fd (ParcelFileDescriptor)
- The 6-step Node lifecycle:
  ```
  1. Node::new(config)
  2. node.enable_app_owned_tun()  →  (app_outbound_tx, app_inbound_rx)
  3. node.control_read_handle()   →  for UI polling (lock-free)
  4. node.start().await
  5. tokio::spawn(node.run_rx_loop())
  6. VpnService fd loop: read fd → push tx, pull rx → write fd
  ```
- AndroidManifest: `BIND_VPN_SERVICE` permission
- Foreground service with persistent notification (survives Doze)

**THREE GOTCHAS — THESE WILL BREAK IF IGNORED:**

1. **Channel type mismatch:**
   - app→mesh uses `tokio::sync::mpsc::Sender` (async)
   - mesh→app uses `std::sync::mpsc::Receiver` (blocking)
   - Different ON PURPOSE. Use `recv_timeout()` on the app side, NOT blocking `recv()`.

2. **fd00::/8 filter bypass:**
   - FIPS does NOT filter destinations in app-owned mode
   - If you push non-fd00::/8 packets, they get MISROUTED SILENTLY
   - VpnService routing rules MUST ensure only mesh-bound traffic hits the TUN
   - Configure `Builder.addAllowedApplication()` or `Builder.addRoute()` carefully

3. **TCP MSS clamping:**
   - You MUST clamp MSS on outbound SYNs yourself
   - Without it, cold TCP connections WEDGE (silent drops, no PTB feedback through userspace TUN)
   - This is the #1 cause of "connection established but no data flows" bugs
   - Clamp MSS to MTU-40 (1280-40=1240 for IPv6, 1280-40=1240 for IPv4)

**Dependencies:** B2 (JNI layer), B3 (UDP transport for VPS1 reachability)
**Acceptance:** App starts FIPS node, VpnService consent dialog appears, FIPS session establishes to VPS1. `curl ifconfig.me` through VPN shows 66.92.204.38. No wedged connections (MSS clamped).
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Kotlin + Rust)
**Pointer:** ANDROID-LLM-POINTERS.md §App-Owned TUN (THE critical one)

---

### B5: FIPS Node Lifecycle + Reconnect

**Problem:** FIPS has no reconnect logic. App must manage lifecycle.
**Scope:**
- Start/stop FIPS node from app UI (toggle in Settings or Status screen)
- Generate FIPS config programmatically (identity, peers, transports)
- Store nsec in Android Keystore (hardware-backed if available)
- Reconnect loop: 5s → 10s → 20s → 40s → 60s (capped)
- Status reporting: connected peers (PeerView), session state, bytes transferred
- Surface status via `poll_event()` → UiState
- Poll `peer_views()` for lock-free peer list updates
- Safe teardown: no lock held across `recv_timeout()` (bugfix already in ble-v2)
**Dependencies:** B4 (VpnService)
**Acceptance:** Phone connects to VPS1 FIPS exit. Logcat shows "Session established". Disconnect → auto-reconnect within 60s. Peer list visible in UI.
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Rust + Kotlin)
**Pointer:** ANDROID-LLM-POINTERS.md §PeerView

---

### B6: BLE Transport (Optional — Mesh Peers)

**Problem:** BLE transport enables nearby peer mesh (phone↔phone). Optional for MVP.
**Scope:**
- Wire AndroidBleBridge from myco-core into TollGate
- Kotlin BleRadio: scan, advertise, L2CAP CoC listen/connect
- FIPS BLE service UUID: `9c90b7902cc542c09f87c9cc40648f4c`
- L2CAP PSM: `0x0085` (dynamic range — PSM rotation fix already in ble-v2 for RPAs)
- BLE performance: ~200kbps up / ~500kbps down (for nearby mesh, NOT internet exit)
- Outbound queue cap: 32 packets (tuned — bufferbloat fix already in ble-v2)
- L2CAP stream reframing: Android byte-stream vs datagram (fix already in ble-v2)
**Dependencies:** B2 (myco-core JNI layer)
**Acceptance:** Two phones discover each other via BLE, establish FIPS peer connection.
**Effort:** 1 session
**Priority:** LOW (UDP exit to VPS1 is more important than BLE mesh)
**Repo:** tollgate-android (Kotlin BLE + Rust bridge)

---

## WORKSTREAM C: Vendor Mode (Phone Sells Internet)

### C1: TollGate Server on Phone

**Problem:** App is customer-only. Needs server logic for vendor mode.
**Scope:**
- Run `tollgate-net` server logic on phone (in Rust, behind UniFFI)
- Accept incoming connections from FIPS mesh peers (or WiFi hotspot clients)
- Validate Cashu payments from peers
- Create metered sessions for paying peers
- Track bytes/seconds delivered per peer
- Relay approved peers' traffic through FIPS tunnel → VPS1 → internet
- Unpaid peers get nothing relayed (FIPS routing = the valve, no firewall needed)
- Expose via UniFFI: `vendor_start()`, `vendor_stop()`, `vendor_status() -> VendorState`
**Dependencies:** B5 (FIPS lifecycle working)
**Acceptance:** Unit test: peer connects, pays, session starts, bytes metered, traffic relayed through VPS1.
**Effort:** 2 sessions
**Repo:** tollgate-android (Rust core)

---

### C2: WiFi Hotspot + Traffic Relay

**Problem:** Phone must relay customer traffic through FIPS tunnel to VPS1.
**Scope:**
- Android WiFi hotspot API (or VpnService routing rules)
- Route customer traffic → FIPS tunnel → VPS1 → internet
- Per-client control: only paying customers get relayed through tunnel
- Track per-client data usage
**Dependencies:** C1 (server logic), B5 (FIPS tunnel)
**Acceptance:** Laptop on phone hotspot → pays Cashu → gets internet through VPS1.
**Effort:** 2 sessions
**Repo:** tollgate-android (Kotlin + Rust)

---

### C3: Vendor Pricing + Earnings UI

**Problem:** No vendor-facing UI.
**Scope:**
- Pricing screen: set per-second, per-byte rates, accepted mints
- Earnings dashboard: sats earned, active sessions, peer list
- Vendor on/off toggle
- FIPS advertisement: publish kind 30078 with pricing when vendor mode active
**Dependencies:** C1, C2
**Acceptance:** Vendor screen shows pricing, toggle works, earnings update in real-time.
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Kotlin)

---

## WORKSTREAM D: Physical Routers (INDEPENDENT)

### D1: Flash Routers with OpenWRT

**Scope:**
- Two physical routers on interfaces:
  - enp0s31f6 (192.168.1.200) — LAN
  - enx00e04c390818 (10.47.41.203) — USB Ethernet
- Flash OpenWRT 25.x (or latest stable)
- Configure WiFi AP + WAN
**Dependencies:** None
**Acceptance:** Both routers boot OpenWRT, accessible via web UI.
**Effort:** 1 session (physical work)
**Note:** These are currently T470 network interfaces. Clarify with operator: are these actual router devices to flash, or T470 interfaces to run tollgate-module-basic-go on?

---

### D2: Install tollgate-wrt Package

**Scope:**
- Fetch latest tollgate-wrt package via Nostr (kind 1063 from CI pubkey)
- Install on both routers
- Configure: testnut.cashu.space mint, pricing, captive portal
- Reseller mode OFF
**Dependencies:** D1
**Acceptance:** Phone connects to router WiFi → captive portal → pays Cashu → internet works.
**Effort:** 1 session

---

### D3: Build FIPS OpenWRT Package

**Scope:**
- Cross-compile FIPS daemon for OpenWRT architectures (aarch64_cortex-a53, mips_24kc)
- Create .ipk/.apk package
- OpenWRT feed entry or standalone package
- Config template for VPS1 peering
**Dependencies:** None (can start immediately, parallel with everything)
**Acceptance:** `opkg install fips` works on OpenWRT. FIPS daemon starts, connects to VPS1.
**Effort:** 2-3 sessions
**Repo:** fork of jmcorgan/fips (ble-v2) + packaging scripts

---

### D4: Configure Router FIPS to VPS1

**Scope:**
- FIPS config: peer to VPS1 exit (npub1mqelkzqp4659..., 66.92.204.38:2121)
- Nostr discovery enabled, advertise true
- WireGuard tunnel to VPS1 wg0
- nftables MASQUERADE for paid_peers
**Dependencies:** D3 (FIPS package), E1 (VPS1 nostr fix)
**Acceptance:** Router FIPS node connects to VPS1. Phone → router → VPS1 → internet.
**Effort:** 1 session

---

## WORKSTREAM E: VPS1 Infrastructure (INDEPENDENT)

### E1: Fix VPS1 FIPS Nostr Advert Publishing

**Problem:** VPS1 log shows `Nostr traversal advert publish timed out timeout_ms=10000`.
       FIPS v0.4.0 source has NO Nostr event publishing code (verified 2026-07-06).
**Scope:**
- SSH to VPS1, check FIPS logs for advert publish errors
- Test relay connectivity from VPS1 (wss://relay1.orangesync.tech etc.)
- FIPS v0.4.0 does NOT publish kind 30078 events natively. Use the separate
  `publish-nostr-announce.sh` script from `fips-exit-e2e/scripts/`
- Verify the script runs and publishes successfully to configured relays
- OR: upgrade VPS1 binary to ble-v2 which may have native Nostr publishing
- Check that existing Hermes crons (daily 06:00 smoke, 15min health) are functional
**Dependencies:** None
**Acceptance:** `nak req -k 30078` from VPS1's npub returns route advert events.
**Effort:** 1 session
**Machine:** SSH to 66.92.204.38

---

### E2: Verify VPS1 Cashu Paygate (Corrected Details)

**Problem:** nftables paid_peers set starts empty. Need to verify paygate works.
**Verified facts (2026-07-06):**
- nftables table is `inet fips-exit` (NOT `ip` — subagent was wrong)
- MASQUERADE conditional on `paid_peers` set membership
- paid_peers set is currently EMPTY (by design)
- Cashu payment adds peer IP to set with 1h timeout
- WireGuard handshake works WITHOUT payment (handshake ≠ internet egress)
- Internet egress REQUIRES Cashu payment to add IP to paid_peers
**Scope:**
- Check if fips-paygate is running on VPS1
- Test: send Cashu token → verify IP added to `paid_peers` set in `inet fips-exit`
- Test: timeout expires (1h) → IP removed from set
- Verify end-to-end: WG handshake alone = no internet. Pay → internet flows.
- Verify: payment expires → internet stops (but WG tunnel stays up)
**Dependencies:** None
**Acceptance:** Cashu payment → nftables rule added → internet flows. Timeout → removed → internet stops.
**Effort:** 1 session
**Machine:** SSH to 66.92.204.38

---

## WORKSTREAM F: Auto-Discovery (AFTER B-TRACK)

### F1: WiFi SSID Scan

**Scope:**
- Kotlin `WifiManager.startScan()` + `BroadcastReceiver`
- AndroidManifest: `ACCESS_FINE_LOCATION`, `ACCESS_WIFI_STATE`, `CHANGE_WIFI_STATE`
- Runtime permission request (GrapheneOS-safe)
- Filter SSIDs starting with `TollGate-`
- Extract gateway info from SSID or DNS
**Dependencies:** None (independent)
**Acceptance:** Phone scans WiFi, finds `TollGate-*` SSID, shows in Discover list.
**Effort:** 1 session
**Repo:** tollgate-android (Kotlin)

---

### F2: FIPS Mesh Discovery

**Scope:**
- Query embedded FIPS node for known peers via `peer_views()`
- Merge FIPS peers into Discover list alongside Nostr + WiFi results
- Show transport type badge (FIPS vs TollGate v2 vs WiFi)
**Dependencies:** B5 (FIPS lifecycle)
**Acceptance:** FIPS peers appear in Discover list automatically.
**Effort:** 1 session

---

### F3: Auto-Connect Orchestration

**Scope:**
- On app launch: run all discovery sources (FIPS mesh + Nostr + WiFi + seed list)
- Every 60s while in foreground: refresh
- Best peer selected by: signal strength → price → latency
- If auto-connect enabled: detect → pay (auto-topup) → consume automatically
- If payment fails: try next-best peer
- Backoff on all-fail
**Dependencies:** F1, F2, A4 (real payment)
**Acceptance:** Phone auto-discovers, auto-pays, auto-connects with zero manual steps.
**Effort:** 1-2 sessions

---

## WORKSTREAM G: Polish (LAST)

### G1: Onboarding Flow
First launch: generate identity, topup wallet, grant VPN permission.
**Effort:** 1 session

### G2: Foreground Service
FIPS runs as foreground service, survives Doze, persistent notification.
**Effort:** 1 session (may overlap with B4)

### G3: ZapStore Listing
`zapstore.yaml`, screenshots, description for decentralized distribution.
**Effort:** 1 session

---

## WORKSTREAM H: Testing & QA Infrastructure (NEW)

### H1: Fix VPS1 Version Drift (VERIFIED BUG)

**Problem:** Three surfaces show three different versions on VPS1:
- Binary: 0.5.0-dev (built July 5)
- Pin file: v0.4.0
- Dashboard HTML: 0.3.0-dev
Pin policy says "DO NOT track master" but binary is a dev build beyond the pinned tag.
**Scope:**
- SSH to VPS1, check actual FIPS binary version
- Reconcile: either pin to ble-v2 (matching the Android branch) or rebuild from v0.4.0
- Update pin file, dashboard HTML, and binary to MATCH
- Document the version policy: which branch/tag is authoritative for VPS1
**Dependencies:** None
**Acceptance:** Binary version = pin file version = dashboard version. All three surfaces agree.
**Effort:** 0.5 sessions
**Machine:** SSH to 66.92.204.38

---

### H2: Fix SMOKE-1 Test Default IP (VERIFIED BUG)

**Problem:** SMOKE-1 test defaults to 23.182.128.51. Needs `FIPS_EXIT_HOST=66.92.204.38` env var.
**Scope:**
- Update SMOKE-1 conftest.py to default to 66.92.204.38
- Verify `pytest tests/smoke-1/` runs without env vars
**Dependencies:** None
**Acceptance:** `pytest tests/smoke-1/` passes without needing `FIPS_EXIT_HOST` env var.
**Effort:** 0.5 sessions
**Repo:** fips-exit-e2e

---

### H3: CI Pipeline Setup

**Problem:** No CI pipeline exists. All tests run manually. Docker test infra is manual shell.
**Scope:**
- GitHub Actions workflow for fips-exit-e2e:
  - Run SMOKE-1 tests on schedule (daily) + on push
  - Docker-based FIPS daemon peer-to-peer tests
  - Dashboard Playwright tests (5 existing)
- CI can NOT run VPS1 tests (no SSH key in CI) — mark as manual-only
- CI CAN run Docker protocol tests + dashboard tests
- Report failures to Signal group via existing Hermes notification infrastructure
**Dependencies:** None
**Acceptance:** GitHub Actions runs Docker protocol tests + dashboard tests on every push.
**Effort:** 1-2 sessions
**Repo:** fips-exit-e2e

---

### H4: DQ05 KVM Exit-Node VM Tests

**Problem:** Docker can't faithfully test NAT (network namespace isolation). Need full WG→nftables→internet forwarding test.
**Scope:**
- Use DQ05 (192.168.1.218, 11GB RAM, HAS KVM) to run a VM that mirrors VPS1:
  - FIPS exit node in VM with WG + nftables
  - Test peer (Docker container) connects to VM exit node
  - Verify: handshake → payment → nftables MASQUERADE → internet egress
  - Verify: payment timeout → internet stops
- This is the ONLY way to test the full forwarding path without touching production VPS1
- VMProvider abstraction from PRTA is reusable here (SHC/GCP/Local QEMU)
**Dependencies:** None (DQ05 is ready, KVM available)
**Acceptance:** VM exit node on DQ05. Test peer pays Cashu → internet flows through VM. Timeout → stops.
**Effort:** 2 sessions
**Machine:** DQ05 (192.168.1.218)

---

### H5: Android Test Environment Setup

**Problem:** No Android emulator or device in current test environment. No Android SDK on T470/DQ05. Playwright/Appium smoke test + video is a HARD GATE (per c08r4d0r mandate).
**Scope:**
- Install Android SDK + emulator on DQ05 (has KVM for hardware acceleration)
- Create Android emulator (arm64-v8a, API 34 = Android 14, GrapheneOS-compatible settings)
- OR: designate a physical Android phone as test device
- Set up Appium or Playwright for Android (Espresso or UI Automator)
- Write first smoke test: launch app → Discover screen visible → screenshot
- Video recording of test run (mandatory evidence per SOUL.md)
**Dependencies:** None (DQ05 has KVM)
**Acceptance:** Android emulator boots on DQ05. App launches. Smoke test passes. Video recorded.
**Effort:** 2 sessions
**Machine:** DQ05 (192.168.1.218)

---

### H6: Dashboard Live Data

**Problem:** Dashboard is static HTML. Was accurate when written but doesn't auto-update. Playwright tests verify rendering, not accuracy.
**Scope:**
- Add a JSON/status endpoint to FIPS exit node that returns: version, WG status, peer count, paid_peers, uptime
- Update dashboard to fetch from endpoint on load
- OR: generate dashboard HTML from a cron job that reads live state
- Update Playwright tests to verify data matches live VPS1 state
**Dependencies:** H1 (version reconciliation)
**Acceptance:** Dashboard shows live data matching `nft list ruleset` and `wg show` output on VPS1.
**Effort:** 1 session
**Machine:** SSH to 66.92.204.38

---

## Task Summary Table

| ID | Task | Depends On | Effort | Priority |
|----|------|-----------|--------|----------|
| **A1** | Fix discovery (0/9 issue) | — | 1 sess | CRITICAL |
| **A2** | CDK Cashu wallet | — | 2 sess | CRITICAL |
| **A3** | Auto-topup logic | A2 | 1 sess | HIGH |
| **A4** | Payment flow wiring | A2,A3 | 1-2 sess | CRITICAL |
| **B1** | Clone ble-v2, verify cross-compile | — | 0.5 sess | CRITICAL |
| **B2** | Fork myco-core JNI embedder | B1 | 1-2 sess | CRITICAL |
| **B3** | Android UDP transport (HARDEST) | B1,B2 | 2-3 sess | CRITICAL |
| **B4** | VpnService + 3 gotchas | B2,B3 | 1-2 sess | HIGH |
| **B5** | FIPS lifecycle + reconnect | B4 | 1-2 sess | HIGH |
| **B6** | BLE transport (mesh peers) | B2 | 1 sess | LOW |
| **C1** | TollGate server on phone | B5 | 2 sess | MEDIUM |
| **C2** | Hotspot + traffic relay | C1,B5 | 2 sess | MEDIUM |
| **C3** | Vendor pricing + earnings UI | C1,C2 | 1-2 sess | MEDIUM |
| **D1** | Flash routers with OpenWRT | — | 1 sess | HIGH |
| **D2** | Install tollgate-wrt | D1 | 1 sess | HIGH |
| **D3** | Build FIPS OpenWRT package | — | 2-3 sess | MEDIUM |
| **D4** | Configure router FIPS | D3,E1 | 1 sess | MEDIUM |
| **E1** | VPS1 Nostr advert fix | — | 1 sess | HIGH |
| **E2** | VPS1 Cashu paygate verify | — | 1 sess | HIGH |
| **F1** | WiFi SSID scan | — | 1 sess | MEDIUM |
| **F2** | FIPS mesh discovery | B5 | 1 sess | MEDIUM |
| **F3** | Auto-connect orchestration | F1,F2,A4 | 1-2 sess | LOW |
| **G1** | Onboarding flow | A4,B5 | 1 sess | LOW |
| **G2** | Foreground service | B4 | 1 sess | LOW |
| **G3** | ZapStore listing | — | 1 sess | LOW |
| **H1** | Fix VPS1 version drift (BUG) | — | 0.5 sess | CRITICAL |
| **H2** | Fix SMOKE-1 default IP (BUG) | — | 0.5 sess | HIGH |
| **H3** | CI pipeline setup | — | 1-2 sess | HIGH |
| **H4** | DQ05 KVM exit-node VM tests | — | 2 sess | HIGH |
| **H5** | Android test env setup | — | 2 sess | HIGH |
| **H6** | Dashboard live data | H1 | 1 sess | MEDIUM |

**Total: ~33-43 sessions across 8 workstreams (A-H)**

## Recommended Execution Order

**Sprint 1 (all parallel, start immediately):**
- A1: Fix discovery
- A2: CDK wallet
- B1: Clone ble-v2, verify cross-compile ← was 2-3 sessions, now 0.5
- D1: Flash routers
- E1: VPS1 Nostr fix
- E2: VPS1 paygate verify
- **H1: Fix VPS1 version drift** (quick win, real bug)
- **H2: Fix SMOKE-1 default IP** (quick win, real bug)
- Contact Origami74 for myco-core access (blocking B2)

**Sprint 2 (parallel):**
- A3: Auto-topup (needs A2)
- A4: Payment wiring (needs A2)
- B2: Fork myco-core JNI layer (needs B1)
- D2: Install tollgate-wrt (needs D1)
- D3: FIPS OpenWRT package (parallel)
- **H3: CI pipeline** (independent)
- **H4: DQ05 KVM exit-node VM** (independent, DQ05 ready)
- **H5: Android test env setup** (independent, DQ05 has KVM)

**Sprint 3 (parallel):**
- B3: Android UDP transport (needs B1,B2) ← CRITICAL PATH
- B6: BLE transport (optional, parallel)

**Sprint 4 (parallel):**
- B4: VpnService + enable_app_owned_tun + 3 gotchas (needs B2,B3)
- B5: FIPS lifecycle (needs B4)
- D4: Router FIPS config (needs D3, E1)
- **H6: Dashboard live data** (needs H1)

**Sprint 5:**
- C1: TollGate server (needs B5)
- F1: WiFi scan (parallel)

**Sprint 6+:**
- C2, C3, F2, F3, G1-G3

## Key Constraints

- **FIPS ble-v2 ONLY** (commit 56062094). NOT v0.4.0 alone (no Android support). NOT master (sans-io refactor breaks everything).
- **myco-core** is the reference embedder — fork it, don't reinvent. Contact Origami74.
- **Custom Android UDP transport** is the hardest task — ble-v2 gates out system UDP on Android.
- Android VpnService for TUN (no root). Use `enable_app_owned_tun()` from ble-v2.
- **No JNI on byte hot path** — use channel bridge pattern (deliver_recv/next_send).
- **Channel type mismatch:** app→mesh = tokio mpsc (async), mesh→app = std mpsc (blocking). Use recv_timeout().
- **fd00::/8 filter bypass:** FIPS does NOT filter in app-owned mode. VpnService routing rules must.
- **TCP MSS clamping:** MUST clamp on outbound SYNs. Without it, cold TCP connections wedge.
- **Cross-compilation:** ring crate needs NDK CC, nostr-sdk needs rustls-tls, tun crate unused, build.rs auto-detects Android.
- testnut.cashu.space for testnet ecash
- DQ05 for builds (T470 OOM-kills cargo-ndk + Gradle)
- DQ05 KVM for exit-node VM tests (Docker can't test NAT faithfully)
- Nostr relays: relay1.orangesync.tech, relay.damus.io, nos.lol
- GrapheneOS: location permissions for WiFi scan, strict background limits
- Reconnect: 5s → 10s → 20s → 40s → 60s (capped) — FIPS has no built-in retry
- **Playwright/Appium video is a HARD GATE** — no video = task not done (per SOUL.md)
- **VPS1 version drift:** binary 0.5.0-dev, pin v0.4.0, dashboard 0.3.0-dev. Must reconcile (H1).
- **nftables:** table is `inet fips-exit`. paid_peers set starts empty. Payment adds IP with 1h timeout.

## References

- **Pointer doc:** `fips-exit-e2e/docs/ANDROID-LLM-POINTERS.md` (308 lines — APIs, line numbers, build config)
- **Full handover:** `fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md` (all stakeholder input consolidated)
- **Build environment:** `docs/build-environment.md` (DQ05 setup)
- **Transport layer:** `docs/fips-transport-layer.md` (Phase 2 integration guide)

## Contacts

- **c08r4d0r** — project owner (Signal group: tollgate-native-android-app)
- **Origami74 (Arjen)** — ble-v2 author, myco-core (working Android embedder). Signal @1624e1bb-94ef-46d1-b03b-f067ea320af9. MUST CONTACT for myco-core access.
- **jmcorgan** — FIPS upstream maintainer
- **Amperstrand** — firmware collaborator (ESP32-C3, RP2040). Endorsed ble-v2.

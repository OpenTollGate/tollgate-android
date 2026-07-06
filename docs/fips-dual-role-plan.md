# TollGate Android — Master Task Plan

> Phone as customer AND vendor. FIPS is the valve. VPS1 is the exit.
> Routers run TollGate + FIPS. Date: 2026-07-06

## Architecture (Reference)

```
VPS1 (66.92.204.38) — FIPS exit node, Cashu-gated nftables (THE VALVE)
  ▲
  │ FIPS mesh (Noise XK)
  │
PHONE — regular FIPS node (like Myco, NOT an exit node)
  │ ┌────────────────────────────┐
  │ │ TollgateMobileNode (UniFFI) │
  │ │ FIPS node · CDK wallet      │
  │ │ Android VpnService (TUN)    │
  │ └────────────────────────────┘
  │
  ├── Customer: pays VPS1 → valve opens → internet flows
  │
  └── Vendor: customer pays phone → phone relays through FIPS → VPS1
       Phone controls WHO gets relayed (no firewall needed — FIPS routing
       is the valve). This was impossible before FIPS (no per-client
       firewall on Android).

TOLLGATE ROUTERS (OpenWRT)
  └── Has WAN → accepts Cashu → opens ndsctl for paid clients
      Reseller mode OFF for now (only needed when router has no WAN)
```

## Dependency Graph

```
A1 (fix discovery) ──────────────────────────────────────────
  │                                                          
  ├── A2 (CDK wallet) ── A3 (auto-topup) ── A4 (pay wiring) 
  │       │                                               ▲
  │       │                                               │
  │       └───────────────────────────────────────────────┘
  │
B1 (patch FIPS for Android) ── B2 (VpnService) ── B3 (lifecycle)
  │
  ├── C1 (tollgate server on phone) ── C2 (hotspot relay) ── C3 (vendor UI)
  │
  └── F2 (FIPS mesh discovery) ── F3 (auto-connect)

D1 (flash routers) ── D2 (install tollgate-wrt)     (independent)
D3 (FIPS OpenWRT pkg) ── D4 (router FIPS config)    (independent)

E1 (VPS1 nostr fix)                                   (independent)
E2 (VPS1 paygate verify)                              (independent)

F1 (WiFi SSID scan)                                   (independent)
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

## WORKSTREAM B: FIPS Integration (HARDEST TRACK)

### B1: Patch FIPS v0.4.0 for Android Cross-Compilation

**Problem:** FIPS v0.4.0 has 15 compile errors targeting `aarch64-linux-android`.
**Scope:**
- Fork FIPS v0.4.0 (tag `da2d0b74b05d`) to `OpenTollGate/fips` or local fork
- Gate Linux-only TUN code behind `#[cfg(target_os = "linux")]`:
  - `fips/src/upper/tun.rs:808` — `platform::delete_interface()`
  - `fips/src/upper/dns.rs:304` — type mismatch `i32` vs `u32`
  - 13 more platform-specific errors
- Create `TunProvider` trait: `create_tun(name, mtu) -> RawFd`
- Implement `AndroidTunProvider` that accepts a pre-opened VpnService fd
- Cross-compile: `cargo ndk -t arm64-v8a build -p fips` succeeds
**Dependencies:** None (can start immediately)
**Acceptance:** `cargo ndk -t arm64-v8a build` exits 0. No errors. `.so` produced.
**Effort:** 2-3 sessions (this is the single hardest task in the entire plan)
**Repo:** fork of jmcorgan/fips (v0.4.0)
**Build:** DQ05
**Reference:** Check if upstream ble-v2 branch has `enable_app_owned_tun()` — may already solve this

---

### B2: Android VpnService Integration

**Problem:** FIPS needs a TUN fd. Android requires VpnService API to create it.
**Scope:**
- Create `FipsVpnService.kt` extending Android `VpnService`
- Request VPN consent (system dialog)
- Call `Builder.establish()` to get TUN fd
- Pass fd to FIPS Rust core via UniFFI/JNI
- Foreground service with persistent notification (survives Doze)
- AndroidManifest: `BIND_VPN_SERVICE` permission
- `FipsService.kt` already stubbed in plan — wire it
**Dependencies:** B1 (FIPS compiles for Android)
**Acceptance:** App starts FIPS node, VpnService consent dialog appears, FIPS session establishes. Logcat shows Noise XK handshake.
**Effort:** 2 sessions
**Repo:** tollgate-android (Kotlin + Rust)

---

### B3: FIPS Node Lifecycle + Reconnect

**Problem:** FIPS v0.4.0 has no reconnect logic. App must manage lifecycle.
**Scope:**
- Start/stop FIPS node from app UI (toggle in Settings or Status screen)
- Generate FIPS config programmatically (identity, peers, transports)
- Store nsec in Android Keystore (hardware-backed if available)
- Reconnect loop: 5s → 10s → 20s → 40s → 60s (capped)
- Status reporting: connected peers, session state, bytes transferred
- Surface status via `poll_event()` → UiState
**Dependencies:** B2 (VpnService)
**Acceptance:** Phone connects to VPS1 FIPS exit (UDP :2121). Logcat shows "Session established". Disconnect → auto-reconnect within 60s.
**Effort:** 1-2 sessions
**Repo:** tollgate-android (Rust + Kotlin)

---

## WORKSTREAM C: Vendor Mode (Phone Sells Internet)

### C1: TollGate Server on Phone

**Problem:** App is customer-only. Needs server logic for vendor mode.
**Scope:**
- Run `tollgate-net` server logic on phone (in Rust, behind UniFFI)
- Accept incoming connections from FIPS mesh peers
- Validate Cashu payments from peers
- Create metered sessions for paying peers
- Track bytes/seconds delivered per peer
- Expose via UniFFI: `vendor_start()`, `vendor_stop()`, `vendor_status() -> VendorState`
**Dependencies:** B3 (FIPS lifecycle working)
**Acceptance:** Unit test: peer connects, pays, session starts, bytes metered.
**Effort:** 2 sessions
**Repo:** tollgate-android (Rust core)

---

### C2: WiFi Hotspot + Traffic Relay

**Problem:** Phone must relay customer traffic through FIPS tunnel.
**Scope:**
- Android WiFi hotspot API (or VpnService routing)
- Route customer traffic → FIPS tunnel → VPS1 → internet
- Per-client control: only paying customers get relayed
- Track per-client data usage
**Dependencies:** C1 (server logic), B3 (FIPS tunnel)
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
**Note:** These are currently T470 network interfaces, not standalone routers. Clarify with operator: are these actual router devices to flash, or T470 interfaces to run tollgate-module-basic-go on?

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
**Repo:** fork of jmcorgan/fips + packaging scripts

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

### E1: Debug VPS1 FIPS Nostr Advert Timeout

**Problem:** VPS1 log shows `Nostr traversal advert publish timed out timeout_ms=10000`
**Scope:**
- SSH to VPS1, check FIPS logs
- Test relay connectivity from VPS1 (wss://relay1.orangesync.tech etc.)
- Check if FIPS advert_relays are reachable
- Fix and verify route adverts publish successfully
**Dependencies:** None
**Acceptance:** `nak req -k 30078` from VPS1's npub returns route advert events.
**Effort:** 1 session
**Machine:** SSH to 66.92.204.38

---

### E2: Verify VPS1 Cashu Paygate

**Problem:** nftables paid_peers set starts empty. Need to verify paygate REST endpoint works.
**Scope:**
- Check if fips-paygate is running on VPS1
- Test: send Cashu token → verify IP added to paid_peers set
- Test: timeout expires → IP removed
- Verify end-to-end: FIPS peer → pay → internet egress works
**Dependencies:** None
**Acceptance:** Cashu payment → nftables rule added → internet flows. Timeout → removed.
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
- Query embedded FIPS node for known peers
- Merge FIPS peers into Discover list alongside Nostr + WiFi results
- Show transport type badge (FIPS vs TollGate v2 vs WiFi)
**Dependencies:** B3 (FIPS lifecycle)
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
**Effort:** 1 session

### G3: ZapStore Listing
`zapstore.yaml`, screenshots, description for decentralized distribution.
**Effort:** 1 session

---

## Task Summary Table

| ID | Task | Depends On | Effort | Priority |
|----|------|-----------|--------|----------|
| A1 | Fix discovery (0/9 issue) | — | 1 sess | CRITICAL |
| A2 | CDK Cashu wallet | — | 2 sess | CRITICAL |
| A3 | Auto-topup logic | A2 | 1 sess | HIGH |
| A4 | Payment flow wiring | A2,A3 | 1-2 sess | CRITICAL |
| B1 | Patch FIPS for Android | — | 2-3 sess | CRITICAL (parallel) |
| B2 | VpnService integration | B1 | 2 sess | HIGH |
| B3 | FIPS lifecycle + reconnect | B2 | 1-2 sess | HIGH |
| C1 | TollGate server on phone | B3 | 2 sess | MEDIUM |
| C2 | Hotspot + traffic relay | C1,B3 | 2 sess | MEDIUM |
| C3 | Vendor pricing + earnings UI | C1,C2 | 1-2 sess | MEDIUM |
| D1 | Flash routers with OpenWRT | — | 1 sess | HIGH |
| D2 | Install tollgate-wrt | D1 | 1 sess | HIGH |
| D3 | Build FIPS OpenWRT package | — | 2-3 sess | MEDIUM |
| D4 | Configure router FIPS | D3,E1 | 1 sess | MEDIUM |
| E1 | VPS1 Nostr advert fix | — | 1 sess | HIGH |
| E2 | VPS1 Cashu paygate verify | — | 1 sess | HIGH |
| F1 | WiFi SSID scan | — | 1 sess | MEDIUM |
| F2 | FIPS mesh discovery | B3 | 1 sess | MEDIUM |
| F3 | Auto-connect orchestration | F1,F2,A4 | 1-2 sess | LOW |
| G1 | Onboarding flow | A4,B3 | 1 sess | LOW |
| G2 | Foreground service | B3 | 1 sess | LOW |
| G3 | ZapStore listing | — | 1 sess | LOW |

**Total: ~28-35 sessions across 7 workstreams**

## Recommended Execution Order

**Sprint 1 (parallel):**
- A1: Fix discovery
- A2: CDK wallet
- B1: Patch FIPS for Android
- D1: Flash routers
- E1: VPS1 Nostr fix
- E2: VPS1 paygate verify

**Sprint 2 (parallel):**
- A3: Auto-topup (needs A2)
- A4: Payment wiring (needs A2)
- B2: VpnService (needs B1)
- D2: Install tollgate-wrt (needs D1)

**Sprint 3 (parallel):**
- B3: FIPS lifecycle (needs B2)
- D3: FIPS OpenWRT package (parallel)

**Sprint 4:**
- C1: TollGate server (needs B3)
- D4: Router FIPS config (needs D3, E1)
- F1: WiFi scan (parallel)

**Sprint 5+:**
- C2, C3, F2, F3, G1-G3

## Key Constraints

- FIPS v0.4.0 ONLY (master is mid-refactor, will break)
- Android VpnService for TUN (no root)
- testnut.cashu.space for testnet ecash
- DQ05 for builds (T470 OOM-kills cargo-ndk + Gradle)
- Nostr relays: relay1.orangesync.tech, relay.damus.io, nos.lol
- GrapheneOS: location permissions for WiFi scan, strict background limits

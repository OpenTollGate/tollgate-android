# TollGate Dual-Role + FIPS Architecture — Plan v3

> **Supersedes:** PLAN-v2-dual-role-fips.md, DUAL-ROLE-FIPS-PLAN.md (v1)
> **Phone:** Regular FIPS mesh node (like Myco). VPS1 is the only exit.
> **FIPS branch:** `ble-v2` on `github.com/jmcorgan/fips` (commit 5606209). Confirmed.

## The Core Insight

Android phones have no firewall. No nftables. No per-client control.

**FIPS solves this.** The phone joins the FIPS mesh as a regular peer. FIPS routing decisions ARE the valve: the phone decides whose mesh traffic to forward toward VPS1. Pay → forward. Stop paying → drop. No firewall rules needed.

VPS1 does the actual NAT to the real internet (nftables MASQUERADE → eth0). The phone never exits traffic itself — it just routes within the mesh.

## Architecture

```
SCENARIO A — Phone as Customer (no FIPS needed):
  Phone → Router WiFi → Router :2121 (Cashu pay) → Router WAN → Internet

SCENARIO B — Phone as Vendor (FIPS = the valve):
  Client → Phone hotspot → Phone :2121 (Cashu pay)
  Phone routes client traffic into FIPS mesh → VPS1 exit → Internet
  Phone controls access via FIPS routing (forward/drop per paying client)

SCENARIO C — Router with FIPS backhaul:
  Phone → Router :2121 (Cashu pay) → Router FIPS → VPS1 → Internet
```

**"Regular FIPS node like Myco" means:**
- Phone joins FIPS mesh (Noise XK handshake)
- Phone owns TUN via `enable_app_owned_tun()` — channel-based
- TUN only carries mesh traffic (fd00::/8 ULA), NOT 0.0.0.0/0
- Phone does NOT run WireGuard egress, does NOT NAT
- VPS1 is THE exit node

---

## FIPS Integration API — The 6-Step Lifecycle

**Branch:** `ble-v2` on `github.com/jmcorgan/fips`
**Commit:** `56062094d604317a885e696f979c425518516cc1`

```rust
// 1. Create node with config
let mut node = Node::new(config);

// 2. Enable app-owned TUN (BEFORE start)
//    Returns: (app_outbound_tx, app_inbound_rx)
let (outbound_tx, inbound_rx) = node.enable_app_owned_tun();

// 3. Get control read handle for UI polling (lock-free)
let control = node.control_read_handle();

// 4. Start the node (skips system TUN creation)
node.start().await;

// 5. Spawn the RX loop
tokio::spawn(node.run_rx_loop());

// 6. VpnService fd loop: read fd → push tx, pull rx → write fd
//    (this is the Kotlin side, pumping bytes between VpnService and channels)

// Shutdown:
node.stop().await;
```

### Three Gotchas (WILL break if ignored)

| # | Gotcha | Impact | Fix |
|---|--------|--------|-----|
| 1 | **Channel type mismatch** — app→mesh uses `tokio::sync::mpsc` (async), mesh→app uses `std::sync::mpsc` (blocking). Different on purpose. | Deadlock if you use async recv on the std channel. | Use `recv_timeout` on the app side for the std channel. |
| 2 | **fd00::/8 filter bypass** — FIPS does NOT filter destinations in app-owned mode. If you push non-fd00::/8 packets, they get misrouted silently. | Traffic goes nowhere, no error. | VpnService routing rules must ensure only mesh-bound traffic hits the TUN. |
| 3 | **TCP MSS clamping** — You must clamp MSS on outbound SYNs yourself. Without it, cold TCP connections wedge (silent drops, no PTB feedback through userspace TUN). | "Connection established but no data flows." #1 cause of this bug. | Clamp MSS on outbound SYNs in the fd loop. |

### Cross-Compilation Watch-Outs

| Crate | Issue | Fix |
|-------|-------|-----|
| `ring` | Needs NDK C compiler (C/assembly crypto) | Set CC to NDK clang in config.toml |
| `nostr-sdk` | May pull native-tls on Android | Use `rustls-tls` feature, not `native-tls` |
| `tun` | Compiles but unused at runtime | App-owned TUN path skips it. Harmless. |
| `fips` build.rs | Auto-detects Android | No `--features` needed |

### Reference: myco-core

Origami74 (Arjen) has a WORKING Android JNI embedder called myco-core. It implements:
- All `Java_..._NativeCore_*` JNI exports
- `AndroidRadio` trait via JNI
- Kotlin BLE radio (scan, advertise, L2CAP, socket read/write)
- VpnService ↔ FIPS TUN channel glue

**Forking myco-core is the fastest path.** Contact Arjen (Signal @1624e1bb) for access. May already have the UDP transport (B8).

---

## Testing Constraints (HONEST)

| Capability | Status |
|-----------|--------|
| Docker FIPS daemon test | ✅ Works (exit node + test peer in Docker) |
| VPS1 live exit node | ✅ Reachable via SSH (66.92.204.38) |
| Android emulator | ❌ None |
| Physical Android device | ❌ None (phone exists but no test harness) |
| Android SDK/NDK on T470 | ⚠️ NDK installed, no emulator, cross-compile works |
| DQ05 (11GB RAM, KVM) | ⚠️ Available for VM tests, unused for FIPS |

**What this means:** Android app testing requires external toolchain or physical device. Docker tests cover FIPS daemon protocol only. DQ05's KVM could test the full WG→nftables→internet forwarding path that Docker can't (NAT/network namespace isolation).

---

## Workstreams with Schedulable Tasks

### Workstream A — App Customer Flow (CRITICAL PATH)

> **Goal:** Phone pays a TollGate router with real Cashu and gets internet.
> **Dependency:** None. Can start immediately.
> **Testing:** T470 gateways + phone on same WiFi. No FIPS needed.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| A1 | Fix discovery 0/9 — phone must be on same WiFi as T470 gateways (192.168.1.x or 10.47.41.x) | 0.5 | — |
| A2 | Finish CDK wallet (wallet.rs 90% done: fix compile errors, wire into TollgateMobileNode, add UniFFI methods) | 1 | — |
| A3 | Auto-topup: on low balance during consume loop, auto-mint from testnut | 0.5 | A2 |
| A4 | Replace `build_bootstrap_token()` stub with `wallet.send_token()` in `pay()` | 0.5 | A2 |
| A5 | Wallet UI: balance display, mint button, transaction history | 1 | A2 |
| A6 | E2E test: phone pays T470 gateway with real testnut tokens | 1 | A4 |

### Workstream B — FIPS Embedded in App (HARDEST TRACK)

> **Goal:** Phone runs FIPS as regular mesh node, routes through VPS1.
> **Dependency:** B1 blocks everything. myco-core fork could collapse B3+B8.
> **Testing:** Requires Android device with the app installed.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| B1 | Clone `jmcorgan/fips` ble-v2 (commit 5606209). Confirm `enable_app_owned_tun()` + build.rs compiles. | 0.5 | — |
| B2 | Wire FIPS Node lifecycle into tollgate-mobile: Node::new → enable_app_owned_tun → control_read_handle → start → run_rx_loop. Handle the 3 gotchas (channel types, fd00::/8 filter, MSS clamping). | 1.5 | B1 |
| B3 | `FipsVpnService.kt` — foreground service, VpnService.Builder, fd↔channel pump. Model on myco-core tun_bridge if available, else from scratch. | 2 | B2 |
| B4 | FIPS config builder: VPS1 endpoint (66.92.204.38:2121), npub, persistent keys from Android KeyStore | 0.5 | B2 |
| B5 | Reconnect loop — 5s→60s exponential backoff. FIPS has none. | 1 | B3 |
| B6 | Cross-compile FIPS ble-v2 to aarch64-linux-android. Watch-outs: ring needs NDK CC, nostr-sdk needs rustls, tun crate unused. Build config in ANDROID-LLM-POINTERS.md. | 1.5 | B1 |
| B7 | Integration test: phone → FIPS mesh → VPS1 → Internet. Success = `curl ifconfig.me` shows 66.92.204.38. Log messages: "Connection promoted to active peer", "Session established (initiator, XK)". | 1 | B3,B6,B8 |
| B8 | Android UDP transport — ble-v2 gates out UDP/TCP on Android (`cfg(unix)` but construction only creates BLE). Write custom transport mirroring AndroidBleBridge with Kotlin DatagramSocket. **Check if myco-core already has this — if yes, B8 = 0.** | 2 | B1 |
| B9 | Contact Origami74 (Signal @1624e1bb) for myco-core access. If granted, fork it — may collapse B3+B8 into "port myco-core JNI layer". | 0 | — |

**If myco-core access granted:** B3 drops from 2→0.5 (port vs build from scratch), B8 may drop to 0 (if Arjen already has UDP transport). This is the single highest-leverage action.

### Workstream C — Vendor Mode (Phone as Gateway)

> **Goal:** Phone is Cashu-gated hotspot. FIPS routing = the valve.
> **Dependency:** B7 (phone on FIPS mesh) + A4 (real Cashu payments).
> **Reseller mode OFF.**

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| C1 | Port `tollgate-net` server logic to tollgate-mobile (accept payment, open session — reverse of client) | 2 | A4 |
| C2 | UniFFI methods: `start_vendor`, `stop_vendor`, `get_earnings`, `get_clients` | 0.5 | C1 |
| C3 | Vendor UI: pricing config, earnings dashboard, connected clients list | 1.5 | C2 |
| C4 | Hotspot via `WifiManager.startLocalOnlyHotspot` (user confirms — GrapheneOS correct behavior) | 1 | C2 |
| C5 | FIPS valve logic: forward paying client's mesh traffic toward VPS1, drop non-payers. No firewall needed — FIPS routing IS access control. | 1.5 | B7,C1 |
| C6 | E2E test: client pays phone hotspot → phone routes via FIPS → VPS1 → Internet. Playwright/Appium video required. | 1 | C5 |

**C5 conceptual detail:**
- Client pays phone at :2121 → phone adds client's mesh address to "forward" list
- Client stops paying → phone removes from list, mesh traffic drops
- VPS1 does NAT; phone decides what to relay
- No per-client firewall needed (the whole point)

### Workstream D — Physical Routers (Independent)

> **Goal:** OpenWRT routers run tollgate-module-basic-go.
> **Dependency:** None. Reseller mode OFF for testing.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| D1 | Clarify: are 192.168.1.200 / 10.47.41.203 physical routers or T470 interfaces? | 0 | — |
| D2 | Flash OpenWRT on target router | 1 | D1 |
| D3 | Build tollgate-module-basic-go for router target (mipsel or armv7) | 1 | D1 |
| D4 | OpenWRT .ipk package + procd init script for tollgate-go | 1 | D3 |
| D5 | FIPS OpenWRT package (optional — only for encrypted backhaul) | 2 | D3 |

### Workstream E — VPS1 Infrastructure (Independent)

> **Goal:** VPS1 stable, Nostr adverts working, Cashu gate verified, version drift fixed.
> **Dependency:** None.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| E1 | Fix VPS1 FIPS Nostr advert timeout (relay connections timing out) | 1 | — |
| E2 | Deploy Nostr publisher cron on VPS1 (script written at ~/repos/fips-exit-e2e/scripts/publish-nostr-announce.sh) | 0.5 | E1 |
| E3 | Verify Cashu paygate: paid_peers nftables set starts EMPTY (inet fips-exit table, NOT ip). Test payment adds IP to set (1h timeout). Internet egress requires payment. | 1 | — |
| E4 | Fix version drift: binary is 0.5.0-dev (built Jul 5), pin says v0.4.0, dashboard says 0.3.0-dev. Rebuild from ble-v2 or v0.4.0 tag. Align all three surfaces. | 1 | — |
| E5 | Fix SMOKE-1 test default IP: hardcoded to 23.182.128.51, should be 66.92.204.38 (FIPS_EXIT_HOST env). | 0.5 | — |
| E6 | Set up DQ05 KVM VM for full WG→nftables→internet forwarding tests (Docker can't test NAT faithfully due to netns isolation). | 1.5 | — |

### Workstream F — Auto-Discovery (After B-track)

> **Goal:** Phone auto-discovers TollGate WiFi + FIPS mesh nodes.
> **Dependency:** B7 (FIPS mesh discovery needs FIPS embedded).

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| F1 | WiFi SSID scan: `WifiManager.startScan()`, filter TollGate-* SSIDs, GrapheneOS location perms | 1 | — |
| F2 | FIPS mesh discovery: query running FIPS node for known peers via `peer_views()` | 1 | B7 |
| F3 | Auto-connect: merge WiFi + FIPS + Nostr + seed results, auto-pay on connect | 1 | F1,F2,A4 |

---

## Dependency Graph

```
A1 ───────────────────────────────────────────────→ (discovery works)
A2 → A3                                           │
A2 → A4 → A6 ────────────────────────────────────→ (phone pays gateway)
A2 → A5                                            

B9 ──→ (myco-core access? transforms B3+B8)
                                                   
B1 → B2 → B3 → B5                                 │
B1 → B6                                            │
B1 → B8                                            │
         B3,B6,B8 → B7 ──────────────────────────→ (phone on FIPS mesh)
                                                   
         A4 → C1 → C2 → C3                        │
              C2 → C4                             │
         B7,C1 → C5 → C6 ────────────────────────→ (phone is vendor)
                                                   
D1 → D2 → D3 → D4 ──────────────────────────────→ (physical router)
                                                   
E1 → E2                                           │
E3 ──────────────────────────────────────────────→ (VPS1 stable)
E4 ──────────────────────────────────────────────│ (version fixed)
E5 ──────────────────────────────────────────────│ (SMOKE-1 fixed)
E6 ──────────────────────────────────────────────│ (DQ05 VM tests)
                                                   
F1                                                │
B7 → F2                                           │
F1,F2,A4 → F3 ───────────────────────────────────→ (auto-discovery)
```

**Critical path:** A2→A4→B1→B2→B3→B8→B7→C5→C6

**High-leverage shortcut:** B9 (contact Arjen for myco-core) can collapse B3 (2→0.5 sessions) and B8 (2→0 sessions). Do this FIRST on B-track.

---

## VPS1 Known Issues (from verified testing investigation)

| Issue | Impact | Fix Task |
|-------|--------|----------|
| Version drift: binary 0.5.0-dev, pin v0.4.0, dashboard 0.3.0-dev | Confusion about what's running | E4 |
| SMOKE-1 test defaults to wrong IP (23.182.128.51) | Tests fail without env var | E5 |
| Cashu paygate: paid_peers set starts EMPTY | WG peers connect but no internet without payment. By design (EXIT-6). | E3 |
| Dashboard is static HTML, not live | Playwright tests verify rendering, not reality | (low priority) |
| No CI pipeline | Manual testing only | (future) |
| No multi-peer test | Single peer only | (future) |

**What works well on VPS1:**
- WG tunnel UP: 15s handshake, bidirectional traffic confirmed
- IP forwarding enabled, internet reachable
- FIPS daemon active (running since Jul 5)
- SMOKE-1 test code quality is high (skip logic, env config, good docstrings)
- Two Hermes crons exist: daily smoke 06:00, health monitoring 15min

---

## What NOT to Do

- Don't use FIPS master (sans-io refactor in progress)
- Don't use v0.4.0 tag alone (no Android support)
- Don't create a system TUN on Android — use `enable_app_owned_tun()`
- Don't push non-fd00::/8 packets through the TUN seam
- Don't call JNI on the byte hot path — use the channel pattern
- Don't forget MSS clamping on outbound SYNs
- Don't forget reconnection logic (5s→60s backoff, FIPS has none)
- Don't require root — VpnService API needs no root
- Don't use native-tls — use rustls for Android cross-compile
- Don't turn on reseller mode for testing

---

## Exit Node Details

```
IP:         66.92.204.38
UDP port:   2121 (primary)
TCP port:   8443 (fallback)
NPUB:       npub1mqelkzqp4659fws35h2wvr7z9caka5ml8qddj3ssnwaulwpxdd9sdc3esw
APP TAG:    fips-overlay-v1
HANDSHAKE:  Noise XK (phone is initiator)
TUN MTU:    1280
NFTable:    inet fips-exit (NOT ip)
Cashu gate: paid_peers set (1h timeout, starts EMPTY)
```

---

*Document version: 3.0 | 2026-07-06*
*Sources: ANDROID-LLM-POINTERS.md (308 lines), HANDOVER-ANDROID-APP.md, VPS1 testing investigation*

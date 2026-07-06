# TollGate Dual-Role + FIPS Architecture — Corrected Plan v2

> **Supersedes:** DUAL-ROLE-FIPS-PLAN.md (v1.0) and fips-dual-role-plan.md
> **Key correction:** Phone is a regular FIPS node (like Myco), NOT an exit node.
> VPS1 is the only exit. FIPS mesh routing IS the valve.

## The Core Insight

Android phones have no firewall. No nftables. No per-client control.

**FIPS solves this.** The phone joins the FIPS mesh as a regular peer. FIPS routing decisions are the valve: the phone decides whose mesh traffic to forward toward VPS1. Pay → forward. Stop paying → drop. No firewall rules needed.

VPS1 does the actual NAT to the real internet (nftables MASQUERADE → eth0). The phone never exits traffic itself — it just routes within the mesh.

## Architecture (Corrected)

```
SCENARIO A — Phone as Customer (no FIPS needed):
  Phone → Router WiFi → Router :2121 (Cashu pay) → Router WAN → Internet

SCENARIO B — Phone as Vendor (FIPS = the valve):
  Client → Phone hotspot → Phone :2121 (Cashu pay)
  Phone routes client traffic into FIPS mesh → VPS1 exit → Internet
  Phone controls access via FIPS routing (forward/drop per paying client)

SCENARIO C — Router with FIPS backhaul:
  Phone → Router :2121 (Cashu pay) → Router FIPS → VPS1 → Internet
  Router handles payment + FIPS backhaul, customer needs nothing special
```

**What "regular FIPS node like Myco" means:**
- Phone joins FIPS mesh (Noise XK handshake)
- Phone owns TUN via `enable_app_owned_tun()` — channel-based, NOT raw fd
- TUN only carries mesh traffic (fd00::/8 ULA), NOT 0.0.0.0/0
- Phone does NOT run WireGuard egress, does NOT NAT to real interface
- VPS1 is THE exit node — does NAT to internet
- Myco narrowed TUN to only route mesh + DNS-intercept .fips/.nsite

## Critical Dependency: enable_app_owned_tun()

This API exists in **Origami74's FIPS fork** (ble-v2 branch), NOT in our `reference/fips` clone (k0sti/fips master). 

**Signature:**
```rust
pub fn enable_app_owned_tun(&mut self) -> (TunOutboundTx, std::sync::mpsc::Receiver<Vec<u8>>)
```
- `TunOutboundTx` = `tokio::sync::mpsc::Sender<Vec<u8>>` (app→mesh)
- `Receiver<Vec<u8>>` = blocking receiver (mesh→app)
- Call after `Node::new()`, before `start()`. `start()` skips system TUN creation.
- Embedder owns VpnService fd, pumps fd↔channels, pushes only fd::/8 IPv6 packets, clamps TCP MSS.

**Myco's pattern** (`myco-core/src/runtime.rs:535`): `tun_bridge::install(...)` stashes channel ends in `OnceLock<Mutex>` statics; JNI exports `send_packet`/`next_packet` pump them lock-free.

**BLOCKER:** Must obtain Origami74/ble-v2 FIPS fork or port the function. Our reference clone lacks it.

---

## Workstreams with Schedulable Tasks

### Workstream A — App Customer Flow (CRITICAL PATH)

> **Goal:** Phone pays a TollGate router with real Cashu and gets internet.
> **Dependency:** None. Can start immediately.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| A1 | Fix discovery 0/9 issue — phone must be on same network as gateway, or add VPS1 HTTP gateway to seed list | 0.5 session | — |
| A2 | Finish CDK wallet integration (wallet.rs is 90% done, needs cargo check clean + UniFFI methods) | 1 session | — |
| A3 | Auto-topup: on low balance during consume loop, auto-mint from testnut | 0.5 session | A2 |
| A4 | Replace `build_bootstrap_token()` stub with `wallet.send_token()` in `pay()` | 0.5 session | A2 |
| A5 | Wallet UI screen: balance display, mint button, transaction history | 1 session | A2 |
| A6 | End-to-end test: phone pays T470 gateway with real testnut tokens, gets session | 1 session | A4 |

**A2 detail:** wallet.rs exists (200 lines, CDK deps added). Remaining: fix last compile errors, wire `CashuWallet` into `TollgateMobileNode` struct, add UniFFI exports (`wallet_balance`, `wallet_mint`, `wallet_send`). Cargo.toml already has `cdk` + `cdk-redb` deps.

### Workstream B — FIPS Embedded in App (HARDEST TRACK)

> **Goal:** Phone runs FIPS as regular mesh node, routes traffic through VPS1.
> **Dependency:** B1 blocks B2-B5. B1 is the bottleneck.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| B1 | Obtain FIPS with `enable_app_owned_tun()` — clone Origami74/ble-v2 fork OR cherry-pick into our reference | 1 session | — |
| B2 | Create `TunProvider` trait in tollgate-mobile, wire FIPS Node with channel-based TUN | 1 session | B1 |
| B3 | `FipsVpnService.kt` — Android foreground service, VpnService.Builder, fd↔channel pump (model on Myco tun_bridge) | 2 sessions | B2 |
| B4 | FIPS config builder: VPS1 endpoint (66.92.204.38:2121), npub, persistent keys | 0.5 session | B2 |
| B5 | Reconnect loop — FIPS v0.4.0 has none, must build app-side retry | 1 session | B3 |
| B6 | Cross-compile FIPS to aarch64-linux-android (cargo ndk) | 1 session | B1 |
| B7 | Integration test: phone → FIPS mesh → VPS1 → Internet, verify egress | 1 session | B3,B6 |

**B1 is the critical unknown.** Options:
1. Clone `github.com/Origami74/fips` branch `ble-v2` — has the function ready
2. Cherry-pick just the `enable_app_owned_tun` + TUN skip changes onto v0.4.0 tag
3. Ask Origami74 for the fork URL (they built Myco on it)

### Workstream C — Vendor Mode (Phone as Gateway)

> **Goal:** Phone is a Cashu-gated hotspot. FIPS routing = the valve.
> **Dependency:** B-track (phone must be on FIPS mesh first). Reseller mode OFF.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| C1 | Port `tollgate-net` server logic to tollgate-mobile (reverse of client: accept payment, open session) | 2 sessions | A4 |
| C2 | UniFFI methods: `start_vendor`, `stop_vendor`, `get_earnings`, `get_clients` | 0.5 session | C1 |
| C3 | Vendor UI: pricing config, earnings dashboard, connected clients list | 1.5 sessions | C2 |
| C4 | Hotspot activation via `WifiManager.startLocalOnlyHotspot` (user confirms — GrapheneOS correct behavior) | 1 session | C2 |
| C5 | FIPS valve logic: phone forwards paying client's mesh traffic toward VPS1, drops non-payers | 1.5 sessions | B7,C1 |
| C6 | End-to-end test: client pays phone hotspot → phone routes via FIPS → VPS1 → Internet | 1 session | C5 |

**C5 is the conceptual core.** The phone doesn't need firewall rules. FIPS mesh routing IS the access control:
- Client pays phone at :2121 → phone adds client's mesh address to "forward" list
- Client stops paying → phone removes from "forward" list, mesh traffic drops
- VPS1 does the actual NAT; phone just decides what to relay

### Workstream D — Physical Routers (Independent)

> **Goal:** OpenWRT routers run tollgate-module-basic-go + optional FIPS backhaul.
> **Dependency:** None (independent track). Reseller mode OFF for testing.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| D1 | Clarify: are 192.168.1.200 / 10.47.41.203 actual router devices to flash, or T470 interfaces? | 0 (question) | — |
| D2 | Flash OpenWRT on target router(s) | 1 session | D1 |
| D3 | Build tollgate-module-basic-go for router target (mipsel or armv7) | 1 session | D1 |
| D4 | Create OpenWRT .ipk package + procd init script for tollgate-go | 1 session | D3 |
| D5 | FIPS OpenWRT package (optional — only if router needs FIPS backhaul) | 2 sessions | D3 |

**For testing, D2-D4 is enough.** Router runs tollgate-go, phone pays it directly. FIPS on router (D5) is a later enhancement for encrypted backhaul.

### Workstream E — VPS1 Infrastructure (Independent)

> **Goal:** VPS1 FIPS exit node stable, Nostr adverts working, Cashu gate verified.
> **Dependency:** None.

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| E1 | Fix VPS1 FIPS Nostr advert timeout (relays timing out) | 1 session | — |
| E2 | Deploy Nostr publisher cron on VPS1 (script written, not deployed) | 0.5 session | E1 |
| E3 | Verify VPS1 Cashu paygate: paid_peers nftables set starts EMPTY, test payment opens it | 1 session | — |

### Workstream F — Auto-Discovery (After B-track)

> **Goal:** Phone auto-discovers TollGate WiFi networks + FIPS mesh nodes.
> **Dependency:** B-track (FIPS mesh discovery needs FIPS embedded).

| Task ID | Description | Est. | Deps |
|---------|-------------|------|------|
| F1 | WiFi SSID scan: `WifiManager.startScan()`, filter TollGate-* SSIDs, GrapheneOS perms | 1 session | — |
| F2 | FIPS mesh discovery: query running FIPS node for known peers | 1 session | B7 |
| F3 | Auto-connect: merge WiFi + FIPS + Nostr + seed results, auto-pay on connect | 1 session | F1,F2,A4 |

---

## Dependency Graph

```
A1 ───────────────────────────────────────────────→ (testable: discovery works)
A2 → A3                                           │
A2 → A4 → A6 ────────────────────────────────────→ (testable: phone pays gateway)
A2 → A5                                             
                                                   
B1 → B2 → B3 → B5                                 │
B1 → B6                                            │
         B3,B6 → B7 ─────────────────────────────→ (testable: phone on FIPS mesh)
                                                   
         A4 → C1 → C2 → C3                        │
              C2 → C4                             │
         B7,C1 → C5 → C6 ────────────────────────→ (testable: phone is vendor)
                                                   
D1 → D2 → D3 → D4 ──────────────────────────────→ (testable: physical router)
                                                   
E1 → E2                                           │
E3 ──────────────────────────────────────────────→ (testable: VPS1 stable)
                                                   
F1                                                │
B7 → F2                                           │
F1,F2,A4 → F3 ───────────────────────────────────→ (testable: auto-discovery)
```

**Critical path:** A2 → A4 → B1 → B2 → B3 → B7 → C5 → C6

## What Was Wrong in v1

| v1 Assumption | Correction |
|---------------|------------|
| Phone is FIPS exit node in vendor mode | Phone is regular FIPS node. VPS1 is the only exit. |
| VpnService routes two subnets (phone + hotspot) | VpnService only carries mesh traffic (fd00::/8). No 0.0.0.0/0 capture. |
| Phone does NAT for hotspot clients | VPS1 does NAT. Phone controls via FIPS routing (the valve). |
| Phone sources internet "from FIPS exit" | Phone routes THROUGH FIPS mesh to VPS1 exit. Phone is never an exit. |
| Reseller mode needed | Reseller mode OFF for testing. Buy directly from router. |

## Reseller Mode (Clarification)

Router reseller mode = router auto-connects to upstream TollGate and buys internet to resell. Useful when router has no direct WAN. 

**For testing: OFF.** Router has direct WAN. Phone pays router directly. Reseller mode is a deployment-time feature for multi-hop chains, not needed for MVP.

## FIPS Fork Required

Our `reference/fips` clone (k0sti/fips master) does NOT have `enable_app_owned_tun()`. Three options:

1. **Clone Origami74/fips ble-v2** — ready to use, what Myco builds on
2. **Cherry-pick** the TUN function onto v0.4.0 tag
3. **Implement from scratch** using Myco's tun_bridge as reference (~200 LOC)

Recommend option 1 (fastest). Need Origami74's fork URL.

---

*Document version: 2.0 | 2026-07-06*
*Supersedes: DUAL-ROLE-FIPS-PLAN.md v1.0, fips-dual-role-plan.md*

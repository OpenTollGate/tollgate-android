# TollGate Dual-Role + FIPS Architecture Plan

## Vision

The phone is **both customer and vendor**. Routers run FIPS as encrypted backhaul.

```
SCENARIO A — Phone as Customer:
  Phone → TollGate WiFi → Router :2121 (Cashu pay) → Internet
  Phone → FIPS tunnel → VPS1 Exit → encrypted egress

SCENARIO B — Phone as Vendor:
  Client → Phone hotspot → Phone :2121 (Cashu pay) → Internet
  Phone sources internet from: cellular / upstream TollGate / FIPS exit

SCENARIO C — Router with FIPS backhaul:
  Phone → Router :2121 (Cashu pay) → Router FIPS → VPS1 Exit → Internet
  Router is both TollGate gateway AND FIPS mesh node
```

## Current State

| Component | Status | Gap |
|-----------|--------|-----|
| tollgate-module-basic-go | ✅ Customer gating, reseller mode, Nostr :4242 | No FIPS integration. No Android build. |
| tollgate-android (app) | ✅ Customer flow (detect→pay→consume), 5 screens | No vendor mode. No real Cashu wallet. No FIPS tunnel. |
| FIPS v0.4.0 | ✅ Noise XK mesh, WireGuard exit, Cashu gate | Android = client-only. ble-v2 has TUN fd solution but unmerged. |
| VPS1 exit node | ✅ Running, reachable | Nostr relay timeout on some relays |

## Architecture: Three Layers

### Layer 1 — TollGate Protocol (payment + access gating)

Already exists in `tollgate-module-basic-go`. HTTP/CBOR on :2121, Nostr on :4242.

**Customer flow (phone → router):**
1. Phone discovers router (WiFi scan or Nostr kind 30078)
2. Phone connects to router WiFi
3. Phone probes `http://<gateway>:2121/` → gets advertisement (pricing)
4. Phone pays with Cashu token → gets session (time/bytes)
5. Router opens nftables/ndsctl gate for phone's MAC
6. Phone has internet

**Vendor flow (phone as gateway):**
1. Phone activates hotspot
2. Phone runs TollGate server on :2121 (reverse of client logic)
3. Client connects, pays, gets access
4. Phone routes client traffic via its own internet source

### Layer 2 — FIPS Mesh (encrypted backhaul)

FIPS creates encrypted tunnels between mesh nodes using Noise XK handshake.
All FIPS traffic exits via VPS1 (66.92.204.38) through WireGuard + nftables.

**Router running FIPS:**
1. Router joins FIPS mesh as a peer
2. Router's backhaul traffic goes through FIPS → VPS1 → Internet
3. Customers still pay at :2121 (TollGate layer)
4. Router's upstream connection is encrypted via FIPS mesh

**Phone running FIPS (customer mode):**
1. Phone has basic internet (from TollGate router or cellular)
2. Phone starts FIPS tunnel to VPS1
3. VpnService routes phone traffic through FIPS → VPS1 → Internet
4. Phone's traffic is encrypted end-to-end

**Phone running FIPS (vendor mode):**
1. Phone sources internet via FIPS tunnel to VPS1
2. Phone runs TollGate server on :2121
3. Clients pay phone → phone routes their traffic through FIPS → VPS1 → Internet
4. Phone is a mobile, encrypted, Cashu-gated hotspot

### Layer 3 — Cashu Wallet (payment + ecash)

**Test mint:** testnut.cashu.space
**Production mints:** configurable

Customer wallet:
- Hold ecash tokens
- Pay TollGate gateways for access
- Auto-topup from Lightning when low

Vendor wallet:
- Receive payments
- Sweep to Lightning
- Track earnings

---

## Phase Plan

### Phase 0 — Foundations (CURRENT) ✅
- [x] Android app builds, launches, renders
- [x] Customer flow works (detect → pay → consume)
- [x] Discover screen with seed candidates + Nostr discovery
- [x] T470 gateways running (192.168.1.200:4747, 10.47.41.203:4747)
- [x] VPS1 FIPS exit running
- [x] FIPS integration documented

### Phase 1 — Real Cashu Wallet (1-2 weeks)
**Goal:** Phone can mint, hold, and spend real testnut ecash.

| Task | Component | Effort |
|------|-----------|--------|
| Replace stub wallet with CDK-based Cashu wallet | Rust core | 3 days |
| Implement NUT-04 (mint) + NUT-05 (melt) | Rust core | 2 days |
| Wallet UI: balance, history, mint management | Kotlin | 2 days |
| Auto-mint testnut tokens on first launch | Rust + Kotlin | 1 day |
| Pay flow: real Cashu token instead of stub | Rust core | 1 day |
| Tests + video evidence | Both | 2 days |

**Deliverable:** Phone can pay a TollGate gateway with real testnut Cashu.

### Phase 2 — WiFi Discovery + Auto-Connect (3-5 days)
**Goal:** Phone auto-discovers TollGate WiFi networks.

| Task | Component | Effort |
|------|-----------|--------|
| WifiManager.startScan() + SSID filter | Kotlin | 1 day |
| Location permission flow (GrapheneOS-compatible) | Kotlin | 0.5 day |
| Auto-connect to TollGate-* SSIDs | Kotlin | 1 day |
| Merge WiFi results into Discover screen | Kotlin | 0.5 day |
| Tests + video evidence | Kotlin | 1 day |

**Deliverable:** Phone auto-discovers TollGate routers in the wild.

### Phase 3 — FIPS Embedded in App (2-3 weeks) — THE HARD PART
**Goal:** Phone runs FIPS tunnel to VPS1 as customer.

**Key insight:** FIPS `ble-v2` branch has `Node::enable_app_owned_tun()` which returns channel-based TUN I/O. This is the Android solution — VpnService owns the fd, FIPS exchanges bytes via channels.

| Task | Component | Effort |
|------|-----------|--------|
| Port `enable_app_owned_tun()` from ble-v2 to v0.4.0 | Rust (fips-core) | 3 days |
| Create `TunProvider` trait in tollgate-mobile | Rust | 1 day |
| Implement Android `TunProvider` via VpnService | Kotlin | 3 days |
| `FipsVpnService` (foreground service, VpnService.Builder) | Kotlin | 2 days |
| FIPS config builder (VPS1 endpoint, npub, keys) | Rust + Kotlin | 2 days |
| Reconnect loop (FIPS v0.4.0 has none) | Rust | 2 days |
| Cross-compile FIPS to aarch64-linux-android | Rust + NDK | 2 days |
| Integration test: phone → FIPS → VPS1 → Internet | Both | 2 days |
| Video evidence | — | 1 day |

**Deliverable:** Phone has encrypted egress through VPS1 via FIPS tunnel.

**Risk:** `enable_app_owned_tun()` is on ble-v2, not v0.4.0. May need manual port or rebasing. Mitigation: start from v0.4.0 tag, cherry-pick the TUN trait changes only.

### Phase 4 — Vendor Mode (2-3 weeks)
**Goal:** Phone is a Cashu-gated hotspot.

| Task | Component | Effort |
|------|-----------|--------|
| Port `tollgate-net` server logic to tollgate-mobile | Rust | 3 days |
| UniFFI methods: `start_vendor`, `stop_vendor`, `get_earnings` | Rust | 1 day |
| Vendor UI: pricing config, earnings dashboard, peer list | Kotlin | 3 days |
| Hotspot activation via `WifiManager.startLocalOnlyHotspot` | Kotlin | 2 days |
| DHCP + routing for hotspot clients | Kotlin (VpnService) | 3 days |
| TollGate server on :2121 (HTTP/CBOR) | Rust | 2 days |
| Route client traffic through FIPS tunnel | Rust + Kotlin | 2 days |
| Tests + video evidence | Both | 2 days |

**Deliverable:** Phone sells internet access. Client pays phone with Cashu, phone routes through FIPS to VPS1.

**GrapheneOS constraint:** `startLocalOnlyHotspot` requires user confirmation dialog. Cannot be fully automatic. This is correct behavior — user must consent to sharing their connection.

### Phase 5 — Router FIPS Integration (1-2 weeks)
**Goal:** OpenWRT routers run FIPS as encrypted backhaul.

| Task | Component | Effort |
|------|-----------|--------|
| Cross-compile FIPS to mipsel/armv7 (OpenWRT targets) | Rust + NDK | 2 days |
| OpenWRT package (.ipk) for FIPS daemon | Shell | 1 day |
| FIPS init script (procd) | Shell | 1 day |
| Integration: FIPS config + TollGate config coexist | Config | 1 day |
| Test: phone pays router → router backhauls via FIPS | Field test | 2 days |
| Documentation: router setup guide | Docs | 1 day |

**Deliverable:** Router sells internet via TollGate, backhauls through FIPS mesh. Customer doesn't need FIPS on their phone — router handles it.

---

## Architecture Diagram

```
                    ┌──────────────────────────────────────┐
                    │           VPS1 (EXIT NODE)            │
                    │  FIPS daemon (Noise XK, :2121)       │
                    │  WireGuard wg0 (10.99.99.0/24)       │
                    │  nftables MASQUERADE → eth0          │
                    │  Cashu gate (paid_peers set)         │
                    └────────────────┬─────────────────────┘
                                     │ FIPS mesh
                    ┌────────────────┴─────────────────────┐
                    │                                      │
          ┌─────────┴──────────┐          ┌───────────────┴────────┐
          │   ROUTER (OpenWRT)  │          │    PHONE (Android)     │
          │                     │          │                        │
          │ tollgate-go :2121   │          │ CUSTOMER MODE:         │
          │   (Cashu gating)    │          │  TollGate client       │
          │ FIPS daemon (mesh)  │          │  → pay router          │
          │ Nostr :4242         │          │  → internet via WiFi   │
          │                     │          │                        │
          │ WiFi AP: TollGate-X │          │ VENDOR MODE:           │
          │                     │          │  TollGate server :2121 │
          │ Backhaul:           │          │  FIPS client → VPS1    │
          │  FIPS → VPS1 → Net  │          │  Hotspot AP            │
          └─────────┬───────────┘          │  Clients pay phone     │
                    │                      │  Traffic exits VPS1    │
                    │ WiFi                 └────────────────────────┘
          ┌─────────┴───────────┐
          │   CLIENT (phone)    │
          │                     │
          │ Connects to WiFi    │
          │ Pays at :2121       │
          │ Gets internet       │
          │ (no FIPS needed)    │
          └─────────────────────┘
```

## Phone App: Dual-Role UI

```
┌─────────────────────────────────────────┐
│  TollGate                          ⚙️   │
├─────────────────────────────────────────┤
│                                         │
│  ┌─────────┐  ┌─────────┐              │
│  │CUSTOMER │  │ VENDOR  │  ← toggle    │
│  └─────────┘  └─────────┘              │
│                                         │
│  ── CUSTOMER MODE ──────────────────    │
│                                         │
│  Discover Gateways          [Scan]      │
│  ┌───────────────────────────────┐     │
│  │ 📶 TollGate-Home  ▓▓▓░ 5ms   │     │
│  │   1 sat/sec · testnut        │     │
│  │   [Connect & Pay]            │     │
│  └───────────────────────────────┘     │
│  ┌───────────────────────────────┐     │
│  │ 🌐 VPS1 Exit    ▓▓▓▓░ 45ms   │     │
│  │   FIPS tunnel · encrypted    │     │
│  │   [Connect]                  │     │
│  └───────────────────────────────┘     │
│                                         │
│  ── WALLET ────────────────────────    │
│  Balance: 1,000 sats (testnut)          │
│  [Top Up]  [History]                    │
│                                         │
│  ── FIPS STATUS ───────────────────    │
│  ● Connected to VPS1 (45ms)            │
│  Egress: encrypted                      │
│                                         │
└─────────────────────────────────────────┘

         ┌─────────────────────────────────┐
         │   VENDOR MODE (toggle)          │
         │                                 │
         │  ── HOTSPOT ────────────────    │
         │  [Start Hotspot]                │
         │  SSID: TollGate-Phone-a3b2     │
         │  Price: 2 sats/sec              │
         │  Internet source: FIPS → VPS1  │
         │                                 │
         │  ── CONNECTED CLIENTS ──────    │
         │  ┌─────────────────────────┐   │
         │  │ 📱 Client-a1b2          │   │
         │  │   Paid: 60 sats         │   │
         │  │   Remaining: 4m 30s     │   │
         │  └─────────────────────────┘   │
         │                                 │
         │  ── EARNINGS ───────────────    │
         │  Today: 340 sats                │
         │  This week: 2,100 sats          │
         │  [Withdraw to Lightning]        │
         │                                 │
         └─────────────────────────────────┘
```

## Technical Decisions

### D1: FIPS Version Strategy
- **Pin:** v0.4.0 (tag da2d0b7)
- **Cherry-pick:** `enable_app_owned_tun()` from ble-v2
- **Avoid:** master branch (sans-io refactor, unstable)
- **Long-term:** When sans-io refactor lands as stable release, migrate

### D2: Phone Vendor Mode — VpnService Routing
Phone in vendor mode needs to:
1. Accept FIPS tunnel (gets fd from VpnService)
2. Accept WiFi clients (via hotspot)
3. Route client traffic → FIPS tunnel → VPS1 → Internet

This requires VpnService to handle TWO subnets:
- Phone's own traffic → FIPS tunnel
- Hotspot client traffic → FIPS tunnel

Android VpnService can do this — add `10.0.0.0/24` (hotspot range) to allowed routes.

### D3: Router FIPS — Two Approaches

**Approach A: FIPS as backhaul (recommended)**
Router runs FIPS daemon alongside tollgate-module-basic-go.
FIPS handles mesh connectivity. TollGate handles payment.
Router's WAN traffic goes through FIPS mesh.

**Approach B: FIPS replaces WireGuard**
Remove WireGuard, use FIPS mesh directly.
More integrated but more work. Not recommended for now.

### D4: Cashu Wallet — CDK in Rust
Use [CDK](https://github.com/cashubtc/cdk) Rust crate for Cashu operations.
Already a dependency in tollgate-mobile.
Implement NUT-04 (mint), NUT-05 (melt), NUT-07 (check spent).

### D5: Nostr Discovery for Vendor Mode
When phone is in vendor mode, it publishes kind 30078 with its hotspot info.
Other phones can discover it via Nostr.
Requires: phone's Nostr key, relay subscription, kind 30078 publishing.

## Dependencies

```
Phase 0 (done) ──→ Phase 1 (Cashu wallet)
                  ──→ Phase 2 (WiFi discovery)
                      ──→ Phase 3 (FIPS embedded)
                          ──→ Phase 4 (Vendor mode)
                          ──→ Phase 5 (Router FIPS)
```

Phases 1 and 2 can run in parallel.
Phase 3 depends on Phase 1 (need wallet for FIPS Cashu gate).
Phase 4 depends on Phase 3 (need FIPS for vendor backhaul).
Phase 5 can start in parallel with Phase 4 (independent — router side).

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `enable_app_owned_tun()` doesn't cleanly port from ble-v2 to v0.4.0 | Medium | High | Start with ble-v2 as base, test early |
| FIPS cross-compile to aarch64 fails on deps | Medium | High | FIPS made optional, isolate behind feature gate |
| Android hotspot + VpnService routing conflicts | Medium | High | Prototype routing early, test with real device |
| GrapheneOS blocks something unexpected | Low | Medium | Test on GrapheneOS early, not just stock Android |
| Cashu CDK API breaking changes | Low | Low | Pin CDK version |
| Router cross-compile (mipsel) fails | Medium | Medium | ARM routers first, mipsel later |

## Success Criteria

**Phase 1:** Phone mints 1000 sats from testnut, pays T470 gateway, gets internet.
**Phase 2:** Phone auto-discovers a TollGate SSID, auto-connects, auto-pays.
**Phase 3:** Phone connects to VPS1 via FIPS, all traffic exits encrypted.
**Phase 4:** Phone hotspot active, client pays, client gets internet through phone → VPS1.
**Phase 5:** Router on FIPS mesh, phone pays router, traffic exits VPS1.

## What to Build Next

**Immediate priority: Phase 1 (Real Cashu Wallet)**

This unblocks everything. Without real Cashu tokens, no payment works.
The stub bootstrap token is a placeholder.

Steps (in order):
1. Implement CDK wallet in Rust core (mint/melt/check)
2. Build wallet UI in Kotlin
3. Replace stub pay flow with real Cashu token
4. Test against T470 gateways
5. Video evidence

**Parallel track: Phase 3 prep (FIPS TUN fd)**
6. Cherry-pick `enable_app_owned_tun()` from ble-v2
7. Create `TunProvider` trait
8. Prototype cross-compile of FIPS to Android

---

*Document version: 1.0 | Last updated: 2026-07-06*
*Author: c03rad0r + Hermes Agent*

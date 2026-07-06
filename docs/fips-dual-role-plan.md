# TollGate Android — FIPS Dual-Role Architecture Plan

> Phone as customer AND vendor. Routers run FIPS. Comprehensive plan.
> Date: 2026-07-06

## Vision

The phone is both:
- **Customer** — buys internet from TollGate routers or FIPS exit nodes
- **Vendor** — sells internet to other phones/devices via FIPS mesh

Both roles require FIPS on the phone:
- **Customer role:** FIPS mesh transport connects to router/exit. Without FIPS, the phone can only use captive-portal WiFi (no mesh, no encrypted tunnel, no vendor capability).
- **Vendor role:** Phone becomes a FIPS node, advertising itself on Nostr. Other devices peer with it, pay it via Cashu, get internet through its FIPS tunnel.

Routers run FIPS as their transport layer — they are FIPS nodes that also expose WiFi APs for non-FIPS clients.

## Architecture: The Full Chain

```
┌─────────────────────────────────────────────────────┐
│                    PHONE (dual-role)                  │
│                                                       │
│  ┌─────────────┐     ┌──────────────┐               │
│  │ CUSTOMER UI  │     │  VENDOR UI   │               │
│  │ Discover     │     │  Pricing     │               │
│  │ Pay          │     │  Earnings    │               │
│  │ Consume      │     │  Connected   │               │
│  └──────┬───────┘     └──────┬───────┘               │
│         │                    │                        │
│  ┌──────┴────────────────────┴───────────────┐       │
│  │         TollgateMobileNode (UniFFI)         │       │
│  │  detect() · pay() · meter() · announce()    │       │
│  ├─────────────────────────────────────────────┤       │
│  │              FIPS NODE (Rust .so)            │       │
│  │  Noise XK · Nostr identity · Peer mgmt       │       │
│  │  Cashu wallet (NIP-60) · Pricing engine      │
│  ├─────────────────────────────────────────────┤       │
│  │          Android VpnService (TUN)            │       │
│  └─────────────────────────────────────────────┘       │
│         │                        │                    │
│    BUY FROM                SELL TO                    │
│         │                        │                    │
└─────────┼────────────────────────┼────────────────────┘
          │                        │
          ▼                        ▼
   ┌──────────────┐        ┌──────────────┐
   │ TOLLGATE      │        │ OTHER PHONE  │
   │ ROUTER (FIPS) │        │ OR DEVICE    │
   │               │        │ (FIPS peer)  │
   │ OpenWRT       │        └──────────────┘
   │ + FIPS daemon │
   │ + ndsctl      │
   │ + Cashu mint  │
   │               │
   │ Reseller mode │
   │ (buys upstream)│
   └───────┬───────┘
           │
           ▼
   ┌──────────────┐
   │ VPS1 FIPS    │
   │ EXIT NODE    │
   │              │
   │ FIPS → WG0   │
   │ → nftables   │
   │ → internet   │
   └──────────────┘
```

## Phone Roles in Detail

### Role 1: Customer (buy internet)

The phone needs internet. It:
1. **Discovers** FIPS-enabled TollGate routers via Nostr (kind 30078 events)
2. **Connects** via FIPS mesh (Noise XK handshake to router's FIPS daemon)
3. **Pays** Cashu token to router's TollGate gateway endpoint
4. **Gets** metered internet access through router's upstream connection
5. **Auto-toppus** when balance runs low or session expires

FIPS is the transport — the encrypted mesh tunnel from phone to router. The TollGate protocol handles payment and metering on top of that tunnel.

**FIPS requirement:** Without FIPS, the phone can't create the encrypted mesh tunnel. It would be limited to plain WiFi captive portals (unencrypted, no vendor capability, no mesh resilience).

### Role 2: Vendor (sell internet)

The phone has internet (via WiFi or cellular) and wants to share/sell it. It:
1. **Starts** FIPS node with `advertise: true` on Nostr
2. **Sets** pricing (per-second, per-byte, or flat rate)
3. **Accepts** incoming FIPS peer connections from other devices
4. **Routes** their traffic through its own internet connection (VpnService)
5. **Charges** Cashu tokens for data delivered
6. **Earns** sats automatically

FIPS is essential here — it's the mesh that other devices connect through. The phone becomes a FIPS exit node itself.

**FIPS requirement:** Without FIPS, the phone can't accept mesh connections. Android's VpnService can route traffic, but without FIPS there's no encrypted peer-to-peer tunnel, no Nostr-based discovery, and no payment-gated access.

## Router Role

Routers run **both** FIPS and TollGate:

```
TollGate Router (OpenWRT)
├── FIPS daemon (mesh transport, Nostr identity, peer discovery)
├── tollgate-module-basic-go (Cashu payment, metering, captive portal)
├── NoDogSplash (ndsctl — gates MAC addresses based on payment)
├── hostapd (WiFi AP for clients)
├── Reseller mode (auto-buys from upstream TollGate/FIPS exit)
└── Cashu wallet (accepts payments, makes upstream payments)
```

The router:
1. Runs FIPS daemon to participate in the mesh network
2. Exposes a WiFi AP for phones to connect
3. Accepts Cashu payments via TollGate protocol
4. Uses FIPS to connect upstream to VPS1 (or another TollGate router)
5. Resells the upstream internet to its WiFi clients

Phones can connect to the router in **two ways**:
- **WiFi AP** (captive portal) — for non-FIPS clients (laptops, old phones)
- **FIPS mesh** — for FIPS-enabled phones (direct peer, encrypted tunnel)

## What FIPS Provides (Why It's Required)

| Capability | Without FIPS | With FIPS |
|------------|-------------|-----------|
| Encrypted tunnel | Plain HTTP/WiFi | Noise XK end-to-end encryption |
| Nostr identity | Separate key management | nsec/npub native to the protocol |
| Peer discovery | Manual URL entry or WiFi SSID scan | Nostr kind 30078 automatic |
| Vendor capability | Captive portal only (local WiFi) | Mesh-wide vendor (any FIPS peer) |
| Multi-hop | Not possible | FIPS mesh routing (resilience) |
| Reconnect | Manual | FIPS re-establishment + retry loop |
| Payment transport | HTTP to gateway | FIPS mesh session (encrypted) |

## Implementation Plan

### Phase 0: Foundation (CURRENT — mostly done)

**Status:** Tollgate-mobile Rust core builds, UniFFI bindings work, APK runs on phone.

| Task | Status |
|------|--------|
| tollgate-mobile Rust core (detect/pay/consume) | ✅ Done |
| UniFFI Kotlin bindings | ✅ Done |
| Jetpack Compose UI (5 screens) | ✅ Done |
| APK builds on DQ05 | ✅ Done |
| JNA fix (@aar) | ✅ Done |
| Nostr relay discovery | ✅ Done (queries kind 30078) |
| LAN gateway testing | ✅ Done (T470 serves :4747) |

**Remaining Phase 0:**
- [ ] Fix 0/9 discovery issue (phone network reachability — likely AP isolation)
- [ ] Verify Nostr discovery finds and probes gateways correctly
- [ ] Test full detect→pay→consume cycle against T470 gateway

### Phase 1: Cashu Wallet Integration (CRITICAL PATH)

The phone needs a real Cashu wallet to pay for internet.

**What needs to be built:**

1. **Cashu mint client** (Rust, in tollgate-mobile)
   - Already have `cashu` crate as dependency (CDK rev 63866dc)
   - Implement: `check_balance()`, `mint_tokens()`, `melt_tokens()`
   - Default mint: testnut.cashu.space (testnet ecash)
   - Store tokens in app-private storage

2. **Wallet UI** (Kotlin/Compose — WalletScreen already exists, needs wiring)
   - Balance display (sats)
   - Mint management (add/remove mints, health check)
   - Token history (minted, melted, spent)
   - Manual topup (paste a Cashu token, or mint from a Lightning invoice)
   - Auto-topup toggle (when balance < threshold, auto-mint)

3. **Auto-topup logic** (Rust, in tollgate-mobile)
   - Monitor balance during consume loop
   - When balance < renewal threshold, automatically mint new tokens
   - For testnet: free minting from testnut.cashu.space
   - For mainnet: would need Lightning payment (future)

4. **Payment integration** (connect wallet to pay flow)
   - `pay()` currently uses bootstrap-token stub
   - Replace with real Cashu token from wallet
   - Gateway validates token, starts session

**Estimated effort:** 2-3 focused sessions. The CDK crate handles most crypto. UI scaffolding exists. Main work is connecting wallet balance → auto-pay → session renewal.

### Phase 2: FIPS Integration (THE HARD PART)

Embed FIPS v0.4.0 into the app as the networking layer.

**Step 2a: Patch FIPS for Android cross-compilation**

FIPS v0.4.0 has 15 compile errors targeting `aarch64-linux-android`:
- `fips/src/upper/tun.rs:808` — `platform::delete_interface()` unresolved
- `fips/src/upper/dns.rs:304` — type mismatch `i32` vs `u32`
- 13 more errors in platform-specific code

**Fix approach:**
1. Create a `TunProvider` trait in FIPS:
   ```rust
   pub trait TunProvider: Send + Sync {
       fn create_tun(&self, name: &str, mtu: u16) -> Result<RawFd>;
       fn delete_tun(&self, name: &str) -> Result<()>;
   }
   ```
2. Gate Linux-specific code behind `#[cfg(target_os = "linux")]`
3. Implement `AndroidTunProvider` that accepts a VpnService fd
4. Gate DNS platform code similarly
5. Cross-compile to `aarch64-linux-android` clean

**This is the hardest single task.** The FIPS TUN code is tightly coupled to Linux `/dev/net/tun`. Android requires `VpnService.Builder.establish()` to get the fd. The Rust code must accept a pre-opened fd instead of creating its own.

**Step 2b: Android VpnService integration**

1. Create `FipsVpnService` extending Android `VpnService`
2. User grants VPN consent (system dialog)
3. VpnService creates TUN fd via `Builder.establish()`
4. Pass fd to FIPS Rust core via JNI/UniFFI
5. FIPS routes mesh traffic through the VpnService TUN

2. Foreground service with persistent notification (required by Android to avoid Doze kill)

3. The VpnService routes:
   - Customer mode: all phone traffic → FIPS tunnel → router/exit → internet
   - Vendor mode: incoming FIPS peer traffic → phone's internet connection

**Step 2c: FIPS node lifecycle management**

1. Start/stop FIPS node from app UI
2. FIPS config generated programmatically (not YAML file)
3. Nostr identity stored in Android Keystore (hardware-backed if available)
4. Reconnect loop: 5s → 10s → 20s → 40s → 60s (capped)
5. Status reporting to UI (connected peers, session state, data transferred)

**Estimated effort:** 4-6 focused sessions. The FIPS TUN patch is the bottleneck — needs careful conditional compilation work and testing.

### Phase 3: Vendor Mode (phone sells internet)

Once FIPS is embedded, vendor mode adds:

1. **Pricing UI** — set per-second and per-byte rates, accepted mints
2. **FIPS advertisement** — publish kind 30078 with pricing + transport info
3. **Incoming peer management** — accept/reject FIPS connections
4. **Payment processing** — Cashu validation, session creation for peers
5. **Metering** — track bytes/seconds delivered to each peer
6. **Earnings dashboard** — sats earned, active sessions, peer list
7. **VpnService routing** — route peer traffic through phone's internet

This reuses the TollGate v2 protocol but in reverse: the phone IS the gateway. The `tollgate-net` server logic needs to run on the phone (in Rust), accepting connections from FIPS mesh peers.

**Estimated effort:** 3-4 sessions after Phase 2. The protocol exists; the work is wrapping it in a vendor-facing UI and wiring it to FIPS mesh sessions.

### Phase 4: Router FIPS Integration

Flash physical routers with OpenWRT + TollGate + FIPS.

**Router software stack:**
```
OpenWRT 25.x (apk packages)
├── tollgate-wrt (tollgate-module-basic-go) — existing
├── fips daemon — NEW: needs OpenWRT package
├── wireguard — existing
├── nodogsplash — existing (captive portal)
└── hostapd — existing (WiFi AP)
```

**Router FIPS config:**
```yaml
node:
  identity:
    nsec: "nsec1..."  # per-router, generated on first boot
  discovery:
    nostr:
      enabled: true
      policy: configured_only
      advertise: true

tun:
  enabled: true
  name: fips0

transports:
  udp:
    bind_addr: "0.0.0.0:2121"
    advertise_on_nostr: true

peers:
  - npub: "npub1mqelkzqp4659..."  # VPS1 exit node
    alias: "vps1-exit"
    addresses:
      - transport: udp
        addr: "66.92.204.38:2121"
    connect_policy: auto_connect
```

**Router reseller flow:**
1. FIPS daemon connects to VPS1 exit (upstream)
2. TollGate module detects FIPS tunnel as upstream internet
3. Phone connects to router WiFi AP (or FIPS mesh directly)
4. Phone pays router via Cashu
5. Router opens access (ndsctl for WiFi clients, FIPS session for mesh peers)
6. Router pays VPS1 for upstream internet (reseller mode)
7. Internet flows: phone → router → VPS1 → internet

**Estimated effort:** 2-3 sessions. Requires physical routers, OpenWRT flashing, FIPS OpenWRT packaging.

### Phase 5: Auto-Discovery and Auto-Connect

The phone automatically finds TollGate routers and connects without manual intervention.

**Discovery sources (in priority order):**
1. **FIPS mesh scan** — query FIPS node for known peers
2. **Nostr relays** — kind 30078 events with `transport: fips` or `transport: tollgate-v2`
3. **WiFi SSID scan** — SSIDs matching `TollGate-*` pattern
4. **Seed list** — hardcoded common router IPs (current Phase 0 approach)
5. **Manual entry** — "add gateway URL" field (already exists)

**Auto-connect logic:**
1. Discovery runs on app launch + every 60s while in foreground
2. Best peer selected by: signal strength → price → latency
3. If auto-connect enabled: detect → pay (auto-topup) → consume automatically
4. If payment fails: try next-best peer
5. If all fail: show error, retry with backoff

**WiFi SSID scan implementation (Kotlin):**
- Requires `ACCESS_FINE_LOCATION` + `ACCESS_WIFI_STATE` permissions
- Use `WifiManager.startScan()` + `BroadcastReceiver`
- Filter results for SSIDs starting with `TollGate-`
- Extract gateway info from SSID suffix (e.g., `TollGate-ABC123` → known router)
- GrapheneOS: must handle location being globally disabled

**Estimated effort:** 2 sessions. Nostr discovery exists. WiFi SSID scan is standard Android. Auto-connect is orchestration logic.

### Phase 6: Polish and Release

1. **Onboarding flow** — first launch: generate identity, topup wallet, grant VPN permission
2. **Background service** — FIPS runs as foreground service, survives Doze
3. **Notifications** — connection status, payment reminders, earnings updates
4. **Dark/light theme** — Material Design 3
5. **i18n** — multi-language support
6. **ZapStore listing** — `zapstore.yaml` for decentralized distribution
7. **Security audit** — Cashu wallet, nsec storage, VpnService isolation

**Estimated effort:** 2-3 sessions.

## Dependency Graph

```
Phase 0 (foundation) ✅ ← we are here
    │
    ├── Phase 1 (Cashu wallet) ← CRITICAL PATH, do next
    │       │
    │       └── Phase 5 (auto-discover/connect)
    │
    ├── Phase 2 (FIPS integration) ← HARDEST, parallel track
    │       │
    │       ├── Phase 3 (vendor mode)
    │       │
    │       └── Phase 4 (router FIPS)
    │
    └── Phase 6 (polish)
```

**Critical path:** Phase 0 → Phase 1 → (customer can buy internet)
**Parallel path:** Phase 0 → Phase 2 → Phase 3 → (vendor can sell internet)
**Infrastructure:** Phase 4 (routers) can start anytime, independent of app

## What to Do Right Now

### Immediate (this session)

1. **Fix the 0/9 discovery issue** — verify phone can reach T470 gateway. If AP isolation, move gateway to 0.0.0.0 bind or use phone's actual network.

2. **Flash the two physical routers** — They have OpenWRT-capable hardware:
   - enp0s31f6 (192.168.1.200) — LAN interface
   - enx00e04c390818 (10.47.41.203) — USB Ethernet adapter

   Install tollgate-wrt package from Nostr (nak fetch kind 1063).

3. **Create FIPS OpenWRT package** — Build FIPS daemon for OpenWRT architectures (aarch64_cortex-a53, mips_24kc, x86_64). This enables routers to join the FIPS mesh.

### Next sessions

4. **Phase 1: Cashu wallet** — Wire testnut.cashu.space into the app. Auto-topup with testnet ecash. This unblocks real payment testing.

5. **Phase 2: FIPS TUN patch** — The single hardest task. Fork FIPS v0.4.0, add `#[cfg(target_os = "android")]` guards, create `AndroidTunProvider`, cross-compile.

6. **Phase 3: Vendor mode** — Once FIPS is embedded, phone becomes a gateway. Reuses TollGate protocol in reverse.

## Key Constraints

- **FIPS v0.4.0 ONLY** — do not track master (sans-io refactor incomplete)
- **Android VpnService** for TUN — no root required, but user must grant VPN consent
- **GrapheneOS hardening** — location permissions for WiFi scan, background execution limits
- **testnut.cashu.space** — default testnet mint for development
- **DQ05 for builds** — T470 OOM-kills cargo-ndk + Gradle
- **Nostr relays** — relay1.orangesync.tech, relay.damus.io, nos.lol (same as VPS1)

## Test Verification Vectors

| Test | Expected Result |
|------|----------------|
| Phone discovers T470 gateway via Nostr | kind 30078 event found, URL probed, gateway detected |
| Phone pays gateway with testnut token | Cashu token accepted, session started |
| Phone consumes internet through gateway | HTTP request succeeds through tunnel |
| Phone auto-toppus when balance low | New tokens minted, session renewed |
| Phone in vendor mode accepts peer | FIPS connection established, peer pays, internet routed |
| Router connects to VPS1 FIPS exit | Noise XK handshake, wg0 tunnel, internet egress |
| Router resells to phone | Phone pays router, router pays VPS1, internet flows |

## References

- TollGate Android master plan: `~/plans/tollgate-android-master-plan.md`
- FIPS transport layer doc: `docs/fips-transport-layer.md`
- Build environment doc: `docs/build-environment.md`
- FIPS handover: `~/repos/fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md`
- TollGate router module: `github.com/OpenTollGate/tollgate-module-basic-go`
- Upstream FIPS: `github.com/jmcorgan/fips` tag v0.4.0
- Myco reference app: `github.com/Origami74/myco` (FIPS Android template)

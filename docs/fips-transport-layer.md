# FIPS as Transport Layer for TollGate Android

> Synthesized from the FIPS exit-node handover document (2026-07-06) and the
> tollgate-android architecture. This is the Phase 2 integration guide: how
> FIPS becomes the networking layer beneath the TollGate v2 protocol.

## Architecture: Two Layers, One App

```
┌─────────────────────────────────────────────┐
│                 Kotlin / Compose UI           │
│  (Status, Pay, Wallet, Discover, Settings)   │
├─────────────────────────────────────────────┤
│            TollgateMobileNode (UniFFI)         │
│   detect() → pay() → startConsume() loop      │
├──────────────────────┬──────────────────────┤
│   TollGate v2 Layer  │    FIPS Mesh Layer    │
│   CBOR protocol      │    Noise XK handshake │
│   Port 4747          │    UDP 2121/TCP 8443  │
│   Payment + metering │    Encrypted tunnel   │
├──────────────────────┴──────────────────────┤
│              Android VpnService (TUN)         │
├─────────────────────────────────────────────┤
│              libtollgate_mobile.so (Rust)     │
└─────────────────────────────────────────────┘
```

**TollGate v2** = payment, pricing, metering (application logic).
**FIPS** = encrypted mesh transport (network layer).

The phone peers with a FIPS exit node (VPS1), gets an encrypted tunnel,
then runs TollGate detect→pay→consume over that tunnel. FIPS is the pipe;
TollGate is the business logic that decides who gets internet and for how
long.

## FIPS Protocol (v0.4.0 — PINNED)

### Version Pin Policy

**CRITICAL: Pin to FIPS v0.4.0, tag commit `da2d0b74b05d7a2bafd67e05fcf7a6edf9afa5d7`.**

- Upstream `jmcorgan/fips` is mid-refactor toward sans-io architecture
- Master branch (v0.5.0-dev) is incomplete and will break features
- v0.4.0 is the last stable pre-refactor release (tagged 2026-06-27)

### Protocol Properties

| Property | Value |
|----------|-------|
| Language | Rust + Tokio async runtime |
| Handshake | Noise XK (encrypted peer connections) |
| Identity | Nostr-native (nsec/npub keypair per node) |
| Discovery | Nostr kind 30078 route advertisements |
| Primary transport | UDP :2121 |
| Fallback transport | TCP :8443 |
| TUN device | Created by daemon for mesh traffic |

### FIPS Internal Crate Structure

| Crate | Responsibility |
|-------|---------------|
| `fips-core` | Noise XK handshake, session management, peer state |
| `fips-net` | UDP/TCP socket layer, transport abstraction |
| `fips-daemon` | TUN device creation, main runtime loop |

For Android, we cross-compile `fips-core` + `fips-net` as a `.so` library.
The daemon's TUN creation is replaced by Android's `VpnService` API.

## VPS1 Exit Node (Test Target)

| Property | Value |
|----------|-------|
| IP | `66.92.204.38` |
| OS | Debian 13 |
| FIPS UDP port | 2121 |
| FIPS TCP port | 8443 |
| WireGuard port | 51821 (UDP) |
| WG subnet | 10.99.99.0/24 (VPS1=10.99.99.1) |
| Exit node npub | `npub1mqelkzqp4659fws35h2wvr7z9caka5ml8qddj3ssnwaulwpxdd9sdc3esw` |
| Nostr relays | relay1.orangesync.tech, relay.damus.io, nos.lol |
| SSH user | debian |
| Domain | fips-exit.orangesync.tech |

### How the Exit Works (Critical)

```
Phone → FIPS mesh → VPS1 fips0 TUN → kernel routing → wg0 (WireGuard)
    → nftables MASQUERADE → eth0 → internet
```

**The nftables MASQUERADE is Cashu-gated.** The `paid_peers` nftables set
starts EMPTY. Without payment, the peer has a tunnel but no internet
(return traffic never comes back — no source NAT).

```
nft add element inet fips-exit paid_peers { 10.99.99.X timeout Ns }
```

### Verified Working

- Raw FIPS Docker container peers with VPS1 in ~5 seconds
- Noise XK handshake completes reliably (never seen a failure)
- Encrypted session established
- E2E bidirectional traffic flows: peer → FIPS → wg0 → nftables → internet
- Session re-establishment works after VPS1 restart (Shutdown + reconnect)

### Verified Broken / Abandoned

| Component | Status | Lesson |
|-----------|--------|--------|
| nvpn (nostr-vpn) | Abandoned | Doesn't reconnect after restart. Use raw FIPS protocol. |
| FIPS v0.2.0 | Too old | Missing Noise XX handshake. Use v0.4.0. |
| FIPS master | Dangerous | Sans-io refactor incomplete. Do NOT use. |
| Docker bridge mode | Broken | TUN needs NET_ADMIN + host network. Irrelevant for Android. |

## FIPS Config for Android (Minimal)

This is the YAML the app must generate at runtime. Note: `tun`, `dns`,
`transports` are at YAML **root level**, NOT nested under `node:`.

```yaml
node:
  identity:
    nsec: "nsec1..."  # generated per-install, stored in Android Keystore
  discovery:
    nostr:
      enabled: true
      policy: configured_only
      app: "fips-overlay-v1"
      advertise: false

tun:
  enabled: true
  name: fips0
  mtu: 1280

transports:
  udp:
    bind_addr: "0.0.0.0:2121"
    advertise_on_nostr: false
  tcp:
    bind_addr: "0.0.0.0:8443"
    advertise_on_nostr: false

peers:
  - npub: "npub1mqelkzqp4659fws35h2wvr7z9caka5ml8qddj3ssnwaulwpxdd9sdc3esw"
    alias: "vps1-exit"
    addresses:
      - transport: udp
        addr: "66.92.204.38:2121"
    connect_policy: auto_connect
```

### Config Gotchas

- `tun/dns/transports` at **root level**, not under `node:` — this caught us out
- `external_addr` is a property of `transports.tcp`, not top-level
- `advertise_on_nostr: true` without `external_addr` = persistent warning log
- Android should generate this config programmatically, not from a file

## The VpnService Challenge (Hardest Part)

FIPS v0.4.0 creates its own TUN device at startup via `/dev/net/tun`. On
Android, TUN devices must be created by the Android framework's
`VpnService` API — direct `/dev/net/tun` access requires root.

### The Problem

FIPS's TUN creation is tightly coupled to the daemon's startup. The Rust
code calls `tun_create()` internally. Android requires:
1. App declares `BIND_VPN_SERVICE` permission
2. User grants VPN consent (system dialog)
3. Android framework creates the TUN fd
4. App receives the fd via `VpnService.Builder.establish()`
5. That fd must be handed to FIPS's routing layer

### The Fix (Two Options)

**Option A: Patch FIPS to accept a pre-opened fd**
- Modify `fips-daemon` to accept a raw fd instead of creating `/dev/net/tun`
- Pass the VpnService fd from Kotlin → JNI → Rust
- This is the cleanest approach but requires forking FIPS

**Option B: Wrap FIPS TUN behind an abstraction**
- Create a trait `TunProvider` in Rust
- Implement `AndroidTunProvider` that reads from the VpnService fd
- Implement `LinuxTunProvider` for testing on desktop
- This is the more maintainable long-term approach

### Android Networking Constraints

| Constraint | Impact |
|------------|--------|
| VpnService required for TUN | No root needed, but user must grant VPN consent |
| Doze mode kills background apps | FIPS must run as foreground service with persistent notification |
| No multi-cast / promiscuous mode | FIPS mesh discovery via Nostr (not local network scan) |
| Keystore for nsec | Hardware-backed key storage if available |

## Reconnect Loop (Must Build)

FIPS v0.4.0 has **no built-in reconnect logic**. The Android app must
implement its own retry loop.

Recommended backoff schedule:
```
5s → 10s → 20s → 40s → 60s (capped)
```

The app should detect disconnection (poll FIPS session state), attempt
reconnect with backoff, and surface status to the user.

## Cashu Payment Gate

Internet egress requires Cashu payment. The flow:

```
1. App generates Cashu token (amount = access duration)
2. App sends token to fips-paygate on VPS1 (REST endpoint)
3. Paygate validates token, adds phone's WG IP to nftables paid_peers set
4. nftables MASQUERADE fires → internet access granted for timeout duration
5. App must re-pay before timeout expires to maintain access
```

### What This Means for the App

Phase 3 (Real Cashu Wallet) in the master plan is not optional — it's
required for the app to actually work. Without payment, the FIPS tunnel
exists but provides no internet egress.

The existing `PayScreen` and `WalletScreen` (Phase 1, committed) will need
to integrate with:
- Real Cashu mint (not bootstrap-token stub)
- VPS1 paygate REST endpoint
- Payment timer / expiry tracking
- Auto-renewal before timeout

## Cross-Compilation

### Targets

| Target | Devices |
|--------|---------|
| `aarch64-linux-android` | Modern ARM64 (most phones) |
| `armv7-linux-androideabi` | Older 32-bit ARM |
| `x86_64-linux-android` | Emulators |

### Toolchain Requirements

```bash
# Rust targets
rustup target add aarch64-linux-android armv7-linux-androideabi

# cargo-ndk (wraps the NDK linker config)
cargo install cargo-ndk

# Android NDK (version 27.2.12479018 as of writing)
# Install via Android SDK command-line tools:
#   sdkmanager "ndk;27.2.12479018"

# JDK (for Gradle)
apt install default-jdk
```

### Build Commands

```bash
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/27.2.12479018

# Cross-compile Rust → .so
cargo ndk -t arm64-v8a build -p tollgate-mobile --release

# Generate UniFFI Kotlin bindings
just bindings   # or: cargo run -p uniffi-bindgen ...

# Build APK
cd android && ./gradlew assembleDebug

# Install on connected device
cd android && ./gradlew installDebug
```

## Upstream Repo Note

The `Cargo.toml` references `reference/fips` as a path dependency. The
handover doc cites upstream as `github.com/jmcorgan/fips`, but the
Cargo.toml clone instructions reference `github.com/k0sti/fips`. These may
be forks — verify which one has the v0.4.0 tag before cloning.

**Pin policy:** whichever repo is used, checkout tag `v0.4.0` commit
`da2d0b74b05d7a2bafd67e05fcf7a6edf9afa5d7`. Do NOT track master.

## References

- Full handover: `~/repos/fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md`
- FIPS exit architecture: `~/repos/fips-exit-e2e/docs/STATUS-AND-DESIGN.md`
- FIPS version pin: `~/repos/fips-exit-e2e/fips-pin.txt`
- TollGate Android master plan: `~/plans/tollgate-android-master-plan.md`
- TollGate/FIPS consolidated plan: `~/plans/tollgate-fips-master-plan.md`
- Skill: `fips-android-handover` (auto-loads on FIPS-Android tasks)

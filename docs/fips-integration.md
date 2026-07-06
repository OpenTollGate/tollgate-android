# FIPS Integration — What We Learned

> Source: `fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md` (commit 7a7570d)
> Synthesized for the tollgate-android app team.

## What FIPS Is

FIPS is a Rust-based mesh networking protocol by jmcorgan. It provides:
- **Encrypted peer-to-peer tunnels** using Noise XK handshake
- **Nostr-native identity** — each node has an nsec/npub keypair
- **Peer discovery** via Nostr kind 30078 events
- **TUN interface** for routing mesh traffic
- **UDP (primary) + TCP (fallback)** transports

## Version Pin: v0.4.0 ONLY

**CRITICAL:** Do NOT use FIPS master branch. It is mid-refactor toward sans-io
architecture and may break everything we depend on.

- Pinned tag: **v0.4.0** (commit `da2d0b74b05d7a2bafd67e05fcf7a6edf9afa5d7`)
- Tagged: 2026-06-27
- Upstream: `github.com/jmcorgan/fips`
- This is the last stable pre-refactor release

## The Exit Node (VPS1)

Fully operational FIPS mesh exit node at **66.92.204.38**.

| Component | Status | Details |
|-----------|--------|---------|
| FIPS daemon | ACTIVE | v0.4.0, UDP :2121, TCP :8443 |
| WireGuard wg0 | UP | :51821, 10.99.99.1/24 |
| nftables | LOADED | MASQUERADE wg0→eth0, **Cashu-gated** |
| IP forwarding | ENABLED | net.ipv4.ip_forward=1 |
| Nostr advert | ACTIVE | Kind 30078 on damus.io, nos.lol |

**Exit npub:** `npub1mqelkzqp4659fws35h2wvr7z9caka5ml8qddj3ssnwaulwpxdd9sdc3esw`

### How the Exit Works

```
1. Peer connects to VPS1 FIPS daemon (UDP :2121 or TCP :8443)
2. FIPS mesh handshake (Noise XK) → encrypted session
3. FIPS creates TUN (fips0) with IP in 10.44.0.0/16
4. Kernel routes fips0 → wg0 (WireGuard, 10.99.99.0/24)
5. wg0 → nftables MASQUERADE → eth0 → internet
6. THE MASQUERADE IS GATED — only IPs in paid_peers nftables set get SNAT
```

### Cashu Payment Gate

The `paid_peers` nftables set starts **EMPTY**. Without payment, tunnel exists
but MASQUERADE doesn't fire → no return traffic → no internet.

```
1. App generates Cashu token (amount = access duration)
2. App sends token to fips-paygate on VPS1 (REST endpoint)
3. Paygate validates, adds phone's WG IP to paid_peers set with timeout
4. Internet access granted for time proportional to payment
5. App must re-pay before timeout expires
```

NFTables rule:
```
nft add element inet fips-exit paid_peers { 10.99.99.X timeout Ns }
```

## FIPS Config Format (v0.4.0)

**Config gotcha:** `tun`/`dns`/`transports` at YAML **root level**, NOT nested
under `node:`. This is the v0.4.0/v0.5.0-dev format.

Minimal config for Android:
```yaml
node:
  identity:
    nsec: "nsec1..."  # generated per-install, stored in Keystore
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

## What's Verified Working

- Raw FIPS Docker container peers with VPS1 in ~5 seconds
- Noise XK handshake reliable (never seen a failure)
- Session re-establishment after disconnect works
- Bidirectional traffic through the tunnel verified
- TCP fallback transport works, UDP preferred (lower latency)

## What Failed / Lessons Learned

| Problem | Cause | Resolution |
|---------|-------|------------|
| FIPS v0.2.0 | Too old, missing Noise XX | Upgraded to v0.4.0 |
| FIPS master | Mid sans-io refactor | Pinned to v0.4.0 |
| nvpn test peer | Didn't reconnect after restarts | Replaced with raw FIPS |
| Docker bridge mode | TUN device needs NET_ADMIN | Use `--network host` |
| `advertise_on_nostr: true` without `external_addr` | Persistent warning | Set external_addr or disable |
| OOM kill (exit 137) | Resource pressure on Hermes machine | Build on higher-RAM machine |

## The Android Challenge: TUN fd Handoff

**This is the single hardest part of the Android port.**

FIPS's TUN creation is tightly coupled to the daemon's startup — it creates
`/dev/net/tun` directly. Android requires the `VpnService` API to create a TUN
file descriptor via the Android framework.

**Options:**
1. Fork FIPS v0.4.0 to accept a pre-opened fd from VpnService instead of
   creating its own `/dev/net/tun`
2. Wrap FIPS in a JNI layer where Rust's TUN creation is replaced with
   Android's VpnService fd
3. Wait for upstream sans-io refactor to land (may decouple TUN from daemon)

## Other Android Considerations

1. **VpnService API** — Android's native way to route device traffic
2. **Foreground service** — FIPS needs a persistent notification to survive Doze
3. **Nostr keys** — Store nsec in Android Keystore (hardware-backed if available)
4. **Config generation** — Build YAML from app settings, not a file
5. **Reconnect loop** — FIPS v0.4.0 has no built-in reconnect; app must implement
   its own retry (recommended: 5s → 10s → 20s → 40s → 60s, capped)

## Key Files

| Resource | Path |
|----------|------|
| Full handover doc | `~/repos/fips-exit-e2e/docs/HANDOVER-ANDROID-APP.md` |
| Docker entrypoint | `~/repos/fips-exit-e2e/docker/entrypoint.sh` |
| Dockerfile | `~/repos/fips-exit-e2e/docker/Dockerfile.fips-node` |
| FIPS upstream | `~/fips/` (v0.4.0 tag) |
| Version pin policy | `~/repos/fips-exit-e2e/fips-pin.txt` |
| Architecture doc | `~/repos/fips-exit-e2e/docs/STATUS-AND-DESIGN.md` |

## Contacts

- **c08r4d0r** — project owner
- **jmcorgan** — FIPS upstream maintainer (johnathan@corganlabs.com)
- **Amperstrand** — contributor

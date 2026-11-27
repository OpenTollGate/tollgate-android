# Live purchase proof — v1 claim, signed and granted against a real exit

**Measured 2026-11-27.** Not a fixture and not a mock: a real
`tollgate-module-basic-go` exit process verified a real BIP-340 schnorr signature
and wrote a real entitlement.

## Result

```
terms    exit=2a16a1d5da25b323 price=1 sat mode=bytes
digest   ff2e7cddac4e9a827c88541cae6ee0ed5c36fe7490a589c0bfc7eca5da75d934
buyer    24653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c
POST /   200 OK
body     {"allotment":60000,
          "device-identifier":"fd53:d228:d23:b402:40f8:2bcf:5482:9611",
          "expires_at":"2026-11-27T11:43:21Z","kind":1022,"metric":"bytes"}
PASS: the exit granted 60000 bytes to fd53:d228:d23:b402:40f8:2bcf:5482:9611
```

The exit's own state file, independently read back:

```json
{"revision": 2, "entitlements": [{"key": [36, 101, ...], "name": "peer-24653eac4344",
 "mode": "bytes", "granted_at": "2026-11-27T12:38:21.403Z",
 "expires_at": "2026-11-27T12:43:21.403Z", "byte_budget": 60000, "revision": 2}]}
```

Replaying the same purchase against the same exit is refused (`400`, "already
holds a grant (renew instead)"), so the 200 is a real state transition rather
than an echo.

## Why the earlier attempt returned 403 `not-yours`

The claim binds the buyer key to a FIPS outer address derived from that key:

```go
// src/fips_exit/address.go
digest := sha256.Sum256(publicKey[:])
address[0] = 0xfd; copy(address[1:], digest[:15])
```

`Claim.Verify` refuses when `Derive(Buyer).outer != observedSource`
(`ErrClaimUnbound` → `403 not-yours`). An invented address therefore fails —
correctly, and with the same status the exit uses for a bad signature so a
caller cannot probe for other peers. The working run passes the **derived**
address.

## Reproduce

The exit's observed-source hook is off unless an operator turns it on, and it
refuses to run where a real buyer can reach it. Stand up a loopback-only exit:

```bash
cd ~/worktrees/tollgate-fips-exit-consumer/src
/usr/local/go/bin/go build -o /tmp/exitserver-test ./cmd/exitserver
mkdir -p /tmp/tgv1test
/tmp/exitserver-test -listen 127.0.0.1:2122 -dir /tmp/tgv1test \
  -price 1 -mint http://127.0.0.1:3338 -allow-test-bind
```

Compute the address the exit will derive from the example's fixed buyer key
(`[0x42; 32]` → x-only `24653eac…`):

```bash
python3 - <<'EOF'
import hashlib, ipaddress
pub = bytes.fromhex('24653eac434488002cc06bbfb7f10fe18991e35f9fe4302dbea6d2353dc0ab1c')
d = hashlib.sha256(pub).digest()
a = bytearray(16); a[0] = 0xfd; a[1:] = d[:15]
print(ipaddress.IPv6Address(bytes(a)))   # fd53:d228:d23:b402:40f8:2bcf:5482:9611
EOF
```

Then run the example with that address as the second argument:

```bash
cargo run -p tollgate-mobile --example buy_live -- \
  http://127.0.0.1:2122 fd53:d228:d23:b402:40f8:2bcf:5482:9611
```

## Limits of this proof — stated, not glossed

- The exit ran over **plain loopback TCP**, not FIPS. What that means: the
  *cryptographic* binding is proven (the exit verified a real signature and
  matched the derived address), but the *transport* binding is not — on a real
  exit `observedSource` reads the accepted FIPS connection, which a caller
  cannot set. `-allow-test-bind` substitutes a caller-supplied address, so this
  run proves the claim format and the verification path, **not** that a
  deployed exit cannot be lied to about its peer.
- The mint on `:3338` is a stub; no money moved. This path is a signed claim,
  not an invoice settlement. The quote→grant invoice flow is a separate lane.
- The buyer key is the example's fixed `[0x42; 32]`, chosen for reproducibility.
  A phone uses its own device identity.
- `reference/fips` is gitignored, so a fresh worktree cannot build until it is
  copied in (the same step `just` and CI perform).

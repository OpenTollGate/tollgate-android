# Architecture — how a phone buys access

This document exists because `tollgate-mobile/src/lib.rs` tells readers that
`ARCHITECTURE.md` tracks the wallet gap, and that file was never written. A
pointer to a decision record that does not exist is worse than no pointer: it
reads as "this was decided" when nothing was.

It records what is built, what is missing, and the choices that must be made
before the missing part can be built. It deliberately does not record a plan.

## What works today

Proven against a real `tollgated` node from the emulator, terms rendered on the
phone from the node's live config:

- The core speaks **v2** over raw TCP to the gateway's control port (`4747` by
  default), length-prefixed CBOR from `tollgate-protocol`.
- `detect()` connects, sends `Announce` (our pubkey and unit), and reads until
  the gateway has both introduced itself and sent an `Offer`, bounded by a
  timeout so a silent gateway cannot hang the caller.
- The UI shows what the gateway actually said: unit, accepted mints, window
  range, upload weighting. It shows **no price**, because `Offer` carries none.

That is the half of the protocol that needs no wallet.

## What is missing, and why it is not a small piece

Buying is a **Spilman channel funded by a Cashu wallet**, and the phone has no
wallet. Concretely, a purchase needs all of:

1. **A Cashu wallet on the device** — keys, proofs, mint interaction, and durable
   proof storage. This crate does not specify a wallet, so the NUTs to speak are
   a decision (see open decisions below). Money, so it needs the same
   crash-consistency rules the router side already learned the hard way: never
   reuse a derivation range, keep keyset counters monotonic, reconcile ambiguous
   results rather than retrying blindly.
2. **A funding blob for the channel** — the payer signs the initial channel
   state; the gateway cannot read a wallet, it verifies signatures.
3. **Signed `TopUp`s as the session runs** — each top-up is what buys the next
   window. `ChannelUpdate`/`RefusedUpdate` and `TopUpReject` carry the answers.
4. **Rollover and close** — `RolloverInit`/`RolloverReady` when a channel fills,
   `ChannelClose`/`CloseAck`/`CloseReason` at the end. A channel left open holds
   money; an unclosed one is not free, it is stuck.
5. **Reconciliation on app death** — Android kills processes. The app must be
   able to resume a channel it funded but never saw acknowledged, and it must
   not double-fund on restart.

None of this is a protocol gap. `tollgate-net` already implements and tests the
gateway side, including a blocked peer being refused and a channel that fills
rolling over while buying continues.

## The price question

`Offer` deliberately carries no price. v2 denominates in a unit and delivers one
voucher per unit, so **the money is the mint's, not the gateway's**. Any UI that
prints a price per second or per unit is stating something the gateway never
said.

So a price shown before payment must come from **the mint's denomination**, and
that raises a choice this document cannot settle alone:

- Show a derived rate (bytes-per-unit from the mint) and label it as derived, or
- show nothing until a channel exists and the first top-up reveals the real cost.

The first informs a buyer; the second never lies. Whatever is chosen, the UI must
not present a gateway price, because there is none.

## What this retires

The earlier v1 exchange (`POST /tollgate/v1/exchange`, `PriceSheet`,
`BootstrapToken`, `MeteringReport`) does not exist in v2. It is documented in
`tollgate-rs` as a *future* transport for peers behind a network that blocks
port 4747 — not as a shipped path. Any implementation of it is a compatibility
lane, and must not be described as the product.

## Open decisions

1. **Wallet library.** In-process Rust (a `cdk`-style wallet) or a narrower
   self-contained implementation? This decides the binary size, the dependency
   surface, and whether the crash-consistency rules above are ours to keep or
   borrowed from a library that already keeps them.
2. **Where proofs live on Android.** The router side uses bbolt under a single
   writer; Android has no equivalent assumption available, and the app is killed
   at will.
3. **How a price is shown** (see above).
4. **What "budget" means to the user.** The gateway's `budget window` is a rate
   limit, not a wallet balance; conflating them would misrepresent the terms.

## Non-goals

- No Tauri, no Flutter, no JavaScript wrapper — native only, per AGENTS.md.
- No price invented on the client to fill a UI field.
- No direct dependency on a mint's HTTP API from Kotlin: everything money-touching
  stays behind the Rust boundary, where it can be tested on the host.

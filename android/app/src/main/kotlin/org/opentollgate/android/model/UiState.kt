package org.opentollgate.android.model

/** Default Cashu mints supported by TollGate routers (from /etc/tollgate/config.json). */
val DEFAULT_MINTS: List<String> = listOf(
    "https://mint.coinos.io",              // coinos — accepted by most gateways, default
    "https://mint.minibits.cash/Bitcoin",  // minibits production mint
    "https://nofee.testnut.cashu.space",   // testnut — zero fees, testing only
    "https://testnut.cashu.exchange",      // alternate testnut
)

/** Primary default mint. coinos.io is accepted by production upstream gateways. */
const val DEFAULT_MINT: String = "https://mint.coinos.io"

/**
 * UI state for the TollGate dashboard. Mirrors the JS captive-portal SPA's
 * Status / Cashu / Balance / Lightning views. Phase 1 splits it across three
 * Compose screens — [PayScreen][org.opentollgate.android.PayScreen] (the
 * payment flow), StatusScreen (live session telemetry), and
 * [WalletScreen][org.opentollgate.android.WalletScreen] (balance, mints, token
 * history) — driven by a single
 * [TollgateViewModel][org.opentollgate.android.TollgateViewModel].
 */
data class UiState(
    val ourPubkey: String = "",
    /** Absolute path of the directory the Rust core persists identity (and
     *  future wallet state) under — Android `context.filesDir`. Shown on the
     *  Settings → Identity card so a user can locate their keys. */
    val dataDir: String = "",
    /** App version (BuildConfig.VERSION_NAME, e.g. "0.1.0") for About. */
    val appVersion: String = "",
    /** Short git SHA baked into the APK (BuildConfig.GIT_HASH) for About. */
    val buildHash: String = "",
    val baseHost: String = "http://192.168.8.1:2121",
    val mintUrl: String = DEFAULT_MINT,
    val detected: DetectedView? = null,
    val paid: PaidView? = null,
    val latest: ConsumeEventView? = null,
    val online: Boolean = false,
    /** True while a `node.pay()` call is in flight — drives the PayScreen
     *  spinner and disables the Pay button. */
    val paying: Boolean = false,
    /** Bootstrap-token amount (sats) the Pay button sends. Editable on PayScreen. */
    val amountSat: Long = 21,
    /** Mint URLs the user can pick from on PayScreen. Grown via onAddMint();
     *  the active selection is [mintUrl]. Real per-gateway mint discovery
     *  (PriceSheet → MintOption) lands in Phase 3. */
    val knownMints: List<String> = DEFAULT_MINTS,
    /** Wall-clock millis (System.currentTimeMillis) of the current consume
     *  session's start, or null when not consuming. Drives the live uptime
     *  display on StatusScreen. Set on the first accepted bootstrap; cleared
     *  when the poll loop ends (stop / max_polls / error). */
    val sessionStartedAt: Long? = null,
    /**
     *  Local Cashu wallet snapshot — per-mint balances + transaction ledger.
     *  Driven by WalletScreen (receive/send); real Cashu mint integration is
     *  Phase 3 (see [WalletState]). */
    val wallet: WalletState = WalletState(),
    /**
     *  Discovered peers from the last Discover scan (see DiscoverScreen).
     *  Empty until the first scan; sorted best-signal-first by the ViewModel.
     *  Phase 2 swaps the seed-list probe for a live FIPS mesh scan. */
    val discovered: List<DiscoveredPeer> = emptyList(),
    /** TollGate WiFi networks found by WiFi SSID scan (Layer 1 discovery). */
    val wifiNetworks: List<String> = emptyList(),
    /** User-added gateway URLs appended to the seed scan candidates. */
    val extraCandidates: List<String> = emptyList(),
    /** True while a Discover scan is probing candidates — drives the spinner. */
    val scanning: Boolean = false,
    /** Cashu token pasted by user for v1 gateway payment. */
    val paymentToken: String? = null,
    /** Gateway-accepted mints from advertisement (GET /). Updated on detect/discover. */
    val gatewayMints: List<String> = emptyList(),
    /** Lightning invoice for Cashu minting (shown to user to pay). */
    val mintInvoice: String? = null,
    /** Cashu quote ID for polling payment status. */
    val mintQuoteId: String? = null,
    /** True while requesting a Lightning invoice from the mint. */
    val mintingInvoice: Boolean = false,
    /** True while polling for Lightning payment confirmation. */
    val mintingWaiting: Boolean = false,
    /** True while minting Cashu tokens after payment. */
    val mintingTokens: Boolean = false,
    /** Minted Cashu token ready for TollGate payment (null = not yet minted). */
    val mintedToken: String? = null,
    /** Device MAC as seen by the gateway (from /whoami). Null until queried. */
    val gatewayMac: String? = null,
    /** Live session balance from gateway (GET /balance). Null when no session. */
    val gatewayBalance: BalanceView? = null,
    /** Last Discover-scan error (per-host reachability is folded into each
     *  DiscoveredPeer; this holds only a whole-scan failure). */
    val discoverError: String? = null,
    val error: String? = null,
)

data class DetectedView(
    val pubkeyHex: String,
    val unit: String,
    val version: UByte,
    val perUnit: Long?,
    val perSecond: Long?,
)

data class PaidView(val peerPubkeyHex: String, val accepted: Boolean, val perUnit: Long?)

data class ConsumeEventView(
    val poll: UInt,
    val remainingScaled: Long,
    val delivered: Long?,
    val cutOff: Boolean,
    val toppedUp: Boolean,
)

/** Live session telemetry from a v1 gateway's /balance endpoint. */
data class BalanceView(
    val sessionActive: Boolean,
    val metric: String,
    val usage: Long,
    val allotment: Long,
    val remaining: Long,
    val startTime: Long,
)

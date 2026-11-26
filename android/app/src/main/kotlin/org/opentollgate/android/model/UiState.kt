package org.opentollgate.android.model

/** Default Cashu mint used to seed [UiState.knownMints] / [UiState.mintUrl]. */
const val DEFAULT_MINT: String = "https://mint.minibits.cash"

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
    val baseHost: String = "http://192.168.8.1:4747",
    val mintUrl: String = DEFAULT_MINT,
    val detected: DetectedView? = null,
    val online: Boolean = false,
    /** Bootstrap-token amount (sats) the Pay button sends. Editable on PayScreen. */
    val amountSat: Long = 21,
    /** Mint URLs the user can pick from on PayScreen. Grown via onAddMint();
     *  the active selection is [mintUrl]. Real per-gateway mint discovery
     *  (PriceSheet → MintOption) lands in Phase 3. */
    val knownMints: List<String> = listOf(DEFAULT_MINT),
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
    /** User-added gateway URLs appended to the seed scan candidates. */
    val extraCandidates: List<String> = emptyList(),
    /** True while a Discover scan is probing candidates — drives the spinner. */
    val scanning: Boolean = false,
    /** Last Discover-scan error (per-host reachability is folded into each
     *  DiscoveredPeer; this holds only a whole-scan failure). */
    val discoverError: String? = null,
    val error: String? = null,
)

/**
 * A gateway as the app knows it after `detect()`.
 *
 * There is no price here, and that is not an omission. Protocol v2's `Offer`
 * carries the mints it accepts and the unit it denominates in, but no money:
 * delivery is one voucher per unit, so the price is set by the mint, not by the
 * gateway. Anything the UI showed as "per unit / per second" would be inventing
 * someone else's price.
 */
data class DetectedView(
    val pubkeyHex: String,
    val unit: String,
    val version: UByte,
    /** Terms, when the gateway offered any. Null means it will not sell. */
    val offer: OfferTermsView?,
)

/** The terms a gateway offered, as shown to the user. */
data class OfferTermsView(
    val acceptedMints: List<String>,
    val unit: String,
    /** Shortest budget window it accepts, milliseconds. */
    val minWindowMs: ULong,
    /** Longest budget it can be kept for, milliseconds. */
    val maxWindowMs: ULong,
    /** What one unit from us draws from the budget, where one unit to us draws one. */
    val fromPayerWeight: UShort,
)

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
    val baseHost: String = "http://192.168.8.1:4747",
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

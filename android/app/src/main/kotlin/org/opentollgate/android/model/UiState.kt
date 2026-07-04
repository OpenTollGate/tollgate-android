package org.opentollgate.android.model

/** UI state for the TollGate dashboard. Mirrors the JS SPA's Status / Cashu /
 *  Balance / Lightning views — here as a single screen pending the §6 source
 *  confirmation, after which it splits into the SPA's component layout. */
data class UiState(
    val ourPubkey: String = "",
    val baseHost: String = "http://192.168.8.1:4747",
    val mintUrl: String = "https://mint.minibits.cash",
    val detected: DetectedView? = null,
    val paid: PaidView? = null,
    val latest: ConsumeEventView? = null,
    val online: Boolean = false,
    val error: String? = null,
)

data class DetectedView(val pubkeyHex: String, val unit: String, val version: UByte, val perUnit: Long?, val perSecond: Long?)
data class PaidView(val peerPubkeyHex: String, val accepted: Boolean, val perUnit: Long?)
data class ConsumeEventView(val poll: UInt, val remainingScaled: Long, val delivered: Long?, val cutOff: Boolean, val toppedUp: Boolean)

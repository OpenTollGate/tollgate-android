package org.opentollgate.android.model

/**
 * One row in the Discover list: the outcome of probing a single candidate
 * TollGate gateway with `TollgateMobileNode.detect()`.
 *
 * Phase 1 sources candidates from [SEED_CANDIDATES] (common router addresses on
 * the v2 `:4747` port) plus any user-added URLs and the current gateway. Each
 * candidate is probed concurrently; reachable peers surface their pubkey, price
 * and a latency-derived [signal] tier. Phase 2 (FIPS integration, master-plan
 * §Phase 2) replaces the seed-list probe with a live FIPS mesh scan feeding
 * these same rows — plus real radio metadata.
 *
 * "Signal" has no real radio RSSI in Phase 1 (no FIPS transport embedded yet),
 * so it is derived from the detect round-trip latency — fast ⇒ STRONG, slow ⇒
 * WEAK. The tier degrades naturally to a real RSSI reading once FIPS lands.
 */
data class DiscoveredPeer(
    /** Full gateway base URL that was probed, e.g. `http://192.168.8.1:4747`. */
    val baseUrl: String,
    /** Peer's compressed pubkey (66 hex chars). Blank when unreachable. */
    val pubkeyHex: String,
    /** `true` when `detect()` returned an Announce within the probe cap. */
    val reachable: Boolean,
    /** Detect round-trip in ms. 0 when unreachable (no response arrived). */
    val latencyMs: Long,
    /** Latency-derived quality tier for the signal-strength indicator. */
    val signal: SignalTier,
    /** Resource unit the gateway meters (from Announce), e.g. "bytes". */
    val unit: String,
    /** Peer's announced TollGate protocol version. */
    val version: UByte,
    /** Per-unit price (scaled milli-sats), or null if the peer published none. */
    val perUnit: Long?,
    /** Per-second price (scaled milli-sats), or null if the peer published none. */
    val perSecond: Long?,
)

/**
 * Coarse signal tier for the Discover list. Ordinal ascends from best to worst
 * so [DiscoveredPeer] lists sort naturally with reachable/strong peers first.
 */
enum class SignalTier { STRONG, MODERATE, WEAK, NONE }

/**
 * Map a detect round-trip latency to a [SignalTier]. Thresholds are tuned for
 * LAN gateway probes (a nearby router answers in tens of ms; a loaded or
 * Wi-Fi-Direct peer in the hundreds). Unreachable always maps to [SignalTier.NONE].
 */
fun signalTier(latencyMs: Long, reachable: Boolean): SignalTier = when {
    !reachable -> SignalTier.NONE
    latencyMs < 150 -> SignalTier.STRONG
    latencyMs < 600 -> SignalTier.MODERATE
    else -> SignalTier.WEAK
}

/**
 * Seed candidate gateway URLs probed by Discover. Curated common-router LAN
 * addresses on the TollGate v2 port `:4747` (the protocol's exchange-endpoint
 * root). The user's current gateway ([UiState.baseHost]) and any user-added
 * URLs are always appended at scan time, then the combined list is deduped.
 *
 * NOTE: this is the Phase 1 stand-in for discovery. Phase 2 embeds a real FIPS
 * node whose mesh-peer list feeds this screen directly — same [DiscoveredPeer]
 * rows, no seed list, real RSSI.
 */
val SEED_CANDIDATES: List<String> = listOf(
    // Known TollGate test gateways (T470 dual-NIC, testnut mint)
    "http://192.168.1.200:4747", // T470 LAN interface — TollGate gateway #1
    "http://10.47.41.203:4747",  // T470 USB-Ethernet — TollGate gateway #2
    // Common router defaults for field deployment
    "http://192.168.8.1:4747",   // H96 / common travel-router default
    "http://192.168.1.1:4747",   // generic home router
    "http://192.168.0.1:4747",   // generic home router
    "http://10.0.0.1:4747",      // alternate home router
    "http://192.168.50.1:4747",  // ASUS / mesh default
    "http://192.168.4.1:4747",   // GL.iNet default
    "http://192.168.43.1:4747",  // Android Wi-Fi hotspot gateway
)

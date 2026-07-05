@file:JvmName("Format")

package org.opentollgate.android.util

import java.util.Locale
import kotlin.math.ln
import kotlin.math.pow

/**
 * Pure formatting helpers shared by the Compose UI. No Android or Compose
 * dependencies, so they are trivially unit-testable on the JVM.
 */

/** Human-readable byte count, SI (1000-base): "512 B", "1.4 MB", "3.0 GB". */
fun formatBytes(bytes: Long): String {
    if (bytes <= 0) return "0 B"
    val units = arrayOf("B", "KB", "MB", "GB", "TB", "PB")
    val digitGroups = (ln(bytes.toDouble()) / ln(1000.0)).toInt().coerceIn(0, units.size - 1)
    if (digitGroups == 0) return "$bytes B"
    val value = bytes / 1000.0.pow(digitGroups.toDouble())
    // US locale → '.' decimal separator, deterministic across devices.
    return String.format(Locale.US, "%.1f %s", value, units[digitGroups])
}

/**
 * Elapsed milliseconds rendered as a compact clock: "3h 5m 12s",
 * "4m 09s", "07s". Leading zero-padded seconds for a stable-width
 * ticking display when minutes/hours are present.
 */
fun formatDurationMillis(ms: Long): String {
    if (ms < 0) return "0s"
    val totalSeconds = ms / 1000
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return buildString {
        if (hours > 0) {
            append("${hours}h ")
            append("${minutes}m ")
            append("%02ds".format(seconds))
        } else if (minutes > 0) {
            append("${minutes}m ")
            append("%02ds".format(seconds))
        } else {
            append("${seconds}s")
        }
    }
}

/**
 * Truncate a hex pubkey for compact display: first [prefix] chars + "…".
 * Returns the input untouched when it is already shorter than [prefix].
 */
fun shortPubkey(hex: String, prefix: Int = 16): String =
    if (hex.length <= prefix) hex else hex.take(prefix) + "…"

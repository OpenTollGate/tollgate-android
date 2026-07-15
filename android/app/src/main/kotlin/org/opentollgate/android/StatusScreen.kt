package org.opentollgate.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import org.opentollgate.android.model.UiState
import org.opentollgate.android.util.formatBytes
import org.opentollgate.android.util.formatDurationMillis
import org.opentollgate.android.util.shortPubkey

/**
 * Phase 1 session dashboard. Shows, top to bottom:
 *  1. FIPS node status — ONLINE/OFFLINE with the connected gateway pubkey;
 *  2. Session telemetry — live-ticking uptime + cumulative data consumed;
 *  3. The gateway's price sheet (when known);
 *  4. The last pollEvent() detail (remaining balance, cut-off / top-up badges);
 *  5. Any error;
 *  6. Stop-session control.
 *
 * The detect/pay controls that used to live here moved to [PayScreen]; this
 * screen is now read-only session monitoring (plus Stop). [state] is driven by
 * [TollgateViewModel], whose consume loop polls `TollgateMobileNode.pollEvent()`
 * and updates [UiState.latest] + the [UiState.sessionStartedAt] that the live
 * uptime display reads.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatusScreen(
    state: UiState,
    onStop: () -> Unit,
) {
    Scaffold(topBar = { TopAppBar(title = { TollGateTitle(subtitle = "Status") }) }) { pad ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            StatusHero(online = state.online, gatewayPubkey = gatewayPubkey(state))

            SessionCard(state = state)

            // V1 gateway balance card — shows live usage from /balance endpoint
            state.gatewayBalance?.let { bal ->
                InfoCard(title = "Gateway session") {
                    TelemetryRow(label = "status", value = if (bal.sessionActive) "active" else "no session")
                    if (bal.sessionActive) {
                        TelemetryRow(label = "used", value = formatBytes(bal.usage))
                        TelemetryRow(label = "allotment", value = formatBytes(bal.allotment))
                        TelemetryRow(label = "remaining", value = formatBytes(bal.remaining))
                        val pct = if (bal.allotment > 0) (bal.usage * 100 / bal.allotment) else 0
                        TelemetryRow(label = "used %", value = "$pct%")
                        TelemetryRow(label = "metric", value = bal.metric)
                    }
                }
            }

            // Gateway info card — accepted mints, MAC, price
            if (state.gatewayMints.isNotEmpty() || state.gatewayMac != null) {
                InfoCard(title = "Gateway info") {
                    state.gatewayMac?.let {
                        TelemetryRow(label = "your MAC", value = it)
                    }
                    if (state.gatewayMints.isNotEmpty()) {
                        TelemetryRow(label = "accepted mints", value = "${state.gatewayMints.size}")
                        state.gatewayMints.forEach { mint ->
                            Text(
                                "  $mint",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
            }

            state.detected?.let { d ->
                if (d.perUnit != null || d.perSecond != null) {
                    InfoCard(title = "Price") {
                        TelemetryRow(label = "per unit", value = d.perUnit?.toString() ?: "—")
                        TelemetryRow(label = "per second", value = d.perSecond?.toString() ?: "—")
                        TelemetryRow(label = "unit", value = d.unit)
                    }
                }
            }

            state.latest?.let { ev ->
                InfoCard(title = "Last poll · #${ev.poll}") {
                    TelemetryRow(label = "remaining", value = ev.remainingScaled.toString())
                    ev.delivered?.let {
                        TelemetryRow(label = "delivered", value = formatBytes(it))
                    }
                    when {
                        ev.cutOff -> Text(
                            "⚠ balance cut off — topping up",
                            color = MaterialTheme.colorScheme.error,
                        )
                        ev.toppedUp -> Text(
                            "topped up ✓",
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }

            state.error?.let { Text("error: $it", color = MaterialTheme.colorScheme.error) }

            Spacer(Modifier.height(4.dp))
            Controls(state = state, onStop = onStop)
        }
    }
}

/** Prefer the confirmed peer pubkey from a paid session; fall back to detected. */
private fun gatewayPubkey(state: UiState): String? =
    state.paid?.peerPubkeyHex?.takeIf { it.isNotBlank() }
        ?: state.detected?.pubkeyHex?.takeIf { it.isNotBlank() }

@Composable
private fun StatusHero(online: Boolean, gatewayPubkey: String?) {
    val (dotColor, label) = if (online) {
        MaterialTheme.colorScheme.primary to "ONLINE"
    } else {
        MaterialTheme.colorScheme.outline to "OFFLINE"
    }
    InfoCard(title = "FIPS node") {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(12.dp)
                    .clip(CircleShape)
                    .background(dotColor),
            )
            Spacer(Modifier.width(8.dp))
            Text(label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
        }
        Spacer(Modifier.height(10.dp))
        Text(
            "connected gateway",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(2.dp))
        Text(
            gatewayPubkey?.let { shortPubkey(it) } ?: "not connected",
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodyLarge,
        )
    }
}

@Composable
private fun SessionCard(state: UiState) {
    val startedAt = state.sessionStartedAt
    // Re-tick once a second while a session is active so the uptime clock moves
    // between polls (the consume loop only emits a pollEvent every ~5s).
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(startedAt) {
        if (startedAt != null) {
            while (true) {
                now = System.currentTimeMillis()
                delay(1000)
            }
        }
    }
    val uptime = startedAt?.let { (now - it).coerceAtLeast(0L) }
    val dataBytes = state.latest?.delivered
    val active = startedAt != null

    InfoCard(title = "Session") {
        TelemetryRow(label = "status", value = if (active) "active" else "idle")
        TelemetryRow(label = "uptime", value = uptime?.let { formatDurationMillis(it) } ?: "—")
        TelemetryRow(label = "data consumed", value = dataBytes?.let { formatBytes(it) } ?: "—")
    }
}

@Composable
private fun Controls(state: UiState, onStop: () -> Unit) {
    val active = state.sessionStartedAt != null
    OutlinedButton(
        onClick = onStop,
        enabled = active,
        modifier = Modifier.fillMaxWidth(),
    ) { Text("Stop session") }
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(
            "host  ${state.baseHost}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Text(
            "mint  ${state.mintUrl}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Text(
            "self  ${shortPubkey(state.ourPubkey)}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
            fontFamily = FontFamily.Monospace,
        )
    }
}

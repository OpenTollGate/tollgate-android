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
import androidx.compose.runtime.getValue
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
 * and updates [UiState.detected], which is all this build knows. (There is no
 * session: see [SessionCard].) The live
 * uptime display reads.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatusScreen(
    state: UiState,
) {
    Scaffold(topBar = { TopAppBar(title = { Text("TollGate · Status") }) }) { pad ->
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

            state.detected?.let { d ->
                InfoCard(title = "Gateway") {
                    TelemetryRow(label = "unit", value = d.unit)
                    TelemetryRow(label = "protocol", value = "v${d.version}")
                    TelemetryRow(
                        label = "mints",
                        value = d.offer?.acceptedMints?.joinToString(", ").orEmpty()
                            .ifBlank { "—" },
                    )
                }
            }

            state.error?.let { Text("error: $it", color = MaterialTheme.colorScheme.error) }

            Spacer(Modifier.height(4.dp))
            Controls(state = state)
        }
    }
}

/** The detected gateway's pubkey, if one has been detected. */
private fun gatewayPubkey(state: UiState): String? =
    state.detected?.pubkeyHex?.takeIf { it.isNotBlank() }

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

/**
 * What the app can honestly say about a session right now.
 *
 * There is no session to report: this build detects gateways but does not buy,
 * because a v2 purchase funds a Spilman channel from a Cashu wallet the app does
 * not have. An uptime clock driven by nothing would be a worse lie than an
 * empty card, so the card says what is missing instead.
 */
@Composable
private fun SessionCard(state: UiState) {
    InfoCard(title = "Session") {
        TelemetryRow(label = "status", value = "no session")
        TelemetryRow(
            label = "gateway",
            value = state.detected?.let { "detected" } ?: "not detected",
        )
        Text(
            "Buying is not implemented: a purchase funds a payment channel from a " +
                "Cashu wallet, which this build does not include.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
private fun Controls(state: UiState) {
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

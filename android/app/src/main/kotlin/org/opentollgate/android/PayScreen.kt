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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import org.opentollgate.android.model.UiState
import org.opentollgate.android.util.formatScaledSats
import org.opentollgate.android.util.shortPubkey
import org.opentollgate.android.util.stripScheme

/**
 * Gateway screen: find a gateway and read what it will sell on.
 *
 * Protocol v2 replaced the old detect/pay flow. There is no price sheet any
 * more — a v2 `Offer` carries the mints a gateway accepts and the limits on a
 * purchase, and the price belongs to the mint that issues the vouchers. So this
 * screen has two parts and no third:
 *
 *  1. **Gateway** — editable endpoint + Detect. On success it shows the peer
 *     pubkey, resource unit, protocol version and its terms.
 *  2. **Terms** — mints accepted, the unit, the window bounds, and how upload is
 *     weighted. Or "nothing to buy" when the gateway answered without an Offer.
 *
 * Buying is absent by design: a v2 purchase funds a Spilman channel from a Cashu
 * wallet, and this build has no wallet. Showing a disabled Pay button would
 * imply the phone could pay if only it tried harder, which is not true.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PayScreen(
    state: UiState,
    onHostChange: (String) -> Unit,
    onDetect: () -> Unit,
    onBuy: () -> Unit,
    onRefreshBalance: () -> Unit,
) {
    Scaffold(topBar = { TopAppBar(title = { Text("TollGate · Pay") }) }) { pad ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            GatewayCard(state = state, onHostChange = onHostChange, onDetect = onDetect)
            state.detected?.let { d ->
                if (d.offer != null) TermsCard(state = state) else NoOfferCard()
            }
            PayResult(state = state, onBuy = onBuy, onRefreshBalance = onRefreshBalance)
        }
    }
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

@Composable
private fun GatewayCard(state: UiState, onHostChange: (String) -> Unit, onDetect: () -> Unit) {
    InfoCard(title = "Gateway") {
        OutlinedTextField(
            value = state.baseHost,
            onValueChange = onHostChange,
            label = { Text("gateway URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedButton(onClick = onDetect, enabled = state.baseHost.isNotBlank()) { Text("Detect") }
            state.detected?.let { d ->
                Text(
                    "v${d.version} · unit ${d.unit}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }
        state.detected?.let { d ->
            Spacer(Modifier.height(6.dp))
            Text("peer", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.outline)
            Text(
                shortPubkey(d.pubkeyHex),
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodyLarge,
            )
        } ?: Text(
            "tap Detect to probe the gateway and learn its price",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

/**
 * What this gateway will sell on. Deliberately no price: protocol v2's Offer
 * carries the mints it accepts and the limits on a purchase, and the price is
 * the mint's, not the gateway's. Showing a number here would be inventing one.
 */
@Composable
private fun TermsCard(state: UiState) {
    val offer = state.detected?.offer ?: return
    InfoCard(title = "What this gateway offers") {
        TelemetryRow(label = "unit", value = offer.unit)
        TelemetryRow(
            label = "mints",
            value = offer.acceptedMints.joinToString(", ").ifBlank { "—" },
        )
        TelemetryRow(
            label = "budget window",
            value = "${offer.minWindowMs / 1000u}s – ${offer.maxWindowMs / 1000u}s",
        )
        TelemetryRow(
            label = "upload counted",
            value = if (offer.fromPayerWeight == 0.toUShort()) "free" else "×${offer.fromPayerWeight}",
        )
    }
}

@Composable
private fun NoOfferCard() {
    InfoCard(title = "Nothing to buy") {
        Text(
            "This gateway introduced itself but offers no terms, so there is " +
                "no purchase to make here.",
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun PayResult(
    state: UiState,
    onBuy: () -> Unit,
    onRefreshBalance: () -> Unit,
) {
    val offer = state.detected?.offer ?: return
    val mint = offer.acceptedMints.firstOrNull()
    val held = state.holdings
        .filter { it.unit == offer.unit && (mint == null || it.mint == mint) }
        .sumOf { it.amount }

    InfoCard(title = if (held > 0uL) "Buy access" else "Nothing to pay with") {
        Text(
            "This device holds $held ${offer.unit}" +
                (mint?.let { " from $it" } ?: ""),
            style = MaterialTheme.typography.bodySmall,
        )
        if (mint == null) {
            Spacer(Modifier.height(8.dp))
            Text(
                "This gateway advertised no mint, so there is nothing to pay.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        } else if (held == 0uL) {
            // Say what to do instead of showing a button that cannot work. A token
            // arrives through Wallet -> Receive; the purchase then has paper to spend.
            Spacer(Modifier.height(8.dp))
            Text(
                "Receive a Cashu token from $mint on the Wallet tab, then come back. " +
                    "Buying spends what this device already holds; it does not ask a " +
                    "bank or a second wallet for anything.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
            )
        }

        Spacer(Modifier.height(12.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = onBuy, enabled = held > 0uL && !state.buying) {
                Text(if (state.buying) "Buying…" else "Buy")
            }
            OutlinedButton(onClick = onRefreshBalance, enabled = !state.buying) {
                Text("Refresh")
            }
        }

        state.buyResult?.let { result ->
            Spacer(Modifier.height(8.dp))
            Text(result, style = MaterialTheme.typography.bodySmall)
        }
    }
    state.error?.let {
        Text("⚠ $it", color = MaterialTheme.colorScheme.error)
    }
}

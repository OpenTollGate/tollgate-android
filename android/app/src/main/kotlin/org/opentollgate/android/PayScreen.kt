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
 * Phase 1 payment screen — the Cashu bootstrap-token flow.
 *
 * Replaces the detect/pay controls that previously lived at the bottom of
 * [StatusScreen]. The flow, top to bottom:
 *
 *  1. **Gateway** — editable base URL + Detect. Once detected, shows the peer
 *     pubkey, resource unit, and protocol version.
 *  2. **Price** — the gateway's price sheet (per-second + per-unit), rendered
 *     as human sats via [formatScaledSats] (wire values are milli-sats, ÷1000).
 *  3. **Mint** — pick which Cashu mint draws the bootstrap token. Modelled on
 *     the captive-portal SPA's `AccessOptions` (radio-selectable mint list +
 *     add-custom). Real per-gateway mint discovery (PriceSheet → MintOption)
 *     lands in Phase 3; today the list is user-managed.
 *  4. **Amount** — bootstrap-token size in sats (default 21), with quick-pick
 *     chips.
 *  5. **Pay** — calls [TollgateViewModel.onPay], which invokes
 *     `TollgateMobileNode.pay(baseHost, mintUrl, amountSat)` and, on acceptance,
 *     starts the consume loop. Disabled while a request is in flight or a
 *     session is already active.
 *  6. **Result** — an "access granted" card on success (mirrors the SPA's
 *     `AccessGranted`), or the error string on failure.
 *
 * All callbacks are ViewModel method references; this composable owns no state
 * of its own except the two ephemeral text fields (new-mint + amount), which
 * are local [remember]s seeded from [state].
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PayScreen(
    state: UiState,
    onHostChange: (String) -> Unit,
    onDetect: () -> Unit,
    onSelectMint: (String) -> Unit,
    onAddMint: (String) -> Unit,
    onAmountChange: (Long) -> Unit,
    onTokenChange: (String) -> Unit,
    onPay: () -> Unit,
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
                if (d.perUnit != null || d.perSecond != null) PriceCard(state = state)
            }
            MintCard(state = state, onSelectMint = onSelectMint, onAddMint = onAddMint)
            TokenCard(state = state, onTokenChange = onTokenChange)
            AmountCard(state = state, onAmountChange = onAmountChange)
            PayButton(state = state, onPay = onPay)
            PayResult(state = state)
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

@Composable
private fun PriceCard(state: UiState) {
    val d = state.detected ?: return
    InfoCard(title = "Price") {
        TelemetryRow(label = "per second", value = formatScaledSats(d.perSecond, "s") ?: "—")
        TelemetryRow(label = "per ${d.unit}", value = formatScaledSats(d.perUnit, d.unit) ?: "—")
    }
}

@Composable
private fun MintCard(state: UiState, onSelectMint: (String) -> Unit, onAddMint: (String) -> Unit) {
    var newMint by remember { mutableStateOf("") }
    InfoCard(title = "Mint") {
        // Radio-selectable list — mirrors the captive-portal SPA's AccessOptions:
        // one row per known mint, the active one (state.mintUrl) selected.
        state.knownMints.forEach { mint ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = mint == state.mintUrl,
                    onClick = { onSelectMint(mint) },
                )
                Spacer(Modifier.size(4.dp))
                Text(
                    stripScheme(mint),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = if (mint == state.mintUrl) FontWeight.Bold else FontWeight.Normal,
                )
            }
        }
        Spacer(Modifier.height(8.dp))
        Row(
            Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedTextField(
                value = newMint,
                onValueChange = { newMint = it },
                label = { Text("add mint URL") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            Button(
                onClick = {
                    onAddMint(newMint)
                    newMint = ""
                },
                enabled = newMint.isNotBlank(),
            ) { Text("Add") }
        }
    }
}

@Composable
private fun TokenCard(state: UiState, onTokenChange: (String) -> Unit) {
    InfoCard(title = "Cashu Token") {
        Text(
            "Paste a Cashu token from ${state.mintUrl.removePrefix("https://")}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(6.dp))
        OutlinedTextField(
            value = state.paymentToken ?: "",
            onValueChange = onTokenChange,
            label = { Text("cashuA…") },
            singleLine = false,
            maxLines = 3,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun AmountCard(state: UiState, onAmountChange: (Long) -> Unit) {
    var amountText by remember(state.amountSat) { mutableStateOf(state.amountSat.toString()) }
    InfoCard(title = "Amount") {
        OutlinedTextField(
            value = amountText,
            onValueChange = {
                amountText = it
                it.toLongOrNull()?.let(onAmountChange)
            },
            label = { Text("sats") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(21L, 100L, 1000L).forEach { preset ->
                FilterChip(
                    selected = state.amountSat == preset,
                    onClick = { onAmountChange(preset) },
                    label = { Text("$preset") },
                )
            }
        }
    }
}

@Composable
private fun PayButton(state: UiState, onPay: () -> Unit) {
    val active = state.sessionStartedAt != null
    val canPay = state.baseHost.isNotBlank() && !state.paying && !active
    Button(
        onClick = onPay,
        enabled = canPay,
        modifier = Modifier.fillMaxWidth(),
    ) {
        if (state.paying) {
            CircularProgressIndicator(
                modifier = Modifier.size(18.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.onPrimary,
            )
            Spacer(Modifier.size(8.dp))
            Text("Paying…")
        } else if (active) {
            Text("Session active — see Status")
        } else {
            Text("Pay ${state.amountSat} sats")
        }
    }
}

@Composable
private fun PayResult(state: UiState) {
    val paid = state.paid
    if (paid != null && paid.accepted) {
        InfoCard(title = "Access granted") {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(12.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.primary),
                )
                Spacer(Modifier.size(8.dp))
                Text("bootstrap accepted", fontWeight = FontWeight.Bold)
            }
            Spacer(Modifier.height(8.dp))
            paid.peerPubkeyHex.takeIf { it.isNotBlank() }?.let {
                Text("gateway  ${shortPubkey(it)}", style = MaterialTheme.typography.bodySmall)
            }
            Text("Switch to Status to watch the session.", style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline)
        }
    }
    state.error?.let {
        Text("⚠ $it", color = MaterialTheme.colorScheme.error)
    }
}

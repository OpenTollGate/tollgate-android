package org.opentollgate.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.opentollgate.android.model.UiState

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StatusScreen(state: UiState, onDetect: () -> Unit, onPay: () -> Unit) {
    Scaffold(topBar = { TopAppBar(title = { Text("TollGate") }) }) { pad ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text("node: ${state.ourPubkey.take(18)}…", style = MaterialTheme.typography.bodySmall)

            // TODO(§6): full SPA-mirror UI — Cashu mint/balance, Lightning, QR,
            // i18n, tip selection. This skeleton wires detect/pay/consume only.
            Button(onClick = onDetect, enabled = !state.online) { Text("Detect gateway") }
            state.detected?.let {
                Text("peer: ${it.pubkeyHex.take(18)}…  unit=${it.unit}  v${it.version}")
                Text("price: ${it.perUnit ?: "?"} per unit / ${it.perSecond ?: "?"} per s")
            }
            Button(onClick = onPay) { Text("Pay 21 sat (bootstrap)") }
            state.paid?.let { Text(if (it.accepted) "accepted ✓" else "rejected", color = if (it.accepted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error) }
            state.latest?.let {
                Spacer(Modifier.height(8.dp))
                Text("poll ${it.poll}: remaining=${it.remainingScaled} delivered=${it.delivered ?: "-"}" +
                    if (it.toppedUp) " [topped up]" else "" + if (it.cutOff) " [CUT OFF]" else "")
            }
            state.error?.let { Text("error: $it", color = MaterialTheme.colorScheme.error) }
        }
    }
}

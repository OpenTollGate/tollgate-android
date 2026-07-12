package org.opentollgate.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.opentollgate.android.model.DiscoveredPeer
import org.opentollgate.android.model.SignalTier
import org.opentollgate.android.model.UiState
import org.opentollgate.android.util.formatScaledSats
import org.opentollgate.android.util.shortPubkey
import org.opentollgate.android.util.stripScheme

/**
 * Phase 1 Discover screen — "TollGate peers near me".
 *
 * Scans candidate gateways (the seed LAN-router list + any user-added URLs) by
 * probing each with `TollgateMobileNode.detect()` and lists the reachable ones:
 * short pubkey, host, a latency-derived signal tier, and price. Tap **Connect**
 * on a peer to adopt it as the active gateway and jump to Pay.
 *
 * Auto-runs the first scan on appearance (mirrors Myco's DiscoverScreen
 * `LaunchedEffect`), and exposes Scan / Refresh / Stop. Reachable peers are
 * shown as cards best-signal-first; unreachable candidates collapse into a one-
 * line footer. The Add bar lets you probe a non-seed gateway.
 *
 * Modelled on Myco's `DiscoverScreen.kt` (Origami74/myco); the row shape is
 * stable so Phase 2 can swap the seed-list probe for a live FIPS mesh scan
 * without touching this composable's contract.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DiscoverScreen(
    state: UiState,
    onScan: () -> Unit,
    onStopScan: () -> Unit,
    onConnect: (DiscoveredPeer) -> Unit,
    onConnectWifi: (String) -> Unit,
    onAddCandidate: (String) -> Unit,
) {
    // Auto-run the first scan when the screen appears, so nearby peers show
    // without a manual tap. Skip if a scan is already running or we already
    // have results (re-entry from the back stack shouldn't re-sweep).
    LaunchedEffect(Unit) {
        if (state.discovered.isEmpty() && !state.scanning) onScan()
    }

    Scaffold(topBar = { TopAppBar(title = { Text("TollGate · Discover") }) }) { pad ->
        Column(Modifier.fillMaxSize().padding(pad)) {
            ScanBar(
                scanning = state.scanning,
                reachable = state.discovered.count { it.reachable },
                total = state.discovered.size,
                onScan = onScan,
                onStop = onStopScan,
            )
            state.discoverError?.let {
                Text(
                    "⚠ $it",
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                )
            }
            val reachable = state.discovered.filter { it.reachable }
            // Show nearby TollGate WiFi networks first — tappable to connect
            if (state.wifiNetworks.isNotEmpty()) {
                Column(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                ) {
                    Text(
                        "TollGate WiFi networks nearby",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                    state.wifiNetworks.forEach { network ->
                        // Extract SSID from display name (format: "TollGate-F794 (GOOD)")
                        val ssid = network.substringBefore(" (")
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(
                                Icons.Filled.Wifi,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.size(16.dp),
                            )
                            Spacer(Modifier.width(6.dp))
                            Text(
                                network,
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.weight(1f),
                            )
                            OutlinedButton(
                                onClick = { onConnectWifi(ssid) },
                                enabled = !state.scanning,
                            ) { Text("Connect") }
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                }
            }
            if (reachable.isEmpty() && !state.scanning) {
                EmptyState(Modifier.weight(1f))
            } else {
                LazyColumn(
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                    contentPadding = PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    items(reachable, key = { it.baseUrl }) { peer ->
                        PeerCard(peer = peer, onConnect = { onConnect(peer) })
                    }
                    val dead = state.discovered.count { !it.reachable }
                    if (dead > 0) {
                        item {
                            Text(
                                "$dead candidate${if (dead == 1) "" else "s"} unreachable",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.outline,
                                modifier = Modifier.padding(top = 4.dp),
                            )
                        }
                    }
                }
            }
            AddCandidateBar(onAddCandidate = onAddCandidate)
        }
    }
}

@Composable
private fun ScanBar(
    scanning: Boolean,
    reachable: Int,
    total: Int,
    onScan: () -> Unit,
    onStop: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            "Nearby TollGate peers",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.weight(1f),
        )
        if (total > 0) {
            Text(
                "$reachable/$total",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.outline,
            )
        }
        if (scanning) {
            CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
            OutlinedButton(onClick = onStop) { Text("Stop") }
        } else {
            OutlinedButton(onClick = onScan) {
                Icon(Icons.Filled.Refresh, contentDescription = null)
                Spacer(Modifier.size(6.dp))
                Text(if (total == 0) "Scan" else "Refresh")
            }
        }
    }
}

@Composable
private fun EmptyState(modifier: Modifier) {
    Column(
        modifier = modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            Icons.Filled.Wifi,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.outline,
            modifier = Modifier.size(40.dp),
        )
        Spacer(Modifier.height(12.dp))
        Text(
            "No peers found.\nMake sure you're on a network with a TollGate gateway, then tap Scan.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
private fun PeerCard(peer: DiscoveredPeer, onConnect: () -> Unit) {
    Surface(
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SignalIndicator(tier = peer.signal)
            Spacer(Modifier.width(16.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    shortPubkey(peer.pubkeyHex),
                    fontWeight = FontWeight.Bold,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.titleSmall,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    stripScheme(peer.baseUrl),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
                val perSec = formatScaledSats(peer.perSecond, "s")
                Text(
                    if (perSec != null) "$perSec  ·  v${peer.version}" else "v${peer.version}  ·  ${peer.latencyMs} ms",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
                if (peer.acceptedMints.isNotEmpty()) {
                    Text(
                        "${peer.acceptedMints.size} mint${if (peer.acceptedMints.size == 1) "" else "s"}: ${peer.acceptedMints.joinToString(", ") { stripScheme(it) }}",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
            }
            Button(onClick = onConnect) { Text("Connect") }
        }
    }
}

/**
 * Three-bar cellular-style signal glyph. Filled bars = tier strength
 * (STRONG 3, MODERATE 2, WEAK 1, NONE 0). Drawn with plain Boxes so it needs no
 * extra asset or vector dependency.
 */
@Composable
private fun SignalIndicator(tier: SignalTier) {
    val filled = when (tier) {
        SignalTier.STRONG -> 3
        SignalTier.MODERATE -> 2
        SignalTier.WEAK -> 1
        SignalTier.NONE -> 0
    }
    val active = MaterialTheme.colorScheme.primary
    val dim = MaterialTheme.colorScheme.outlineVariant
    Row(
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        for (i in 0 until 3) {
            val h = (6 + i * 4).dp // 6, 10, 14 dp — ascending bars
            Box(
                Modifier
                    .size(width = 5.dp, height = h)
                    .clip(RoundedCornerShape(1.dp))
                    .background(if (i < filled) active else dim),
            )
        }
    }
}

@Composable
private fun AddCandidateBar(onAddCandidate: (String) -> Unit) {
    var host by remember { mutableStateOf("") }
    Row(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            value = host,
            onValueChange = { host = it },
            label = { Text("add gateway URL") },
            placeholder = { Text("http://192.168.8.1:2121") },
            singleLine = true,
            modifier = Modifier.weight(1f),
        )
        Button(
            onClick = {
                onAddCandidate(host)
                host = ""
            },
            enabled = host.isNotBlank(),
        ) { Text("Add") }
    }
}

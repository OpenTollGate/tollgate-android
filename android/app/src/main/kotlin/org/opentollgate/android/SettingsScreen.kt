package org.opentollgate.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import org.opentollgate.android.model.UiState
import org.opentollgate.android.util.shortPubkey

/**
 * Phase 1 Settings — read-only identity / FIPS / about view. Modeled on Myco's
 * SettingsScreen (Origami74/myco) but built from TollGate's own [InfoCard] +
 * [TelemetryRow] primitives, and driven by [UiState] exactly like the other
 * Phase 1 screens.
 *
 * Three grouped cards, top to bottom:
 *  1. **Identity** — the device's secp256k1 compressed pubkey (66 hex), the
 *     on-disk directory the Rust core persists it under, and a button that
 *     opens a copyable full-pubkey dialog.
 *  2. **FIPS** — honest Phase-1 status: the FIPS crate is linked as a path
 *     dependency (Phase 0), but the embedded node + foreground service that
 *     actually mesh-network are Phase 2. Surfaced so the user understands why
 *     networking still goes over plain IP right now.
 *  3. **About** — app version (BuildConfig.VERSION_NAME) + build hash
 *     (BuildConfig.GIT_HASH, a short git SHA stamped at build time) + the
 *     protocol / payment stack in use.
 *
 * Nothing here mutates state — [UiState] is populated once at construction in
 * [TollgateViewModel] (pubkey/dataDir from the native node, version/hash from
 * BuildConfig), so this screen needs no callbacks.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(state: UiState) {
    var showIdentity by remember { mutableStateOf(false) }

    Scaffold(topBar = { TopAppBar(title = { Text("TollGate · Settings") }) }) { pad ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // ---- Identity --------------------------------------------------
            InfoCard(title = "Identity") {
                TelemetryRow(label = "pubkey", value = shortPubkey(state.ourPubkey))
                TelemetryRow(label = "key type", value = "secp256k1 compressed")
                TelemetryRow(label = "stored at", value = state.dataDir.ifEmpty { "—" })
                Spacer(Modifier.height(10.dp))
                OutlinedButton(
                    onClick = { showIdentity = true },
                    enabled = state.ourPubkey.isNotEmpty(),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("Show full pubkey") }
            }

            // ---- FIPS ------------------------------------------------------
            InfoCard(title = "FIPS") {
                TelemetryRow(label = "status", value = "linked (path dep · Phase 0)")
                TelemetryRow(label = "embedded node", value = "Phase 2")
                Spacer(Modifier.height(6.dp))
                Text(
                    "FIPS mesh networking is compiled in but not yet running in-app. " +
                        "The embedded node and foreground service arrive in Phase 2; " +
                        "until then networking uses plain IP to the gateway.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }

            // ---- About -----------------------------------------------------
            InfoCard(title = "About") {
                TelemetryRow(label = "version", value = state.appVersion.ifEmpty { "—" })
                TelemetryRow(label = "build", value = state.buildHash.ifEmpty { "—" })
                TelemetryRow(label = "protocol", value = "TollGate v2 (CBOR)")
                TelemetryRow(label = "payment", value = "Cashu bootstrap token")
            }

            Text(
                "TollGate · open metered-resource payments",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }

    if (showIdentity) {
        AlertDialog(
            onDismissRequest = { showIdentity = false },
            confirmButton = {
                TextButton(onClick = { showIdentity = false }) { Text("Close") }
            },
            title = { Text("Device pubkey") },
            text = {
                // SelectionContainer so the user can long-press → copy the full
                // 66-char compressed pubkey (the truncated row above is not).
                SelectionContainer {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            "compressed secp256k1 · 66 hex chars",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.outline,
                        )
                        Text(
                            state.ourPubkey.ifEmpty { "—" },
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            },
        )
    }
}

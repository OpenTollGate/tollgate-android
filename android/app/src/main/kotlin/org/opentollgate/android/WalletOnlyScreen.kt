package org.opentollgate.android

import android.util.Log
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.opentollgate.android.model.DEFAULT_MINTS
import uniffi.tollgate_mobile.TollgateMobileNode

private const val TAG = "WalletOnlyScreen"

/** Local FakeWallet mint for dev/testing (auto-settles invoices instantly). */
private const val FAKE_MINT = "http://192.168.2.33:4444"

/** All selectable mints: FakeWallet first, then public testnut/coinos mints. */
private val MINT_OPTIONS: List<String> = listOf(FAKE_MINT) + DEFAULT_MINTS

@OptIn(ExperimentalMaterial3Api::class)
@Composable
// Preset testnut mints — first is default (FakeWallet), rest are public testnut mints.
data class MintPreset(val label: String, val url: String, val settleSecs: ULong)

private val MINT_PRESETS = listOf(
    MintPreset("FakeWallet (dev)", "http://10.230.237.203:4444", 30uL),
    MintPreset("lnwallet.app", "https://mint.lnwallet.app", 120uL),
    MintPreset("Custom…", "", 60uL),
)

@Composable
fun WalletOnlyScreen(node: TollgateMobileNode) {
    val scope = rememberCoroutineScope()

    var mintUrl by remember { mutableStateOf(MINT_PRESETS[0].url) }
    var tokenText by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Ready. Tap a button to mint or swap.") }
    var busy by remember { mutableStateOf(false) }

    // Resolve settle timeout from matching preset, default 60s
    val settleSecs: ULong = MINT_PRESETS.find { it.url == mintUrl }?.settleSecs ?: 60uL

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = "CDK Wallet — Testnut Ecash",
            style = MaterialTheme.typography.headlineSmall,
        )

        // Preset mint selector
        Text("Select Mint:", style = MaterialTheme.typography.labelMedium)
        MINT_PRESETS.forEach { preset ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = mintUrl == preset.url,
                    onClick = { if (preset.url.isNotEmpty()) mintUrl = preset.url },
                    enabled = !busy,
                )
                Text(preset.label, style = MaterialTheme.typography.bodySmall)
                if (preset.url.isNotEmpty() && preset.url != MINT_PRESETS[0].url) {
                    Text(
                        " (${settleSecs}s)",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.outline,
                    )
                }
            }
        }

        // Mint URL input (editable when "Custom…" or always)
        OutlinedTextField(
            value = mintUrl,
            onValueChange = { mintUrl = it },
            label = { Text("Mint URL") },
            singleLine = true,
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )
                onValueChange = { mintUrl = it },
                label = { Text("Mint URL") },
                singleLine = true,
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = mintExpanded) },
                modifier = Modifier
                    .fillMaxWidth()
                    .menuAnchor(),
            )
            ExposedDropdownMenu(
                expanded = mintExpanded,
                onDismissRequest = { mintExpanded = false },
            ) {
                MINT_OPTIONS.forEach { mint ->
                    val label = when {
                        mint.startsWith("192.168") -> "FakeWallet (local dev)"
                        mint.contains("testnut") -> "Testnut: $mint"
                        mint.contains("minibits") -> "Minibits: $mint"
                        mint.contains("coinos") -> "Coinos: $mint"
                        else -> mint
                    }
                    DropdownMenuItem(
                        text = { Text(label) },
                        onClick = {
                            mintUrl = mint
                            mintExpanded = false
                        },
                    )
                }
            }
        }

        // ── Mint buttons ──
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Button(
                onClick = {
                    if (busy) return@Button
                    busy = true
                    status = "Minting 21 sats from ${mintUrl}..."
                    scope.launch(Dispatchers.IO) {
                        try {
                            val token = node.autoMint(mintUrl, 21uL, 30uL)
                            tokenText = token
                            status = "Minted 21 sats — token ready"
                        } catch (e: Exception) {
                            Log.e(TAG, "mint 21 failed: ${e.message}")
                            status = "Mint failed: ${e.message}"
                        } finally { busy = false }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Mint 21") }
            Button(
                onClick = {
                    if (busy) return@Button
                    busy = true
                    status = "Minting 1 sat from ${mintUrl}..."
                    scope.launch(Dispatchers.IO) {
                        try {
                            val token = node.autoMint(mintUrl, 1uL, 30uL)
                            tokenText = token
                            status = "Minted 1 sat — token ready"
                        } catch (e: Exception) {
                            Log.e(TAG, "mint 1 failed: ${e.message}")
                            status = "Mint failed: ${e.message}"
                        } finally { busy = false }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Mint 1") }
        }

        // ── Swap button ──
        Button(
            onClick = {
                if (busy) return@Button
                if (tokenText.isBlank()) {
                    status = "No token to swap. Mint first."
                    return@Button
                }
                busy = true
                status = "Swapping token..."
                scope.launch(Dispatchers.IO) {
                    try {
                        val newToken = node.swapTokens(mintUrl, tokenText)
                        tokenText = newToken
                        status = "Swap complete — fresh proofs"
                    } catch (e: Exception) {
                        Log.e(TAG, "swap failed: ${e.message}")
                        status = "Swap failed: ${e.message}"
                    } finally { busy = false }
                }
            },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Swap Token") }

        // ── Receive button ──
        Button(
            onClick = {
                if (busy) return@Button
                if (tokenText.isBlank()) {
                    status = "Paste a cashuA token first."
                    return@Button
                }
                busy = true
                status = "Receiving token into wallet..."
                scope.launch(Dispatchers.IO) {
                    try {
                        val receivedSats = node.receiveToken(mintUrl, tokenText)
                        status = "Received $receivedSats sats — wallet updated"
                    } catch (e: Exception) {
                        Log.e(TAG, "receive failed: ${e.message}")
                        status = "Receive failed: ${e.message}"
                    } finally { busy = false }
                }
            },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Receive Token") }

        // ── Token display / input ──
        HorizontalDivider(modifier = Modifier.fillMaxWidth())
        OutlinedTextField(
            value = tokenText,
            onValueChange = { tokenText = it },
            label = { Text("Token (cashuA...)") },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 120.dp),
            textStyle = LocalTextStyle.current.copy(
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
            ),
            maxLines = 10,
        )

        if (tokenText.isNotEmpty()) {
            TextButton(onClick = { tokenText = "" }) { Text("Clear token") }
        }

        // ── Status ──
        Text(
            text = status,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.fillMaxWidth(),
            overflow = TextOverflow.Visible,
        )

        if (busy) {
            Spacer(modifier = Modifier.height(8.dp))
            CircularProgressIndicator(modifier = Modifier.size(24.dp))
        }
    }
}

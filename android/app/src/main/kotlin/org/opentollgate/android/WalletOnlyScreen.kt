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
private const val FAKE_MINT = "http://10.230.237.203:4444"

/**
 * All selectable mints: FakeWallet first, then public testnut/coinos mints.
 *
 * settleSecs is the max wait for the mint quote to reach PAID state.
 * FakeWallet settles instantly (30s is plenty); public mints need LN
 * routing so we give them 120s.
 */
data class MintOption(val label: String, val url: String, val settleSecs: ULong)

private val MINT_OPTIONS: List<MintOption> = listOf(
    MintOption("FakeWallet (dev)", FAKE_MINT, 30uL),
    MintOption("coinos.io", "https://mint.coinos.io", 120uL),
    MintOption("minibits", "https://mint.minibits.cash/Bitcoin", 120uL),
    MintOption("nofree testnut", "https://nofee.testnut.cashu.space", 120uL),
    MintOption("lnwallet.app", "https://mint.lnwallet.app", 120uL),
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WalletOnlyScreen(node: TollgateMobileNode) {
    val scope = rememberCoroutineScope()

    var mintUrl by remember { mutableStateOf(MINT_OPTIONS[0].url) }
    var tokenText by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Ready. Pick a mint, then mint or swap.") }
    var busy by remember { mutableStateOf(false) }

    // Resolve settle timeout from matching preset, default 60s for custom URLs
    val settleSecs: ULong = MINT_OPTIONS.find { it.url == mintUrl }?.settleSecs ?: 60uL

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "CDK Wallet — Testnut Ecash",
            style = MaterialTheme.typography.headlineSmall,
        )

        // ── Mint selector (radio buttons) ──
        Text("Select Mint:", style = MaterialTheme.typography.labelMedium)
        MINT_OPTIONS.forEach { option ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = mintUrl == option.url,
                    onClick = { mintUrl = option.url },
                    enabled = !busy,
                )
                Text(option.label, style = MaterialTheme.typography.bodySmall)
                Text(
                    " (${option.settleSecs}s)",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }

        // ── Custom mint URL input ──
        OutlinedTextField(
            value = mintUrl,
            onValueChange = { mintUrl = it },
            label = { Text("Mint URL (or type custom)") },
            singleLine = true,
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )

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
                            val token = node.autoMint(mintUrl, 21uL, settleSecs)
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
                            val token = node.autoMint(mintUrl, 1uL, settleSecs)
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

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
import kotlinx.coroutines.withContext
import uniffi.tollgate_mobile.TollgateMobileNode

private const val TAG = "WalletOnlyScreen"

@Composable
fun WalletOnlyScreen(node: TollgateMobileNode) {
    val scope = rememberCoroutineScope()

    var mintUrl by remember { mutableStateOf("http://192.168.2.33:4444") }
    var tokenText by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Ready. Tap a button to mint or swap.") }
    var busy by remember { mutableStateOf(false) }

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

        // Mint URL input
        OutlinedTextField(
            value = mintUrl,
            onValueChange = { mintUrl = it },
            label = { Text("Mint URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )

        // Mint buttons
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Button(
                onClick = {
                    if (busy) return@Button
                    busy = true
                    status = "Minting 21 sats…"
                    scope.launch(Dispatchers.IO) {
                        try {
                            val token = node.autoMint(mintUrl, 21uL, 30uL)
                            tokenText = token
                            status = "✅ Minted 21 sats — token ready"
                        } catch (e: Exception) {
                            Log.e(TAG, "mint 21 failed: ${e.message}")
                            status = "❌ Mint failed: ${e.message}"
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Mint 21 sats") }
            Button(
                onClick = {
                    if (busy) return@Button
                    busy = true
                    status = "Minting 1 sat…"
                    scope.launch(Dispatchers.IO) {
                        try {
                            val token = node.autoMint(mintUrl, 1uL, 30uL)
                            tokenText = token
                            status = "✅ Minted 1 sat — token ready"
                        } catch (e: Exception) {
                            Log.e(TAG, "mint 1 failed: ${e.message}")
                            status = "❌ Mint failed: ${e.message}"
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Mint 1 sat") }
        }

        // Swap button
        Button(
            onClick = {
                if (busy) return@Button
                if (tokenText.isBlank()) {
                    status = "⚠️ No token to swap. Mint first."
                    return@Button
                }
                busy = true
                status = "Swapping token…"
                scope.launch(Dispatchers.IO) {
                    try {
                        val newToken = node.swapTokens(mintUrl, tokenText)
                        tokenText = newToken
                        status = "✅ Swap complete — fresh proofs"
                    } catch (e: Exception) {
                        Log.e(TAG, "swap failed: ${e.message}")
                        status = "❌ Swap failed: ${e.message}"
                    } finally {
                        busy = false
                    }
                }
            },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Swap Token") }

        // Token display / input
        OutlinedTextField(
            value = tokenText,
            onValueChange = { tokenText = it },
            label = { Text("Token (cashuA…)") },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 120.dp),
            textStyle = LocalTextStyle.current.copy(
                fontFamily = FontFamily.Monospace,
                fontSize = 11.sp,
            ),
            maxLines = 10,
        )

        // Clear button
        if (tokenText.isNotEmpty()) {
            TextButton(onClick = { tokenText = "" }) { Text("Clear token") }
        }

        // Status
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
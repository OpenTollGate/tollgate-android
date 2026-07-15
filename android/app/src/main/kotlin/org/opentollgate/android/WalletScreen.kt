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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
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
import org.opentollgate.android.model.ALL_MINTS
import org.opentollgate.android.model.TARGET_BALANCE_SATS
import org.opentollgate.android.model.TxEntry
import org.opentollgate.android.model.TxKind
import org.opentollgate.android.model.UiState
import org.opentollgate.android.util.stripScheme
import uniffi.tollgate_mobile.TollgateMobileNode
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import androidx.compose.runtime.rememberCoroutineScope
import android.util.Log
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Phase 1 wallet screen — balance, mint breakdown, token mint/melt, transaction
 * history. Mirrors the captive-portal SPA's Balance tab (face-value derivation
 * from a pasted cashu token) and the Alex Flutter wallet's balance/send/receive
 * layout, ported to Material3.
 *
 * Top to bottom:
 *  1. **Balance hero** — total sats across every known mint (sum of
 *     [UiState.wallet] balances).
 *  2. **Mints** — one row per mint with its local balance (the breakdown).
 *  3. **Receive** — paste/scan a cashu token; on parse, credits each mint it
 *     draws on (see [TollgateViewModel.onReceiveToken]). Mirrors the SPA's
 *     `BalancePage` token input.
 *  4. **Send** — pick a funded mint + amount, mint a local token of that size,
 *     debit the balance (see [TollgateViewModel.onSend]). The produced token is
 *     shown for copy/share.
 *  5. **History** — newest-first ledger of every wallet op.
 *
 * All callbacks are ViewModel method references; this composable owns only its
 * ephemeral text fields (receive-token + send-amount). Real Cashu NUT-03/05/06/07
 * integration against a live mint lands in Phase 3 (master plan §Phase 3); today
 * the wallet is a truthful local model — it never fabricates a backend.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WalletScreen(
    state: UiState,
    node: TollgateMobileNode,
    onReceiveToken: (String) -> Unit,
    onSend: (mint: String, amountSat: Long) -> Unit,
    onDismissLastSent: () -> Unit,
    onMintFrom: (mintUrl: String, amountSat: Long) -> Unit,
    onSwapToken: (mintUrl: String, tokenStr: String) -> Unit,
    onReceiveIntoWallet: (mintUrl: String, tokenStr: String) -> Unit,
    onTopupAll: () -> Unit,
) {
    Scaffold(topBar = { TopAppBar(title = { TollGateTitle(subtitle = "Wallet") }) }) { pad ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            BalanceHero(state = state)
            MintBreakdownCard(state = state)
            MintActionCard(
                state = state,
                node = node,
                onMintFrom = onMintFrom,
                onSwapToken = onSwapToken,
                onReceiveIntoWallet = onReceiveIntoWallet,
                onTopupAll = onTopupAll,
            )
            ReceiveCard(state = state, onReceiveToken = onReceiveToken)
            SendCard(
                state = state,
                onSend = onSend,
                onDismissLastSent = onDismissLastSent,
            )
            HistoryCard(state = state)
        }
    }
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

@Composable
private fun BalanceHero(state: UiState) {
    val total = state.wallet.totalSat
    InfoCard(title = "Balance") {
        Row(verticalAlignment = Alignment.Bottom) {
            Text(
                formatSats(total),
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.Bold,
            )
            Spacer(Modifier.size(6.dp))
            Text(
                if (total == 1L) "sat" else "sats",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.outline,
            )
        }
        Spacer(Modifier.height(4.dp))
        Text(
            "local balance across ${state.wallet.balances.size} mint${if (state.wallet.balances.size == 1) "" else "s"}",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
    }
}

@Composable
private fun MintBreakdownCard(state: UiState) {
    val rows = state.wallet.mintRows
    InfoCard(title = "Mints") {
        if (rows.isEmpty()) {
            Text(
                "no balances yet — receive a cashu token to add funds",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
            )
        } else {
            rows.forEach { row ->
                TelemetryRow(label = stripScheme(row.mint), value = formatSats(row.balanceSat))
            }
        }
    }
}

@Composable
private fun MintActionCard(
    state: UiState,
    node: TollgateMobileNode,
    onMintFrom: (mintUrl: String, amountSat: Long) -> Unit,
    onSwapToken: (mintUrl: String, tokenStr: String) -> Unit,
    onReceiveIntoWallet: (mintUrl: String, tokenStr: String) -> Unit,
    onTopupAll: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var selectedMint by remember { mutableStateOf(ALL_MINTS.first().url) }
    var tokenText by remember { mutableStateOf("") }
    var status by remember { mutableStateOf("Ready.") }
    var busy by remember { mutableStateOf(false) }

    val mintLabel = ALL_MINTS.find { it.url == selectedMint }?.label ?: "custom"
    val settleSecs = ALL_MINTS.find { it.url == selectedMint }?.settleSecs ?: 60uL

    InfoCard(title = "Mint / Swap / Receive") {
        // Target balance indicator
        val currentBal = state.wallet.balanceOf(selectedMint)
        Text(
            "Target: $TARGET_BALANCE_SATS sats per mint | Balance: $currentBal sats",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(8.dp))

        // Mint selector
        ALL_MINTS.forEach { option ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 1.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = selectedMint == option.url,
                    onClick = { if (!busy) selectedMint = option.url },
                    enabled = !busy,
                )
                Spacer(Modifier.size(4.dp))
                Text(
                    "${option.label} (${state.wallet.balanceOf(option.url)} sats)",
                    style = MaterialTheme.typography.bodySmall,
                    fontWeight = if (selectedMint == option.url) FontWeight.Bold else FontWeight.Normal,
                )
            }
        }

        // Custom URL field
        OutlinedTextField(
            value = selectedMint,
            onValueChange = { if (!busy) selectedMint = it },
            label = { Text("Mint URL") },
            singleLine = true,
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))

        // Mint buttons row
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = {
                    if (busy) return@Button
                    busy = true
                    status = "Minting 21 from $mintLabel..."
                    scope.launch(Dispatchers.IO) {
                        try {
                            val token = node.autoMint(selectedMint, 21uL, settleSecs)
                            tokenText = token
                            status = "Minted 21 sats from $mintLabel"
                            onMintFrom(selectedMint, 21L)
                        } catch (e: Exception) {
                            Log.e("MintActionCard", "mint: ${e.message}")
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
                    status = "Topup all mints to $TARGET_BALANCE_SATS..."
                    onTopupAll()
                    scope.launch(Dispatchers.IO) {
                        // Topup runs in VM — just track status here
                        delay(500)
                        status = "Topup running in background..."
                        busy = false
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Topup All") }
        }

        // Swap + Receive
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = {
                    if (busy) return@Button
                    if (tokenText.isBlank()) { status = "Mint or paste a token first."; return@Button }
                    busy = true
                    status = "Swapping..."
                    scope.launch(Dispatchers.IO) {
                        try {
                            val newToken = node.swapTokens(selectedMint, tokenText)
                            tokenText = newToken
                            status = "Swap complete"
                            onSwapToken(selectedMint, tokenText)
                        } catch (e: Exception) {
                            status = "Swap failed: ${e.message}"
                        } finally { busy = false }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Swap") }
            Button(
                onClick = {
                    if (busy) return@Button
                    if (tokenText.isBlank()) { status = "Paste a token first."; return@Button }
                    busy = true
                    status = "Receiving..."
                    scope.launch(Dispatchers.IO) {
                        try {
                            val sats = node.receiveToken(selectedMint, tokenText)
                            status = "Received $sats sats"
                            onReceiveIntoWallet(selectedMint, tokenText)
                        } catch (e: Exception) {
                            status = "Receive failed: ${e.message}"
                        } finally { busy = false }
                    }
                },
                enabled = !busy,
                modifier = Modifier.weight(1f),
            ) { Text("Receive") }
        }

        // Token field
        OutlinedTextField(
            value = tokenText,
            onValueChange = { tokenText = it },
            label = { Text("Token (cashuA...)") },
            modifier = Modifier.fillMaxWidth().heightIn(min = 80.dp, max = 120.dp),
            textStyle = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            enabled = !busy,
        )

        Text(status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
        if (busy) { Spacer(Modifier.height(4.dp)); Text("⏳ Working...", style = MaterialTheme.typography.labelSmall) }
    }
}

@Composable
private fun ReceiveCard(state: UiState, onReceiveToken: (String) -> Unit) {
    var tokenText by remember { mutableStateOf("") }
    InfoCard(title = "Receive") {
        OutlinedTextField(
            value = tokenText,
            onValueChange = { tokenText = it },
            label = { Text("cashu token") },
            placeholder = { Text("paste a cashuA… token") },
            // Tokens are long; allow a few lines but keep it scrollable.
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp, max = 120.dp),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii),
        )
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(
                onClick = {
                    onReceiveToken(tokenText)
                    tokenText = ""
                },
                enabled = tokenText.isNotBlank(),
            ) { Text("Receive") }
            if (tokenText.isNotBlank()) {
                OutlinedButton(onClick = { tokenText = "" }) { Text("Clear") }
            }
        }
    }
}

@Composable
private fun SendCard(
    state: UiState,
    onSend: (mint: String, amountSat: Long) -> Unit,
    onDismissLastSent: () -> Unit,
) {
    // Funded mints = wallet balances > 0. Falls back to knownMints so the user
    // can still target a mint with zero balance (the send will refuse with a
    // clear "insufficient balance" error).
    val fundedMints = remember(state.wallet) {
        state.wallet.mintRows.map { it.mint }.ifEmpty { state.knownMints }
    }
    var selectedMint by remember(state.wallet.balances) {
        mutableStateOf(fundedMints.firstOrNull() ?: state.mintUrl)
    }
    var amountText by remember { mutableStateOf("") }

    InfoCard(title = "Send") {
        if (fundedMints.isEmpty()) {
            Text(
                "no mints available — receive funds first",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
            )
            return@InfoCard
        }
        fundedMints.forEach { mint ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(
                    selected = mint == selectedMint,
                    onClick = { selectedMint = mint },
                )
                Spacer(Modifier.size(4.dp))
                Text(
                    stripScheme(mint),
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = if (mint == selectedMint) FontWeight.Bold else FontWeight.Normal,
                )
                Spacer(Modifier.weight(1f))
                Text(
                    formatSats(state.wallet.balanceOf(mint)),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.outline,
                )
            }
        }
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(
            value = amountText,
            onValueChange = { amountText = it.filter(Char::isDigit) },
            label = { Text("sats") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(10L, 100L, 1000L).forEach { preset ->
                FilterChip(
                    selected = false,
                    onClick = { amountText = preset.toString() },
                    label = { Text("$preset") },
                )
            }
        }
        Spacer(Modifier.height(8.dp))
        val amount = amountText.toLongOrNull() ?: 0L
        Button(
            onClick = {
                val mint = selectedMint ?: return@Button
                onSend(mint, amount)
                amountText = ""
            },
            enabled = selectedMint != null && amount > 0,
            modifier = Modifier.fillMaxWidth(),
        ) { Text(if (amount > 0) "Send $amount sats" else "Send") }

        state.wallet.lastSentToken?.let { token ->
            Spacer(Modifier.height(10.dp))
            LastSentToken(token = token, onDismiss = onDismissLastSent)
        }
    }
}

@Composable
private fun LastSentToken(token: String, onDismiss: () -> Unit) {
    Column {
        Text(
            "local token (share to receive back) — real Cashu send lands in Phase 3",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(4.dp))
        Text(
            token,
            fontFamily = FontFamily.Monospace,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .padding(8.dp),
        )
        Spacer(Modifier.height(4.dp))
        OutlinedButton(onClick = onDismiss, modifier = Modifier.fillMaxWidth()) {
            Text("Dismiss")
        }
    }
}

@Composable
private fun HistoryCard(state: UiState) {
    val history = state.wallet.history
    InfoCard(title = "History (${history.size})") {
        if (history.isEmpty()) {
            Text(
                "no transactions yet",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.outline,
            )
        } else {
            // Plain Column (not LazyColumn): the whole screen already scrolls, and a
            // lazy list nested in a verticalScroll throws infinite-constraints.
            Column(Modifier.fillMaxWidth()) {
                history.forEachIndexed { i, entry ->
                    HistoryRow(entry = entry)
                    if (i != history.lastIndex) {
                        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    }
                }
            }
        }
        state.error?.let {
            Spacer(Modifier.height(8.dp))
            Text("⚠ $it", color = MaterialTheme.colorScheme.error)
        }
    }
}

@Composable
private fun HistoryRow(entry: TxEntry) {
    // RECEIVE credits the wallet (incoming); everything else (SEND/PAY/TOPUP) is
    // an outgoing debit. Reads MaterialTheme.colorScheme directly in composable
    // scope so `tint` is a typed Color (avoids Triple-erased inference).
    val incoming = entry.kind == TxKind.RECEIVE
    val tint = if (incoming) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error
    val sign = if (incoming) "+" else "−"
    val icon = if (incoming) Icons.Default.ArrowDownward else Icons.Default.ArrowUpward
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(22.dp)
                .clip(CircleShape)
                .background(tint.copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = entry.kind.name, modifier = Modifier.size(14.dp), tint = tint)
        }
        Spacer(Modifier.size(8.dp))
        Column(Modifier.weight(1f)) {
            Text(entry.memo, style = MaterialTheme.typography.bodyMedium)
            Text(
                "${entry.kind.name.lowercase()} · ${stripScheme(entry.mint)} · ${formatTime(entry.epochMs)}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.outline,
            )
        }
        Text(
            "$sign${formatSats(entry.amountSat.absoluteSat())}",
            fontWeight = FontWeight.Medium,
            color = tint,
        )
    }
}

// ---------------------------------------------------------------------------
// Format helpers (whole-sat wallet values; distinct from util/Format's
// scaled wire-sat helpers, which divide by the 1000 milli-unit scale).
// ---------------------------------------------------------------------------

/** Render a whole-sat wallet amount: 0 → "0", 1000 → "1,000". */
private fun formatSats(sat: Long): String = String.format(Locale.US, "%,d", sat)

/** Absolute value for display (the sign is rendered separately by the row). */
private fun Long.absoluteSat(): Long = kotlin.math.abs(this)

private val TIME_FMT = SimpleDateFormat("MMM d HH:mm", Locale.US)
private fun formatTime(epochMs: Long): String = TIME_FMT.format(Date(epochMs))

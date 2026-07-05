package org.opentollgate.android.model

/**
 * Local Cashu wallet model for Phase 1.
 *
 * The Rust core (`TollgateMobileNode`) exposes `detect` / `pay` /
 * `start_consume` but not yet real Cashu mint operations (NUT-03 mint,
 * NUT-05 melt, NUT-06 check, NUT-07 swap). Those land in **Phase 3** ("Real
 * Cashu Wallet", master plan). Until then this model tracks balances per mint
 * and a transaction ledger **locally**:
 *
 *  - **Receive** parses an existing `cashuA` token and derives its face value
 *    from the proof `amount` sums — the same derivation the captive-portal SPA's
 *    `helpers/balance.js` does via cashu-ts `validateToken` (read the token's
 *    self-declared value, WITHOUT contacting the mint). Real spendability
 *    (proofs unspent at the mint) is verified in Phase 3 via NUT-06/07.
 *  - **Send** mints a fresh local token of N sats from a chosen mint, debits the
 *    balance, and records a ledger line. The token is round-trippable by this
 *    app's own parser but is **not** spendable at a real mint until Phase 3.
 *
 * The model is immutable (data classes + copy-on-write); the
 * [TollgateViewModel][org.opentollgate.android.TollgateViewModel] owns the single
 * [MutableStateFlow]<[WalletState]> and applies every mutation as one atomic
 * `update` so the Compose UI sees consistent snapshots.
 */

/** Kind of ledger line. Drives the history-row sign and colour. */
enum class TxKind { RECEIVE, SEND, PAY, TOPUP }

/**
 * One ledger entry. Immutable. Newest-first ordering is owned by
 * [WalletState.history]; ids are monotonic ([WalletState.nextTxId]) so the UI's
 * `key()` stays stable across recompositions.
 *
 * @property id        monotonic wallet-internal id
 * @property epochMs   wall-clock timestamp (System.currentTimeMillis)
 * @property kind      what produced this line
 * @property mint      mint URL the tx touches (normalized — no trailing slash)
 * @property amountSat signed: `+` credit (receive/mint), `−` debit (send/melt)
 * @property memo      short human label / token memo preview
 */
data class TxEntry(
    val id: Long,
    val epochMs: Long,
    val kind: TxKind,
    val mint: String,
    val amountSat: Long,
    val memo: String,
)

/** A single per-mint balance row, used by the breakdown list. */
data class MintBalance(val mint: String, val balanceSat: Long)

/**
 * Wallet snapshot held inside [UiState][org.opentollgate.android.model.UiState].
 * Pure data — no Android/Compose dependency, so it is trivially JVM-testable.
 *
 *  - [balances] maps a (normalized) mint URL → sats held locally.
 *  - [history] is newest-first.
 *  - [lastSentToken] holds the most recent "Send" token so WalletScreen can show
 *    it for copy/share; cleared via `onDismissLastSent()`.
 */
data class WalletState(
    val balances: Map<String, Long> = emptyMap(),
    val history: List<TxEntry> = emptyList(),
    private val nextTxId: Long = 1L,
    val lastSentToken: String? = null,
) {
    /** Total balance across every known mint, in sats. */
    val totalSat: Long get() = balances.values.sum()

    /**
     * Per-mint rows for the breakdown card: sorted by balance descending then
     * mint ascending, so the richest mint sits on top and ties are alphabetical.
     */
    val mintRows: List<MintBalance>
        get() = balances.entries
            .map { MintBalance(it.key, it.value) }
            .sortedWith(
                compareByDescending<MintBalance> { it.balanceSat }.thenBy { it.mint },
            )

    /**
     * Credit/debit [deltaSat] on [mint] and append a ledger line. Returns a new
     * [WalletState]; the old one is untouched (copy-on-write). [now] is injected
     * so tests are deterministic.
     */
    fun applyTx(
        mint: String,
        deltaSat: Long,
        kind: TxKind,
        memo: String,
        now: Long = System.currentTimeMillis(),
    ): WalletState {
        val newBalances = balances.toMutableMap()
        newBalances.merge(mint, deltaSat) { a, b -> a + b }
        val entry = TxEntry(
            id = nextTxId,
            epochMs = now,
            kind = kind,
            mint = mint,
            amountSat = deltaSat,
            memo = memo,
        )
        return copy(
            balances = newBalances,
            history = listOf(entry) + history,
            nextTxId = nextTxId + 1L,
        )
    }

    /** Balance held on [mint] (0 when the mint is unknown). */
    fun balanceOf(mint: String): Long = balances[mint] ?: 0L
}

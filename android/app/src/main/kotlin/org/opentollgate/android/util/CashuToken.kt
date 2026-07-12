// No @file:JvmName here: the data class `CashuToken` below would clash with a
// file-facade of the same JVM name. Top-level helpers land in `CashuTokenKt`.

package org.opentollgate.android.util

import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import kotlin.random.Random

/**
 * Minimal Cashu token (V3, `cashuA` JSON encoding) parser + token builder for
 * Phase 1.
 *
 * The `cashuA` payload is base64url-of-JSON with this shape (confirmed against
 * the real `cashubtc/cdk` `TokenV3` output the Rust core builds in
 * `build_bootstrap_token`):
 *
 * ```json
 * {
 *   "token": [
 *     { "mint": "https://mint.example",
 *       "proofs": [ { "amount": 4, "id": "...", "secret": "...", "C": "..." } ] }
 *   ],
 *   "unit": "sat",
 *   "memo": "..."
 * }
 * ```
 *
 * A single token can span multiple mints (one entry per mint in the `token`
 * array), so [parseCashuToken] returns a [CashuToken] whose [CashuToken.groups]
 * carries a per-mint subtotal — the wallet credits each mint independently.
 *
 * Only `cashuA` (JSON) is decoded. `cashuB` (CBOR) and legacy bare `cashu`
 * tokens parse to a clear [TokenResult.Error]; full CBOR support lands with the
 * Phase 3 wallet. Nothing here contacts a mint — spendability (proofs unspent)
 * is checked later via NUT-06/07.
 */

/** One mint-group inside a token: the mint URL + the subtotal of its proofs. */
data class TokenGroup(val mint: String, val amountSat: Long)

/** A parsed cashu token. */
data class CashuToken(
    val raw: String,
    /** One [TokenGroup] per mint the token draws on (usually one). */
    val groups: List<TokenGroup>,
    val unit: String,
    val memo: String?,
) {
    /** Total face value across all mint-groups, in sats. */
    val amountSat: Long get() = groups.sumOf { it.amountSat }

    /** The single mint when the token draws on exactly one, else null. */
    val singleMint: String? get() = if (groups.size == 1) groups[0].mint else null
}

/** Result of [parseCashuToken]: never throws. */
sealed interface TokenResult {
    data class Ok(val token: CashuToken) : TokenResult
    data class Error(val reason: String) : TokenResult
}

/**
 * Tolerantly parse a cashu token string. Accepts leading/trailing whitespace and
 * either `cashuA` (base64url-JSON, the modern default) or a bare `cashu` prefix
 * (treated as the same JSON payload). Returns [TokenResult.Error] for anything
 * else (including `cashuB` CBOR).
 */
fun parseCashuToken(raw: String): TokenResult {
    val trimmed = raw.trim()
    val payload: String = when {
        trimmed.startsWith("cashuA") -> trimmed.removePrefix("cashuA")
        // Legacy bare prefix: same JSON payload, just unprefixed scheme.
        trimmed.startsWith("cashu") -> trimmed.removePrefix("cashu")
        else -> return TokenResult.Error("not a cashu token (expected cashuA…)")
    }
    if (payload.isEmpty()) return TokenResult.Error("empty token payload")

    val jsonText = try {
        decodeBase64UrlUtf8(payload)
    } catch (_: IllegalArgumentException) {
        return TokenResult.Error("token payload is not valid base64")
    }
    val obj = try {
        JSONObject(jsonText)
    } catch (_: Exception) {
        return TokenResult.Error("token payload is not valid JSON")
    }

    val groups = mutableListOf<TokenGroup>()
    // Standard V3 shape: top-level "token" array of {mint, proofs}.
    val tokenArr = obj.opt("token")
    if (tokenArr is JSONArray) {
        for (i in 0 until tokenArr.length()) {
            val group = tokenArr.optJSONObject(i) ?: continue
            val mint = normalizeMint(group.optString("mint"))
            val sum = sumProofs(group.optJSONArray("proofs"))
            if (mint.isNotEmpty()) groups.add(TokenGroup(mint, sum))
        }
    } else {
        // Fallback flat shape (mint + proofs at top level) — some variant tokens.
        val mint = normalizeMint(obj.optString("mint"))
        val sum = sumProofs(obj.optJSONArray("proofs"))
        if (mint.isNotEmpty()) groups.add(TokenGroup(mint, sum))
    }
    if (groups.isEmpty()) return TokenResult.Error("token has no mint groups")

    return TokenResult.Ok(
        CashuToken(
            raw = trimmed,
            groups = groups,
            unit = obj.optString("unit", "sat"),
            memo = obj.optString("memo").takeIf { it.isNotEmpty() },
        ),
    )
}

private fun sumProofs(proofs: JSONArray?): Long {
    if (proofs == null) return 0L
    var sum = 0L
    for (i in 0 until proofs.length()) {
        val p = proofs.optJSONObject(i) ?: continue
        sum += p.optLong("amount", 0L)
    }
    return sum
}

private fun normalizeMint(mint: String): String = mint.trim().removeSuffix("/")

private fun decodeBase64UrlUtf8(b64url: String): String {
    // cashuA is base64url without padding. URL_SAFE mode handles the url-safe
    // alphabet; add '=' padding so decode is unambiguous across API levels.
    val padded = StringBuilder(b64url).apply {
        while (length % 4 != 0) append('=')
    }.toString()
    val bytes = Base64.decode(padded, Base64.URL_SAFE)
    return String(bytes, Charsets.UTF_8)
}

/**
 * Build a **local** `cashuA` token of [amountSat] drawn on [mint].
 *
 * The proof's `id` / `secret` / `C` are filler (a random secret + a zero `C`),
 * exactly the spirit of the Rust core's `build_test_token`: the token parses and
 * embeds the mint + amount so this app's own parser round-trips it, but it is
 * **not** spendable at a real Cashu mint. Real NUT-03 minting (blind signature
 * exchange with the mint) lands in Phase 3. The returned string is prefixed
 * `cashuA` so it is unambiguously a Cashu token.
 */
fun buildLocalToken(mint: String, amountSat: Long, memo: String? = null): String {
    require(amountSat > 0) { "amount must be > 0" }
    val proof = JSONObject().apply {
        put("amount", amountSat)
        put("id", LOCAL_KEYSET_ID)
        put("secret", randomSecret())
        // Filler commitment point; real C comes from the mint's blind signature.
        put("C", "")
    }
    val group = JSONObject().apply {
        put("mint", normalizeMint(mint))
        put("proofs", JSONArray().apply { put(proof) })
    }
    val obj = JSONObject().apply {
        put("token", JSONArray().apply { put(group) })
        put("unit", "sat")
        memo?.takeIf { it.isNotEmpty() }?.let { put("memo", it) }
    }
    val json = obj.toString()
    val b64 = Base64.encodeToString(json.toByteArray(Charsets.UTF_8), Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
    return "cashuA$b64"
}

private const val LOCAL_KEYSET_ID = "0000000000000000"

private fun randomSecret(): String {
    // 16 random bytes as hex — opaque, unique per token. Pure local.
    val bytes = ByteArray(16)
    Random.Default.nextBytes(bytes)
    return bytes.joinToString("") { "%02x".format(it) }
}

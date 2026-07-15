// No @file:JvmName here: the data class `CashuToken` below would clash with a
// file-facade of the same JVM name. Top-level helpers land in `CashuTokenKt`.

package org.opentollgate.android.util

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
 * `cashuA` (JSON) and `cashuB` (CBOR, Nutshell / cashu.me) are both decoded.
 * The CBOR decoder is a minimal inline implementation — no external dependency.
 * Nothing here contacts a mint — spendability (proofs unspent) is checked later
 * via NUT-06/07.
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
 * any of:
 * - `cashuA` (base64url-JSON, the modern default)
 * - `cashuB` (base64url-CBOR, used by Nutshell / cashu.me)
 * - bare `cashu` prefix (treated as the same JSON payload as `cashuA`)
 *
 * Returns [TokenResult.Error] for anything else.
 */
fun parseCashuToken(raw: String): TokenResult {
    val trimmed = raw.trim()
    // cashuB (CBOR) is structurally distinct — dispatch before JSON handling.
    if (trimmed.startsWith("cashuB")) {
        return parseCashuBToken(trimmed, trimmed.removePrefix("cashuB"))
    }
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

// ---------------------------------------------------------------------------
// cashuB (CBOR) parsing
// ---------------------------------------------------------------------------

/**
 * Parse a `cashuB` token (base64url-encoded CBOR) produced by Nutshell /
 * cashu.me and other wallets.
 *
 * CBOR structure (from NUT-00 token V4):
 * ```
 * { "t": [ { "i": bytes, "p": [ { "a": int, "s": str, "c": bytes }, … ] } ],
 *   "m": str,   // mint URL
 *   "u": str }  // unit
 * ```
 *
 * We only need the mint URL (`m`) and the sum of proof amounts (`a`). The
 * minimal [CborReader] below handles exactly the CBOR features this structure
 * uses — no external dependency required.
 */
private fun parseCashuBToken(raw: String, payload: String): TokenResult {
    if (payload.isEmpty()) return TokenResult.Error("empty cashuB payload")

    val cborBytes = try {
        decodeBase64UrlBytes(payload)
    } catch (_: IllegalArgumentException) {
        return TokenResult.Error("cashuB payload is not valid base64")
    }

    val root = try {
        CborReader(cborBytes).readItem()
    } catch (e: Exception) {
        return TokenResult.Error("cashuB payload is not valid CBOR: ${e.message}")
    }

    @Suppress("UNCHECKED_CAST")
    val rootMap = root as? Map<String, Any?>
        ?: return TokenResult.Error("cashuB root is not a CBOR map")

    val mint = normalizeMint(rootMap["m"] as? String ?: "")
    if (mint.isEmpty()) return TokenResult.Error("cashuB token missing mint URL")

    val groups = mutableListOf<TokenGroup>()
    val tokenGroups = rootMap["t"] as? List<*>
    if (tokenGroups != null) {
        for (tg in tokenGroups) {
            @Suppress("UNCHECKED_CAST")
            val tgMap = tg as? Map<String, Any?> ?: continue
            val proofs = tgMap["p"] as? List<*>
            var sum = 0L
            if (proofs != null) {
                for (p in proofs) {
                    @Suppress("UNCHECKED_CAST")
                    val pMap = p as? Map<String, Any?> ?: continue
                    val amt = pMap["a"]
                    if (amt is Number) sum += amt.toLong()
                }
            }
            groups.add(TokenGroup(mint, sum))
        }
    }

    if (groups.isEmpty()) return TokenResult.Error("cashuB token has no mint groups")

    return TokenResult.Ok(
        CashuToken(
            raw = raw,
            groups = groups,
            unit = rootMap["u"] as? String ?: "sat",
            memo = null,
        ),
    )
}

/**
 * Minimal RFC 8949 CBOR decoder. Supports the subset of CBOR used by cashuB
 * tokens: unsigned/negative integers, byte strings, text strings, arrays, maps,
 * and the common simple values (true / false / null). Indefinite-length items
 * (0x5F / 0x7F / 0x9F / 0xBF with 0xFF break) are handled for robustness.
 *
 * Float values (major type 7, ai 25-27) are consumed but returned as [Double];
 * their precision is irrelevant for token parsing.
 */
private class CborReader(private val data: ByteArray) {
    private var pos = 0

    fun readItem(): Any? {
        val initial = readByte()
        val majorType = initial shr 5
        val ai = initial and 0x1F

        // Additional info → length / argument value.
        val arg: Long = when {
            ai < 24 -> ai.toLong()
            ai == 24 -> readByte().toLong()
            ai == 25 -> readUint16().toLong()
            ai == 26 -> readUint32().toLong()
            ai == 27 -> readUint64()
            ai == 31 -> INDEFINITE
            else -> throw IllegalStateException("reserved additional info $ai")
        }

        return when (majorType) {
            0 -> arg                          // unsigned int
            1 -> -1L - arg                     // negative int
            2 -> readByteString(arg)           // byte string
            3 -> readTextString(arg)           // text string
            4 -> readArray(arg)                // array
            5 -> readMap(arg)                  // map
            7 -> readSimpleOrFloat(ai, arg)    // simple / float / break
            else -> throw IllegalStateException("unknown major type $majorType")
        }
    }

    private fun readByte(): Int {
        if (pos >= data.size) throw IllegalStateException("unexpected end of CBOR data at $pos")
        return data[pos++].toInt() and 0xFF
    }

    private fun readUint16(): Int =
        (readByte() shl 8) or readByte()

    private fun readUint32(): Long =
        ((readByte().toLong() shl 24)
            or (readByte().toLong() shl 16)
            or (readByte().toLong() shl 8)
            or readByte().toLong())

    private fun readUint64(): Long {
        var v = 0L
        repeat(8) { v = (v shl 8) or readByte().toLong() }
        return v
    }

    private fun readN(n: Int): ByteArray {
        if (pos + n > data.size) throw IllegalStateException("unexpected end of CBOR data at $pos")
        val out = data.copyOfRange(pos, pos + n)
        pos += n
        return out
    }

    // ---- major-type decoders ------------------------------------------------

    private fun readByteString(arg: Long): Any {
        if (arg == INDEFINITE) {
            // Indefinite-length byte string: concatenate chunks until break.
            val chunks = mutableListOf<ByteArray>()
            while (!peekBreak()) chunks.add(readItem() as ByteArray)
            return chunks.reduce { acc, chunk -> acc + chunk }
        }
        return readN(arg.toInt())
    }

    private fun readTextString(arg: Long): String {
        if (arg == INDEFINITE) {
            val sb = StringBuilder()
            while (!peekBreak()) sb.append(readItem() as String)
            return sb.toString()
        }
        return String(readN(arg.toInt()), Charsets.UTF_8)
    }

    private fun readArray(arg: Long): List<Any?> {
        val items = mutableListOf<Any?>()
        if (arg == INDEFINITE) {
            while (!peekBreak()) items.add(readItem())
        } else {
            repeat(arg.toInt()) { items.add(readItem()) }
        }
        return items
    }

    private fun readMap(arg: Long): Map<String, Any?> {
        val map = LinkedHashMap<String, Any?>()
        if (arg == INDEFINITE) {
            while (!peekBreak()) {
                val key = readItem()
                map[key.toString()] = readItem()
            }
        } else {
            repeat(arg.toInt()) {
                val key = readItem()
                map[key.toString()] = readItem()
            }
        }
        return map
    }

    private fun readSimpleOrFloat(ai: Int, arg: Long): Any? = when {
        ai == 20 -> false
        ai == 21 -> true
        ai == 22 -> null          // null
        ai == 23 -> null          // undefined → treat as null
        ai == 25 -> { readUint16(); 0.0 }     // half-float (skip precision)
        ai == 26 -> { readUint32(); 0.0 }     // single-float
        ai == 27 -> { readUint64(); 0.0 }     // double-float
        ai == 31 -> BREAK_SENTINEL             // break code in non-indefinite context
        else -> arg
    }

    /** Peek at the next byte; if it is 0xFF (break), consume it and return true. */
    private fun peekBreak(): Boolean {
        if (pos < data.size && (data[pos].toInt() and 0xFF) == 0xFF) {
            pos++
            return true
        }
        return false
    }

    companion object {
        private const val INDEFINITE = -1L
        private val BREAK_SENTINEL = Any()
    }
}

private fun decodeBase64UrlBytes(b64url: String): ByteArray {
    return java.util.Base64.getUrlDecoder().decode(b64url)
}

private fun decodeBase64UrlUtf8(b64url: String): String {
    val bytes = java.util.Base64.getUrlDecoder().decode(b64url)
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
    val b64 = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray(Charsets.UTF_8))
    return "cashuA$b64"
}

private const val LOCAL_KEYSET_ID = "0000000000000000"

private fun randomSecret(): String {
    // 16 random bytes as hex — opaque, unique per token. Pure local.
    val bytes = ByteArray(16)
    Random.Default.nextBytes(bytes)
    return bytes.joinToString("") { "%02x".format(it) }
}

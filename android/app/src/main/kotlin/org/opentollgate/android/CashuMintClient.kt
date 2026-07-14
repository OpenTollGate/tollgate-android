package org.opentollgate.android

import android.util.Base64
import android.util.Log
import org.bouncycastle.asn1.x9.X9ECParameters
import org.bouncycastle.crypto.ec.CustomNamedCurves
import org.bouncycastle.crypto.params.ECDomainParameters
import org.bouncycastle.math.ec.ECAlgorithms
import org.bouncycastle.math.ec.ECPoint
import org.json.JSONArray
import org.json.JSONObject
import java.math.BigInteger
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.security.SecureRandom
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Cashu mint client implementing NUT-00 (blinded signatures) and NUT-04 (minting via Lightning).
 *
 * Flow:
 * 1. requestQuote(mintUrl, amount) → Lightning invoice + quote ID
 * 2. Poll checkQuote(mintUrl, quoteId) until state == "PAID"
 * 3. mintTokens(mintUrl, quoteId, amount) → Cashu proofs (e-cash)
 * 4. buildToken(proofs, mintUrl) → cashuB... string for TollGate payment
 *
 * The blind signature protocol:
 * - Generate random secret + blinding factor r
 * - B = hash_to_curve(secret)
 * - B_ = B * r^(-1)  (blinded message)
 * - Send B_ to mint → mint returns C_ = B_ * k_mint (blinded signature)
 * - C = C_ * r  (unblinded signature = the proof)
 * - Verification: C == hash_to_curve(secret) * k_mint
 */
object CashuMintClient {

    private const val TAG = "CashuMintClient"
    private const val TIMEOUT_MS = 10000

    // secp256k1 domain parameters
    private val CURVE_PARAMS: X9ECParameters = CustomNamedCurves.getByName("secp256k1")
    private val DOMAIN = ECDomainParameters(CURVE_PARAMS.curve, CURVE_PARAMS.g, CURVE_PARAMS.n, CURVE_PARAMS.h)
    private val N: BigInteger = DOMAIN.n
    private val G: ECPoint = DOMAIN.g
    private val random = SecureRandom()

    data class Quote(
        val quoteId: String,
        val invoice: String,
        val amount: Long,
        val state: String, // UNPAID, PAID, EXPIRED
    )

    data class Proof(
        val amount: Long,
        val secret: String,   // hex
        val c: String,        // hex (compressed point C)
        val id: String = "",  // keyset ID (hex) — needed for gateway to verify proofs
    )

    // ── Cashu API ──────────────────────────────────────────────────

    /**
     * Step 1: Request a Lightning invoice from the mint.
     * POST /v1/mint/quote/bolt11
     */
    fun requestQuote(mintUrl: String, amountSat: Long): Quote? {
        return try {
            val url = URL("${mintUrl.trimEnd('/')}/v1/mint/quote/bolt11")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
            }
            val body = JSONObject().apply {
                put("amount", amountSat)
                put("unit", "sat")
            }.toString()
            conn.outputStream.use { it.write(body.toByteArray()) }

            if (conn.responseCode !in 200..299) {
                Log.e(TAG, "requestQuote → HTTP ${conn.responseCode}")
                return null
            }
            val resp = conn.inputStream.bufferedReader().readText()
            conn.disconnect()

            val json = JSONObject(resp)
            Quote(
                quoteId = json.getString("quote"),
                invoice = json.getString("request"),
                amount = amountSat,
                state = json.optString("state", "UNPAID"),
            )
        } catch (e: Exception) {
            Log.e(TAG, "requestQuote failed: ${e.message}")
            null
        }
    }

    /**
     * Step 2: Check if the Lightning invoice has been paid.
     * GET /v1/mint/quote/bolt11/{quoteId}
     */
    fun checkQuote(mintUrl: String, quoteId: String): String? {
        return try {
            val url = URL("${mintUrl.trimEnd('/')}/v1/mint/quote/bolt11/$quoteId")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
            if (conn.responseCode !in 200..299) return null
            val resp = conn.inputStream.bufferedReader().readText()
            conn.disconnect()
            JSONObject(resp).optString("state", "UNPAID")
        } catch (e: Exception) {
            Log.d(TAG, "checkQuote failed: ${e.message}")
            null
        }
    }

    /**
     * Step 3: Mint tokens after Lightning invoice is paid.
     * Implements the full blind signature protocol (NUT-00).
     *
     * POST /v1/mint/bolt11
     *
     * @return list of Cashu proofs + the assembled token string
     */
    data class MintResult(
        val proofs: List<Proof>,
        val token: String,  // cashuA... format (V3 JSON)
    )

    fun mintTokens(mintUrl: String, quoteId: String, totalAmount: Long): MintResult? {
        // Fetch the active keyset ID for "sat" unit (NUT-02)
        val keysetId = getActiveKeysetId(mintUrl)
        if (keysetId == null) {
            Log.e(TAG, "mintTokens: failed to fetch keyset ID from mint")
            return null
        }
        Log.d(TAG, "mintTokens: using keysetId=$keysetId")

        // Split amount into powers of 2 (Cashu amount denomination)
        val amounts = splitAmount(totalAmount)

        // Generate blinded messages for each amount
        val blindedOutputs = mutableListOf<JSONObject>()
        val blindingData = mutableListOf<Pair<String, BigInteger>>() // (secretHex, r)

        for (amt in amounts) {
            val secret = randomBytes(32)
            val secretHex = secret.toHex()
            val r = randomModN()
            // CRITICAL: hash the hex string bytes, not the raw bytes.
            // The gateway does HashToCurve([]byte(secret)) where secret is the
            // hex string stored in the proof. If we hash raw bytes here, the
            // gateway computes a different Y → proofs can't be verified.
            val bPoint = hashToCurve(secretHex.toByteArray(Charsets.UTF_8))
            val rInv = r.modInverse(N)
            val bBlinded = bPoint.multiply(rInv).normalize()
            val bHex = bBlinded.getEncoded(true).toHex()

            blindingData.add(secretHex to r)
            blindedOutputs.add(JSONObject().apply {
                put("amount", amt)
                put("id", keysetId)
                put("B_", bHex)
            })
        }

        // POST blinded messages to mint
        val result = try {
            val url = URL("${mintUrl.trimEnd('/')}/v1/mint/bolt11")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
            }
            val reqBody = JSONObject().apply {
                put("quote", quoteId)
                put("outputs", JSONArray(blindedOutputs))
            }.toString()
            conn.outputStream.use { it.write(reqBody.toByteArray()) }

            if (conn.responseCode !in 200..299) {
                Log.e(TAG, "mintTokens → HTTP ${conn.responseCode}: ${conn.errorStream?.bufferedReader()?.readText()}")
                return null
            }
            val resp = conn.inputStream.bufferedReader().readText()
            conn.disconnect()
            JSONObject(resp)
        } catch (e: Exception) {
            Log.e(TAG, "mintTokens API call failed: ${e.message}")
            return null
        }

        // Parse signatures and unblind them
        val signatures = result.optJSONArray("signatures") ?: run {
            Log.e(TAG, "No signatures in mint response")
            return null
        }

        val proofs = mutableListOf<Proof>()
        for (i in 0 until signatures.length()) {
            val sig = signatures.optJSONObject(i) ?: continue
            val amount = sig.optLong("amount", 0)
            val cBlindedHex = sig.optString("C_", "")
            if (cBlindedHex.isBlank()) continue

            // Unblind: C = C_ * r
            val cBlindedPoint = decompressPoint(cBlindedHex.hexToBytes())
            val (secretHex, r) = blindingData[i]
            val cUnblinded = cBlindedPoint.multiply(r).normalize()
            val cHex = cUnblinded.getEncoded(true).toHex()

            proofs.add(Proof(amount = amount, secret = secretHex, c = cHex, id = keysetId))
        }

        // Build the Cashu token
        val token = buildToken(proofs, mintUrl)
        return MintResult(proofs = proofs, token = token)
    }

    // ── Keyset Discovery ──────────────────────────────────────────

    /**
     * Fetch the active keyset ID for "sat" unit from the mint.
     * GET /v1/keysets → {"keysets": [{"id":"...", "unit":"sat", "active":true}, ...]}
     */
    private fun getActiveKeysetId(mintUrl: String): String? {
        return try {
            val url = URL("${mintUrl.trimEnd('/')}/v1/keysets")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
            if (conn.responseCode !in 200..299) {
                Log.e(TAG, "getActiveKeysetId → HTTP ${conn.responseCode}")
                return null
            }
            val resp = conn.inputStream.bufferedReader().readText()
            conn.disconnect()

            val keysets = JSONObject(resp).optJSONArray("keysets") ?: return null
            for (i in 0 until keysets.length()) {
                val ks = keysets.optJSONObject(i) ?: continue
                val unit = ks.optString("unit", "")
                val active = ks.optBoolean("active", true)
                if (unit == "sat" && active) {
                    return ks.getString("id")
                }
            }
            // Fallback: first keyset with sat unit
            for (i in 0 until keysets.length()) {
                val ks = keysets.optJSONObject(i) ?: continue
                if (ks.optString("unit", "") == "sat") return ks.getString("id")
            }
            null
        } catch (e: Exception) {
            Log.e(TAG, "getActiveKeysetId failed: ${e.message}")
            null
        }
    }

    // ── Cashu Token Building ───────────────────────────────────────

    /**
     * Build a Cashu V3 token string from proofs.
     * Format: cashuA<base64url(json({"token":[{"mint":"url","proofs":[{"amount":N,"secret":"hex","C":"hex"}]}],"unit":"sat"}))>
     *
     * CRITICAL: V3 JSON tokens MUST use full key names (mint/proofs/amount/secret/C).
     * Short keys (i/p/a/s/c) are the V4 CBOR convention and are NOT recognized
     * by V3 JSON decoders like gonuts-tollgate on the gateway side.
     */
    private fun buildToken(proofs: List<Proof>, mintUrl: String): String {
        val proofsArray = JSONArray()
        for (p in proofs) {
            proofsArray.put(JSONObject().apply {
                put("amount", p.amount)
                put("secret", p.secret)
                put("C", p.c)
                put("id", p.id)
            })
        }

        val tokenInner = JSONObject().apply {
            put("mint", mintUrl.trimEnd('/'))
            put("proofs", proofsArray)
        }

        val outer = JSONObject().apply {
            put("token", JSONArray().put(tokenInner))
            put("unit", "sat")
            put("memo", "TollGate payment")
        }

        val jsonStr = outer.toString()
        val b64 = Base64.encodeToString(jsonStr.toByteArray(), Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
        // FIX: use cashuA prefix for JSON-encoded V3 tokens.
        // cashuB prefix means CBOR (V4) — gateway's V4 decoder tries CBOR
        // unmarshal on JSON bytes and fails, then V3 fallback expects cashuA.
        // This caused "invalid V3 token" rejections on every payment attempt.
        return "cashuA$b64"
    }

    // ── secp256k1 Crypto Operations ────────────────────────────────

    /**
     * Cashu hash_to_curve (NUT-00) — matches gonuts-tollgate gateway implementation.
     *
     * Algorithm:
     * 1. msgToHash = SHA256("Secp256k1_HashToCurve_Cashu_" + message)
     * 2. For counter = 0, 1, 2, ... (4-byte little-endian):
     *    hash = SHA256(msgToHash + counter_bytes)
     *    Try compressed point 0x02 || hash
     *    If valid and on curve → return
     *
     * CRITICAL: must match gonuts' crypto.HashToCurve exactly, otherwise
     * the gateway cannot verify proofs (C != k * Y_gateway).
     */
    private fun hashToCurve(message: ByteArray): ECPoint {
        val DOMAIN_SEPARATOR = "Secp256k1_HashToCurve_Cashu_".toByteArray()
        val sha256 = MessageDigest.getInstance("SHA-256")
        val msgToHash = sha256.digest(DOMAIN_SEPARATOR + message)

        var counter = 0
        while (counter < 65536) {
            // 4-byte little-endian counter (matches gonuts binary.LittleEndian.PutUint32)
            val counterBytes = ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(counter).array()
            val hash = sha256.digest(msgToHash + counterBytes)
            val compressed = byteArrayOf(0x02) + hash
            try {
                // decodePoint throws if the x-coordinate doesn't yield a valid
                // curve point (equivalent to gonuts' ParsePubKey + IsOnCurve)
                return CURVE_PARAMS.curve.decodePoint(compressed)
            } catch (_: Exception) {
            }
            counter++
        }
        throw RuntimeException("hashToCurve: no valid point found after 65536 iterations")
    }

    /**
     * Decompress a 33-byte compressed secp256k1 public key into an ECPoint.
     */
    private fun decompressPoint(compressed: ByteArray): ECPoint {
        return CURVE_PARAMS.curve.decodePoint(compressed)
    }

    /**
     * Generate a random number in [1, n-1] where n is the curve order.
     */
    private fun randomModN(): BigInteger {
        var r: BigInteger
        do {
            r = BigInteger(N.bitLength(), random)
        } while (r >= N || r == BigInteger.ZERO)
        return r
    }

    /**
     * Generate random bytes.
     */
    private fun randomBytes(n: Int): ByteArray {
        val bytes = ByteArray(n)
        random.nextBytes(bytes)
        return bytes
    }

    /**
     * Split an amount into powers of 2 (Cashu denomination scheme).
     * e.g. 13 = [1, 4, 8], 21 = [1, 4, 16]
     */
    private fun splitAmount(amount: Long): List<Long> {
        val result = mutableListOf<Long>()
        var remaining = amount
        var pow = 1L
        while (remaining > 0) {
            if (remaining and 1 == 1L) result.add(pow)
            remaining = remaining shr 1
            pow = pow shl 1
        }
        return result
    }

    // ── Hex utilities ──────────────────────────────────────────────

    private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
    private fun String.hexToBytes(): ByteArray {
        val len = length / 2
        val result = ByteArray(len)
        for (i in 0 until len) {
            val hi = this[i * 2].digitToInt(16)
            val lo = this[i * 2 + 1].digitToInt(16)
            result[i] = ((hi shl 4) or lo).toByte()
        }
        return result
    }
}

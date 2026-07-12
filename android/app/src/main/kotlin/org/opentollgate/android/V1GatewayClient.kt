package org.opentollgate.android

import android.util.Log
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * TollGate v1 HTTP gateway client.
 *
 * The production routers run `tollgate-module-basic-go` which exposes a simple
 * REST API on port 2121 (NOT the v2 CBOR protocol on port 4747). This client
 * implements the v1 protocol:
 *
 *  - `GET /`  → Nostr event kind 10021 (advertisement with pricing + mints)
 *  - `POST /` → Cashu token body → session event (kind 1022) or notice (kind 21023)
 *
 * The v2 CBOR protocol (tollgate-rs) is a future upgrade path; today's routers
 * all speak v1 HTTP.
 */
object V1GatewayClient {

    private const val TAG = "V1GatewayClient"
    private const val TIMEOUT_MS = 4000

    data class Advertisement(
        val pubkeyHex: String,
        val metric: String,
        val stepSize: Long,
        val mints: List<String>,
        val pricePerStep: Long,
        val priceUnit: String,
        val tips: List<String>,
        val rawJson: String,
    )

    data class PaymentResult(
        val accepted: Boolean,
        val rawJson: String,
        val error: String? = null,
    )

    /**
     * Probe a gateway's advertisement endpoint.
     * @param baseUrl e.g. "http://10.230.237.1:2121"
     * @return parsed advertisement, or null if unreachable / invalid
     */
    fun detect(baseUrl: String): Advertisement? {
        return try {
            val cleanUrl = baseUrl.trimEnd('/')
            val url = URL("$cleanUrl/")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
                setRequestProperty("Accept", "application/json")
            }
            val code = conn.responseCode
            if (code != 200) {
                Log.w(TAG, "detect $cleanUrl → HTTP $code")
                return null
            }
            val body = conn.inputStream.bufferedReader().readText()
            conn.disconnect()
            parseAdvertisement(body)
        } catch (e: Exception) {
            Log.d(TAG, "detect $baseUrl failed: ${e.message}")
            null
        }
    }

    /**
     * Submit a Cashu token to purchase internet access.
     * @param baseUrl e.g. "http://10.230.237.1:2121"
     * @param cashuToken raw Cashu token string (cashuA...)
     * @return payment result
     */
    fun pay(baseUrl: String, cashuToken: String): PaymentResult {
        return try {
            val cleanUrl = baseUrl.trimEnd('/')
            val url = URL("$cleanUrl/")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                connectTimeout = TIMEOUT_MS
                readTimeout = 15000  // payment may take longer
                doOutput = true
                setRequestProperty("Content-Type", "text/plain")
            }
            conn.outputStream.use { it.write(cashuToken.toByteArray()) }
            val code = conn.responseCode
            val body = if (code in 200..299) {
                conn.inputStream.bufferedReader().readText()
            } else {
                conn.errorStream?.bufferedReader()?.readText() ?: "{}"
            }
            conn.disconnect()

            // Response is a Nostr event (kind 1022 = session, 21023 = notice/error)
            val json = JSONObject(body)
            val kind = json.optInt("kind", 0)
            val accepted = kind == 1022 || (kind == 0 && code == 200)

            PaymentResult(
                accepted = accepted,
                rawJson = body,
                error = if (!accepted) extractNoticeMessage(json) else null,
            )
        } catch (e: Exception) {
            Log.e(TAG, "pay $baseUrl failed: ${e.message}")
            PaymentResult(accepted = false, rawJson = "", error = e.message)
        }
    }

    /**
     * Query usage from the gateway.
     * @return pair of (used, total) or null on failure
     */
    fun getUsage(baseUrl: String, macAddress: String? = null): Pair<Long, Long>? {
        return try {
            val cleanUrl = baseUrl.trimEnd('/')
            val url = URL("$cleanUrl/usage")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
            if (conn.responseCode != 200) return null
            val body = conn.inputStream.bufferedReader().readText().trim()
            conn.disconnect()
            // Format: "used/total" (e.g. "1234567/22020096")
            val parts = body.split("/")
            if (parts.size == 2) {
                Pair(parts[0].toLongOrNull() ?: -1, parts[1].toLongOrNull() ?: -1)
            } else null
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Query session balance from the gateway.
     * GET /balance → JSON with session_active, usage, allotment, remaining.
     * The gateway identifies the device by MAC (from ARP table), so no
     * auth needed — just be on the router's network.
     */
    data class Balance(
        val sessionActive: Boolean,
        val metric: String,
        val usage: Long,
        val allotment: Long,
        val remaining: Long,
        val startTime: Long,
    )

    fun getBalance(baseUrl: String): Balance? {
        return try {
            val cleanUrl = baseUrl.trimEnd('/')
            val url = URL("$cleanUrl/balance")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
            if (conn.responseCode != 200) return null
            val body = conn.inputStream.bufferedReader().readText()
            conn.disconnect()
            val json = JSONObject(body)
            Balance(
                sessionActive = json.optBoolean("session_active", false),
                metric = json.optString("metric", "bytes"),
                usage = json.optLong("usage", 0),
                allotment = json.optLong("allotment", 0),
                remaining = json.optLong("remaining", 0),
                startTime = json.optLong("start_time", 0),
            )
        } catch (e: Exception) {
            Log.d(TAG, "getBalance $baseUrl failed: ${e.message}")
            null
        }
    }

    /**
     * Get the device's MAC address as seen by the gateway.
     * GET /whoami → "mac=XX:XX:XX:XX:XX:XX"
     * Useful for debugging and for the gateway to identify the device.
     */
    fun getWhoami(baseUrl: String): String? {
        return try {
            val cleanUrl = baseUrl.trimEnd('/')
            val url = URL("$cleanUrl/whoami")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
            if (conn.responseCode != 200) return null
            val body = conn.inputStream.bufferedReader().readText().trim()
            conn.disconnect()
            // Format: "mac=XX:XX:XX:XX:XX:XX"
            body.removePrefix("mac=").ifBlank { null }
        } catch (e: Exception) {
            null
        }
    }

    private fun parseAdvertisement(body: String): Advertisement? {
        return try {
            val json = JSONObject(body)
            val kind = json.optInt("kind", 0)
            if (kind != 10021) {
                Log.w(TAG, "Not a TollGate advertisement (kind=$kind)")
                return null
            }

            val pubkey = json.optString("pubkey", "")
            val tags = json.optJSONArray("tags") ?: return null

            var metric = "bytes"
            var stepSize = 0L
            val mints = mutableSetOf<String>()
            var pricePerStep = 1L
            var priceUnit = "sats"
            val tips = mutableListOf<String>()

            for (i in 0 until tags.length()) {
                val tag = tags.optJSONArray(i) ?: continue
                when (tag.optString(0)) {
                    "metric" -> metric = tag.optString(1, "bytes")
                    "step_size" -> stepSize = tag.optString(1, "0").toLongOrNull() ?: 0L
                    "price_per_step" -> {
                        // ["price_per_step", "cashu", "1", "sats", "https://mint.url", "0"]
                        if (tag.length() >= 5) {
                            pricePerStep = tag.optString(2, "1").toLongOrNull() ?: 1L
                            priceUnit = tag.optString(3, "sats")
                            mints.add(tag.optString(4))
                        }
                    }
                    "tips" -> {
                        for (j in 1 until tag.length()) {
                            tips.add(tag.optString(j))
                        }
                    }
                }
            }

            Advertisement(
                pubkeyHex = pubkey,
                metric = metric,
                stepSize = stepSize,
                mints = mints.toList(),
                pricePerStep = pricePerStep,
                priceUnit = priceUnit,
                tips = tips,
                rawJson = body,
            )
        } catch (e: Exception) {
            Log.e(TAG, "parseAdvertisement failed: ${e.message}")
            null
        }
    }

    private fun extractNoticeMessage(json: JSONObject): String? {
        return try {
            val tags = json.optJSONArray("tags") ?: return null
            for (i in 0 until tags.length()) {
                val tag = tags.optJSONArray(i) ?: continue
                if (tag.optString(0) == "message") {
                    return tag.optString(1)
                }
            }
            null
        } catch (_: Exception) {
            null
        }
    }
}

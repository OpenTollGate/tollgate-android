package org.opentollgate.android.discovery

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/**
 * Queries Nostr relays for kind 30078 tollgate-gateway announcements.
 *
 * Publisher format:
 *   tags: [["d", "tollgate-gateway"], ["t", "tollgate"]]
 *   content: {"http":"http://<ip>:4747","npub":"npub1...","transport":"tollgate-v2"}
 */
class NostrDiscovery(
    private val relays: List<String> = DEFAULT_RELAYS,
    private val timeoutMs: Long = 5_000L,
) {
    companion object {
        val DEFAULT_RELAYS = listOf(
            "wss://relay1.orangesync.tech",
            "wss://nos.lol",
        )
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(3, TimeUnit.SECONDS)
        .readTimeout(5, TimeUnit.SECONDS)
        .build()

    /** Query relays and return gateway HTTP URLs. */
    suspend fun discoverGateways(): List<String> = withContext(Dispatchers.IO) {
        val allUrls = mutableSetOf<String>()
        for (relayUrl in relays) {
            allUrls.addAll(queryRelay(relayUrl))
        }
        allUrls.toList()
    }

    private fun queryRelay(relayUrl: String): List<String> {
        val urls = mutableListOf<String>()
        val latch = CountDownLatch(1)
        val errorRef = AtomicReference<Throwable>()

        val ws = client.newWebSocket(
            Request.Builder().url(relayUrl).build(),
            object : WebSocketListener() {
                override fun onOpen(ws: WebSocket, response: Response) {
                    val filter = JSONObject().apply {
                        put("kinds", JSONArray(listOf(30078)))
                        put("#d", JSONArray(listOf("tollgate-gateway")))
                        put("limit", 10)
                    }
                    val req = JSONArray(listOf("REQ", "tg-disc", filter))
                    ws.send(req.toString())
                }

                override fun onMessage(ws: WebSocket, text: String) {
                    try {
                        val arr = JSONArray(text)
                        when (arr.optString(0)) {
                            "EVENT" -> {
                                val ev = arr.optJSONObject(2) ?: return
                                if (ev.optInt("kind") == 30078) {
                                    val c = JSONObject(ev.optString("content", "{}"))
                                    val http = c.optString("http", "").trim()
                                    if (http.isNotBlank() && http.startsWith("http")) {
                                        synchronized(urls) { urls.add(http) }
                                    }
                                }
                            }
                            "EOSE" -> latch.countDown()
                        }
                    } catch (_: Exception) { /* skip */ }
                }

                override fun onFailure(ws: WebSocket, t: Throwable, response: Response?) {
                    errorRef.set(t)
                    latch.countDown()
                }

                override fun onClosed(ws: WebSocket, code: Int, reason: String) {
                    latch.countDown()
                }
            }
        )

        // Wait for EOSE or timeout
        latch.await(timeoutMs, TimeUnit.MILLISECONDS)
        ws.close(1000, "done")
        return synchronized(urls) { urls.toList() }
    }
}

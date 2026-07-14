package org.opentollgate.android

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiNetworkSpecifier
import android.os.Build
import android.os.OutcomeReceiver
import android.util.Log
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Connects to a TollGate WiFi SSID and routes HTTP traffic through that network.
 *
 * Uses WifiNetworkSpecifier (Android 10+) to create a system-managed connection
 * to the TollGate SSID. The system shows a confirmation dialog to the user.
 * Once connected, the returned [Network] object is used to route HTTP requests
 * to the gateway (via Network.openConnection).
 *
 * This is the ONLY way for an app to programmatically join a WiFi network on
 * Android 10+ without being a system/device-owner app. The deprecated
 * WifiManager.addNetwork() / enableNetwork() throws SecurityException on
 * API 29+.
 *
 * The connection is per-app: only this app's traffic flows through the TollGate
 * network. Other apps keep using the default (upstream internet) network.
 * This is actually ideal for TollGate: the app negotiates payment, then the
 * gateway opens the firewall for the device MAC — which benefits ALL apps
 * because the router's captive portal / firewall rule is MAC-based.
 */
class WifiNetworkConnector(private val context: Context) {

    companion object {
        private const val TAG = "WifiNetworkConnector"
    }

    private val connectivityManager: ConnectivityManager? =
        context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager

    /** The active TollGate network, if connected. Used for HTTP routing. */
    private val _activeNetwork = MutableStateFlow<Network?>(null)
    val activeNetwork: StateFlow<Network?> = _activeNetwork

    /** The SSID we're currently connected to (or null). */
    private val _connectedSsid = MutableStateFlow<String?>(null)
    val connectedSsid: StateFlow<String?> = _connectedSsid

    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    /**
     * Connect to a TollGate SSID.
     *
     * Shows a system dialog asking the user to confirm. On success, [activeNetwork]
     * is populated and can be used for HTTP routing.
     *
     * @param ssid e.g. "TollGate-F794"
     * @param timeoutMs how long to wait for connection (default 30s)
     * @return the connected Network, or null on failure/timeout
     */
    suspend fun connectToSsid(
        ssid: String,
        timeoutMs: Long = 30_000L,
    ): Network? {
        if (connectivityManager == null) {
            Log.e(TAG, "ConnectivityManager unavailable")
            return null
        }

        // Disconnect from any existing TollGate network first
        disconnect()

        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            Log.e(TAG, "WifiNetworkSpecifier requires Android 10+")
            return null
        }

        val result = CompletableDeferred<Network?>()

        val specifier = WifiNetworkSpecifier.Builder()
            .setSsid(ssid)
            // Open networks (most TollGate captive portals start open)
            // If WPA2 is needed, add: .setWpa2Passphrase(password)
            .build()

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .setNetworkSpecifier(specifier)
            .build()

        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                Log.i(TAG, "Connected to $ssid — network available")
                // Do NOT bindProcessToNetwork — it would route mint HTTP requests
                // through TollGate WiFi which has no DNS/internet. Instead, store
                // the network and pass it only to gateway detect/pay calls.
                _activeNetwork.value = network
                _connectedSsid.value = ssid
                result.complete(network)
            }

            override fun onUnavailable() {
                Log.w(TAG, "Connection to $ssid unavailable (user cancelled or timeout)")
                result.complete(null)
            }

            override fun onLost(network: Network) {
                Log.w(TAG, "Lost connection to $ssid")
                _activeNetwork.value = null
                _connectedSsid.value = null
                try {
                    connectivityManager.bindProcessToNetwork(null)
                } catch (_: Exception) {}
            }
        }

        networkCallback = callback

        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                // Android 12+: use OutcomeReceiver API
                connectivityManager.requestNetwork(
                    request,
                    callback,
                    timeoutMs.toInt(),
                )
            } else {
                connectivityManager.requestNetwork(request, callback)
            }
        } catch (e: Exception) {
            Log.e(TAG, "requestNetwork failed: ${e.message}")
            return null
        }

        return withTimeoutOrNull(timeoutMs + 5000L) { result.await() }
    }

    /** Disconnect from the current TollGate network. */
    fun disconnect() {
        networkCallback?.let {
            try {
                connectivityManager?.unregisterNetworkCallback(it)
            } catch (_: Exception) {}
        }
        networkCallback = null
        _activeNetwork.value = null
        _connectedSsid.value = null
        try {
            connectivityManager?.bindProcessToNetwork(null)
        } catch (_: Exception) {}
    }

    /**
     * Get all candidate gateway URLs for the connected TollGate network.
     *
     * Tries multiple sources to build a list of likely gateway IPs:
     * 1. LinkProperties route gateways (IPv4 only)
     * 2. DHCP gateway (from dhcpInfo)
     * 3. DHCP server address
     * 4. Derived from our IP: x.x.x.1, x.x.x.254, x.x.x.2
     *
     * The caller should probe ALL of these in parallel and use whichever responds.
     */
    fun getGatewayCandidates(network: Network? = activeNetwork.value): List<String> {
        val result = mutableSetOf<String>()

        // Source 1: LinkProperties routes (IPv4 gateways)
        if (network != null && connectivityManager != null) {
            val lp = connectivityManager.getLinkProperties(network)
            if (lp != null) {
                for (route in lp.routes) {
                    val gw = route.gateway
                    if (gw != null && !gw.isLoopbackAddress && gw is java.net.Inet4Address) {
                        val ip = gw.hostAddress ?: continue
                        Log.i(TAG, "Route gateway candidate: $ip")
                        result.add("http://$ip:2121")
                    }
                }
                // Source 2: derive from our link address
                for (linkAddr in lp.linkAddresses) {
                    val ip = linkAddr.address
                    if (ip is java.net.Inet4Address && !ip.isLoopbackAddress) {
                        val b = ip.address
                        val prefix = "${b[0].toInt() and 0xFF}.${b[1].toInt() and 0xFF}.${b[2].toInt() and 0xFF}"
                        Log.i(TAG, "Our IP on TollGate: ${ip.hostAddress}, deriving candidates")
                        result.add("http://$prefix.1:2121")
                        result.add("http://$prefix.254:2121")
                        result.add("http://$prefix.2:2121")
                    }
                }
            }
        }

        // Source 3: WifiManager dhcpInfo
        val wifiManager = context.getSystemService(Context.WIFI_SERVICE) as? android.net.wifi.WifiManager
        if (wifiManager != null) {
            val dhcp = wifiManager.dhcpInfo
            if (dhcp != null) {
                if (dhcp.gateway != 0) {
                    val gwIp = "${dhcp.gateway and 0xFF}.${(dhcp.gateway shr 8) and 0xFF}.${(dhcp.gateway shr 16) and 0xFF}.${(dhcp.gateway shr 24) and 0xFF}"
                    Log.d(TAG, "DHCP gateway candidate: $gwIp")
                    result.add("http://$gwIp:2121")
                }
                if (dhcp.serverAddress != 0) {
                    val srvIp = "${dhcp.serverAddress and 0xFF}.${(dhcp.serverAddress shr 8) and 0xFF}.${(dhcp.serverAddress shr 16) and 0xFF}.${(dhcp.serverAddress shr 24) and 0xFF}"
                    Log.d(TAG, "DHCP server candidate: $srvIp")
                    result.add("http://$srvIp:2121")
                }
            }
        }

        // Filter out obviously invalid entries
        return result.filter { !it.contains("::") && !it.contains("http://0.") && !it.contains(":0:2121") }
    }

    /** Legacy single-URL getter — delegates to [getGatewayCandidates]. */
    fun getGatewayUrl(network: Network? = activeNetwork.value): String? =
        getGatewayCandidates(network).firstOrNull()
}

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

    /** Get the DHCP gateway URL of the connected TollGate network. */
    fun getGatewayUrl(): String? {
        // After connecting, the gateway IP comes from WifiManager dhcpInfo
        val wifiManager = context.getSystemService(Context.WIFI_SERVICE) as? android.net.wifi.WifiManager
            ?: return null
        val dhcp = wifiManager.dhcpInfo ?: return null
        if (dhcp.gateway == 0) return null
        val ip = "${dhcp.gateway and 0xFF}.${(dhcp.gateway shr 8) and 0xFF}.${(dhcp.gateway shr 16) and 0xFF}.${(dhcp.gateway shr 24) and 0xFF}"
        return "http://$ip:2121"
    }
}

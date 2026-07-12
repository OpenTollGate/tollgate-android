package org.opentollgate.android

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.net.wifi.ScanResult
import android.net.wifi.WifiManager
import android.os.Build
import android.util.Log
import androidx.core.content.ContextCompat

/**
 * WiFi SSID scanner for Layer 1 TollGate discovery.
 *
 * Scans for nearby WiFi networks whose SSID starts with "TollGate-" (case-insensitive).
 * Returns the SSID, BSSID, signal level (RSSI), and frequency band for each match.
 *
 * On Android 10+ (API 29+) requires ACCESS_FINE_LOCATION permission (runtime grant).
 * Uses the deprecated WifiManager.startScan() + broadcast receiver pattern, which is
 * the simplest approach for a foreground-only scan. Throttled by the OS (max ~4 scans
 * per 2 minutes), so callers should not spam re-scan.
 *
 * Modelled on Myco's WifiScanner (Origami74/myco) but simplified for TollGate SSIDs.
 */
class WifiTollGateScanner(private val context: Context) {

    companion object {
        private const val TAG = "WifiTollGateScanner"
        const val SSID_PREFIX = "TollGate-"

        /**
         * Check if location permission is granted (required for WiFi scan results on API 29+).
         */
        fun hasLocationPermission(context: Context): Boolean {
            return ContextCompat.checkSelfPermission(
                context, Manifest.permission.ACCESS_FINE_LOCATION
            ) == PackageManager.PERMISSION_GRANTED
        }
    }

    private val wifiManager: WifiManager? =
        context.getSystemService(Context.WIFI_SERVICE) as? WifiManager

    private var scanReceiver: BroadcastReceiver? = null

    /**
     * A discovered TollGate WiFi network.
     */
    data class TollGateWifiNetwork(
        val ssid: String,
        val bssid: String,
        val rssi: Int,          // dBm, typically -30 to -100
        val frequencyMhz: Int,  // 2400-ish for 2.4GHz, 5000-ish for 5GHz
        val is5GHz: Boolean,
    ) {
        /** Coarse signal quality derived from RSSI. */
        val signalQuality: SignalQuality get() = when {
            rssi >= -55 -> SignalQuality.EXCELLENT
            rssi >= -67 -> SignalQuality.GOOD
            rssi >= -78 -> SignalQuality.FAIR
            else -> SignalQuality.WEAK
        }

        /**
         * Estimated gateway URL. TollGate routers run the gateway on port 2121.
         * We don't know the IP yet (that requires connecting), but we can show
         * the SSID as a candidate. Once connected, DhcpInfo gives the gateway IP.
         */
        val displayName: String get() = "$ssid (${signalQuality.name})"
    }

    enum class SignalQuality { EXCELLENT, GOOD, FAIR, WEAK }

    /**
     * Start a WiFi scan. Results delivered via [onResults] callback.
     * Returns false if WiFi is disabled or permission missing.
     */
    fun startScan(onResults: (List<TollGateWifiNetwork>) -> Unit): Boolean {
        if (wifiManager == null) {
            Log.e(TAG, "WifiManager unavailable")
            return false
        }

        if (!wifiManager.isWifiEnabled) {
            Log.w(TAG, "WiFi is disabled")
            return false
        }

        if (!hasLocationPermission(context)) {
            Log.w(TAG, "ACCESS_FINE_LOCATION not granted — scan results will be empty on API 29+")
        }

        // Remove any existing receiver
        scanReceiver?.let { 
            try { context.unregisterReceiver(it) } catch (_: Exception) {}
        }

        scanReceiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context, intent: Intent) {
                val results = wifiManager.scanResults ?: emptyList()
                val tollGateNetworks = filterTollGate(results)
                Log.i(TAG, "WiFi scan complete: ${results.size} networks found, ${tollGateNetworks.size} TollGate")
                onResults(tollGateNetworks)

                // Auto-unregister after delivering results
                try { context.unregisterReceiver(this) } catch (_: Exception) {}
                scanReceiver = null
            }
        }

        val filter = IntentFilter(WifiManager.SCAN_RESULTS_AVAILABLE_ACTION)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            context.registerReceiver(scanReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            context.registerReceiver(scanReceiver, filter)
        }

        val success = wifiManager.startScan()
        if (!success) {
            Log.w(TAG, "startScan() returned false — may be throttled by OS")
        }
        return success
    }

    /**
     * Get cached scan results (without triggering a new scan).
     * Useful for instant display before the user taps Scan.
     */
    fun getCachedResults(): List<TollGateWifiNetwork> {
        if (wifiManager == null) return emptyList()
        return filterTollGate(wifiManager.scanResults ?: emptyList())
    }

    /**
     * Get the DHCP gateway IP of the currently connected WiFi network.
     * Returns "http://X.X.X.X:4747" or null if not connected.
     */
    fun getConnectedGatewayUrl(): String? {
        if (wifiManager == null) return null
        val dhcp = wifiManager.dhcpInfo ?: return null
        if (dhcp.gateway == 0) return null
        val ip = intToIp(dhcp.gateway)
        Log.d(TAG, "DHCP gateway: $ip")
        return "http://$ip:2121"
    }

    /**
     * Get the SSID of the currently connected WiFi network (without quotes).
     */
    fun getConnectedSsid(): String? {
        if (wifiManager == null) return null
        val info = wifiManager.connectionInfo ?: return null
        val ssid = info.ssid ?: return null
        // Android wraps SSID in quotes: "\"TollGate-D139\""
        return ssid.removePrefix("\"").removeSuffix("\"").ifBlank { null }
    }

    fun stopScan() {
        scanReceiver?.let {
            try { context.unregisterReceiver(it) } catch (_: Exception) {}
            scanReceiver = null
        }
    }

    private fun filterTollGate(results: List<ScanResult>): List<TollGateWifiNetwork> {
        return results
            .filter { result ->
                val ssid = result.SSID?.removePrefix("\"")?.removeSuffix("\"") ?: ""
                ssid.startsWith(SSID_PREFIX, ignoreCase = true)
            }
            .map { result ->
                val freq = result.frequency
                TollGateWifiNetwork(
                    ssid = result.SSID.removePrefix("\"").removeSuffix("\""),
                    bssid = result.BSSID ?: "",
                    rssi = result.level,
                    frequencyMhz = freq,
                    is5GHz = freq > 4000,
                )
            }
            .sortedByDescending { it.rssi } // strongest first
    }

    private fun intToIp(ip: Int): String {
        return "${ip and 0xFF}.${(ip shr 8) and 0xFF}.${(ip shr 16) and 0xFF}.${(ip shr 24) and 0xFF}"
    }
}

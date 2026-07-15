package org.opentollgate.android

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme
import androidx.core.content.ContextCompat

class MainActivity : ComponentActivity() {

    companion object {
        private const val TAG = "MainActivity"
    }

    private lateinit var vm: TollgateViewModel

    /**
     * Runtime permission launcher. Requests all permissions needed for WiFi
     * scanning + network access in one shot. Called from [onCreate] after
     * the UI is set up so the dialog appears immediately.
     *
     * - ACCESS_FINE_LOCATION: required for WiFi scan results on API 29+
     * - NEARBY_WIFI_DEVICES: required for WiFi scan on API 33+ (replaces location)
     */
    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { results ->
        val locationGranted = results[Manifest.permission.ACCESS_FINE_LOCATION] ?: false
        val nearbyGranted = results[Manifest.permission.NEARBY_WIFI_DEVICES] ?: false
        Log.i(TAG, "Permission results: fineLocation=$locationGranted nearbyWifi=$nearbyGranted")
        // Trigger a fresh discover scan now that permissions are granted
        if (locationGranted || nearbyGranted) {
            vm.onDiscover()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        vm = viewModels<TollgateViewModel>().value
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface {
                    TollGateApp(vm)
                }
            }
        }

        // Request runtime permissions needed for WiFi discovery.
        // Without these, WifiTollGateScanner returns empty results on Android 10+.
        requestRuntimePermissions()
    }

    private fun requestRuntimePermissions() {
        val needed = mutableListOf<String>()

        // ACCESS_FINE_LOCATION — needed for WiFi scan on API 29+
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION)
            != PackageManager.PERMISSION_GRANTED
        ) {
            needed.add(Manifest.permission.ACCESS_FINE_LOCATION)
        }

        // NEARBY_WIFI_DEVICES — needed for WiFi scan on API 33+ (Tiramisu)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.NEARBY_WIFI_DEVICES)
                != PackageManager.PERMISSION_GRANTED
            ) {
                needed.add(Manifest.permission.NEARBY_WIFI_DEVICES)
            }
        }

        if (needed.isNotEmpty()) {
            Log.i(TAG, "Requesting runtime permissions: $needed")
            permissionLauncher.launch(needed.toTypedArray())
        } else {
            Log.d(TAG, "All runtime permissions already granted")
        }
    }
}

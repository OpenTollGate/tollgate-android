package org.opentollgate.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.darkColorScheme

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val vm by viewModels<TollgateViewModel>()
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface {
                    // Minimal wallet-only UI: mint, swap, display tokens.
                    // No tollgate scanner, no WiFi, no gateway detection, no VPN.
                    WalletOnlyScreen(node = vm.node)
                }
            }
        }
    }
}
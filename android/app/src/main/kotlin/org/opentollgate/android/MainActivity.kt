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
                    // Full TollGate app: Discover, Pay, Wallet, Status, Settings.
                    // Auto-mints ecash from all known testnut mints on startup.
                    TollGateApp(vm)
                }
            }
        }
    }
}
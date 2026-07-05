package org.opentollgate.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val vm by viewModels<TollgateViewModel>()
        setContent {
            // TollGateApp owns the NavHost (Pay + Status), bottom nav, and the
            // dark MaterialTheme wrapper. It reads vm.state itself.
            TollGateApp(vm)
        }
    }
}

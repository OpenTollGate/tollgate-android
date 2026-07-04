package org.opentollgate.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val vm by viewModels<TollgateViewModel>()
        setContent {
            val state by vm.state.collectAsState()
            StatusScreen(state = state, onDetect = vm::onDetect, onPay = vm::onPay)
        }
    }
}

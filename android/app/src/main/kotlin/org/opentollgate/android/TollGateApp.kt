package org.opentollgate.android

import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountBalanceWallet
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Radar
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController

/**
 * Top-level TollGate scaffold: a five-destination [NavHost] (Discover + Pay +
 * Wallet + Status + Settings) with a Material3 bottom navigation bar, wrapped in
 * a dark [MaterialTheme] (the captive-portal SPA is dark-themed). This is the
 * single root composable [MainActivity] renders.
 *
 * Discover is the start destination — it scans for nearby TollGate peers and,
 * on Connect, adopts the peer as the active gateway
 * ([TollgateViewModel.onSelectPeer]) and navigates to Pay. The master plan's
 * remaining screens (Pair) slot in here as additional [composable] destinations
 * in later Phase 1 tasks; for now Discover (find a peer), Pay (the
 * bootstrap-token flow), Wallet (balance / mints / token history), Status (live
 * session telemetry) and Settings (identity / FIPS / about) cover the full
 * client loop.
 */
@Composable
fun TollGateApp(vm: TollgateViewModel) {
    val state by vm.state.collectAsState()
    val nav = rememberNavController()
    val destinations = listOf(
        TopDestination("discover", "Discover", Icons.Default.Radar),
        TopDestination("pay", "Pay", Icons.Default.Send),
        TopDestination("wallet", "Wallet", Icons.Default.AccountBalanceWallet),
        TopDestination("status", "Status", Icons.Default.Home),
        TopDestination("settings", "Settings", Icons.Default.Settings),
    )

    MaterialTheme(colorScheme = DarkColors) {
        Scaffold(
            bottomBar = {
                NavigationBar {
                    val backStack by nav.currentBackStackEntryAsState()
                    val currentRoute = backStack?.destination?.route
                    destinations.forEach { d ->
                        NavigationBarItem(
                            selected = currentRoute == d.route,
                            onClick = {
                                nav.navigate(d.route) {
                                    // Pop the start dest up to its saved state, avoid
                                    // recomposing a fresh instance when re-selecting.
                                    popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(d.icon, contentDescription = d.label) },
                            label = { Text(d.label) },
                        )
                    }
                }
            },
        ) { pad ->
            NavHost(
                navController = nav,
                startDestination = "discover",
                modifier = Modifier.padding(pad),
            ) {
                composable("discover") {
                    DiscoverScreen(
                        state = state,
                        onScan = vm::onDiscover,
                        onStopScan = vm::onStopDiscover,
                        // Lambda (not a bare ::onSelectPeer reference): selecting a
                        // peer both updates the active gateway AND navigates to Pay.
                        onConnect = { peer ->
                            vm.onSelectPeer(peer)
                            nav.navigate("pay") {
                                popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        onAddCandidate = vm::onAddCandidate,
                    )
                }
                composable("pay") {
                    PayScreen(
                        state = state,
                        onHostChange = vm::onHostChange,
                        onDetect = vm::onDetect,
                        onSelectMint = vm::onSelectMint,
                        onAddMint = vm::onAddMint,
                        onAmountChange = vm::onAmountChange,
                        onTokenChange = vm::onTokenChange,
                        onPay = { vm.onPay() },
                    )
                }
                composable("wallet") {
                    WalletScreen(
                        state = state,
                        onReceiveToken = vm::onReceiveToken,
                        onSend = vm::onSend,
                        onDismissLastSent = vm::onDismissLastSent,
                    )
                }
                composable("status") {
                    StatusScreen(state = state, onStop = vm::onStop)
                }
                composable("settings") {
                    SettingsScreen(state = state)
                }
            }
        }
    }
}

private data class TopDestination(val route: String, val label: String, val icon: ImageVector)

/** TollGate dark palette. Kept minimal — Phase 5 (polish) will brand it. */
private val DarkColors = darkColorScheme()

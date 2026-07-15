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
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
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
import org.opentollgate.android.ui.TollGateTheme
import org.opentollgate.android.ui.TollGateBackground

/**
 * Top-level TollGate scaffold: a five-destination [NavHost] (Discover + Pay +
 * Wallet + Status + Settings) with a Material3 bottom navigation bar, wrapped in
 * the branded TollGate dark theme (amber CTA, deep navy gradient background,
 * Quicksand/Fredoka/Inter typography — matching tollgate.me and the
 * captive-portal SPA). This is the single root composable [MainActivity] renders.
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

    TollGateTheme {
        Scaffold(
            containerColor = TollGateBackground,
            bottomBar = {
                NavigationBar(
                    containerColor = TollGateBackground,
                ) {
                    val backStack by nav.currentBackStackEntryAsState()
                    val currentRoute = backStack?.destination?.route
                    destinations.forEach { d ->
                        NavigationBarItem(
                            selected = currentRoute == d.route,
                            onClick = {
                                nav.navigate(d.route) {
                                    popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(d.icon, contentDescription = d.label) },
                            label = { Text(d.label) },
                            colors = NavigationBarItemDefaults.colors(
                                selectedIconColor = MaterialTheme.colorScheme.primary,
                                selectedTextColor = MaterialTheme.colorScheme.primary,
                                unselectedIconColor = MaterialTheme.colorScheme.onSurfaceVariant,
                                unselectedTextColor = MaterialTheme.colorScheme.onSurfaceVariant,
                                indicatorColor = MaterialTheme.colorScheme.surfaceVariant,
                            ),
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
                        onConnect = { peer ->
                            vm.onSelectPeer(peer)
                            nav.navigate("pay") {
                                popUpTo(nav.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        onConnectWifi = { ssid ->
                            vm.onConnectToWifi(ssid)
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
                        onStop = vm::onStop,
                        onRequestInvoice = vm::onRequestInvoice,
                        onCancelMinting = vm::onCancelMinting,
                    )
                }
                composable("wallet") {
                    WalletScreen(
                        state = state,
                        node = vm.node,
                        onReceiveToken = vm::onReceiveToken,
                        onSend = vm::onSend,
                        onDismissLastSent = vm::onDismissLastSent,
                        onMintFrom = vm::onMintFrom,
                        onSwapToken = vm::onSwapToken,
                        onReceiveIntoWallet = vm::onReceiveIntoWallet,
                        onTopupAll = vm::onTopupAll,
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

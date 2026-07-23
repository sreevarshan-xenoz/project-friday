package com.friday.console.ui

import androidx.compose.foundation.layout.padding
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import com.friday.console.network.ConnectionStatus
import com.friday.console.ui.screens.*
import com.friday.console.ui.viewmodel.MainViewModel

private val BOTTOM_ROUTES = listOf(
    "home" to "Dashboard",
    "tasks" to "Tasks",
    "terminal" to "Terminal",
    "files" to "Files",
    "settings" to "Settings",
)

@Composable
fun FridayApp(vm: MainViewModel, nav: NavHostController) {
    val status by vm.status.collectAsState()

    if (status !is ConnectionStatus.Connected) {
        PairingScreen(vm)
        return
    }

    Scaffold(
        bottomBar = {
            NavigationBar {
                val current by nav.currentBackStackEntryAsState()
                BOTTOM_ROUTES.forEach { (route, label) ->
                    NavigationBarItem(
                        selected = current?.destination?.route == route,
                        onClick = {
                            nav.navigate(route) {
                                launchSingleTop = true
                                popUpTo("home")
                            }
                        },
                        label = { Text(label) },
                        icon = { },
                    )
                }
            }
        },
    ) { padding ->
        NavHost(nav, startDestination = "home", Modifier.padding(padding)) {
            composable("home") { DashboardScreen(vm) }
            composable("tasks") { TasksScreen(vm, nav) }
            composable("terminal") { TerminalScreen(vm) }
            composable("files") { FilesScreen() }
            composable("settings") { SettingsScreen(vm) }
            composable("task/{id}") { back ->
                TaskDetailScreen(vm, back.arguments?.getString("id") ?: "")
            }
        }
    }
}

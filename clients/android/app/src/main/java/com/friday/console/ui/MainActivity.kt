package com.friday.console.ui

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.lifecycle.lifecycleScope
import androidx.navigation.compose.rememberNavController
import com.friday.console.data.AppContainer
import com.friday.console.data.SessionRepository
import com.friday.console.ui.theme.FridayTheme
import com.friday.console.ui.viewmodel.MainViewModel
import com.friday.console.util.Debug
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        Debug.init(application)

        val repo = SessionRepository(applicationContext)
        val vm = MainViewModel.Factory(application, repo).create(MainViewModel::class.java)

        setContent {
            FridayTheme {
                Surface(color = MaterialTheme.colorScheme.background) {
                    val nav = rememberNavController()
                    FridayApp(vm, nav)
                }
            }
        }

        // Reconnect automatically if this device was already paired.
        lifecycleScope.launch {
            try {
                Debug.log("LIFE", "launch start")
                val t = repo.token.first()
                Debug.log("LIFE", "token.first=$t")
                t?.let { AppContainer.deviceToken = it }
                Debug.log("LIFE", "calling vm.connect()")
                vm.connect()
                Debug.log("LIFE", "vm.connect() returned")
            } catch (e: Exception) {
                Debug.log("LIFE", "EXCEPTION ${e::class.simpleName}: ${e.message}")
            }
        }
    }
}

package com.friday.console.ui

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.friday.console.ui.theme.FridayTheme

/**
 * Calm lock-screen companion (M6.2b): wake the phone and immediately see the last
 * task without opening the app. Shown over the keyguard via `showOnLockScreen`.
 */
class LockScreenActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        }
        setContent {
            FridayTheme {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    Column(
                        Modifier.fillMaxSize().padding(24.dp),
                        verticalArrangement = Arrangement.Center,
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Text("FRIDAY", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.primary)
                        Text("PC — Online", style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(20.dp))
                        Text("Last Task", style = MaterialTheme.typography.labelMedium)
                        Text("Cargo Test", style = MaterialTheme.typography.bodyLarge)
                        Text("✓ Passed · 2m ago", color = MaterialTheme.colorScheme.primary)
                        Spacer(Modifier.height(28.dp))
                        Button(onClick = { finish() }, modifier = Modifier.fillMaxWidth(0.6f)) { Text("Unlock") }
                    }
                }
            }
        }
    }
}

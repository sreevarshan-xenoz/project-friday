package com.friday.console.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.friday.console.network.ConnectionStatus
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun PairingScreen(vm: MainViewModel) {
    val status by vm.status.collectAsState()
    val challenge by vm.challenge.collectAsState()
    val serverUrl by vm.serverUrl.collectAsState()
    val feedback by vm.feedback.collectAsState()
    var urlInput by remember { mutableStateOf(serverUrl) }

    Column(
        Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("FRIDAY", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.primary)
        Text("Pair this device", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(28.dp))

        OutlinedTextField(
            value = urlInput,
            onValueChange = { urlInput = it },
            label = { Text("PC WebSocket URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(12.dp))
        Button(onClick = { vm.setServerUrl(urlInput); vm.connect() }, Modifier.fillMaxWidth()) {
            Text("Connect")
        }
        Spacer(Modifier.height(20.dp))

        when {
            challenge != null -> {
                Text("Pairing code", style = MaterialTheme.typography.labelMedium)
                Text(
                    challenge!!.code,
                    style = MaterialTheme.typography.headlineLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
                Spacer(Modifier.height(12.dp))
                Button(onClick = { vm.pair() }, Modifier.fillMaxWidth()) { Text("Pair") }
            }
            status is ConnectionStatus.Connecting -> Text("Connecting…")
            status is ConnectionStatus.Authenticating -> Text("Authenticating…")
            status is ConnectionStatus.Reconnecting -> Text("Reconnecting…")
            status is ConnectionStatus.Degraded -> Text("Connection degraded — retrying")
            else -> Text("Not connected", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f))
        }

        feedback?.let { fb ->
            if (!fb.ok) {
                Spacer(Modifier.height(12.dp))
                Text(fb.message, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

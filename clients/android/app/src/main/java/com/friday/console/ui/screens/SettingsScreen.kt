package com.friday.console.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun SettingsScreen(vm: MainViewModel) {
    val serverUrl by vm.serverUrl.collectAsState()
    var urlInput by remember { mutableStateOf(serverUrl) }
    val dashboard by vm.dashboard.collectAsState()
    val protocol by vm.protocol.collectAsState()

    Column(Modifier.fillMaxSize().padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text("Settings", style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.primary)
        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)

        OutlinedTextField(
            value = urlInput,
            onValueChange = { urlInput = it },
            label = { Text("PC WebSocket URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        Button(onClick = { vm.setServerUrl(urlInput) }, Modifier.fillMaxWidth()) { Text("Save URL") }

        Text("Device: ${dashboard.pcText}", style = MaterialTheme.typography.bodyMedium)
        Text("Mobile Protocol: $protocol", style = MaterialTheme.typography.bodyMedium)
        Text("App version: 0.1.0", style = MaterialTheme.typography.bodyMedium)

        OutlinedButton(onClick = { vm.disconnect() }, Modifier.fillMaxWidth()) { Text("Disconnect") }
        OutlinedButton(
            onClick = { /* M6.2b.2: forget token -> repo.clearToken + reconnect to pairing */ },
            Modifier.fillMaxWidth(),
        ) { Text("Forget this device") }
    }
}

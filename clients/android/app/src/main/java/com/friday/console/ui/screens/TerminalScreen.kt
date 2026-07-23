package com.friday.console.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun TerminalScreen(vm: MainViewModel) {
    val feedback by vm.feedback.collectAsState()
    val log by vm.log.collectAsState()
    var input by remember { mutableStateOf("") }

    Column(Modifier.fillMaxSize().padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Command Console", style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.primary)
        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                placeholder = { Text("> Run cargo test") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            Button(onClick = { vm.sendGoal(input); input = "" }) { Text("Send") }
        }

        feedback?.let { fb ->
            val latency = if (fb.latencyMs > 0) " · %.1f s".format(fb.latencyMs / 1000.0) else ""
            Text(
                if (fb.ok) "✓ ${fb.id}$latency" else "✕ ${fb.message}",
                color = if (fb.ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
            )
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)
        Text("Event stream", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
        LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            items(log.take(50)) { e ->
                Column(Modifier.fillMaxWidth()) {
                    Text("${e.kind} · ${e.topic}", style = MaterialTheme.typography.labelSmall)
                    Text(e.summary, style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
    }
}

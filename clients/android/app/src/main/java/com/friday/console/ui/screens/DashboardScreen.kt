package com.friday.console.ui.screens

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.friday.console.domain.StatusLevel
import com.friday.console.ui.components.MetricBar
import com.friday.console.ui.components.SectionCard
import com.friday.console.ui.components.StatusPill
import com.friday.console.ui.components.statusColor
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun DashboardScreen(vm: MainViewModel) {
    val d by vm.dashboard.collectAsState()
    val context = LocalContext.current
    var goal by remember { mutableStateOf("") }

    Column(
        Modifier.fillMaxSize().padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text(
            "FRIDAY",
            style = MaterialTheme.typography.headlineSmall,
            color = MaterialTheme.colorScheme.primary,
        )
        Text("${d.greeting}, Sree", style = MaterialTheme.typography.titleMedium)
        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)

        SectionCard("PC") {
            StatusPill(d.pc, d.pcText)
        }

        SectionCard("Planner") {
            StatusPill(d.planner, d.plannerLabel)
        }

        SectionCard("Current Goal") {
            Text(d.lastGoal, style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.height(8.dp))
            Text("Running: ${d.runningCount}", style = MaterialTheme.typography.bodyMedium)
            if (d.lastLatencyMs > 0) {
                Spacer(Modifier.height(8.dp))
                Text(
                    "Latency: %.1f s".format(d.lastLatencyMs / 1000.0),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }

        SectionCard("Resources") {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                val cpuFrac = if (d.cpuPercent.isFinite()) d.cpuPercent / 100f else 0f
                MetricBar("CPU", "%.0f%%".format(d.cpuPercent), cpuFrac)
                val ramFrac = if (d.memoryTotalMb > 0) d.memoryUsedMb.toFloat() / d.memoryTotalMb else 0f
                MetricBar(
                    "RAM",
                    "${d.memoryUsedMb} / ${d.memoryTotalMb} MB",
                    ramFrac,
                )
                val diskFrac = if (d.diskTotalMb > 0) d.diskUsedMb.toFloat() / d.diskTotalMb else 0f
                MetricBar(
                    "Disk",
                    "${d.diskUsedMb} / ${d.diskTotalMb} MB",
                    diskFrac,
                )
            }
        }

        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(
                value = goal,
                onValueChange = { goal = it },
                placeholder = { Text("> command or goal") },
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            Button(onClick = { vm.sendGoal(goal); goal = "" }) { Text("Send") }
        }

        OutlinedButton(
            onClick = {
                Toast.makeText(context, "Voice remote arrives in M6.3", Toast.LENGTH_SHORT).show()
            },
            modifier = Modifier.fillMaxWidth(),
        ) { Text("Speak") }
    }
}

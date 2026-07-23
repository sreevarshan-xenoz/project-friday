package com.friday.console.ui.screens

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.friday.console.domain.TaskStatus
import com.friday.console.ui.components.SectionCard
import com.friday.console.ui.components.taskColor
import com.friday.console.ui.components.taskGlyph
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun TaskDetailScreen(vm: MainViewModel, taskId: String) {
    val tasks by vm.tasks.collectAsState()
    val task = tasks.firstOrNull { it.id == taskId }

    Column(
        Modifier.fillMaxSize().padding(20.dp).verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Task", style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.primary)
        if (task == null) {
            Text("Task not found.", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f))
            return@Column
        }
        SectionCard(title = task.title) {
            Text("Status: ${taskGlyph(task.status)} ${task.status.name}", color = taskColor(task.status))
            Spacer(Modifier.height(6.dp))
            Text("Source: ${task.source.ifBlank { "friday" }}", style = MaterialTheme.typography.bodyMedium)
        }

        // Structured execution timeline (M6.2b.4), ordered chronologically as a
        // user thinks: Goal -> Planner -> Execution -> Recovery -> Evidence -> Logs.
        // Evidence outranks raw Logs because it is the more decision-relevant view.
        val phases = listOf("Goal", "Planner", "Execution", "Recovery", "Evidence", "Logs")
        phases.forEach { phase ->
            SectionCard(phase) {
                val note = when (phase) {
                    "Goal" -> task.title
                    "Planner" -> "Goal parsed by planner"
                    "Execution" -> if (task.status == TaskStatus.Running) "In progress…" else "Completed step stream"
                    "Recovery" -> if (task.status == TaskStatus.Failed) "Awaiting retry/ignore" else "No recovery needed"
                    "Evidence" -> "Outcome: ${task.status.name} (source: ${task.source.ifBlank { "friday" }})"
                    "Logs" -> "Event kind: ${task.status.name.lowercase()}"
                    else -> "—"
                }
                Text(note, style = MaterialTheme.typography.bodyMedium)
            }
        }

        if (task.status == TaskStatus.Running) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedButton(onClick = { /* M6.2c: task.pause */ }) { Text("Pause") }
                OutlinedButton(onClick = { /* M6.2c: task.cancel */ }) { Text("Cancel") }
            }
        }
        if (task.status == TaskStatus.Failed) {
            Button(onClick = { /* M6.2e: task.approve */ }) { Text("Approve / Retry") }
        }
    }
}

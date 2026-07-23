package com.friday.console.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.friday.console.ui.components.SectionCard
import com.friday.console.ui.components.taskColor
import com.friday.console.ui.components.taskGlyph
import com.friday.console.ui.viewmodel.MainViewModel

@Composable
fun TasksScreen(vm: MainViewModel, nav: NavController) {
    val tasks by vm.tasks.collectAsState()

    Column(Modifier.fillMaxSize().padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text("Running Tasks", style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.primary)
        HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)
        if (tasks.isEmpty()) {
            Text("No activity yet.", color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.6f))
        } else {
            LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                items(tasks) { t ->
                    SectionCard(title = t.title) {
                        Row(
                            Modifier.fillMaxWidth().clickable { nav.navigate("task/${t.id}") },
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(t.source.ifBlank { "friday" }, style = MaterialTheme.typography.labelMedium)
                            Text(
                                "${taskGlyph(t.status)} ${t.status.name}",
                                color = taskColor(t.status),
                                style = MaterialTheme.typography.labelLarge,
                            )
                        }
                    }
                }
            }
        }
    }
}

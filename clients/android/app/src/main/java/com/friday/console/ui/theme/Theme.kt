package com.friday.console.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val FridayDark = darkColorScheme(
    primary = Color(0xFF5BC8FF),
    secondary = Color(0xFF9D7BFF),
    background = Color(0xFF0B132B),
    surface = Color(0xFF16203B),
    surfaceVariant = Color(0xFF1F2B47),
    onBackground = Color(0xFFE6EDF6),
    onSurface = Color(0xFFE6EDF6),
    onPrimary = Color(0xFF06101F),
)

@Composable
fun FridayTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = FridayDark, content = content)
}

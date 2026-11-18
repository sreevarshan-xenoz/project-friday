package com.friday.console.domain

/** Color-coded health level used across the dashboard and task rows. */
enum class StatusLevel(val label: String) {
    Green("OK"),
    Amber("Warn"),
    Red("Down"),
    Standby("Standby"),
    Unknown("—"),
}

data class DashboardState(
    val connected: Boolean = false,
    val authenticated: Boolean = false,
    val pc: StatusLevel = StatusLevel.Unknown,
    val pcText: String = "Offline",
    val planner: StatusLevel = StatusLevel.Standby,
    val plannerLabel: String = "—",
    val inference: StatusLevel = StatusLevel.Standby,
    val windows: StatusLevel = StatusLevel.Standby,
    val recovery: StatusLevel = StatusLevel.Standby,
    val cpuPercent: Float = 0f,
    val memoryUsedMb: Long = 0,
    val memoryTotalMb: Long = 0,
    val diskUsedMb: Long = 0,
    val diskTotalMb: Long = 0,
    val lastGoal: String = "—",
    val runningCount: Int = 0,
    val lastLatencyMs: Long = 0,
    val greeting: String = "",
)

enum class TaskStatus { Done, Running, Waiting, Failed }

data class TaskItem(
    val id: String,
    val title: String,
    val status: TaskStatus,
    val source: String = "",
    val detail: String = "",
    val timestamp: Long = 0L,
)

data class LogEntry(
    val id: String,
    val topic: String,
    val kind: String,
    val summary: String,
    val timestamp: Long = 0L,
)

data class CommandFeedback(
    val id: String,
    val ok: Boolean,
    val message: String = "",
    val latencyMs: Long = 0,
)

/** A live pairing challenge from the server. The pairing code is not in it: the user reads it off the PC and types it. */
data class PairingChallenge(
    val challenge: String,
)

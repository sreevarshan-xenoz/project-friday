package com.friday.console.ui.viewmodel

import android.app.Application
import android.os.Build
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.friday.console.data.AppContainer
import com.friday.console.data.SessionRepository
import com.friday.console.domain.goalParams
import com.friday.console.domain.PairingChallenge
import com.friday.console.domain.TaskItem
import com.friday.console.domain.TaskStatus
import com.friday.console.domain.parseEvent
import com.friday.console.domain.signPairing
import com.friday.console.domain.greetingNow
import com.friday.console.network.ConnectionStatus
import com.friday.console.network.FridayWebSocket
import com.friday.console.notifications.NotificationHelper
import com.friday.console.protocol.ClientInfo
import com.friday.console.util.Debug
import com.friday.console.protocol.ClientMessage
import com.friday.console.protocol.Method
import com.friday.console.protocol.ServerMessage
import com.friday.console.protocol.TelemetrySample
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import java.util.UUID
import android.util.Log

class MainViewModel(
    application: Application,
    private val repo: SessionRepository,
) : AndroidViewModel(application), FridayWebSocket.Listener {

    private val _status = MutableStateFlow<ConnectionStatus>(ConnectionStatus.Disconnected)
    val status: StateFlow<ConnectionStatus> = _status.asStateFlow()

    private val _dashboard = MutableStateFlow(
        com.friday.console.domain.DashboardState(greeting = greetingNow()),
    )
    val dashboard: StateFlow<com.friday.console.domain.DashboardState> = _dashboard.asStateFlow()

    private val _tasks = MutableStateFlow<List<TaskItem>>(emptyList())
    val tasks: StateFlow<List<TaskItem>> = _tasks.asStateFlow()

    private val _log = MutableStateFlow<List<com.friday.console.domain.LogEntry>>(emptyList())
    val log: StateFlow<List<com.friday.console.domain.LogEntry>> = _log.asStateFlow()

    private val _feedback = MutableStateFlow<com.friday.console.domain.CommandFeedback?>(null)
    val feedback: StateFlow<com.friday.console.domain.CommandFeedback?> = _feedback.asStateFlow()

    private val _challenge = MutableStateFlow<PairingChallenge?>(null)
    val challenge: StateFlow<PairingChallenge?> = _challenge.asStateFlow()

    private val _serverUrl = MutableStateFlow(AppContainer.serverUrl)
    val serverUrl: StateFlow<String> = _serverUrl.asStateFlow()

    private val _protocol = MutableStateFlow("1.0")
    val protocol: StateFlow<String> = _protocol.asStateFlow()

    private var socket: FridayWebSocket? = null
    private var hadToken = false
    private var manuallyClosed = false
    private var reconnectAttempts = 0
    private val maxReconnect = 3
    private val commandStart = mutableMapOf<String, Long>()

    private val clientInfo = ClientInfo(
        device = "${Build.MANUFACTURER} ${Build.MODEL}",
        model = Build.MODEL,
        app_version = "0.1.0",
    )

    init {
        AppContainer.commandSink = { method, params -> sendCommand(method, params) }
        viewModelScope.launch {
            repo.token.collect { AppContainer.deviceToken = it }
        }
        viewModelScope.launch {
            repo.serverUrl.collect { url ->
                if (!url.isNullOrBlank()) {
                    AppContainer.serverUrl = url
                    _serverUrl.value = url
                }
            }
        }
    }

    fun connect() {
        Debug.log("CONN", "connect() reached url=${AppContainer.serverUrl} token=${AppContainer.deviceToken != null}")
        Log.i("FRIDAY", "connect() url=${AppContainer.serverUrl} token=${AppContainer.deviceToken != null}")
        manuallyClosed = false
        reconnectAttempts = 0
        hadToken = AppContainer.deviceToken != null
        socket = FridayWebSocket(AppContainer.serverUrl, clientInfo, this)
        AppContainer.webSocket = socket
        socket?.connect(AppContainer.deviceToken)
    }

    fun setServerUrl(url: String) {
        _serverUrl.value = url
        AppContainer.serverUrl = url
        viewModelScope.launch { repo.saveServerUrl(url) }
    }

    fun pair() {
        val c = _challenge.value ?: return
        val signature = signPairing(c.challenge, c.code)
        socket?.send(ClientMessage.AuthResponse(code = c.code, signature = signature))
    }

    fun sendGoal(goal: String) {
        if (goal.isBlank()) return
        _dashboard.update { it.copy(lastGoal = goal) }
        sendCommand(Method.PLANNER_GOAL, goalParams(goal))
    }

    fun sendCommand(method: String, params: JsonElement) {
        val id = UUID.randomUUID().toString()
        commandStart[id] = System.currentTimeMillis()
        socket?.send(ClientMessage.Command(id = id, method = method, params = params))
    }

    fun disconnect() {
        manuallyClosed = true
        socket?.close()
        socket = null
        _status.value = ConnectionStatus.Disconnected
        _dashboard.update { it.copy(connected = false, authenticated = false) }
    }

    fun clearError() {
        _feedback.value = null
    }

    // ---- FridayWebSocket.Listener ----

    override fun onStatus(status: ConnectionStatus) {
        Log.i("FRIDAY", "onStatus: $status")
        when (status) {
            is ConnectionStatus.Reconnecting -> reconnect()
            else -> _status.value = status
        }
        updatePcPill(status)
    }

    private fun updatePcPill(status: ConnectionStatus) {
        val (level, text) = when (status) {
            ConnectionStatus.Connecting -> com.friday.console.domain.StatusLevel.Standby to "Connecting"
            ConnectionStatus.Authenticating -> com.friday.console.domain.StatusLevel.Amber to "Authenticating"
            ConnectionStatus.Connected -> com.friday.console.domain.StatusLevel.Green to "Connected"
            ConnectionStatus.Degraded -> com.friday.console.domain.StatusLevel.Amber to "Degraded"
            ConnectionStatus.Reconnecting -> com.friday.console.domain.StatusLevel.Amber to "Reconnecting"
            ConnectionStatus.Disconnected -> com.friday.console.domain.StatusLevel.Unknown to "Offline"
        }
        _dashboard.update {
            it.copy(pc = level, pcText = text, connected = status == ConnectionStatus.Connected)
        }
    }

    private fun reconnect() {
        if (manuallyClosed) return
        if (reconnectAttempts >= maxReconnect) {
            _status.value = ConnectionStatus.Disconnected
            return
        }
        reconnectAttempts++
        _status.value = ConnectionStatus.Reconnecting
        viewModelScope.launch {
            kotlinx.coroutines.delay(2000)
            if (manuallyClosed) return@launch
            socket = FridayWebSocket(AppContainer.serverUrl, clientInfo, this@MainViewModel)
            AppContainer.webSocket = socket
            socket?.connect(AppContainer.deviceToken)
        }
    }

    override fun onServerMessage(msg: ServerMessage) {
        when (msg) {
            is ServerMessage.Welcome -> {
                _protocol.value = msg.protocol
                if (hadToken) markAuthenticated() else updatePcPill(ConnectionStatus.Authenticating)
            }
            is ServerMessage.AuthChallenge -> {
                _challenge.value = PairingChallenge(code = msg.pairing_code, challenge = msg.challenge)
            }
            is ServerMessage.AuthComplete -> {
                viewModelScope.launch { repo.saveToken(msg.token) }
                AppContainer.deviceToken = msg.token
                _challenge.value = null
                markAuthenticated()
            }
            is ServerMessage.Event -> handleEvent(msg.topic, msg.payload)
            is ServerMessage.Telemetry -> updateTelemetry(msg.sample)
            is ServerMessage.Health -> updateHealth(msg)
            is ServerMessage.CommandResult -> {
                val started = commandStart.remove(msg.id) ?: System.currentTimeMillis()
                val latency = System.currentTimeMillis() - started
                _feedback.value = com.friday.console.domain.CommandFeedback(
                    id = msg.id,
                    ok = msg.ok,
                    message = msg.result.toString(),
                    latencyMs = latency,
                )
                if (msg.ok) {
                    _dashboard.update { it.copy(lastLatencyMs = latency) }
                }
            }
            is ServerMessage.Pong -> Unit
            is ServerMessage.Error -> handleError(msg)
        }
    }

    private fun markAuthenticated() {
        reconnectAttempts = 0
        _status.value = ConnectionStatus.Connected
        _dashboard.update { it.copy(authenticated = true, greeting = greetingNow()) }
        socket?.send(ClientMessage.Subscribe(listOf("tasks", "plans", "notifications", "telemetry.system")))
    }

    private fun handleError(msg: ServerMessage.Error) {
        if (msg.code == "auth" || msg.message.contains("token", ignoreCase = true)) {
            // Stale/revoked token: drop it and restart pairing.
            viewModelScope.launch { repo.clearToken() }
            AppContainer.deviceToken = null
            _dashboard.update { it.copy(authenticated = false) }
            _status.value = ConnectionStatus.Reconnecting
            socket?.send(ClientMessage.Hello(clientInfo, null, "1.0"))
        } else {
            // Non-auth server error: the link is impaired but the socket is open.
            _status.value = ConnectionStatus.Degraded
        }
        _feedback.value = com.friday.console.domain.CommandFeedback("err", false, msg.message)
    }

    private fun handleEvent(topic: String, payload: JsonElement) {
        val parsed = parseEvent(topic, payload)
        val entry = com.friday.console.domain.LogEntry(
            id = UUID.randomUUID().toString(),
            topic = topic,
            kind = parsed.kind,
            summary = parsed.summary,
            timestamp = System.currentTimeMillis(),
        )
        _log.update { (listOf(entry) + it).take(200) }

        parsed.task?.let { t -> upsertTask(t) }
        updatePlannerFromEvent(parsed.kind)

        // M6.2.6: surface failures and explicit notifications on the phone.
        if (parsed.kind.contains("failed") || topic == "notifications") {
            NotificationHelper.showTaskFailed(getApplication(), parsed.task?.title ?: parsed.summary)
        }
        if (topic == "telemetry.system") { /* handled by Telemetry frame */ }
    }

    private fun upsertTask(t: TaskItem) {
        _tasks.update { list ->
            val without = list.filter { it.id != t.id }
            (without + t).sortedWith(
                compareBy<TaskItem> { it.status != TaskStatus.Running }
                    .thenByDescending { it.timestamp },
            )
        }
        _dashboard.update { d -> d.copy(runningCount = _tasks.value.count { it.status == TaskStatus.Running }) }
    }

    private fun updatePlannerFromEvent(kind: String) {
        val (level, label) = when {
            kind.contains("failed") || kind.contains("error") ->
                com.friday.console.domain.StatusLevel.Amber to "Attention"
            kind.contains("recovery") ->
                com.friday.console.domain.StatusLevel.Amber to "Recovering"
            kind.contains("started") || kind.contains("running") || kind.contains("exec") ->
                com.friday.console.domain.StatusLevel.Green to "Executing"
            kind.contains("goal") || kind.contains("plan") || kind.contains("intent") ->
                com.friday.console.domain.StatusLevel.Green to "Planning"
            kind.contains("completed") || kind.contains("succeeded") ->
                com.friday.console.domain.StatusLevel.Standby to "Idle"
            else -> return
        }
        _dashboard.update { it.copy(planner = level, plannerLabel = label) }
    }

    private fun updateTelemetry(s: TelemetrySample) {
        _dashboard.update {
            it.copy(
                cpuPercent = s.cpu_percent,
                memoryUsedMb = s.memory_used_mb,
                memoryTotalMb = s.memory_total_mb,
                diskUsedMb = s.disk_used_mb,
                diskTotalMb = s.disk_total_mb,
            )
        }
    }

    @Suppress("UNUSED_PARAMETER")
    private fun updateHealth(msg: ServerMessage.Health) {
        // Forward-compatible: the server can emit a full snapshot; we map the
        // well-known subsystems we render. No-op until the daemon emits Health.
    }

    class Factory(
        private val app: Application,
        private val repo: SessionRepository,
    ) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            MainViewModel(app, repo) as T
    }
}

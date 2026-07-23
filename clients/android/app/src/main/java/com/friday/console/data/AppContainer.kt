package com.friday.console.data

import com.friday.console.network.FridayWebSocket
import kotlinx.serialization.json.JsonElement

/**
 * Process-wide handles the app needs outside the Compose tree — notably the live
 * WebSocket (so a notification action receiver can reach it) and the command sink
 * the [com.friday.console.ui.viewmodel.MainViewModel] registers.
 *
 * This is intentionally tiny: it holds no FRIDAY logic, only transport plumbing.
 */
object AppContainer {
    var webSocket: FridayWebSocket? = null
    var serverUrl: String = "ws://127.0.0.1:18432"
    var deviceToken: String? = null

    /** Set by the ViewModel; lets notification action buttons emit protocol commands. */
    var commandSink: ((method: String, params: JsonElement) -> Unit)? = null
}

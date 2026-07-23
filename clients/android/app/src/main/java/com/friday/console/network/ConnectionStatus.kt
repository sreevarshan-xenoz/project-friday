package com.friday.console.network

/**
 * Explicit lifecycle of the WebSocket connection. The launcher must never guess:
 * every UI string is derived from one of these exact states.
 *
 * - Disconnected: no socket (initial, or after a clean/permanent close)
 * - Connecting: socket being established
 * - Authenticating: socket open, exchanging Hello / pairing challenge
 * - Connected: authenticated and in the relay phase
 * - Degraded: was Connected, now impaired (server error, stalled stream)
 * - Reconnecting: a failure occurred and the client is retrying
 */
sealed interface ConnectionStatus {
    data object Disconnected : ConnectionStatus
    data object Connecting : ConnectionStatus
    data object Authenticating : ConnectionStatus
    data object Connected : ConnectionStatus
    data object Degraded : ConnectionStatus
    data object Reconnecting : ConnectionStatus
}

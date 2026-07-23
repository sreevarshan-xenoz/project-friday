package com.friday.console.network

import com.friday.console.protocol.ClientInfo
import com.friday.console.protocol.ClientMessage
import com.friday.console.protocol.ServerMessage
import com.friday.console.util.Debug
import android.util.Log
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.concurrent.TimeUnit

/**
 * Thin WebSocket client for the FRIDAY Mobile Protocol. It knows nothing about
 * FRIDAY internals — it only speaks [ClientMessage] / [ServerMessage].
 *
 * Flow: connect() -> server sends Welcome -> we send Hello -> server replies with
 * AuthChallenge (if unpaired) or treats us as authenticated (if a valid token was
 * supplied). The [Listener] surfaces every server frame for the ViewModel to
 * reduce into UI state.
 */
class FridayWebSocket(
    private val url: String,
    private val clientInfo: ClientInfo,
    private val listener: Listener,
) {
    interface Listener {
        fun onStatus(status: ConnectionStatus)
        fun onServerMessage(msg: ServerMessage)
    }

    private val okHttp = OkHttpClient.Builder()
        .pingInterval(30, TimeUnit.SECONDS)
        .build()

    private var ws: WebSocket? = null

    fun connect(token: String?) {
        Debug.log("OPEN", "connect() token=${token != null} url=$url")
        listener.onStatus(ConnectionStatus.Connecting)
        val request = Request.Builder().url(url).build()
        ws = okHttp.newWebSocket(request, buildListener(token))
    }

    private fun buildListener(token: String?): WebSocketListener {
        return object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                Debug.log("OPEN", "WS OPEN -> sending Hello (token=${token != null})")
                Log.i("FRIDAY", "WS OPEN -> sending Hello (token=${token != null})")
                // Socket is up but we are still exchanging credentials/challenge.
                listener.onStatus(ConnectionStatus.Authenticating)
                send(ClientMessage.Hello(clientInfo, token, "1.0"))
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                Debug.log("RX", text)
                Log.i("FRIDAY", "WS RX: $text")
                try {
                    val msg = FridayJson.decodeFromString<ServerMessage>(text)
                    Debug.log("RX", "parsed=${msg::class.simpleName}")
                    Log.i("FRIDAY", "WS RX parsed: ${msg::class.simpleName}")
                    listener.onServerMessage(msg)
                } catch (e: Exception) {
                    Debug.log("RX", "decode FAILED: $text :: ${e.message}")
                    Log.e("FRIDAY", "WS RX decode FAILED: $text", e)
                }
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                Debug.log("FAIL", "${t.message}")
                Log.e("FRIDAY", "WS FAILURE", t)
                listener.onStatus(ConnectionStatus.Reconnecting)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                Debug.log("CLOSED", "$code $reason")
                listener.onStatus(ConnectionStatus.Disconnected)
            }
        }
    }

    fun send(msg: ClientMessage) {
        ws?.send(FridayJson.encodeToString(ClientMessage.serializer(), msg))
    }

    fun close() {
        ws?.close(1000, null)
        ws = null
    }
}

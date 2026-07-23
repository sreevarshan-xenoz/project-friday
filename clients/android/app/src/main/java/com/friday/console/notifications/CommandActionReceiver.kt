package com.friday.console.notifications

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.friday.console.data.AppContainer
import com.friday.console.protocol.Method
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Translates a notification action tap into a protocol command for FRIDAY.
 * The actual send goes through the ViewModel's registered [AppContainer.commandSink].
 */
class CommandActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        val kind = intent?.getStringExtra("kind") ?: return
        val task = intent.getStringExtra("task") ?: return
        val params = buildJsonObject { put("task_id", task) }
        val method = when (kind) {
            "retry" -> Method.TASK_APPROVE
            "ignore" -> Method.TASK_CANCEL
            else -> Method.TASK_CANCEL
        }
        AppContainer.commandSink?.invoke(method, params)
    }
}

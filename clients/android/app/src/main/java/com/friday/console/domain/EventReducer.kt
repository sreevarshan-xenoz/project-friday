package com.friday.console.domain

import com.friday.console.protocol.ServerMessage
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Reduces an inbound [ServerMessage.Event] into a structured [LogEntry] and an
 * optional [TaskItem]. The client never inspects FRIDAY internals — it only reads
 * the `_kind`/`_source` tags the bridge attaches and a few well-known payload
 * fields (`goal`, `task`, `name`, `message`).
 */
fun parseEvent(topic: String, payload: JsonElement): ParsedEvent {
    val obj = payload as? JsonObject ?: return ParsedEvent(topic, "", topic, null)
    val kind = obj["_kind"]?.jsonPrimitive?.content ?: topic
    val source = obj["_source"]?.jsonPrimitive?.content ?: ""
    val summary = summarize(kind, obj)
    val task = buildTask(kind, obj)
    return ParsedEvent(kind = kind, source = source, summary = summary, task = task)
}

data class ParsedEvent(
    val kind: String,
    val source: String,
    val summary: String,
    val task: TaskItem?,
)

private fun str(obj: JsonObject, key: String): String? =
    obj[key]?.jsonPrimitive?.content

private fun summarize(kind: String, obj: JsonObject): String =
    str(obj, "goal") ?: str(obj, "message") ?: str(obj, "task")
        ?: str(obj, "name") ?: kind

private fun buildTask(kind: String, obj: JsonObject): TaskItem? {
    val status = when {
        kind.endsWith(".completed") || kind.endsWith(".succeeded") -> TaskStatus.Done
        kind.endsWith(".failed") || kind.endsWith(".error") -> TaskStatus.Failed
        kind.endsWith(".started") || kind.endsWith(".running") -> TaskStatus.Running
        kind.endsWith(".created") || kind.endsWith(".planned") -> TaskStatus.Waiting
        else -> return null
    }
    val id = str(obj, "id") ?: str(obj, "task_id") ?: str(obj, "goal") ?: return null
    val title = str(obj, "goal") ?: str(obj, "task") ?: str(obj, "name") ?: kind
    return TaskItem(id = id, title = title, status = status, source = str(obj, "_source") ?: "")
}

/** Build a command `params` object for a natural-language goal. */
fun goalParams(goal: String): JsonElement = buildJsonObject { put("goal", goal) }

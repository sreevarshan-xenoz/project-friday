@file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)

package com.friday.console.protocol

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull

/** Identifying information a phone sends on connect. */
@Serializable
data class ClientInfo(
    val device: String,
    val model: String,
    val app_version: String,
)

/**
 * Messages sent *from* the phone *to* the mobile service.
 * Mirrors `services/mobile/src/protocol.rs` exactly (serde `rename_all =
 * "snake_case"`, tag = "type").
 */
@Serializable
@JsonClassDiscriminator("type")
sealed interface ClientMessage {
    @Serializable
    @SerialName("hello")
    data class Hello(
        val client: ClientInfo,
        val token: String? = null,
        @SerialName("protocol") val protocol: String = "1.0",
    ) : ClientMessage

    @Serializable
    @SerialName("auth_response")
    data class AuthResponse(
        val code: String,
        val signature: String,
    ) : ClientMessage

    @Serializable
    @SerialName("command")
    data class Command(
        val id: String,
        val method: String,
        val params: JsonElement = JsonNull,
    ) : ClientMessage

    @Serializable
    @SerialName("command_ack")
    data class CommandAck(
        val id: String,
        val ok: Boolean,
    ) : ClientMessage

    @Serializable
    @SerialName("subscribe")
    data class Subscribe(
        val topics: List<String>,
    ) : ClientMessage

    @Serializable
    @SerialName("ping")
    data object Ping : ClientMessage
}

/**
 * Messages sent *from* the mobile service *to* the phone.
 */
@Serializable
@JsonClassDiscriminator("type")
sealed interface ServerMessage {
    @Serializable
    @SerialName("welcome")
    data class Welcome(
        val session_id: String,
        val requires_auth: Boolean,
        @SerialName("protocol") val protocol: String = "1.0",
    ) : ServerMessage

    @Serializable
    @SerialName("auth_challenge")
    data class AuthChallenge(
        val challenge: String,
    ) : ServerMessage

    @Serializable
    @SerialName("auth_complete")
    data class AuthComplete(
        val token: String,
    ) : ServerMessage

    @Serializable
    @SerialName("event")
    data class Event(
        val topic: String,
        val payload: JsonElement,
    ) : ServerMessage

    @Serializable
    @SerialName("health")
    data class Health(
        val snapshot: SystemHealth,
    ) : ServerMessage

    @Serializable
    @SerialName("telemetry")
    data class Telemetry(
        val sample: TelemetrySample,
    ) : ServerMessage

    @Serializable
    @SerialName("command_result")
    data class CommandResult(
        val id: String,
        val ok: Boolean,
        val result: JsonElement,
    ) : ServerMessage

    @Serializable
    @SerialName("pong")
    data object Pong : ServerMessage

    @Serializable
    @SerialName("error")
    data class Error(
        val code: String,
        val message: String,
    ) : ServerMessage
}

/** A full health snapshot for the dashboard (forward-compatible; not yet emitted). */
@Serializable
data class SystemHealth(
    val subsystems: Map<String, ServiceStatus> = emptyMap(),
    val latency_ms: Long = 0,
)

/** Real host metrics from `SystemProbe` (sysinfo). */
@Serializable
data class TelemetrySample(
    val cpu_percent: Float = 0f,
    val memory_used_mb: Long = 0,
    val memory_total_mb: Long = 0,
    val disk_used_mb: Long = 0,
    val disk_total_mb: Long = 0,
)

/** Lifecycle state of a whole service (matches `ServiceState` on the server). */
@Serializable
enum class ServiceState {
    Created,
    Starting,
    Healthy,
    Busy,
    Recovering,
    Offline,
    Disabled,
}

/** Health of a service/capability (matches `Health` on the server). */
@Serializable
enum class HealthLevel {
    Healthy,
    Busy,
    Offline,
    Recovering,
    Disabled,
}

@Serializable
data class ServiceStatus(
    val state: ServiceState = ServiceState.Healthy,
    val since: String = "",
    val health: HealthLevel = HealthLevel.Healthy,
    val message: String = "",
)

/** Command method constants (mirror `services/mobile/src/commands.rs`). */
object Method {
    const val PLANNER_GOAL = "planner.goal"
    const val TASK_CANCEL = "task.cancel"
    const val TASK_PAUSE = "task.pause"
    const val TASK_APPROVE = "task.approve"
    const val SYSTEM_INFO = "system.info"
}

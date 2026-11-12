package com.mikerosoft.threadbear.kit

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.json.JsonElement

/**
 * Raw JSON in and out of Convex (ConvexTransport.swift). The real one wraps
 * the Convex Android client's FFI layer; tests use a fake. The phone token
 * travels as an argument, not as Convex auth.
 */
interface ConvexTransport {
    suspend fun query(name: String, args: Map<String, JsonElement>): String
    suspend fun mutation(name: String, args: Map<String, JsonElement>): String

    /** A live query: every new result, ending with a [TransportError] if the function throws. */
    fun subscribe(name: String, args: Map<String, JsonElement>): Flow<String>

    /** True while the WebSocket is connected. */
    val connected: StateFlow<Boolean>
}

sealed class TransportError(message: String) : Exception(message) {
    /** The phone token was refused: missing, wrong or expired. */
    class Unauthorized(message: String) : TransportError("Not paired: $message")

    /** The function threw. Retrying won't help. */
    class Server(message: String) : TransportError(message)

    /** Couldn't reach the deployment. */
    class Network(message: String) : TransportError("Network: $message")

    /** The thread or command isn't in the mirror. */
    class NotFound(message: String) : TransportError(message)

    companion object {
        /** phone.ts throws ConvexError({kind: "auth" | "notFound", message}). */
        fun fromConvexError(data: String): TransportError {
            val value = runCatching { parseJson(data).asObject() }.getOrNull()
            val message = value?.str("message") ?: data
            return when (value?.str("kind")) {
                "auth" -> Unauthorized(message)
                "notFound" -> NotFound(message)
                else -> Server(message)
            }
        }
    }
}

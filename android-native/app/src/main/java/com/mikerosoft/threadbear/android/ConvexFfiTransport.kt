package com.mikerosoft.threadbear.android

import com.mikerosoft.threadbear.kit.ConvexTransport
import com.mikerosoft.threadbear.kit.TransportError
import com.mikerosoft.threadbear.kit.jsonArgs
import dev.convex.android.ClientException
import dev.convex.android.MobileConvexClient
import dev.convex.android.QuerySubscriber
import dev.convex.android.WebSocketState
import dev.convex.android.WebSocketStateSubscriber
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.buffer
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.serialization.json.JsonElement

/**
 * The real transport, over the Convex Android client's FFI layer
 * (MobileConvexClient) so answers stay raw JSON, as the iPhone's
 * ConvexMobileTransport does over convex-swift's.
 */
class ConvexFfiTransport(deploymentUrl: String) : ConvexTransport, AutoCloseable {
    private val socket = MutableStateFlow(false)
    override val connected: StateFlow<Boolean> = socket

    private val client = MobileConvexClient(
        deploymentUrl,
        "threadbear-android",
        object : WebSocketStateSubscriber {
            override fun onStateChange(state: WebSocketState) {
                socket.value = state == WebSocketState.CONNECTED
            }
        },
    )

    override suspend fun query(name: String, args: Map<String, JsonElement>): String =
        call { client.query(name, jsonArgs(args)) }

    override suspend fun mutation(name: String, args: Map<String, JsonElement>): String =
        call { client.mutation(name, jsonArgs(args)) }

    private suspend inline fun call(crossinline body: suspend () -> String): String =
        try { body() } catch (e: ClientException) { throw map(e) }

    override fun subscribe(name: String, args: Map<String, JsonElement>): Flow<String> = callbackFlow {
        val subscriber = object : QuerySubscriber {
            override fun onUpdate(value: String) { trySend(value) }
            override fun onError(message: String, value: String?) {
                close(if (value != null) TransportError.fromConvexError(value) else TransportError.Server(message))
            }
        }
        val handle = try {
            client.subscribe(name, jsonArgs(args), subscriber)
        } catch (e: ClientException) {
            throw map(e)
        }
        awaitClose { handle.cancel() }
    }.buffer(1, BufferOverflow.DROP_OLDEST) // only the newest answer matters

    override fun close() = client.close()

    companion object {
        fun map(e: ClientException): TransportError = when (e) {
            is ClientException.ConvexException -> TransportError.fromConvexError(e.data)
            is ClientException.ServerException -> TransportError.Server(e.msg)
            is ClientException.InternalException -> TransportError.Network(e.msg)
        }
    }
}

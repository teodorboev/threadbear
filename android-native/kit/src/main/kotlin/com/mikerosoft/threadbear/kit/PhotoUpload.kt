package com.mikerosoft.threadbear.kit

import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * One photo's copy in file storage, started the moment the photo's picked
 * so that Send usually has nothing left to wait for (PhotoUpload.swift).
 *
 * That means uploading photos that might never be sent, and the backend only
 * deletes a photo once a command that carried it has finished. So this owns
 * its copy until [keep]: dropped before then (the photo's taken out of the
 * composer, or the composer goes away), it stops the upload and takes the
 * copy back. Swift does that in deinit; Kotlin has none, so whoever holds
 * one calls [drop]. A photo left behind by the app being killed before that
 * stays in storage.
 */
class PhotoUpload(
    private val scope: CoroutineScope,
    upload: suspend (progress: (Float) -> Unit) -> String,
    private val discard: suspend (String) -> Unit,
) {
    enum class State { GOING, UP, FAILED }

    private val _state = MutableStateFlow(State.GOING)
    val state: StateFlow<State> = _state

    /** 0 to 1, as the bytes go. */
    private val _progress = MutableStateFlow(0f)
    val progress: StateFlow<Float> = _progress

    @Volatile private var kept = false
    @Volatile private var dropped = false

    // Its own supervisor under the scope: a failed upload mustn't cancel the scope.
    private val task = scope.async(SupervisorJob(scope.coroutineContext[Job])) {
        try {
            upload { _progress.value = it.coerceIn(0f, 1f) }.also {
                _progress.value = 1f
                _state.value = State.UP
            }
        } catch (e: Throwable) {
            _state.value = State.FAILED
            throw e
        }
    }

    /** The storage id, once it's up; throws if the upload failed. */
    suspend fun storageId(): String = task.await()

    /** It's gone with a message, so it's the backend's to delete from here. */
    fun keep() {
        kept = true
    }

    /** Not sent after all: the upload stops and the copy, if it got there, is taken back. Once. */
    fun drop() {
        if (kept || dropped) return
        dropped = true
        task.cancel()
        scope.launch {
            // Done just before the cancel: it has an id, which goes back.
            val id = runCatching { task.await() }.getOrNull() ?: return@launch
            discard(id)
        }
    }
}

/** POSTs a photo's bytes to an upload URL and returns the response body. */
fun interface PhotoPost {
    suspend fun post(url: String, jpeg: ByteArray, progress: (Float) -> Unit): String
}

/**
 * The real POST, on HttpURLConnection so the kit needs nothing more. In
 * chunks, for the progress ring and so a dropped photo stops between them
 * (a blocking socket write doesn't hear a coroutine's cancel).
 */
object HttpPhotoPost : PhotoPost {
    private const val CHUNK = 32 * 1024

    override suspend fun post(url: String, jpeg: ByteArray, progress: (Float) -> Unit): String = withContext(Dispatchers.IO) {
        val conn = URL(url).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "image/jpeg")
            conn.setFixedLengthStreamingMode(jpeg.size)
            conn.connectTimeout = 20_000
            conn.readTimeout = 60_000
            conn.outputStream.use { out ->
                var sent = 0
                while (sent < jpeg.size) {
                    ensureActive()
                    val n = minOf(CHUNK, jpeg.size - sent)
                    out.write(jpeg, sent, n)
                    sent += n
                    // The last few percent wait for the answer.
                    progress(0.95f * sent / jpeg.size)
                }
            }
            if (conn.responseCode != 200) throw TransportError.Server("Couldn't upload the photo.")
            conn.inputStream.use { it.readBytes().decodeToString() }
        } catch (e: TransportError) {
            throw e
        } catch (e: java.io.IOException) {
            throw TransportError.Network(e.message ?: "Couldn't upload the photo.")
        } finally {
            conn.disconnect()
        }
    }
}

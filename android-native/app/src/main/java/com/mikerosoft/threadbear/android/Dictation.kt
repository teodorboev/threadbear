package com.mikerosoft.threadbear.android

import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.util.Log
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.TextRange
import com.mikerosoft.threadbear.kit.Carry
import com.mikerosoft.threadbear.kit.DictationSpot
import com.mikerosoft.threadbear.kit.MicLevels
import java.util.Locale

/**
 * Tap the mic, talk, tap again, and the words land in the box where the
 * cursor was (Dictation.swift). The iPhone has Apple's recognizer and an
 * optional Whisper; here it's the phone's own SpeechRecognizer, on-device
 * where the phone has the model (Android 12+), else the system's, asked to
 * stay offline where it can, rather than bundling Whisper. It never sends
 * on its own: a message is worth a glance before it goes.
 *
 * Android's recognizer ends a session at each pause, so while the mic's
 * on, each one that ends starts the next, and [Carry] keeps what came
 * before (the iPhone's wipe of 31 Oct was the same thing: a restart taken
 * for the whole transcript).
 */
@Stable
class Dictation(private val context: Context) {
    var isListening by mutableStateOf(false)
        private set
    /** Tapped off: the last words are still on their way from the recognizer. */
    var isFinishing by mutableStateOf(false)
        private set
    /** When the mic started, for the elapsed time on the bar. */
    var startedAt by mutableStateOf<Long?>(null)
        private set
    var problem by mutableStateOf<String?>(null)
    /** The waveform's bars, 0 to 1. */
    var levels by mutableStateOf(List(24) { 0f })
        private set

    /** Words said that aren't in the box yet: sending now would leave them behind. */
    val holdsWords: Boolean get() = isListening || isFinishing
    val isHearing: Boolean get() = isListening || isFinishing

    private val main = Handler(Looper.getMainLooper())
    private val mic = MicLevels()
    private var recognizer: SpeechRecognizer? = null
    private var onDevice = false
    /** The on-device recognizer wouldn't start (no language pack, say): the system's from here on. */
    private var onDeviceBroken = false
    private var generation = 0
    private var carry = Carry()
    /** The current session's words so far, kept if it ends in an error rather than results. */
    private var session = ""
    private var onText: ((String) -> Unit)? = null
    /** A session is open on the recognizer (not between one ending and the next starting). */
    private var active = false

    /** Dictates into `text` at its cursor, each transcript replacing the last. */
    fun start(text: TextFieldState) {
        val spot = DictationSpot.at(text.text.toString(), text.selection.start)
        start { transcript ->
            val placed = spot.place(text.text.toString(), transcript)
            text.edit {
                replace(0, length, placed.text)
                selection = TextRange(placed.cursor)
            }
        }
    }

    /** `onText` gets the whole transcript so far, every time it changes. */
    fun start(onText: (String) -> Unit) {
        problem = null
        val available = SpeechRecognizer.isRecognitionAvailable(context) ||
            (Build.VERSION.SDK_INT >= 31 && SpeechRecognizer.isOnDeviceRecognitionAvailable(context))
        if (!available) {
            problem = "Speech recognition isn't available on this phone."
            return
        }
        stop()
        generation++
        this.onText = onText
        carry = Carry()
        session = ""
        mic.reset()
        levels = mic.shown
        isListening = true
        startedAt = SystemClock.elapsedRealtime()
        listen(generation)
    }

    /** The mic tapped off: the recognizer finishes what it heard, then it's done. */
    fun finish() {
        if (!isListening) return
        isListening = false
        isFinishing = true
        // A restart queued after a pause (or a busy recognizer) mustn't open the mic again.
        main.removeCallbacksAndMessages(null)
        // Between sessions: nothing's still to come.
        if (!active) {
            end()
            return
        }
        recognizer?.stopListening()
        val current = generation
        // A recognizer that never answers mustn't hold Send for good, nor keep
        // the mic: it's cancelled as the bar goes.
        main.postDelayed({
            if (generation == current && isFinishing) {
                generation++
                recognizer?.cancel()
                end()
            }
        }, 4000)
    }

    /** Off at once, whatever's still coming: a send, or the box going away. */
    fun stop() {
        generation++
        // Nothing queued (a restart, the finishing timeout) outlives a stop.
        main.removeCallbacksAndMessages(null)
        onText = null
        val held = isHearing
        isListening = false
        isFinishing = false
        startedAt = null
        // Only if it was listening: cancel() on an idle recognizer is harmless, but there's nothing to undo.
        if (held) recognizer?.cancel()
        active = false
    }

    /** The screen's gone. */
    fun close() {
        stop()
        recognizer?.destroy()
        recognizer = null
    }

    private fun end() {
        isListening = false
        isFinishing = false
        startedAt = null
        onText = null
    }

    /** Only ever for the listen Mike started and hasn't stopped: nothing else opens the mic. */
    private fun listen(current: Int) {
        if (generation != current || !isListening) return
        val r = recognizer ?: create().also { recognizer = it }
        r.setRecognitionListener(Listener(current))
        session = ""
        try {
            r.startListening(intent())
            active = true
        } catch (e: Exception) {
            Log.e("Dictation", "startListening", e)
            problem = "Couldn't start the microphone. Tap the mic to try again."
            end()
        }
    }

    private fun create(): SpeechRecognizer {
        onDevice = Build.VERSION.SDK_INT >= 31 && !onDeviceBroken && SpeechRecognizer.isOnDeviceRecognitionAvailable(context)
        return if (onDevice && Build.VERSION.SDK_INT >= 31) SpeechRecognizer.createOnDeviceSpeechRecognizer(context)
        else SpeechRecognizer.createSpeechRecognizer(context)
    }

    private fun intent() = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
        putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
        putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
        // English as the phone has it, else British English, as on the iPhone.
        val locale = Locale.getDefault()
        putExtra(RecognizerIntent.EXTRA_LANGUAGE, if (locale.language == "en") locale.toLanguageTag() else "en-GB")
        // The system's recognizer: on the phone where it can be.
        putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
        // Punctuation, as the iPhone's addsPunctuation.
        if (Build.VERSION.SDK_INT >= 33) putExtra(RecognizerIntent.EXTRA_ENABLE_FORMATTING, RecognizerIntent.FORMATTING_OPTIMIZE_QUALITY)
    }

    private fun words(results: Bundle?): String =
        results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()?.trim().orEmpty()

    private fun deliver(text: String) {
        onText?.invoke(text)
    }

    private inner class Listener(private val current: Int) : RecognitionListener {
        private val mine get() = generation == current

        override fun onReadyForSpeech(params: Bundle?) {}
        override fun onBeginningOfSpeech() {}
        override fun onBufferReceived(buffer: ByteArray?) {}
        override fun onEndOfSpeech() {}
        override fun onEvent(eventType: Int, params: Bundle?) {}

        override fun onRmsChanged(rmsdB: Float) {
            if (!mine) return
            mic.push(rmsdB)
            levels = mic.shown
        }

        override fun onPartialResults(partialResults: Bundle?) {
            if (!mine) return
            val text = words(partialResults)
            if (text.isEmpty()) return
            session = text
            deliver(carry.add(text))
        }

        override fun onResults(results: Bundle?) {
            if (!mine) return
            active = false
            val text = words(results).ifEmpty { session }
            deliver(carry.commit(text))
            session = ""
            // Still on: the session ended at a pause, so the next one starts.
            if (isListening) main.post { listen(current) } else end()
        }

        override fun onError(error: Int) {
            if (!mine) return
            active = false
            // The words of a session cut short stay.
            if (session.isNotEmpty()) {
                deliver(carry.commit(session))
                session = ""
            }
            if (error == SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS) problem = MIC_REFUSED
            // Tapped off already: whatever it says now, it's done.
            if (!isListening) {
                end()
                return
            }
            when (error) {
                SpeechRecognizer.ERROR_NO_MATCH, SpeechRecognizer.ERROR_SPEECH_TIMEOUT ->
                    // Nothing said in this stretch: not worth showing.
                    main.post { listen(current) }
                SpeechRecognizer.ERROR_RECOGNIZER_BUSY -> main.postDelayed({ listen(current) }, 250)
                SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS -> end()
                else -> if (onDevice && !onDeviceBroken) {
                    // As the iPhone does when on-device recognition won't
                    // start: the system's recognizer from here on.
                    Log.w("Dictation", "On-device recognition stopped ($error); using the system's")
                    onDeviceBroken = true
                    recognizer?.destroy()
                    recognizer = null
                    main.post { listen(current) }
                } else {
                    Log.w("Dictation", "Recognition error $error")
                    problem = "Speech recognition stopped. Tap the mic to try again."
                    end()
                }
            }
        }
    }

    companion object {
        const val MIC_REFUSED = "Threadbear can't use the microphone. Turn it on in Settings to dictate."
    }
}

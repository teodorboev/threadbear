package com.mikerosoft.threadbear.android

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.SystemClock
import com.mikerosoft.threadbear.kit.AppUpdate
import java.net.HttpURLConnection
import java.net.URL
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * A newer release of this app on GitHub (AppUpdate, docs/android-native.md
 * "Releases"), asked at most every six hours as the app comes forward. Only
 * a release build asks; nothing about the phone goes with the request.
 * Installing is Android's: the APK opens in the browser, which downloads it,
 * and Android asks to install it over this one (same key, newer version).
 */
object Updates {
    private val _available = MutableStateFlow<AppUpdate.Available?>(null)
    val available: StateFlow<AppUpdate.Available?> = _available
    private var lastAsked = 0L
    private const val EVERY_MS = 6 * 60 * 60 * 1000L
    private const val LIMIT = 512 * 1024

    val installed: String? get() = BuildConfig.RELEASE_DATE.takeIf { it.isNotEmpty() }

    fun check(scope: CoroutineScope) {
        val mine = installed ?: return
        val url = AppUpdate.latestUrl(BuildConfig.UPDATE_REPO) ?: return
        val now = SystemClock.elapsedRealtime()
        if (lastAsked != 0L && now - lastAsked < EVERY_MS) return
        lastAsked = now
        scope.launch {
            val body = withContext(Dispatchers.IO) { runCatching { fetch(url) }.getOrNull() } ?: return@launch
            _available.value = AppUpdate.newer(body, mine, BuildConfig.UPDATE_REPO)
        }
    }

    private fun fetch(url: String): String? {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 10_000
        conn.readTimeout = 10_000
        conn.setRequestProperty("Accept", "application/vnd.github+json")
        return try {
            if (conn.responseCode != 200) null
            // A release's JSON is a few KB; more than this isn't one. Read by
            // hand: readNBytes is Android 13+, and the app runs from Android 10.
            else conn.inputStream.use { input ->
                val out = java.io.ByteArrayOutputStream()
                val buf = ByteArray(8192)
                while (out.size() < LIMIT) {
                    val n = input.read(buf, 0, minOf(buf.size, LIMIT - out.size()))
                    if (n < 0) break
                    out.write(buf, 0, n)
                }
                out.toString(Charsets.UTF_8.name())
            }
        } finally {
            conn.disconnect()
        }
    }

    /** "12 Nov", for the menu and Settings. */
    fun label(date: String): String = runCatching {
        LocalDate.parse(date).format(DateTimeFormatter.ofPattern("d MMM", Locale.UK))
    }.getOrDefault(date)

    /** Opens the APK in the browser to download and install. */
    fun open(context: Context, update: AppUpdate.Available) {
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(update.apkUrl)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }
}

package com.mikerosoft.threadbear.kit

import java.io.ByteArrayOutputStream
import java.net.URI
import java.util.Base64
import kotlinx.serialization.json.JsonPrimitive

/**
 * Where the app connects and with what token (Pairing.swift). The Mac's
 * Settings › Phone & browser draws a QR code of `TBPAIR1_` and the base64url
 * of `url=…&code=<one-use code>&key=<runnerId>:<key>` (runner/src/devices.ts,
 * pairingText), which only the app's own scanner reads: it trades the code
 * for the phone's token ([claimed]). Never through the Mac clipboard.
 *
 * It used to be a `threadbear://pair?url=…&token=…` link the camera opened,
 * and any app can claim that scheme, so another app could be handed the
 * 90-day token (Daybreak finding 127, 15 Nov 2026). Such a link is refused
 * from anywhere ([PairingSource.isOldLink]).
 */
data class Pairing(
    /** The deployment's origin, e.g. https://happy-bear-123.convex.cloud */
    val deploymentUrl: String,
    /** Empty until a pairing code has been traded for it. */
    val token: String,
    /** Runner id to secret-box public key (base64), from the link's `key` items. */
    val runnerKeys: Map<String, String> = emptyMap(),
    /** The one-use pairing code from the Mac's QR code, to trade for [token]. */
    val code: String? = null,
) {
    /**
     * The token for a pairing code, from `tokens:claim` (backend API 80),
     * which takes each code once: this pairing with its token and no code.
     * One that has its token already is handed back as it is.
     */
    suspend fun claimed(transport: ConvexTransport): Pairing {
        val code = code ?: return this
        val raw = transport.mutation("tokens:claim", mapOf("code" to JsonPrimitive(code)))
        val token = runCatching { parseJson(raw).asObject()?.str("token") }.getOrNull()
        if (token == null || !token.startsWith("tb_")) throw TransportError.Server("The server didn't give the phone a token for that code.")
        return copy(token = token, code = null)
    }

    val serverName: String get() = serverName(deploymentUrl)

    /** The old token link, for tests only: nothing the app shows or sends makes one (Daybreak 127). */
    internal val link: String
        get() {
            val parts = mutableListOf("url=${encode(deploymentUrl)}", "token=${encode(token)}")
            runnerKeys.toSortedMap().forEach { (id, key) -> parts += "key=${encode("$id:$key")}" }
            return "threadbear://pair?" + parts.joinToString("&")
        }

    override fun toString() = "Pairing($serverName)" // never the token or the code

    companion object {
        /** What starts the Mac's pairing text. */
        const val TEXT_PREFIX = "TBPAIR1_"

        /** `tbp_` and hex, as convex/tokens.ts makes them. */
        internal fun isCode(code: String) = code.startsWith("tbp_") && code.length in 5..200 && code.substring(4).all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }

        /**
         * The Mac's pairing text, or an older Mac's `threadbear://pair` link.
         * Null for anything else. Where it came from is [PairingRequest.ask]'s to judge.
         */
        fun parse(text: String, allowLocal: Boolean = false): Pairing? {
            val t = text.trim()
            if (!t.startsWith(TEXT_PREFIX)) return parseLink(t, allowLocal)
            val body = t.substring(TEXT_PREFIX.length)
            if (body.isEmpty() || body.length > 4096 || !body.all { it in 'A'..'Z' || it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '_' }) return null
            // Padded back first: the Mac leaves it off, and not every Android
            // decoder takes base64 without it.
            val padded = body + "=".repeat((4 - body.length % 4) % 4)
            val query = try { String(Base64.getUrlDecoder().decode(padded), Charsets.UTF_8) } catch (_: Exception) { return null }
            val items = queryItems(query)
            val code = items.firstOrNull { it.first == "code" }?.second?.takeIf(::isCode) ?: return null
            // A code and a token both is no text the Mac makes.
            if (items.any { it.first == "token" }) return null
            return fromItems(items, allowLocal)?.copy(code = code)
        }

        /**
         * Convex cloud deployments, or a self-hosted backend at any https
         * address: on the tailnet, or one of the user's own (setup --url,
         * 14 Nov 2026; Pairing.swift says why the host isn't checked).
         * `allowLocal` (debug builds) also takes http://127.0.0.1 and the
         * emulator's 10.0.2.2.
         */
        private fun parseLink(link: String, allowLocal: Boolean): Pairing? {
            val uri = try { URI(link.trim().replace(" ", "%20")) } catch (_: Exception) { return null }
            if (uri.scheme != "threadbear" || uri.host != "pair") return null
            val items = queryItems(uri.rawQuery ?: return null)
            val token = items.firstOrNull { it.first == "token" }?.second?.takeIf { it.isNotEmpty() } ?: return null
            return fromItems(items, allowLocal)?.copy(token = token)
        }

        /** The server and runner keys of a link's or text's items, with no token yet. */
        private fun fromItems(items: List<Pair<String, String>>, allowLocal: Boolean): Pairing? {
            val token = ""
            val raw = items.firstOrNull { it.first == "url" }?.second ?: return null
            val deployment = try { URI(raw) } catch (_: Exception) { return null }
            val host = deployment.host?.takeIf { it.isNotEmpty() } ?: return null
            val keys = runnerKeys(items)
            if (allowLocal && deployment.scheme == "http" && host in setOf("127.0.0.1", "localhost", "10.0.2.2")) {
                val port = if (deployment.port > 0) ":${deployment.port}" else ""
                return Pairing("http://$host$port", token, keys)
            }
            if (deployment.scheme != "https") return null
            val lower = host.lowercase()
            if (!isCloud(lower)) return selfHostedBackend(deployment, lower)?.let { Pairing(it, token, keys) }
            if (deployment.rawUserInfo != null) return null
            val cloud = lower.removeSuffix(".convex.site").let { if (it == lower) lower else "$it.convex.cloud" }
            return Pairing("https://$cloud", token, keys)
        }

        /** `<runnerId>:<base64 key>`; the last colon splits them. */
        internal fun runnerKeys(items: List<Pair<String, String>>): Map<String, String> {
            val keys = LinkedHashMap<String, String>()
            for ((name, raw) in items) {
                if (name != "key") continue
                val colon = raw.lastIndexOf(':')
                if (colon < 0) continue
                val runnerId = raw.substring(0, colon)
                // A "+" a link maker left unescaped reads as a space.
                val key = raw.substring(colon + 1).replace(' ', '+')
                if (runnerId.isEmpty() || !isUsableKey(key)) continue
                keys[runnerId] = key
            }
            return keys
        }

        fun isUsableKey(key: String): Boolean =
            try { Base64.getDecoder().decode(key).size == 32 } catch (_: Exception) { false }

        internal fun isCloud(host: String) = host.endsWith(".convex.cloud") || host.endsWith(".convex.site")

        internal fun isLoopback(host: String): Boolean {
            val h = host.trim('[', ']').lowercase()
            return h == "localhost" || h == "::1" || h.startsWith("127.") || h.endsWith(".localhost")
        }

        /** A self-hosted backend: https, any host but this phone's loopback, and just its origin. */
        internal fun selfHostedBackend(url: URI, host: String): String? {
            val name = host.lowercase()
            if (url.scheme != "https" || name.isEmpty() || isLoopback(name)) return null
            if (url.rawUserInfo != null || url.rawQuery != null || url.rawFragment != null) return null
            val path = url.rawPath ?: ""
            if (path.isNotEmpty() && path != "/") return null
            return "https://$name" + if (url.port > 0) ":${url.port}" else ""
        }

        /**
         * A server's name: a self-hosted backend's host and port, else the
         * host (Convex cloud and the dev backend, keyed as they always were).
         */
        fun serverName(url: String): String {
            val uri = try { URI(url) } catch (_: Exception) { return url }
            val host = uri.host ?: return url
            val lower = host.lowercase()
            return if (!isCloud(lower) && !isLoopback(lower) && lower != "10.0.2.2" && uri.port > 0) "$host:${uri.port}" else host
        }

        /** Query items, percent-decoded, "+" left as it is (as URLComponents reads them). */
        internal fun queryItems(raw: String): List<Pair<String, String>> = raw.split('&').filter { it.isNotEmpty() }.map {
            val eq = it.indexOf('=')
            if (eq < 0) decode(it) to "" else decode(it.substring(0, eq)) to decode(it.substring(eq + 1))
        }

        internal fun decode(s: String): String {
            if ('%' !in s) return s
            val out = ByteArrayOutputStream()
            var i = 0
            while (i < s.length) {
                val c = s[i]
                if (c == '%' && i + 2 < s.length) {
                    val hex = s.substring(i + 1, i + 3).toIntOrNull(16)
                    if (hex != null) { out.write(hex); i += 3; continue }
                }
                out.write(c.toString().toByteArray(Charsets.UTF_8))
                i++
            }
            return out.toString(Charsets.UTF_8)
        }

        private fun encode(s: String): String = buildString {
            for (b in s.toByteArray(Charsets.UTF_8)) {
                val c = b.toInt() and 0xff
                val ch = c.toChar()
                if (ch.isLetterOrDigit() && c < 128 || ch in "-._~") append(ch) else append("%%%02X".format(c))
            }
        }
    }
}

/** Where a pairing code or link came from (PairingRequest.swift). */
enum class PairingSource {
    /** The app's own scanner, on its pairing screen. */
    SCANNED,
    /** The clipboard. */
    PASTED,
    /**
     * A link another app, a web page or the camera opened the app with.
     * Never taken: any app can claim the threadbear scheme and be handed the
     * same link (Daybreak finding 127). Scan the code in the app instead.
     */
    OPENED;

    /** Whether this is a pairing refused only because of where it came from. */
    fun refuses(text: String, allowLocal: Boolean): Boolean = this == OPENED && Pairing.parse(text, allowLocal) != null

    companion object {
        /**
         * An older Mac's `threadbear://pair` link with the token in it: never
         * taken, from anywhere. The token may have reached another app on its
         * way, and the clipboard is anyone's to fill, so an app that read the
         * Mac's one-use code could swap in a link with the token it claimed.
         */
        fun isOldLink(text: String, allowLocal: Boolean): Boolean = Pairing.parse(text, allowLocal)?.let { it.code == null } ?: false
    }
}

/** What a pairing link would change, asked about before anything is kept (PairingRequest.swift). */
data class PairingRequest(
    val pairing: Pairing,
    val replacesHost: String?,
    val samePairedServer: Boolean,
    val addedKeys: Int,
    val changedKeys: Int,
) {
    val host get() = pairing.serverName
    val title get() = "Pair with $host?"

    val message: String
        get() {
            val lines = mutableListOf<String>()
            lines += when {
                replacesHost != null -> "This link moves the phone from $replacesHost to $host, with a new token."
                samePairedServer -> "This link gives the phone a new token for $host."
                else -> "This link pairs the phone with the server at $host."
            }
            lines += when {
                addedKeys == 0 && changedKeys == 0 -> "The Macs the phone can send secrets to stay the same."
                changedKeys == 0 -> "It adds ${macs(addedKeys)} the phone can send secrets to."
                addedKeys == 0 -> "It changes the key of ${macs(changedKeys)} the phone sends secrets to."
                else -> "It adds ${macs(addedKeys)} and changes the key of ${macs(changedKeys)} the phone sends secrets to."
            }
            lines += "Only say yes if you just scanned the code on your own Mac."
            return lines.joinToString(" ")
        }

    companion object {
        /** Null for anything that isn't a pairing, and for any opened from outside ([PairingSource.OPENED]). */
        fun of(link: String, source: PairingSource, allowLocal: Boolean, current: Pairing?, keptKey: (String) -> String?): PairingRequest? {
            if (source == PairingSource.OPENED) return null
            val pairing = Pairing.parse(link, allowLocal)?.takeIf { it.code != null } ?: return null
            val newHost = pairing.serverName
            val currentHost = current?.serverName
            var added = 0
            var changed = 0
            for ((runnerId, key) in pairing.runnerKeys) {
                val kept = keptKey(runnerId)
                if (kept == null) { added++; continue }
                val same = runCatching { Base64.getDecoder().decode(kept).contentEquals(Base64.getDecoder().decode(key)) }.getOrDefault(false)
                if (!same) changed++
            }
            return PairingRequest(
                pairing = pairing,
                replacesHost = currentHost?.takeIf { it != newHost },
                samePairedServer = currentHost == newHost,
                addedKeys = added,
                changedKeys = changed,
            )
        }

        private fun macs(n: Int) = if (n == 1) "1 Mac" else "$n Macs"
    }
}

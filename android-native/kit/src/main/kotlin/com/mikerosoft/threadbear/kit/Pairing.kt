package com.mikerosoft.threadbear.kit

import java.io.ByteArrayOutputStream
import java.net.URI
import java.util.Base64

/**
 * Where the app connects and with what token (Pairing.swift): a
 * `threadbear://pair?url=…&token=…&key=<runnerId>:<key>` link, from the
 * Mac's QR code or pasted. Never through the Mac clipboard.
 */
data class Pairing(
    /** The deployment's origin, e.g. https://happy-bear-123.convex.cloud */
    val deploymentUrl: String,
    val token: String,
    /** Runner id to secret-box public key (base64), from the link's `key` items. */
    val runnerKeys: Map<String, String> = emptyMap(),
) {
    val serverName: String get() = serverName(deploymentUrl)

    val link: String
        get() {
            val parts = mutableListOf("url=${encode(deploymentUrl)}", "token=${encode(token)}")
            runnerKeys.toSortedMap().forEach { (id, key) -> parts += "key=${encode("$id:$key")}" }
            return "threadbear://pair?" + parts.joinToString("&")
        }

    override fun toString() = "Pairing($serverName)" // never the token

    companion object {
        /**
         * Convex cloud deployments, or a self-hosted backend on the tailnet
         * over https. `allowLocal` (debug builds) also takes http://127.0.0.1
         * and the emulator's 10.0.2.2.
         */
        fun parse(link: String, allowLocal: Boolean = false): Pairing? {
            val uri = try { URI(link.trim().replace(" ", "%20")) } catch (_: Exception) { return null }
            if (uri.scheme != "threadbear" || uri.host != "pair") return null
            val items = queryItems(uri.rawQuery ?: return null)
            val raw = items.firstOrNull { it.first == "url" }?.second ?: return null
            val token = items.firstOrNull { it.first == "token" }?.second?.takeIf { it.isNotEmpty() } ?: return null
            val deployment = try { URI(raw) } catch (_: Exception) { return null }
            val host = deployment.host?.takeIf { it.isNotEmpty() } ?: return null
            val keys = runnerKeys(items)
            if (allowLocal && deployment.scheme == "http" && host in setOf("127.0.0.1", "localhost", "10.0.2.2")) {
                val port = if (deployment.port > 0) ":${deployment.port}" else ""
                return Pairing("http://$host$port", token, keys)
            }
            tailnetBackend(deployment, host)?.let { return Pairing(it, token, keys) }
            if (deployment.scheme != "https") return null
            val lower = host.lowercase()
            if (!(lower.endsWith(".convex.cloud") || lower.endsWith(".convex.site"))) return null
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

        internal fun tailnetBackend(url: URI, host: String): String? {
            val name = host.lowercase()
            if (url.scheme != "https" || !name.endsWith(".ts.net") || name.split('.').size < 4) return null
            if (url.rawUserInfo != null || url.rawQuery != null || url.rawFragment != null) return null
            val path = url.rawPath ?: ""
            if (path.isNotEmpty() && path != "/") return null
            return "https://$name" + if (url.port > 0) ":${url.port}" else ""
        }

        /** A server's name: a tailnet backend's host and port, else the host. */
        fun serverName(url: String): String {
            val uri = try { URI(url) } catch (_: Exception) { return url }
            val host = uri.host ?: return url
            return if (host.lowercase().endsWith(".ts.net") && uri.port > 0) "$host:${uri.port}" else host
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
        fun of(link: String, allowLocal: Boolean, current: Pairing?, keptKey: (String) -> String?): PairingRequest? {
            val pairing = Pairing.parse(link, allowLocal) ?: return null
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

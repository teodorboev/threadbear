package com.mikerosoft.threadbear.kit

import java.net.URI

/**
 * What Threadbear opens without asking, for a link that came from anything
 * it doesn't fully trust: an agent's words, a web page, another Mac's runner
 * or the backend (SECURITY.md, "Links and files"; the iPhone's
 * LinkPolicy.swift, the parts a phone uses). The app's LinkGate is the only
 * place that hands a URL to the system, and decides with this.
 */
object LinkPolicy {
    sealed class Kind {
        /** http or https with a host: the browser, without asking. */
        data class Web(val url: String) : Kind()
        /** A file: on the Mac, never opened here. */
        data class File(val url: String) : Kind()
        /**
         * Any other scheme (`mailto:`, `slack:`, `intent:`): it starts another
         * app, which can act on what the link says, so only once Mike has said
         * yes to that link by name.
         */
        data class Ask(val url: String) : Kind()
        /** Nothing that can be opened: no scheme, or web with no host. */
        data object Refuse : Kind()
    }

    private val schemePattern = Regex("^([A-Za-z][A-Za-z0-9+.-]*):")

    fun scheme(url: String): String? = schemePattern.find(url)?.groupValues?.get(1)?.lowercase()

    fun kind(url: String): Kind = when (scheme(url)) {
        null -> Kind.Refuse
        "http", "https" -> web(url)?.let { Kind.Web(it) } ?: Kind.Refuse
        "file" -> Kind.File(url)
        else -> Kind.Ask(url)
    }

    /**
     * An http(s) address with a host, else null. The scheme is compared
     * without case (`HTTPS:` is web too); anything else is not web.
     */
    fun web(url: String?): String? {
        if (url == null || url.length > 8_192) return null
        val s = scheme(url)
        if (s != "http" && s != "https") return null
        val host = try {
            URI(url).host
        } catch (_: Exception) {
            null
        }
        return if (host.isNullOrEmpty()) null else url
    }

    /** A link shown in a question about it: long ones cut in the middle, so both the scheme and the end of it show. */
    fun shown(url: String, limit: Int = 300): String {
        if (url.length <= limit) return url
        return url.take(limit * 2 / 3) + "…" + url.takeLast(limit / 3)
    }
}

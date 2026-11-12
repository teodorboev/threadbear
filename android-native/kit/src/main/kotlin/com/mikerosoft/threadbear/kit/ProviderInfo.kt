package com.mikerosoft.threadbear.kit

import java.util.concurrent.ConcurrentHashMap
import kotlinx.serialization.json.JsonObject

/**
 * What a provider is called and can do, from `hello.providers`
 * (docs/protocol.md, "Types"; ProviderInfo.swift). A provider id alone
 * never decides what a control offers, so a provider this app has never
 * heard of still gets the right menus.
 */
data class ProviderInfo(
    val id: String,
    val name: String,
    val cliName: String,
    val loginCommand: String = "",
    /** "claude", "codex" (with ultra) or "none". */
    val effort: String = "none",
    /** "classifier", or "asks" where Auto behaves like Ask. */
    val autoMode: String = "asks",
    val sandbox: Boolean = false,
    val rewind: Boolean = false,
    val smallJobs: Boolean = false,
    val usageReadings: Boolean = false,
) {
    companion object {
        /** Lenient as the Swift decoder: only the id is needed, the rest falls back. */
        fun read(o: JsonObject): ProviderInfo {
            val id = o.needStr("id")
            return ProviderInfo(
                id = id,
                name = o.str("name") ?: id,
                cliName = o.str("cliName") ?: id,
                loginCommand = o.str("loginCommand") ?: "",
                effort = o.str("effort") ?: "none",
                autoMode = o.str("autoMode") ?: "asks",
                sandbox = o.bool("sandbox") ?: false,
                rewind = o.bool("rewind") ?: false,
                smallJobs = o.bool("smallJobs") ?: false,
                usageReadings = o.bool("usageReadings") ?: false,
            )
        }
    }
}

/**
 * The one place the app asks what a provider is. It holds what connected
 * runners have said, over what's built in for Claude and Codex, which is
 * what an older runner's threads get.
 */
object Providers {
    private val builtIn = mapOf(
        "claude" to ProviderInfo(
            id = "claude", name = "Claude", cliName = "Claude Code", loginCommand = "claude auth login", effort = "claude",
            autoMode = "classifier", sandbox = true, rewind = true, smallJobs = true, usageReadings = true,
        ),
        "codex" to ProviderInfo(
            id = "codex", name = "Codex", cliName = "Codex", loginCommand = "codex login", effort = "codex",
            autoMode = "asks", sandbox = false, rewind = true, smallJobs = false, usageReadings = true,
        ),
    )

    private val told = ConcurrentHashMap<String, ProviderInfo>()

    /** From each runner's hello. Added to, as a thread can be another runner's. */
    fun update(list: List<ProviderInfo>) {
        for (info in list) if (info.id.isNotEmpty()) told[info.id] = info
    }

    /** Null when nothing has said what this provider is. */
    fun known(id: String): ProviderInfo? {
        val key = id.ifEmpty { "claude" }
        return told[key] ?: builtIn[key]
    }

    /**
     * Never fails. One nothing has said anything about gets its id for a
     * name and can do nothing, so no control offers what it might not have.
     */
    fun info(id: String): ProviderInfo {
        known(id)?.let { return it }
        val name = id.replaceFirstChar { it.uppercase() }
        return ProviderInfo(id = id, name = name, cliName = name)
    }

    /** For tests: back to the built-in two. */
    internal fun reset() = told.clear()
}

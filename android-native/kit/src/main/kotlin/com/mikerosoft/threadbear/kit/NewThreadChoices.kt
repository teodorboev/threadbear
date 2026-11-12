package com.mikerosoft.threadbear.kit

import java.time.Instant
import kotlinx.serialization.json.JsonObject

/**
 * One model an account offers (backend API 20), as the Mac's model picker
 * lists it: `id` is what a new thread sends.
 */
data class PhoneModelChoice(
    val id: String,
    val title: String,
    val effortLevels: List<String>? = null,
    val defaultEffort: String? = null,
) {
    companion object {
        fun read(o: JsonObject) = PhoneModelChoice(
            id = o.needStr("id"),
            title = o.needStr("title"),
            effortLevels = o.strings("effortLevels"),
            defaultEffort = o.str("defaultEffort"),
        )
    }
}

/**
 * A row of phone:accounts: a login on one of the runners. Only what the
 * new thread pickers need; the rest of the row is left alone.
 */
data class PhoneAccount(
    val id: String,
    val runnerId: String? = null,
    val provider: String,
    val title: String,
    val models: List<PhoneModelChoice> = emptyList(),
) {
    companion object {
        fun read(o: JsonObject): PhoneAccount {
            val id = o.needStr("id")
            return PhoneAccount(
                id = id,
                runnerId = o.str("runnerId"),
                provider = o.str("provider") ?: "claude",
                title = o.str("title") ?: id,
                // A backend before API 20 has no models: no pickers, not a broken row.
                models = o.arr("models")?.lossy { PhoneModelChoice.read(it as JsonObject) } ?: emptyList(),
            )
        }
    }
}

/**
 * What a new thread from the phone can name. Null is the runner's usual.
 * No login: naming a model is enough for the runner to pick a login of its
 * provider with room (runner.ts, pickForNew), as on the Mac.
 */
data class NewThreadSettings(val model: String? = null, val effort: String? = null)

/**
 * The new thread screen's runner, model and effort pickers, from the
 * mirrored runners and accounts (NewThreadChoices.swift).
 */
object NewThreadChoices {
    /**
     * Where a new thread starts unless Mike picks another: the first runner
     * online of those he's placed in his order (phone:runners lists them in
     * it), or else the main Mac, to wait for. A runner he hasn't placed is
     * never picked for him, so a Mac with a runner key only gets his words
     * when he's said so. Null with neither, which leaves it to the backend
     * (phone:create's own choice). Draining ones are passed over.
     */
    fun runner(runners: List<PhoneRunner>, now: Instant): PhoneRunner? =
        runners.firstOrNull { it.placed == true && it.isOnline(now) && it.draining != true }
            ?: runners.firstOrNull { it.main == true }

    /**
     * The models on offer on one runner: its own logins' (a row with no
     * runner is anyone's). None while its logins haven't come up yet, so it
     * starts on its own default rather than another Mac's model.
     */
    fun models(accounts: List<PhoneAccount>, runnerId: String?): List<PhoneModelChoice> {
        if (runnerId == null) return models(accounts)
        return models(accounts.filter { it.runnerId == null || it.runnerId == runnerId })
    }

    /** Every model on offer, Claude's first, each once. */
    fun models(accounts: List<PhoneAccount>): List<PhoneModelChoice> {
        val order = listOf("claude", "codex")
        fun rank(a: PhoneAccount) = order.indexOf(a.provider).let { if (it < 0) order.size else it }
        // sortedBy is stable, so accounts of one provider keep their order.
        val seen = HashSet<String>()
        return accounts.sortedBy(::rank).flatMap { it.models }.filter { seen.add(it.id) }
    }

    /**
     * The efforts a model offers, or with no model picked Claude's levels
     * (the runner drops one the model it lands on doesn't offer).
     */
    fun efforts(model: PhoneModelChoice?): List<String> {
        val levels = model?.effortLevels ?: emptyList()
        return if (levels.isEmpty()) CLAUDE_LEVELS else levels.filter { it in ALL }
    }

    fun effortTitle(level: String): String =
        TITLES[level] ?: level.replaceFirstChar { it.uppercase() }

    /** Whether a picked effort still fits once the model changes. */
    fun keeps(effort: String?, model: PhoneModelChoice?): Boolean = effort == null || effort in efforts(model)

    /**
     * "Mac offline since 10:42" when no runner has checked in for three
     * minutes (AppModel.offlineText). Things done meanwhile wait until it's back.
     */
    fun offlineText(runners: List<PhoneRunner>, now: Instant): String? {
        if (runners.isEmpty() || runners.any { it.isOnline(now) }) return null
        val newest = runners.maxBy { it.lastSeenAt }
        val name = if (runners.size == 1) "Mac" else newest.name
        return "$name offline since ${RelativeTime.clock(newest.lastSeenAt, now)}"
    }

    /** With a runner picked, whether that one's asleep. */
    fun offlineText(runner: PhoneRunner, now: Instant): String? =
        if (runner.isOnline(now)) null else "${runner.name} offline since ${RelativeTime.clock(runner.lastSeenAt, now)}"

    private val ALL = listOf("low", "medium", "high", "xhigh", "max", "ultra")
    private val CLAUDE_LEVELS = listOf("low", "medium", "high", "xhigh", "max")
    private val TITLES = mapOf(
        "low" to "Low", "medium" to "Medium", "high" to "High", "xhigh" to "Extra high", "max" to "Max", "ultra" to "Ultra",
    )
}

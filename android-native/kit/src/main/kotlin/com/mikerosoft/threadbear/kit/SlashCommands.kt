package com.mikerosoft.threadbear.kit

import kotlinx.serialization.json.JsonObject

/**
 * A command the composer's palette offers (docs/protocol.md, "Slash
 * commands"; SlashCommands.swift), as `threads.commands` lists it for a thread.
 *
 * `source`: "threadbear", "cli" or "skill"; anything newer counts as the CLI's.
 * `aliases`: other names that run it (/usage is /cost and /stats too).
 * `scope`: where it was defined: "user", "project", "plugin" (with `plugin`),
 * "claude.ai", "system", or one this app doesn't know.
 * `path`: its file on the runner's machine, when the runner found it.
 * `userOnly`: runs only as the start of a message: its frontmatter says
 * `disable-model-invocation: true`, so the agent can't run it when it's
 * named partway through one.
 */
data class SlashCommand(
    val name: String,
    val description: String? = null,
    val argumentHint: String? = null,
    val source: String,
    val aliases: List<String>? = null,
    val scope: String? = null,
    val plugin: String? = null,
    val path: String? = null,
    val userOnly: Boolean? = null,
) {
    val id get() = name

    /** What to type, with its arguments: "/code-review [low|medium|high] [target]". */
    val usage: String
        get() {
            val hint = argumentHint?.trim()
            return if (hint.isNullOrEmpty()) "/$name" else "/$name $hint"
        }

    val isSkill get() = source == "skill"
    val isThreadbear get() = source == "threadbear"

    companion object {
        fun read(o: JsonObject) = SlashCommand(
            name = o.needStr("name"),
            description = o.str("description"),
            argumentHint = o.str("argumentHint"),
            source = o.needStr("source"),
            aliases = o.strings("aliases"),
            scope = o.str("scope"),
            plugin = o.str("plugin"),
            path = o.str("path"),
            userOnly = o.bool("userOnly"),
        )
    }
}

/**
 * The palette that opens over the composer when a message starts with "/"
 * (mockup C, picked on 6 Nov): categories, the matches, and each described.
 * The Mac-only column and window sizing isn't ported.
 */
object SlashPalette {
    enum class Category {
        ALL, THREADBEAR, CLI, SKILLS, RECENT;

        /** The CLI's own are named after it ("Claude Code", "Codex"). */
        fun title(provider: String): String = when (this) {
            ALL -> "All"
            THREADBEAR -> "Threadbear"
            CLI -> cliName(provider)
            SKILLS -> "Skills"
            RECENT -> "Recent"
        }
    }

    /**
     * The provider's CLI by name, as Providers.info(id).cliName has it for
     * the built-in two; one nothing has said anything about gets its id
     * with a capital.
     */
    fun cliName(provider: String): String = when (provider.ifEmpty { "claude" }) {
        "claude" -> "Claude Code"
        "codex" -> "Codex"
        else -> provider.take(1).uppercase() + provider.drop(1)
    }

    /**
     * A command being typed as the last word of the message: at its start
     * ("/co"), or partway through it ("then use /td"). `start` is where its
     * slash is, in UTF-16 units as the text field counts.
     */
    data class Typed(val query: String, val start: Int) {
        val midMessage get() = start > 0
    }

    /**
     * The word at the end of the text, when it's a command being typed: a
     * slash at the start or after a space or line break, and no second slash
     * (a path, "/Users/"). Null once a space follows it.
     */
    fun typed(text: String): Typed? {
        val word = text.split(Regex("\\s")).last()
        if (!word.startsWith("/")) return null
        val rest = word.drop(1)
        if ('/' in rest) return null
        return Typed(rest, text.length - word.length)
    }

    /**
     * What the palette offers partway through a message: skills the agent can
     * run when it reads one named there. A CLI's own commands, and skills that
     * only run as the start of a message, do nothing there (claude -p, 9 Nov 2026).
     */
    fun midMessage(commands: List<SlashCommand>): List<SlashCommand> = commands.filter { it.isSkill && it.userOnly != true }

    /**
     * The command being typed, without its slash, while the message is only
     * that: "/", "/co". Null once there's a space (its arguments), a second
     * line, or a second slash (a path, "/Users/").
     */
    fun query(text: String): String? {
        if (!text.startsWith("/")) return null
        val rest = text.drop(1)
        if (rest.any { it.isWhitespace() || it == '/' }) return null
        return rest
    }

    /** Where a command came from, for its description. */
    fun origin(command: SlashCommand, provider: String): String {
        if (command.isThreadbear) return "Threadbear's own: it changes this thread's settings"
        val kind = command.path?.let { if (it.endsWith("/SKILL.md")) "skill" else "command" } ?: if (command.isSkill) "skill" else "command"
        return when (command.scope) {
            "user" -> "Your $kind"
            "project", "repo" -> "This folder's $kind"
            "plugin" -> command.plugin?.let { "From the $it plugin" } ?: "From a plugin"
            "claude.ai" -> "Synced from claude.ai"
            "admin" -> "Set up by your organisation"
            "system" -> "${cliName(provider)}'s own"
            else -> if (command.isSkill) "A skill" else "${cliName(provider)}'s own"
        }
    }

    private val markdownLink = Regex("""(?<!!)\[([^\]\n]*)\]\([^)\n]*\)""")
    private val autolink = Regex("""<((?:https?|mailto|file):[^>\s]*)>""", RegexOption.IGNORE_CASE)

    /**
     * A command's description as it should read. Skills write theirs with
     * inline markdown (`computercub`, **bold**), which the palette renders,
     * so that's left in; links keep their words but stop being links. A
     * description comes from whoever wrote the skill (any cloned repo can
     * have one), and on the Mac a `file:` link in the palette went straight
     * to macOS and ran a script, past OpenGate's ask (Daybreak, 8 Nov). The
     * palette only needs the formatting: draw this without linking anything.
     */
    fun description(text: String): String = autolink.replace(markdownLink.replace(text) { it.groupValues[1] }) { it.groupValues[1] }

    /** A path to show: the home folder as ~. */
    fun shortPath(path: String, home: String = System.getProperty("user.home") ?: ""): String =
        if (home.isNotEmpty() && (path == home || path.startsWith("$home/"))) "~" + path.drop(home.length) else path

    fun inCategory(command: SlashCommand, category: Category, recent: List<String>): Boolean = when (category) {
        Category.ALL -> true
        Category.THREADBEAR -> command.isThreadbear
        Category.CLI -> !command.isThreadbear && !command.isSkill
        Category.SKILLS -> command.isSkill
        Category.RECENT -> command.name in recent
    }

    /**
     * The matches for what's typed: names that start with it first, then
     * names that contain it, then ones whose description does, each in the
     * runner's order. Recent is in the order they were used.
     */
    fun matches(commands: List<SlashCommand>, query: String, category: Category, recent: List<String>): List<SlashCommand> {
        val q = query.lowercase()
        val pool = commands.filter { inCategory(it, category, recent) }
        fun rank(c: SlashCommand): Int? {
            val name = c.name.lowercase()
            if (q.isEmpty() || name.startsWith(q)) return 0
            if (name.contains(q)) return 1
            if (c.description?.lowercase()?.contains(q) == true) return 2
            return null
        }
        var ranked = pool.withIndex().mapNotNull { (i, c) -> rank(c)?.let { Triple(it, i, c) } }
        if (category == Category.RECENT) {
            ranked = ranked.map { (r, order, c) -> Triple(r, recent.indexOf(c.name).takeIf { it >= 0 } ?: order, c) }
        }
        return ranked.sortedWith(compareBy({ it.first }, { it.second })).map { it.third }
    }

    /**
     * How many each category has for what's typed. Recent is left out where
     * there are none: a thread with nothing used yet has no Recent to tap.
     */
    fun counts(commands: List<SlashCommand>, query: String, recent: List<String>): List<Pair<Category, Int>> =
        Category.entries.mapNotNull { cat ->
            if (cat == Category.RECENT && recent.isEmpty()) null else cat to matches(commands, query, cat, recent).size
        }

    /** The text a pick leaves in the box: the command, and a space when its arguments are to be typed next. */
    fun fill(command: SlashCommand, forArguments: Boolean): String = if (forArguments) "/${command.name} " else "/${command.name}"

    /** The recently used list after using `name`: newest first, eight kept. */
    fun used(name: String, recent: List<String>): List<String> = (listOf(name) + recent.filter { it != name }).take(8)
}

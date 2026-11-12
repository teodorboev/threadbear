package com.mikerosoft.threadbear.kit

import java.time.Instant
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// The rows convex/phone.ts returns, as ThreadbearKit's Models.swift reads
// them. Field names follow the runner's ThreadSummary.

enum class ThreadStatus {
    IDLE, STARTING, RUNNING, WAITING, ERROR, STOPPED,
    /** A status a newer runner sends that this build doesn't know. */
    UNKNOWN;

    companion object {
        fun of(raw: String?): ThreadStatus = when (raw) {
            "idle" -> IDLE
            "starting" -> STARTING
            "running" -> RUNNING
            "waiting" -> WAITING
            "error" -> ERROR
            "stopped" -> STOPPED
            else -> UNKNOWN
        }
    }
}

enum class ApprovalOption(val wire: String) {
    ALLOW("allow"), ALLOW_ALWAYS("allowAlways"), DENY("deny");

    companion object {
        fun of(raw: String): ApprovalOption? = entries.firstOrNull { it.wire == raw }
    }
}

data class QuestionOption(val label: String, val description: String? = null)

data class Question(
    val question: String,
    val header: String? = null,
    val multiSelect: Boolean = false,
    val options: List<QuestionOption>,
) {
    companion object {
        fun read(o: JsonObject) = Question(
            question = o.needStr("question"),
            header = o.str("header"),
            multiSelect = o.bool("multiSelect") ?: false,
            options = o.arr("options")?.lossy { e ->
                val x = e as JsonObject
                QuestionOption(x.needStr("label"), x.str("description"))
            } ?: emptyList(),
        )
    }
}

/** What a thread is stuck on: a tool wanting permission, questions, or a secret. */
sealed class PendingApproval {
    abstract val id: String

    data class Approval(
        override val id: String,
        val name: String,
        val title: String,
        val options: List<ApprovalOption>,
        val inputSummary: String?,
        /** On Claude's ExitPlanMode card: the seq of the plan row it waits on (backend API 72). */
        val planSeq: Int? = null,
    ) : PendingApproval()

    data class Questions(override val id: String, val questions: List<Question>) : PendingApproval()

    /** `key`: the runner's X25519 public key (base64) the value is sealed to. */
    data class Secret(
        override val id: String,
        val label: String,
        val reason: String,
        val placeholder: String?,
        val key: String,
    ) : PendingApproval()

    /** A kind this build doesn't know: answer it on the Mac. */
    data class Other(override val id: String, val kind: String) : PendingApproval()

    companion object {
        fun read(o: JsonObject): PendingApproval {
            val id = o.needStr("approvalId")
            return when (val kind = o.str("kind")) {
                "question" -> Questions(id, o.arr("questions")?.lossy { Question.read(it as JsonObject) } ?: throw BadRow("No questions"))
                "secret" -> {
                    val key = o.str("key") ?: return Other(id, "secret")
                    Secret(id, o.str("label") ?: "A secret", o.str("reason") ?: "", o.str("placeholder"), key)
                }
                null, "approval" -> Approval(
                    id = id,
                    name = o.str("name") ?: "",
                    title = o.str("title") ?: "Permission",
                    options = (o.strings("options") ?: listOf("allow", "deny")).mapNotNull(ApprovalOption::of),
                    inputSummary = o.str("inputSummary"),
                    planSeq = o.num("planSeq")?.wholeCount(),
                )
                else -> Other(id, kind)
            }
        }
    }
}

data class ProjectRef(val id: String, val title: String)

data class PhoneProjectIcon(val symbol: String? = null, val url: String? = null, val by: String? = null) {
    companion object {
        fun read(o: JsonObject) = PhoneProjectIcon(
            symbol = o.str("symbol")?.takeIf { it.isNotEmpty() },
            url = o.str("url")?.takeIf { it.startsWith("https://") || it.startsWith("http://") },
            by = o.str("by"),
        )
    }
}

data class PhoneProject(
    val id: String,
    val runnerId: String? = null,
    val title: String,
    val archived: Boolean? = null,
    val icon: PhoneProjectIcon? = null,
) {
    companion object {
        fun read(o: JsonObject) = PhoneProject(
            id = o.needStr("id"),
            runnerId = o.str("runnerId"),
            title = o.str("title") ?: "",
            archived = o.bool("archived"),
            icon = o.obj("icon")?.let(PhoneProjectIcon::read),
        )
    }
}

/** "C" for "Convex docs", "7" for "7 day plan", "?" for neither. */
object ProjectMark {
    fun letter(title: String): String {
        val first = title.firstOrNull { it.isLetterOrDigit() } ?: return "?"
        return first.uppercase()
    }
}

data class Woke(val at: Instant, val since: Instant? = null)

data class StartedBy(val kind: String, val label: String, val icon: String? = null, val why: String? = null)

data class PhoneLink(val title: String, val url: String, val icon: String? = null) {
    /** Only http or https pages, never a file or another app. */
    val page: String?
        get() {
            val lower = url.lowercase()
            if (!(lower.startsWith("http://") || lower.startsWith("https://"))) return null
            val host = url.substringAfter("://").substringBefore('/').substringBefore('?')
            return if (host.isEmpty()) null else url
        }
}

data class PhoneWaitingForRoom(val until: String, val accountId: String? = null)
data class PhoneWaitingForNetwork(val since: String)

data class PhoneAskLink(val title: String, val url: String)

data class PhoneAskRun(
    val command: String,
    val buttonId: String,
    val cwd: String? = null,
    val running: Boolean? = null,
    val failed: String? = null,
)

data class PhoneThread(
    val threadId: String,
    val runnerId: String = "mac",
    val title: String,
    val cwd: String = "~/ThreadbearThreads",
    val accountId: String = "claude-main",
    val provider: String = "claude",
    val model: String? = null,
    val status: ThreadStatus = ThreadStatus.IDLE,
    val needsYou: Boolean = false,
    val unread: Boolean = false,
    val preview: String = "",
    val lastActivityAt: Instant,
    val snoozedUntil: Instant? = null,
    val snoozedUntilRestart: Boolean? = null,
    val wakeNote: String? = null,
    val woke: Woke? = null,
    val startedBy: StartedBy? = null,
    val asked: String? = null,
    val archivedAt: Instant? = null,
    val starredAt: Instant? = null,
    val costUsd: Double? = null,
    val projects: List<ProjectRef>? = null,
    val working: String? = null,
    val pendingApproval: PendingApproval? = null,
    val earliestSeq: Double? = null,
    val asks: List<PhoneAsk>? = null,
    val asksSeq: Double? = null,
    val next: List<String>? = null,
    val needs: String? = null,
    val pendingReviews: Double? = null,
    val waitingForRoom: PhoneWaitingForRoom? = null,
    val waitingForNetwork: PhoneWaitingForNetwork? = null,
    val links: List<PhoneLink>? = null,
) {
    val id get() = threadId
    val isBusy get() = status == ThreadStatus.RUNNING || status == ThreadStatus.STARTING

    /** A design review waits on Mike and the thread has finished working. */
    val reviewWaiting get() = (pendingReviews ?: 0.0) > 0 && !isBusy && working == null

    /** The last turn ended asking Mike for something he hasn't done yet. */
    val waitingOnMike: Boolean
        get() {
            if (needs != "you" || isBusy) return false
            val asks = asks ?: emptyList()
            return asks.isEmpty() || asks.any { it.ticked != true }
        }

    fun hasEarlier(oldestSeq: Int?): Boolean {
        val earliest = earliestSeq ?: return false
        return oldestSeq != null && oldestSeq.toDouble() > earliest
    }

    val folderName: String
        get() {
            val trimmed = cwd.removeSuffix("/")
            return trimmed.split('/').lastOrNull { it.isNotEmpty() } ?: cwd
        }

    /** Line 2 of a row: the projects, or the folder. */
    val placeText: String
        get() = projects?.takeIf { it.isNotEmpty() }?.joinToString(", ") { it.title } ?: folderName

    companion object {
        fun read(o: JsonObject): PhoneThread = PhoneThread(
            threadId = o.needStr("threadId"),
            runnerId = o.needStr("runnerId"),
            title = o.needStr("title"),
            cwd = o.needStr("cwd"),
            accountId = o.needStr("accountId"),
            provider = o.needStr("provider"),
            model = o.str("model"),
            status = ThreadStatus.of(o.needStr("status")),
            needsYou = o.needBool("needsYou"),
            unread = o.needBool("unread"),
            preview = o.needStr("preview"),
            lastActivityAt = o.needDate("lastActivityAt"),
            snoozedUntil = o.date("snoozedUntil"),
            snoozedUntilRestart = o.bool("snoozedUntilRestart"),
            wakeNote = o.str("wakeNote"),
            woke = o.obj("woke")?.let { w -> w.date("at")?.let { Woke(it, w.date("since")) } },
            startedBy = o.obj("startedBy")?.let { s ->
                val kind = s.str("kind") ?: return@let null
                StartedBy(kind, s.str("label") ?: "", s.str("icon"), s.str("why"))
            },
            asked = o.str("asked"),
            archivedAt = o.date("archivedAt"),
            starredAt = o.date("starredAt"),
            costUsd = o.num("costUsd"),
            projects = o.arr("projects")?.lossy { val p = it as JsonObject; ProjectRef(p.needStr("id"), p.needStr("title")) },
            working = o.str("working"),
            pendingApproval = o.obj("pendingApproval")?.let { runCatching { PendingApproval.read(it) }.getOrNull() },
            earliestSeq = o.num("earliestSeq"),
            asks = o.arr("asks")?.lossy { PhoneAsk.read(it as JsonObject) },
            asksSeq = o.num("asksSeq"),
            next = o.strings("next"),
            needs = o.str("needs"),
            pendingReviews = o.num("pendingReviews"),
            waitingForRoom = o.obj("waitingForRoom")?.let { w -> w.str("until")?.let { PhoneWaitingForRoom(it, w.str("accountId")) } },
            waitingForNetwork = o.obj("waitingForNetwork")?.let { w -> w.str("since")?.let { PhoneWaitingForNetwork(it) } },
            links = o.arr("links")?.lossy { val l = it as JsonObject; PhoneLink(l.needStr("title"), l.needStr("url"), l.str("icon")) },
        )
    }
}

/** One thing a turn needs from Mike, from its end_turn. */
data class PhoneAsk(
    val action: String,
    val why: String? = null,
    val reply: String? = null,
    val options: List<String>? = null,
    val optionNotes: List<String>? = null,
    val suggested: Double? = null,
    val links: List<PhoneAskLink>? = null,
    val ticked: Boolean? = null,
    val run: PhoneAskRun? = null,
) {
    /** What to pick from: its options, else the reply it wants ("done"). */
    val choices: List<String>
        get() {
            options?.takeIf { it.isNotEmpty() }?.let { return it }
            val r = reply?.trim()
            return if (r.isNullOrEmpty()) emptyList() else listOf(r)
        }

    /** What choosing `choice` does, only when there's a note for each option. */
    fun note(choice: Int): String? {
        val opts = options?.takeIf { it.isNotEmpty() } ?: return null
        val notes = optionNotes ?: return null
        if (notes.size != opts.size || choice !in notes.indices) return null
        return notes[choice].trim().ifEmpty { null }
    }

    val hasNotes get() = choices.indices.any { note(it) != null }

    val suggestedIndex: Int? get() = suggested?.wholeCount()

    /** The action on one line, a full stop if needed, then the why. */
    val paragraph: String
        get() {
            val line = action.split('\n').map { it.trim() }.filter { it.isNotEmpty() }.joinToString(" ")
            val why = (why ?: "").trim()
            if (why.isEmpty()) return line
            val last = line.lastOrNull { it !in "\"'”’)]*_`" }
            val ends = last != null && last in ".?!…:"
            return (if (ends) line else "$line.") + " " + why
        }

    companion object {
        fun read(o: JsonObject) = PhoneAsk(
            action = o.needStr("action"),
            why = o.str("why"),
            reply = o.str("reply"),
            options = o.strings("options"),
            optionNotes = o.strings("optionNotes"),
            suggested = o.num("suggested"),
            links = o.arr("links")?.lossy { val l = it as JsonObject; PhoneAskLink(l.needStr("title"), l.needStr("url")) },
            ticked = o.bool("ticked"),
            run = o.obj("run")?.let { r ->
                PhoneAskRun(r.needStr("command"), r.needStr("buttonId"), r.str("cwd"), r.bool("running"), r.str("failed"))
            },
        )
    }
}

data class WokeTask(val kind: String, val description: String)

data class MessageButton(
    val id: String,
    val title: String,
    val kind: String,
    val target: String? = null,
    val pressedThreadId: String? = null,
    val failed: String? = null,
)

enum class Role { USER, ASSISTANT, TOOL }

data class PhoneMessage(
    val seq: Int,
    val role: Role,
    val text: String,
    val origin: String? = null,
    val toolKind: String? = null,
    val summary: Boolean = false,
    val turn: String? = null,
    val wokeBy: List<WokeTask>? = null,
    val buttons: List<MessageButton>? = null,
    val at: Instant,
    /** A Plan first plan in full (backend API 72): drawn as a plan card, never folded into the turn's working. */
    val plan: Boolean = false,
) {
    companion object {
        fun read(o: JsonObject): PhoneMessage = PhoneMessage(
            // Convex numbers are float64, so seq comes back as 12.0.
            seq = o.needNum("seq").wholeCount() ?: throw BadRow("Not a seq"),
            role = when (o.needStr("role")) {
                "user" -> Role.USER
                "assistant" -> Role.ASSISTANT
                "tool" -> Role.TOOL
                else -> throw BadRow("Unknown role")
            },
            text = o.needStr("text"),
            origin = o.str("origin"),
            toolKind = o.str("toolKind"),
            summary = o.bool("summary") ?: false,
            turn = o.str("turn"),
            wokeBy = o.arr("wokeBy")?.lossy { val w = it as JsonObject; WokeTask(w.needStr("kind"), w.str("description") ?: "") },
            buttons = o.arr("buttons")?.lossy { val b = it as JsonObject
                MessageButton(b.needStr("id"), b.needStr("title"), b.needStr("kind"), b.str("target"), b.str("pressedThreadId"), b.str("failed"))
            },
            at = o.needDate("at"),
            plan = o.bool("plan") ?: false,
        )
    }
}

data class MessageImage(
    val seq: Int,
    val src: String? = null,
    val pinned: Boolean? = null,
    val url: String,
    val width: Int? = null,
    val height: Int? = null,
) {
    companion object {
        fun read(o: JsonObject) = MessageImage(
            seq = o.needNum("seq").wholeCount() ?: throw BadRow("Not a seq"),
            src = o.str("src"),
            pinned = o.bool("pinned"),
            url = o.needStr("url"),
            width = o.num("width")?.wholeCount(),
            height = o.num("height")?.wholeCount(),
        )
    }
}

data class MessagePage(val messages: List<PhoneMessage>, val hasMore: Boolean, val images: List<MessageImage> = emptyList()) {
    companion object {
        fun read(o: JsonObject) = MessagePage(
            messages = o.arr("messages")?.lossy { PhoneMessage.read(it as JsonObject) } ?: throw BadRow("No messages"),
            hasMore = o.needBool("hasMore"),
            images = o.arr("images")?.lossy { MessageImage.read(it as JsonObject) } ?: emptyList(),
        )
    }
}

data class ThreadDetail(val thread: PhoneThread, val messages: List<PhoneMessage>) {
    companion object {
        fun read(o: JsonObject) = ThreadDetail(
            thread = PhoneThread.read(o.obj("thread") ?: throw BadRow("No thread")),
            messages = o.arr("messages")?.lossy { PhoneMessage.read(it as JsonObject) } ?: emptyList(),
        )
    }
}

data class PhoneRunner(
    val runnerId: String,
    val name: String,
    val version: String? = null,
    val lastSeenAt: Instant,
    val draining: Boolean? = null,
    val placed: Boolean? = null,
    val main: Boolean? = null,
) {
    fun isOnline(now: Instant): Boolean = now.toEpochMilli() - lastSeenAt.toEpochMilli() < OFFLINE_AFTER_MS

    companion object {
        /** A heartbeat every 60 s; three missed is offline. */
        const val OFFLINE_AFTER_MS = 180_000L

        fun read(o: JsonObject) = PhoneRunner(
            runnerId = o.needStr("runnerId"),
            name = o.needStr("name"),
            version = o.str("version"),
            lastSeenAt = o.needDate("lastSeenAt"),
            draining = o.bool("draining"),
            placed = o.bool("placed"),
            main = o.bool("main"),
        )
    }
}

data class PhoneIdentity(val name: String, val expiresAt: Instant?, val apiVersion: Int? = null, val pushOff: Boolean? = null) {
    val sendsNotifications get() = pushOff != true

    companion object {
        fun read(o: JsonObject) = PhoneIdentity(
            name = o.needStr("name"),
            expiresAt = o.date("expiresAt"),
            apiVersion = o.num("apiVersion")?.wholeCount(),
            pushOff = o.bool("pushOff"),
        )
    }
}

/** What the app relies on from the backend (BackendCompatibility). */
object BackendApi {
    const val MESSAGE_PAGES = 2
    const val ASKS = 14
    const val SECRETS = 35
    const val ARCHIVE_VERDICT = 42
    const val THREAD_CHANGES = 43
    const val WAKES = 45
    const val WAITING_SENDS = 57
    const val STOP = 69
    const val STARS = 27
    /** From 3 each phone chooses when it's notified (phone:setNotifications). */
    const val NOTIFICATION_SETTING = 3
    /** From 16 phone:search looks through archived threads past the newest 100. */
    const val SEARCH = 16
    /** From 19 agents file cards in the Pocket (phone:pocket). */
    const val POCKET = 19
    /** From 56 runners have an order (phone:setRunnerOrder). */
    const val RUNNER_ORDER = 56
    /** A message or new thread can carry photos (phone:imageUploadUrl). */
    const val PHOTOS = 18
    /** Accounts carry their models and phone:create takes an effort. */
    const val MODELS = 20
    const val IMAGES = 37
    const val BUTTONS = 59
    const val SLASH_COMMANDS = 62
    const val PLANS = 72
}

enum class CommandState { PENDING, TAKEN, DONE, FAILED }

data class CommandStatus(
    val state: CommandState,
    val note: String? = null,
    val threadId: String? = null,
    /** A slashCommands command's answer (API 62): phone:command's `result`. Anything that isn't a list of commands reads as none. */
    val commands: List<SlashCommand>? = null,
) {
    companion object {
        fun read(o: JsonObject) = CommandStatus(
            state = when (o.needStr("state")) {
                "pending" -> CommandState.PENDING
                "taken" -> CommandState.TAKEN
                "done" -> CommandState.DONE
                "failed" -> CommandState.FAILED
                else -> throw BadRow("Unknown state")
            },
            note = o.str("note"),
            threadId = o.str("threadId"),
            commands = o.arr("result")?.let { list ->
                runCatching { list.map { SlashCommand.read(it as JsonObject) } }.getOrNull()
            },
        )
    }
}

data class PhoneCounts(val hibernating: Int, val den: Int) {
    companion object {
        fun read(o: JsonObject) = PhoneCounts(o.num("hibernating")?.wholeCount() ?: 0, o.num("den")?.wholeCount() ?: 0)
    }
}

/** A row of phone:wakes: an active schedule and the thread it messages. */
data class ScheduleWake(val scheduleId: String, val threadId: String?, val nextRunAt: Instant?) {
    companion object {
        fun read(o: JsonObject) = ScheduleWake(o.needStr("scheduleId"), o.str("threadId"), o.date("nextRunAt"))
    }
}

enum class ThreadsView(val wire: String) { INBOX("inbox"), HIBERNATING("hibernating"), DEN("den") }

enum class ArchiveVerdict(val wire: String) { WORKED("worked"), DIDNT_WORK("didntWork"), NOT_NEEDED("notNeeded") }

sealed class ThreadChange {
    data class Snooze(val until: Instant) : ThreadChange()
    data object SnoozeUntilRestart : ThreadChange()
    data object Wake : ThreadChange()
    data object Archive : ThreadChange()
    data class ArchiveWithVerdict(val verdict: ArchiveVerdict) : ThreadChange()
    data object Unarchive : ThreadChange()
    data object MarkSeen : ThreadChange()
    data class Rename(val title: String) : ThreadChange()
    data class Star(val on: Boolean) : ThreadChange()

    val json: JsonElement
        get() = when (this) {
            is Snooze -> buildJsonObject { put("snooze", buildJsonObject { put("until", until.toString()) }) }
            SnoozeUntilRestart -> buildJsonObject { put("snooze", buildJsonObject { put("untilRestart", true) }) }
            Wake -> buildJsonObject { put("wake", true) }
            Archive -> buildJsonObject { put("archive", true) }
            is ArchiveWithVerdict -> buildJsonObject { put("archive", true); put("verdict", verdict.wire) }
            Unarchive -> buildJsonObject { put("unarchive", true) }
            MarkSeen -> buildJsonObject { put("markSeen", true) }
            is Rename -> buildJsonObject { put("title", title) }
            is Star -> buildJsonObject { put("star", on) }
        }
}

/** phone:waiting: a message from the phone the runner hasn't applied yet. */
data class WaitingSend(
    val commandId: String,
    val state: CommandState,
    val text: String,
    val photos: Int = 0,
    val createdAt: Double,
) {
    val id get() = commandId
    val canCancel get() = state == CommandState.PENDING

    companion object {
        fun read(o: JsonObject) = WaitingSend(
            commandId = o.needStr("commandId"),
            state = CommandStatus.read(o).state,
            text = o.needStr("text"),
            photos = o.num("photos")?.wholeCount() ?: 0,
            createdAt = o.needNum("createdAt"),
        )
    }
}

internal fun JsonPrimitive.stringOrNull(): String? = if (isString) content else null

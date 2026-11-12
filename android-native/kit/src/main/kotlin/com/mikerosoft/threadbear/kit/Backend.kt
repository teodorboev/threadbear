package com.mikerosoft.threadbear.kit

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.takeWhile
import kotlinx.coroutines.flow.transformWhile
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * What the app needs from Threadbear's Convex mirror (convex/phone.ts),
 * called with the phone token (Backend.swift's ConvexBackend). Reads are
 * live; writes are commands the owning runner picks up, each answering a
 * command id to watch.
 */
class PhoneBackend(private val transport: ConvexTransport, private val token: String) {
    val connected: StateFlow<Boolean> get() = transport.connected

    private fun args(vararg extra: Pair<String, JsonElement>): Map<String, JsonElement> =
        mapOf(*extra) + ("token" to JsonPrimitive(token))

    private fun <T> live(name: String, args: Map<String, JsonElement>, read: (JsonElement) -> T): Flow<T> =
        transport.subscribe(name, args).map { text ->
            try {
                read(parseJson(text))
            } catch (e: TransportError) {
                throw e
            } catch (e: Exception) {
                throw TransportError.Server("Couldn't read the server's answer: ${e.message}")
            }
        }

    /** A live query that can say "not found": that becomes null, and the flow ends. */
    private fun <T : Any> liveOrGone(name: String, args: Map<String, JsonElement>, read: (JsonObject) -> T): Flow<T?> =
        flow {
            try {
                live(name, args) { if (it is JsonNull) null else read(it as JsonObject) }.collect { emit(it) }
            } catch (_: TransportError.NotFound) {
                emit(null)
            }
        }

    private suspend fun command(name: String, args: Map<String, JsonElement>): String {
        val answer = parseJson(transport.mutation(name, args))
        return (answer as? JsonPrimitive)?.takeIf { it.isString }?.content
            ?: throw TransportError.Server("Couldn't read the server's answer")
    }

    suspend fun me(): PhoneIdentity =
        PhoneIdentity.read(parseJson(transport.query("phone:me", args())) as? JsonObject ?: throw TransportError.Server("No identity"))

    fun runners(): Flow<List<PhoneRunner>> = live("phone:runners", args()) { it.lossyList(PhoneRunner::read) }

    fun projects(): Flow<List<PhoneProject>> = live("phone:projects", args()) { it.lossyList(PhoneProject::read) }

    fun threads(view: ThreadsView): Flow<List<PhoneThread>> =
        live("phone:threads", args("view" to JsonPrimitive(view.wire))) { it.lossyList(PhoneThread::read) }

    fun counts(): Flow<PhoneCounts> = live("phone:counts", args()) { PhoneCounts.read(it as JsonObject) }

    fun wakes(): Flow<List<ScheduleWake>> = live("phone:wakes", args()) { it.lossyList(ScheduleWake::read) }

    /**
     * The inbox, read whole once and then only the rows written since
     * (backend API 43), so a phone left open doesn't read the whole list for
     * every row a runner writes.
     */
    fun inboxChanges(): Flow<List<PhoneThread>> = flow {
        suspend fun readAt(): InboxChanges {
            val o = parseJson(transport.query("phone:threadsAt", args("view" to JsonPrimitive(ThreadsView.INBOX.wire)))) as JsonObject
            return InboxChanges.fresh(o.arr("rows")?.lossy { PhoneThread.read(it as JsonObject) } ?: emptyList(), o.needNum("at"))
        }
        var list = readAt()
        emit(list.rows)
        while (true) {
            val since = list.since
            var reload = false
            var ended = true
            transport.subscribe("phone:threadsSince", args("since" to JsonPrimitive(since)))
                .transformWhile { text ->
                    val answer = ThreadsSinceAnswer.read(parseJson(text) as JsonObject)
                    if (answer.more) {
                        reload = true
                        return@transformWhile false
                    }
                    list = list.merge(answer)
                    emit(list.rows)
                    // A new place to ask from is a new subscription.
                    if (list.since != since) { ended = false; false } else true
                }
                .collect { emit(it) }
            if (reload) {
                // After the margin: loading now would hear `more` again.
                delay(InboxChanges.MARGIN_MS.toLong() + 1000)
                list = readAt()
                emit(list.rows)
            } else if (ended && list.since == since) {
                // The subscription ended with nothing new: nothing more will come.
                break
            }
        }
    }

    /** Null once the thread's gone from the mirror. */
    fun thread(threadId: String): Flow<ThreadDetail?> =
        liveOrGone("phone:thread", args("threadId" to JsonPrimitive(threadId)), ThreadDetail::read)

    /** The newest `limit` messages, tool lines included, live (backend API 2). */
    fun messages(threadId: String, limit: Int): Flow<MessagePage> = live(
        "phone:messages",
        args("threadId" to JsonPrimitive(threadId), "limit" to JsonPrimitive(limit), "tools" to JsonPrimitive(true)),
    ) { MessagePage.read(it as JsonObject) }

    suspend fun olderMessages(threadId: String, beforeSeq: Int, limit: Int): MessagePage {
        val text = transport.query(
            "phone:messages",
            args("threadId" to JsonPrimitive(threadId), "beforeSeq" to JsonPrimitive(beforeSeq), "limit" to JsonPrimitive(limit), "tools" to JsonPrimitive(true)),
        )
        return MessagePage.read(parseJson(text) as JsonObject)
    }

    fun command(commandId: String): Flow<CommandStatus?> =
        liveOrGone("phone:command", args("commandId" to JsonPrimitive(commandId)), CommandStatus::read)

    /** The thread's messages from the phone its runner hasn't taken yet (backend API 57). */
    fun waiting(threadId: String): Flow<List<WaitingSend>> =
        live("phone:waiting", args("threadId" to JsonPrimitive(threadId))) { it.lossyList(WaitingSend::read) }

    /** Takes back a waiting message. False once the runner has it. */
    suspend fun cancel(commandId: String): Boolean =
        (parseJson(transport.mutation("phone:cancel", args("commandId" to JsonPrimitive(commandId)))) as? JsonPrimitive)?.content == "true"

    suspend fun send(threadId: String, text: String, answers: List<AskAnswer> = emptyList(), images: List<String> = emptyList()): String {
        val a = mutableListOf<Pair<String, JsonElement>>("threadId" to JsonPrimitive(threadId), "text" to JsonPrimitive(text))
        // Only when there are some: an older backend refuses the fields.
        if (images.isNotEmpty()) a += "images" to JsonArray(images.map(::JsonPrimitive))
        if (answers.isNotEmpty()) a += "answers" to JsonArray(answers.map {
            buildJsonObject { put("action", it.action); put("answer", it.answer) }
        })
        return command("phone:send", args(*a.toTypedArray()))
    }

    /** Stop (backend API 69): ends the thread's running turn. */
    suspend fun interrupt(threadId: String): String = command("phone:interrupt", args("threadId" to JsonPrimitive(threadId)))

    suspend fun respond(threadId: String, approvalId: String, decision: ApprovalOption): String = command(
        "phone:respond",
        args("threadId" to JsonPrimitive(threadId), "approvalId" to JsonPrimitive(approvalId), "decision" to JsonPrimitive(decision.wire)),
    )

    suspend fun answer(threadId: String, approvalId: String, answers: Map<String, String>, summary: String): String = command(
        "phone:answer",
        args(
            "threadId" to JsonPrimitive(threadId), "approvalId" to JsonPrimitive(approvalId),
            "answers" to JsonObject(answers.mapValues { JsonPrimitive(it.value) }), "message" to JsonPrimitive(summary),
        ),
    )

    /** A secret card's answer (backend API 35): sealed to the runner's key, or declined. */
    suspend fun secret(threadId: String, approvalId: String, answer: SecretAnswer): String {
        val extra = when (answer) {
            is SecretAnswer.Sealed -> "sealed" to buildJsonObject {
                put("epk", answer.sealed.epk); put("nonce", answer.sealed.nonce); put("box", answer.sealed.box)
            }
            SecretAnswer.Decline -> "decline" to JsonPrimitive(true)
        }
        return command("phone:secret", args("threadId" to JsonPrimitive(threadId), "approvalId" to JsonPrimitive(approvalId), extra))
    }

    suspend fun press(threadId: String, buttonId: String): String =
        command("phone:press", args("threadId" to JsonPrimitive(threadId), "buttonId" to JsonPrimitive(buttonId)))

    suspend fun update(threadId: String, change: ThreadChange): String =
        command("phone:update", args("threadId" to JsonPrimitive(threadId), "change" to change.json))

    suspend fun create(text: String, runnerId: String? = null, cwd: String? = null): String {
        val a = mutableListOf<Pair<String, JsonElement>>("text" to JsonPrimitive(text))
        runnerId?.let { a += "runnerId" to JsonPrimitive(it) }
        cwd?.let { a += "cwd" to JsonPrimitive(it) }
        return command("phone:create", args(*a.toTypedArray()))
    }

    /**
     * API 37: pictures of rows read once, from `from` on. A picture can
     * finish uploading after its row went into a chunk or slid off the live
     * page, and neither is read again.
     */
    fun images(threadId: String, from: Int): Flow<List<MessageImage>> =
        live("phone:images", args("threadId" to JsonPrimitive(threadId), "from" to JsonPrimitive(from))) { it.lossyList(MessageImage::read) }

    /** Asks the thread's runner for its slash commands (API 62): the list comes back as the command's result. */
    suspend fun slashCommands(threadId: String): String = command("phone:slashCommands", args("threadId" to JsonPrimitive(threadId)))

    /** Before API 2: asks the runner to page older messages into phone:thread. */
    suspend fun loadEarlier(threadId: String, beforeSeq: Int): String =
        command("phone:loadEarlier", args("threadId" to JsonPrimitive(threadId), "beforeSeq" to JsonPrimitive(beforeSeq)))

    suspend fun registerDevice(pushToken: String, environment: String, platform: String, pushKey: String?) {
        val a = mutableListOf<Pair<String, JsonElement>>(
            "apnsToken" to JsonPrimitive(pushToken), "environment" to JsonPrimitive(environment), "platform" to JsonPrimitive(platform),
        )
        pushKey?.let { a += "pushKey" to JsonPrimitive(it) }
        transport.mutation("phone:registerDevice", args(*a.toTypedArray()))
    }

    // New threads and photos (NewThreadScreen, the composers' photos).

    /** Every runner's logins with the models they offer (backend API 20), for the new thread pickers. */
    fun accounts(): Flow<List<PhoneAccount>> = live("phone:accounts", args()) { it.lossyList(PhoneAccount::read) }

    /** `settings` (backend API 20 for an effort): the model and effort it starts on; `images` (API 18) its photos. */
    suspend fun create(text: String, runnerId: String?, cwd: String?, images: List<String>, settings: NewThreadSettings): String {
        val a = mutableListOf<Pair<String, JsonElement>>("text" to JsonPrimitive(text))
        settings.model?.let { a += "model" to JsonPrimitive(it) }
        settings.effort?.let { a += "effort" to JsonPrimitive(it) }
        // Only when there are some: a backend before API 18 refuses the field.
        if (images.isNotEmpty()) a += "images" to JsonArray(images.map(::JsonPrimitive))
        runnerId?.let { a += "runnerId" to JsonPrimitive(it) }
        cwd?.let { a += "cwd" to JsonPrimitive(it) }
        return command("phone:create", args(*a.toTypedArray()))
    }

    /** A photo's bytes into file storage (backend API 18): its storage id, for send or create. */
    suspend fun uploadPhoto(jpeg: ByteArray, post: PhotoPost = HttpPhotoPost, progress: (Float) -> Unit = {}): String {
        val url = (parseJson(transport.mutation("phone:imageUploadUrl", args())) as? JsonPrimitive)?.takeIf { it.isString }?.content
            ?: throw TransportError.Server("Couldn't upload the photo.")
        val body = post.post(url, jpeg, progress)
        val answer = runCatching { parseJson(body) as? JsonObject }.getOrNull()
        return answer?.str("storageId") ?: throw TransportError.Server("Couldn't upload the photo.")
    }

    /** Takes back a photo that was uploaded and then never sent (backend API 25). */
    suspend fun discardPhoto(storageId: String) {
        transport.mutation("phone:discardImage", args("storageId" to JsonPrimitive(storageId)))
    }

    /** Archived threads past the den's newest 100 with these words in (backend API 16). */
    suspend fun search(text: String): List<PhoneThread> =
        parseJson(transport.query("phone:search", args("text" to JsonPrimitive(text)))).lossyList(PhoneThread::read)

    /** The Pocket's cards (backend API 19), live. */
    fun pocket(): Flow<List<PocketCard>> = live("phone:pocket", args()) { it.lossyList(PocketCard::read) }

    /** Straight to the table, not a command: the Mac may well be asleep. */
    suspend fun removePocketCard(id: String) {
        transport.mutation("phone:removePocketCard", args("id" to JsonPrimitive(id)))
    }

    /** When this phone is notified (backend API 3), by its push token. */
    suspend fun setNotifications(pushToken: String, notify: NotifyMode) {
        transport.mutation("phone:setNotifications", args("apnsToken" to JsonPrimitive(pushToken), "notify" to JsonPrimitive(notify.wire)))
    }

    /** New threads start on the first of these that's online (backend API 56). */
    suspend fun setRunnerOrder(runnerIds: List<String>) {
        transport.mutation("phone:setRunnerOrder", args("runnerIds" to JsonArray(runnerIds.map(::JsonPrimitive))))
    }
}

/** phone:threadsSince: rows written since, in any view, and threads gone. */
internal data class ThreadsSinceAnswer(val rows: List<Row>, val gone: List<String>, val more: Boolean) {
    data class Row(val thread: PhoneThread, val view: String, val updatedMs: Double)

    companion object {
        fun read(o: JsonObject) = ThreadsSinceAnswer(
            rows = o.arr("rows")?.lossy { val r = it as JsonObject; Row(PhoneThread.read(r), r.needStr("view"), r.needNum("updatedMs")) } ?: emptyList(),
            gone = o.strings("gone") ?: emptyList(),
            more = o.bool("more") ?: false,
        )
    }
}

/** The inbox as a phone holds it while following phone:threadsSince. */
internal data class InboxChanges(val rows: List<PhoneThread>, val since: Double) {
    fun merge(answer: ThreadsSinceAnswer): InboxChanges {
        val byId = LinkedHashMap<String, PhoneThread>()
        rows.forEach { byId[it.threadId] = it }
        var since = this.since
        for (row in answer.rows) {
            if (row.view == ThreadsView.INBOX.wire) byId[row.thread.threadId] = row.thread else byId.remove(row.thread.threadId)
            since = maxOf(since, row.updatedMs - MARGIN_MS)
        }
        answer.gone.forEach { byId.remove(it) }
        return InboxChanges(byId.values.sortedByDescending { it.lastActivityAt }, since)
    }

    companion object {
        /** A row can commit just after one stamped later, so ask from a little before. */
        const val MARGIN_MS = 5000.0

        fun fresh(rows: List<PhoneThread>, at: Double) = InboxChanges(rows, at - MARGIN_MS)
    }
}

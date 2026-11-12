package com.mikerosoft.threadbear.kit

import java.io.File
import kotlin.math.abs
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * A card in the Pocket (backend API 19): something an agent filed for when
 * Mike's out, from the `pocket` tool (Pocket.swift). `id` is its title in
 * lower case.
 */
data class PocketCard(
    val id: String,
    val title: String,
    val text: String,
    /** What to search the map for, to tell whether the card applies here. */
    val place: String? = null,
    /** Which picture and colour it has (PocketIcon). */
    val icon: String? = null,
    val threadId: String? = null,
    /** The project or thread it came from. */
    val source: String? = null,
    val updatedAt: String,
) {
    val json: JsonObject
        get() = buildJsonObject {
            put("id", id); put("title", title); put("text", text)
            place?.let { put("place", it) }
            icon?.let { put("icon", it) }
            threadId?.let { put("threadId", it) }
            source?.let { put("source", it) }
            put("updatedAt", updatedAt)
        }

    companion object {
        fun read(o: JsonObject) = PocketCard(
            id = o.needStr("id"),
            title = o.needStr("title"),
            text = o.needStr("text"),
            place = o.str("place"),
            icon = o.str("icon"),
            threadId = o.str("threadId"),
            source = o.str("source"),
            updatedAt = o.needStr("updatedAt"),
        )
    }
}

object PocketDeck {
    /**
     * Back of the deck to front. The front card is the one shown whole, so
     * that's where a card for here goes, then the newest.
     */
    fun ordered(cards: List<PocketCard>, here: Set<String> = emptySet()): List<PocketCard> =
        cards.sortedWith(compareBy<PocketCard> { it.id in here }.thenBy { it.updatedAt }.thenBy { it.id })

    /**
     * Whether a card has been filed or changed since the Pocket was last
     * open. A changed card counts: the librarian updates a card when the facts
     * change, and that's as worth a look as a new one. Nothing seen yet means
     * every card is new.
     */
    fun hasNew(cards: List<PocketCard>, seenUpTo: String?): Boolean {
        if (seenUpTo == null) return cards.isNotEmpty()
        return cards.any { it.updatedAt > seenUpTo }
    }

    /**
     * What `seenUpTo` becomes once these cards have been looked at. It only
     * goes forward: removing the newest card mustn't make an older one new.
     */
    fun seenUpTo(cards: List<PocketCard>, after: String?): String? =
        (listOfNotNull(after) + cards.map { it.updatedAt }).maxOrNull()
}

/**
 * The cards as last seen, in a file, as the Pocket is for places with no
 * signal: the app has them before it's connected to anything. The app puts
 * the file in noBackupFilesDir: these can be addresses and booking codes.
 */
class PocketStore(private val file: File) {
    fun load(): List<PocketCard> = try {
        parseJson(file.readText()).lossyList(PocketCard::read)
    } catch (_: Exception) {
        emptyList()
    }

    fun save(cards: List<PocketCard>) {
        try {
            file.parentFile?.mkdirs()
            // Written beside and moved over, so a crash mid-write can't leave half a file.
            val next = File(file.parentFile, file.name + ".tmp")
            next.writeText(JsonArray(cards.map { it.json }).toString())
            if (!next.renameTo(file)) { file.delete(); next.renameTo(file) }
        } catch (_: Exception) {
            // The live list still shows; only the copy for no signal is missed.
        }
    }

    fun clear() { file.delete() }
}

/**
 * A card's icon and colour, from the name its agent chose
 * (POCKET_ICONS in runner/src/pocket-icons.ts). The colour goes with the
 * icon, so the same kind of card always looks the same. `name` is the
 * runner's name for it, which the app maps to a drawn icon.
 */
data class PocketIcon(val name: String, val red: Int, val green: Int, val blue: Int) {
    /** 0xFFRRGGBB, for a Compose Color. */
    val argb: Long get() = 0xFF000000L or (red.toLong() shl 16) or (green.toLong() shl 8) or blue.toLong()

    companion object {
        /**
         * No icon, or one this version doesn't know: a card from an older agent
         * or a newer runner still looks like a card.
         */
        val PLAIN = PocketIcon("plain", 150, 160, 190)

        private val NAMED = mapOf(
            "plane" to PocketIcon("plane", 127, 178, 240),
            "parcel" to PocketIcon("parcel", 217, 143, 176),
            "shopping" to PocketIcon("shopping", 124, 196, 160),
            "tools" to PocketIcon("tools", 224, 164, 88),
            "food" to PocketIcon("food", 232, 134, 100),
            "car" to PocketIcon("car", 96, 190, 200),
            "health" to PocketIcon("health", 226, 110, 120),
            "ticket" to PocketIcon("ticket", 170, 140, 232),
            "money" to PocketIcon("money", 214, 196, 96),
            "home" to PocketIcon("home", 160, 190, 120),
        )

        fun named(name: String?): PocketIcon = name?.let { NAMED[it] } ?: PLAIN

        val names: Set<String> get() = NAMED.keys
    }
}

/**
 * When a swipe takes a card out: far enough left, or flicked. Up and down
 * is the deck scrolling, so a drag has to be mostly sideways to count.
 * In dp, as the iPhone's points.
 */
object PocketSwipe {
    const val REMOVE_AT = 110.0
    const val FLICK_AT = 240.0

    fun isSideways(width: Double, height: Double) = abs(width) > abs(height) * 1.5

    fun removes(width: Double, predictedWidth: Double) =
        width <= -REMOVE_AT || (width < 0 && predictedWidth <= -FLICK_AT)
}

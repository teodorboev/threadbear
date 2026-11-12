package com.mikerosoft.threadbear.kit

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull

// Reading the backend's rows by hand, leniently, as ThreadbearKit's Codable
// types do: a required field missing or the wrong type fails that row, an
// optional one the wrong type reads as absent, and a list skips the rows it
// can't read rather than failing whole (LossyList).

/** A row this build can't read. */
class BadRow(message: String) : Exception(message)

val lenientJson = Json { ignoreUnknownKeys = true; isLenient = true }

fun parseJson(text: String): JsonElement = lenientJson.parseToJsonElement(text)

fun JsonElement?.asObject(): JsonObject? = this as? JsonObject
fun JsonElement?.asArray(): JsonArray? = this as? JsonArray

fun JsonObject.str(key: String): String? =
    (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content

fun JsonObject.num(key: String): Double? =
    (this[key] as? JsonPrimitive)?.takeIf { !it.isString }?.doubleOrNull?.takeIf { it.isFinite() }

fun JsonObject.bool(key: String): Boolean? =
    (this[key] as? JsonPrimitive)?.takeIf { !it.isString }?.booleanOrNull

fun JsonObject.obj(key: String): JsonObject? = this[key] as? JsonObject
fun JsonObject.arr(key: String): JsonArray? = this[key] as? JsonArray

fun JsonObject.needStr(key: String): String = str(key) ?: throw BadRow("No $key")
fun JsonObject.needNum(key: String): Double = num(key) ?: throw BadRow("No $key")
fun JsonObject.needBool(key: String): Boolean = bool(key) ?: throw BadRow("No $key")
fun JsonObject.isNull(key: String): Boolean = this[key] == null || this[key] is JsonNull

/** Strings in a list, skipping anything else. */
fun JsonObject.strings(key: String): List<String>? =
    arr(key)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }

/** Every item `read` can make sense of; the rest are skipped. */
fun <T> JsonArray.lossy(read: (JsonElement) -> T): List<T> = mapNotNull {
    try {
        read(it)
    } catch (_: Exception) {
        null
    }
}

fun <T> JsonElement.lossyList(read: (JsonObject) -> T): List<T> =
    (this as? JsonArray ?: throw BadRow("Not a list")).lossy { read(it as? JsonObject ?: throw BadRow("Not an object")) }

/** A JSON number as a whole count from 0 up, or null (SafeNumbers.wholeCount). */
fun Double.wholeCount(atMost: Double = 9_007_199_254_740_991.0): Int? {
    if (!isFinite() || this < 0 || this > atMost || this != Math.floor(this)) return null
    return if (this > Int.MAX_VALUE) null else toInt()
}

/** Never throws: for words on screen. */
fun Double.saturatingInt(): Int = when {
    isNaN() -> 0
    this >= Int.MAX_VALUE -> Int.MAX_VALUE
    this <= Int.MIN_VALUE -> Int.MIN_VALUE
    else -> toInt()
}

/** Arguments as the Convex FFI takes them: each value as JSON text. */
fun jsonArgs(args: Map<String, JsonElement>): Map<String, String> = args.mapValues { it.value.toString() }

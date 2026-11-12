package com.mikerosoft.threadbear.kit

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Whether there's a newer Android app than this one (docs/android-native.md,
 * "Releases"): each release/YYYY-MM-DD tag's GitHub release carries
 * threadbear-android.apk, and GitHub's "latest release" is the newest. A
 * build that isn't a release (no date) never offers one, so a debug build
 * on Mike's Pixel doesn't nag him to replace itself.
 */
object AppUpdate {
    const val ASSET = "threadbear-android.apk"

    data class Available(val date: String, val apkUrl: String, val pageUrl: String)

    private val DATE = Regex("^\\d{4}-\\d{2}-\\d{2}$")

    /**
     * GitHub's answer for releases/latest, as text: the newer release, or
     * null. Only the tag and whether the APK is attached are taken from it:
     * the download and page addresses are made here from the repo and the
     * tag, so an odd answer can't send the phone anywhere else. Anything
     * the wrong shape is null, never a throw.
     */
    fun newer(latestJson: String, installedDate: String?, repo: String): Available? = try {
        newerOrThrow(latestJson, installedDate, repo)
    } catch (_: Exception) {
        null
    }

    private fun newerOrThrow(latestJson: String, installedDate: String?, repo: String): Available? {
        val mine = installedDate?.takeIf { DATE.matches(it) } ?: return null
        if (!REPO.matches(repo)) return null
        val o = Json.parseToJsonElement(latestJson) as? JsonObject ?: return null
        val tag = (o["tag_name"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return null
        val date = tag.removePrefix("release/").takeIf { tag.startsWith("release/") && DATE.matches(it) } ?: return null
        // Dates compare as text: yyyy-MM-dd sorts in time order.
        if (date <= mine) return null
        // The Mac's daily tag can come before the workflow has attached the APK.
        val attached = (o["assets"] as? JsonArray)?.any { a -> ((a as? JsonObject)?.get("name") as? JsonPrimitive)?.content == ASSET } == true
        if (!attached) return null
        val base = "https://github.com/$repo/releases"
        return Available(date, "$base/download/$tag/$ASSET", "$base/tag/$tag")
    }

    // owner/name, each starting with a letter or digit, so "." and ".." can't
    // walk out of the repo's path in the addresses made from it.
    private val REPO = Regex("^[A-Za-z0-9][A-Za-z0-9_.-]{0,99}/[A-Za-z0-9][A-Za-z0-9_.-]{0,99}$")

    /** Where to ask: GitHub's API for this repo's latest release. */
    fun latestUrl(repo: String): String? =
        repo.takeIf { REPO.matches(it) }?.let { "https://api.github.com/repos/$it/releases/latest" }
}

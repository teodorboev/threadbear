package com.mikerosoft.threadbear.kit

import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Opens what the backend sealed for this phone (convex/pushSeal.ts seal, the
 * iPhone's PushSeal): AES-256-GCM, a 12-byte nonce then ciphertext then the
 * 16-byte tag, base64, around {"title","body"}. The relay only ever sees the
 * sealed text (docs/push-relay.md). Nil when it doesn't open, and the app
 * shows its stand-in instead.
 */
object PushSeal {
    data class Opened(val title: String, val body: String)

    fun open(sealed: String, key: ByteArray): Opened? = try {
        val bytes = Base64.getDecoder().decode(sealed)
        if (key.size != 32 || bytes.size < 12 + 16) null else {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, bytes, 0, 12))
            val plain = cipher.doFinal(bytes, 12, bytes.size - 12)
            val o = Json.parseToJsonElement(String(plain, Charsets.UTF_8)) as? JsonObject
            val title = o?.get("title")?.jsonPrimitive?.takeIf { it.isString }?.content
            val body = o?.get("body")?.jsonPrimitive?.takeIf { it.isString }?.content
            if (title != null && body != null) Opened(title, body) else null
        }
    } catch (_: Exception) {
        null
    }
}

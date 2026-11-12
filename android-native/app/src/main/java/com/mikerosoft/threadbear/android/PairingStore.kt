package com.mikerosoft.threadbear.android

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import com.mikerosoft.threadbear.kit.Pairing
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The pairing (server and phone token), kept encrypted with a key in the
 * Android Keystore that never leaves it, as the iPhone keeps it in the
 * Keychain. Runner keys for secrets are kept per server beside it.
 */
class PairingStore(context: Context) {
    private val prefs = context.getSharedPreferences("pairing", Context.MODE_PRIVATE)

    fun load(): Pairing? {
        val url = prefs.getString("url", null) ?: return null
        val sealed = prefs.getString("token", null) ?: return null
        val token = runCatching { open(sealed) }.getOrNull() ?: return null
        return Pairing(url, token)
    }

    fun save(pairing: Pairing) {
        val editor = prefs.edit().putString("url", pairing.deploymentUrl).putString("token", seal(pairing.token))
        for ((runnerId, key) in pairing.runnerKeys) editor.putString(keyName(pairing.serverName, runnerId), key)
        editor.apply()
    }

    /** Whether there's a pairing, without opening the token (the push service asks). */
    fun paired(): Boolean = prefs.contains("url") && prefs.contains("token")

    fun clear() {
        prefs.edit().remove("url").remove("token").apply()
    }

    /** The key kept for a runner on a server: secrets are sealed to it (slice 2). */
    fun runnerKey(server: String, runnerId: String): String? = prefs.getString(keyName(server, runnerId), null)

    private fun keyName(server: String, runnerId: String) = "runnerKey:$server:$runnerId"

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return gen.generateKey()
    }

    private fun seal(text: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val out = cipher.iv + cipher.doFinal(text.toByteArray())
        return Base64.encodeToString(out, Base64.NO_WRAP)
    }

    private fun open(sealed: String): String {
        val bytes = Base64.decode(sealed, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes, 0, 12))
        return String(cipher.doFinal(bytes, 12, bytes.size - 12))
    }

    private companion object {
        const val ALIAS = "threadbear-pairing"
    }
}

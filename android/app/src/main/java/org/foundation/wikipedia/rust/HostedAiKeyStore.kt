package org.foundation.wikipedia.rust

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.hostedAiKeyIsValid

/** Secure key storage failed; no key was saved or read. Never carries key text. */
class HostedAiKeyStorageException(message: String) : Exception(message)

/**
 * App-owned hosted-AI API keys (AI-003 owner decision: keys live in OS secret
 * storage and Rust never stores them).
 *
 * Each key is encrypted with AES-256-GCM under a non-exportable key in the
 * Android Keystore and written to `noBackupFilesDir`, so it is excluded from
 * cloud backup and device transfer. The provider name is bound as associated
 * data, so one provider's file cannot be replayed as another's. There is no
 * plain-text fallback: if the Keystore is unavailable, saving fails.
 *
 * All methods block; call them off the main thread.
 */
class HostedAiKeyStore(context: Context) {
    private val dir = File(context.applicationContext.noBackupFilesDir, "hosted-ai-keys")

    /** Validate (printable ASCII, no whitespace) and store [apiKey] for [provider]. */
    fun save(provider: HostedAiProvider, apiKey: String) {
        requireOffMainThread()
        val key = apiKey.trim()
        require(hostedAiKeyIsValid(key)) { "That does not look like a valid API key" }
        val sealed = try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
            cipher.updateAAD(aad(provider))
            val iv = cipher.iv
            val ciphertext = cipher.doFinal(key.toByteArray(Charsets.US_ASCII))
            ByteBuffer.allocate(2 + iv.size + ciphertext.size)
                .put(FORMAT).put(iv.size.toByte()).put(iv).put(ciphertext).array()
        } catch (error: GeneralSecurityException) {
            throw HostedAiKeyStorageException(
                "Secure key storage is unavailable on this device; the key was not saved.",
            )
        }
        writeAtomically(file(provider), sealed)
    }

    fun isConfigured(provider: HostedAiProvider): Boolean {
        requireOffMainThread()
        return file(provider).isFile
    }

    /** Returns whether a key existed. */
    fun delete(provider: HostedAiProvider): Boolean {
        requireOffMainThread()
        val target = file(provider)
        if (!target.exists()) return false
        if (!target.delete()) {
            throw HostedAiKeyStorageException("The saved key could not be deleted.")
        }
        return true
    }

    /** Decrypt the stored key for one explicit request; null if none is saved. */
    internal fun load(provider: HostedAiProvider): String? {
        requireOffMainThread()
        val target = file(provider)
        if (!target.isFile) return null
        try {
            val bytes = target.readBytes()
            val buffer = ByteBuffer.wrap(bytes)
            if (bytes.size < 2 + 12 + 16 || buffer.get() != FORMAT) throw unreadable()
            val iv = ByteArray(buffer.get().toInt()).also { buffer.get(it) }
            val ciphertext = ByteArray(buffer.remaining()).also { buffer.get(it) }
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, existingWrappingKey() ?: throw unreadable(),
                GCMParameterSpec(TAG_BITS, iv))
            cipher.updateAAD(aad(provider))
            return String(cipher.doFinal(ciphertext), Charsets.US_ASCII)
        } catch (error: GeneralSecurityException) {
            throw unreadable()
        } catch (error: RuntimeException) {
            if (error is HostedAiKeyStorageException) throw error
            throw unreadable()
        }
    }

    private fun unreadable() = HostedAiKeyStorageException(
        "The saved key could not be read from secure storage. Save it again.",
    )

    private fun file(provider: HostedAiProvider) = File(dir, "${provider.name.lowercase()}.key")

    private fun aad(provider: HostedAiProvider) =
        "org.foundation.wikipedia/hosted-ai/${provider.name}".toByteArray(Charsets.US_ASCII)

    private fun keyStore(): KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    private fun existingWrappingKey(): SecretKey? =
        (keyStore().getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey

    private fun wrappingKey(): SecretKey = synchronized(HostedAiKeyStore::class.java) {
        existingWrappingKey() ?: KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE,
        ).apply {
            init(
                KeyGenParameterSpec.Builder(
                    ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setRandomizedEncryptionRequired(true)
                    .build(),
            )
        }.generateKey()
    }

    /** Write via a synced temporary file and rename, so a crash never leaves a torn key. */
    private fun writeAtomically(target: File, bytes: ByteArray) {
        if (!dir.isDirectory && !dir.mkdirs()) {
            throw HostedAiKeyStorageException("The key could not be saved.")
        }
        val temp = File(dir, "${target.name}.tmp")
        try {
            FileOutputStream(temp).use { out ->
                out.write(bytes)
                out.fd.sync()
            }
            if (!temp.renameTo(target)) throw java.io.IOException("rename failed")
        } catch (error: java.io.IOException) {
            temp.delete()
            throw HostedAiKeyStorageException("The key could not be saved.")
        }
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "org.foundation.wikipedia.hosted-ai-keys.v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val TAG_BITS = 128
        const val FORMAT: Byte = 1
    }
}

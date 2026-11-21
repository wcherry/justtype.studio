package studio.justtype.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import studio.justtype.core.api.TokenStore
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The session at rest, sealed with an AES-256-GCM key that lives in the Android Keystore and never
 * leaves it — the Android counterpart of the iOS Keychain. One file per item,
 * `[12-byte IV][ciphertext+tag]`, in `noBackupFilesDir`, written atomically.
 */
class SecureTokenStore(context: Context) : TokenStore {
    private val dir = File(context.noBackupFilesDir, "secrets").apply { mkdirs() }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    @Synchronized
    override fun read(key: String): String? {
        val file = File(dir, "$key.bin")
        if (!file.exists()) return null
        return try {
            val data = file.readBytes()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, data, 0, IV_BYTES))
            cipher.doFinal(data, IV_BYTES, data.size - IV_BYTES).decodeToString()
        } catch (e: Exception) {
            // An item that will not open is gone for good; absent routes the writer to sign in.
            Log.e(TAG, "could not open $key: ${e.javaClass.simpleName}")
            null
        }
    }

    @Synchronized
    override fun write(key: String, value: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.iv + cipher.doFinal(value.encodeToByteArray())
        val tmp = File(dir, "$key.tmp")
        tmp.writeBytes(sealed)
        if (!tmp.renameTo(File(dir, "$key.bin"))) Log.e(TAG, "could not save $key")
    }

    @Synchronized
    override fun delete(key: String) {
        File(dir, "$key.bin").delete()
    }

    private companion object {
        const val TAG = "SecureTokenStore"
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "justtype.session"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
    }
}

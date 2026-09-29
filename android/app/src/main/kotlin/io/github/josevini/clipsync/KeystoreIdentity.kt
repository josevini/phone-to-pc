package io.github.josevini.clipsync

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import io.github.josevini.clipsync.session.Identity
import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.util.Date
import java.util.GregorianCalendar
import javax.security.auth.x500.X500Principal

/**
 * This device's identity in the Android Keystore (spec §2): an EC P-256 key that never leaves it, and the
 * self-signed certificate the Keystore issues for it. The device ID is the hash of that key, so it lasts as long as
 * the key: until the app's data is cleared.
 */
object KeystoreIdentity {
    const val ALIAS = "clipsync-identity"
    private const val PROVIDER = "AndroidKeyStore"

    fun loadOrCreate(alias: String = ALIAS): Identity {
        val keyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }
        if (!keyStore.containsAlias(alias)) generate(alias)
        val key = keyStore.getKey(alias, null) as PrivateKey
        return Identity(key, keyStore.getCertificate(alias) as X509Certificate)
    }

    fun delete(alias: String) {
        KeyStore.getInstance(PROVIDER).apply { load(null) }.deleteEntry(alias)
    }

    private fun generate(alias: String) {
        // Peers ignore everything in the certificate but the key: the subject and validity are placeholders.
        val spec =
            KeyGenParameterSpec
                .Builder(alias, KeyProperties.PURPOSE_SIGN)
                .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                // TLS 1.3 signs SHA-256 digests; some providers hand the Keystore the digest itself.
                .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_NONE)
                .setCertificateSubject(X500Principal("CN=clipsync"))
                .setCertificateSerialNumber(BigInteger.ONE)
                .setCertificateNotBefore(Date(0))
                .setCertificateNotAfter(GregorianCalendar(2099, 11, 31).time)
                .build()
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, PROVIDER).run {
            initialize(spec)
            generateKeyPair()
        }
    }
}

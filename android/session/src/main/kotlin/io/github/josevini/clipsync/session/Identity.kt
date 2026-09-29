package io.github.josevini.clipsync.session

import io.github.josevini.clipsync.core.deviceIdFromSpki
import java.security.PrivateKey
import java.security.cert.X509Certificate

/**
 * This device's TLS identity: an EC P-256 key and a self-signed certificate for it (spec §2).
 *
 * On Android the key lives in the Keystore and never leaves it; this class only holds a handle to it.
 */
class Identity(
    val privateKey: PrivateKey,
    val certificate: X509Certificate,
) {
    /** SHA-256 of the certificate's SubjectPublicKeyInfo, as hex. */
    val id: String = deviceIdFromSpki(certificate.publicKey.encoded)
}

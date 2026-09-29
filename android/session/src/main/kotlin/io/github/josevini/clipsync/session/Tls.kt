package io.github.josevini.clipsync.session

import io.github.josevini.clipsync.core.alpn
import io.github.josevini.clipsync.core.deviceIdFromSpki
import java.net.InetSocketAddress
import java.net.Socket
import java.security.Principal
import java.security.PrivateKey
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.SSLParameters
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509ExtendedKeyManager
import javax.net.ssl.X509ExtendedTrustManager

/** ALPN protocol ID (spec §4). */
internal val ALPN: String = alpn()

private const val CONNECT_TIMEOUT_MS = 5_000
private const val HANDSHAKE_TIMEOUT_MS = 10_000

/**
 * TLS 1.3 with mutual certificates and no CA (spec §4).
 *
 * Certificates are not validated against anything: a peer is whoever holds the private key of the certificate it
 * presents. The TLS stack still verifies the handshake signatures, so the peer's device ID (the hash of its public
 * key) is proven; the engine then decides whether that device is trusted.
 */
class Tls(
    identity: Identity,
) {
    private val context: SSLContext =
        SSLContext.getInstance("TLSv1.3").apply {
            init(arrayOf(IdentityKeyManager(identity)), arrayOf(AnyPeerTrustManager()), null)
        }

    fun listen(address: InetSocketAddress): SSLServerSocket {
        val server = context.serverSocketFactory.createServerSocket() as SSLServerSocket
        server.reuseAddress = true
        server.bind(address)
        return server
    }

    /** Server side: runs the handshake on an accepted socket; returns it with the peer's device ID. */
    fun handshake(socket: SSLSocket): Pair<SSLSocket, String> {
        socket.useClientMode = false
        socket.sslParameters = socket.sslParameters.apply { needClientAuth = true }.configured()
        return finishHandshake(socket)
    }

    /** Client side: connects and runs the handshake; returns the socket with the peer's device ID. */
    fun connect(address: InetSocketAddress): Pair<SSLSocket, String> {
        val tcp = Socket()
        try {
            tcp.tcpNoDelay = true
            tcp.connect(address, CONNECT_TIMEOUT_MS)
        } catch (e: Exception) {
            tcp.close()
            throw e
        }
        // Peers are identified by key, not by name: the host name is never checked.
        val socket = context.socketFactory.createSocket(tcp, address.hostString, address.port, true) as SSLSocket
        socket.useClientMode = true
        socket.sslParameters = socket.sslParameters.configured()
        return finishHandshake(socket)
    }

    private fun SSLParameters.configured() =
        apply {
            protocols = arrayOf("TLSv1.3")
            applicationProtocols = arrayOf(ALPN)
        }

    private fun finishHandshake(socket: SSLSocket): Pair<SSLSocket, String> {
        try {
            socket.soTimeout = HANDSHAKE_TIMEOUT_MS
            socket.startHandshake()
            socket.soTimeout = 0
            check(socket.applicationProtocol == ALPN) { "the peer did not negotiate ALPN $ALPN" }
            val certificate = socket.session.peerCertificates.first() as X509Certificate
            return socket to deviceIdFromSpki(certificate.publicKey.encoded)
        } catch (e: Exception) {
            socket.close()
            throw e
        }
    }
}

private const val ALIAS = "clipsync"

/** Presents this device's certificate, whatever the peer asks for. */
internal class IdentityKeyManager(
    private val identity: Identity,
) : X509ExtendedKeyManager() {
    override fun getClientAliases(
        keyType: String?,
        issuers: Array<out Principal>?,
    ) = arrayOf(ALIAS)

    override fun chooseClientAlias(
        keyType: Array<out String>?,
        issuers: Array<out Principal>?,
        socket: Socket?,
    ) = ALIAS

    override fun chooseEngineClientAlias(
        keyType: Array<out String>?,
        issuers: Array<out Principal>?,
        engine: SSLEngine?,
    ) = ALIAS

    override fun getServerAliases(
        keyType: String?,
        issuers: Array<out Principal>?,
    ) = arrayOf(ALIAS)

    override fun chooseServerAlias(
        keyType: String?,
        issuers: Array<out Principal>?,
        socket: Socket?,
    ) = ALIAS

    override fun chooseEngineServerAlias(
        keyType: String?,
        issuers: Array<out Principal>?,
        engine: SSLEngine?,
    ) = ALIAS

    override fun getCertificateChain(alias: String?): Array<X509Certificate> = arrayOf(identity.certificate)

    override fun getPrivateKey(alias: String?): PrivateKey = identity.privateKey
}

/** Accepts any certificate: identity is the key, checked by the engine against the paired devices (spec §4). */
internal class AnyPeerTrustManager : X509ExtendedTrustManager() {
    private fun check(chain: Array<out X509Certificate>?) {
        if (chain.isNullOrEmpty()) throw CertificateException("the peer presented no certificate")
    }

    override fun checkClientTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
    ) = check(chain)

    override fun checkClientTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
        socket: Socket?,
    ) = check(chain)

    override fun checkClientTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
        engine: SSLEngine?,
    ) = check(chain)

    override fun checkServerTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
    ) = check(chain)

    override fun checkServerTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
        socket: Socket?,
    ) = check(chain)

    override fun checkServerTrusted(
        chain: Array<out X509Certificate>?,
        authType: String?,
        engine: SSLEngine?,
    ) = check(chain)

    override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
}

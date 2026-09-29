package io.github.josevini.clipsync.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket

class TlsTest {
    private val alice = testIdentity("alice")
    private val bob = testIdentity("bob")

    private fun loopback(port: Int) = InetSocketAddress(InetAddress.getLoopbackAddress(), port)

    @Test
    fun `both sides learn each other's device ID and exchange data`() {
        val server = Tls(bob).listen(InetSocketAddress(InetAddress.getLoopbackAddress(), 0))
        val accepted =
            Executors.newSingleThreadExecutor().submit<Pair<String, String>> {
                val (socket, peer) = Tls(bob).handshake(server.accept() as SSLSocket)
                val got = socket.inputStream.readNBytes(5).decodeToString()
                socket.outputStream.write("world".encodeToByteArray())
                socket.outputStream.flush()
                peer to got
            }
        val (socket, peer) = Tls(alice).connect(loopback(server.localPort))
        socket.outputStream.write("hello".encodeToByteArray())
        socket.outputStream.flush()
        assertEquals("world", socket.inputStream.readNBytes(5).decodeToString())
        assertEquals(bob.id, peer)
        assertEquals(alice.id to "hello", accepted.get(10, TimeUnit.SECONDS))
        socket.close()
        server.close()
    }

    @Test
    fun `the device ID is the hash of the certificate's public key`() {
        // Computed independently with openssl: the certificate's public key as DER, then sha256sum.
        assertEquals("bcc3305f6f42328ce481842db8268e61e897cc7be503f1be84509eea31611be5", alice.id)
        assertEquals("fae7952f0d6a0d3113a0cc02389261958874b672b6a7b5b45212ef41cc0b46d4", bob.id)
    }

    @Test
    fun `a client without a certificate is refused`() {
        val server = Tls(bob).listen(loopback(0))
        val accepted = Executors.newSingleThreadExecutor().submit<String> { Tls(bob).handshake(server.accept() as SSLSocket).second }
        val anonymous = SSLContext.getInstance("TLSv1.3")
        anonymous.init(null, arrayOf(AnyPeerTrustManager()), null)
        val socket = anonymous.socketFactory.createSocket("127.0.0.1", server.localPort) as SSLSocket
        socket.sslParameters = socket.sslParameters.apply { applicationProtocols = arrayOf(ALPN) }
        runCatching { socket.startHandshake() }
        assertThrows(Exception::class.java) { accepted.get(10, TimeUnit.SECONDS) }
        socket.close()
        server.close()
    }

    @Test
    fun `a client that negotiates no protocol is refused`() {
        val server = Tls(bob).listen(loopback(0))
        val accepted = Executors.newSingleThreadExecutor().submit<String> { Tls(bob).handshake(server.accept() as SSLSocket).second }
        // Another program with a valid certificate but no ALPN.
        val context = SSLContext.getInstance("TLSv1.3")
        context.init(arrayOf(IdentityKeyManager(alice)), arrayOf(AnyPeerTrustManager()), null)
        val socket = context.socketFactory.createSocket("127.0.0.1", server.localPort) as SSLSocket
        runCatching { socket.startHandshake() }
        assertThrows(Exception::class.java) { accepted.get(10, TimeUnit.SECONDS) }
        socket.close()
        server.close()
    }

    @Test
    fun `a server that negotiates no protocol is refused`() {
        val server = Tls(bob).listen(loopback(0))
        Executors.newSingleThreadExecutor().submit {
            val socket = server.accept() as SSLSocket
            socket.sslParameters = socket.sslParameters.apply { applicationProtocols = arrayOf() }
            runCatching {
                socket.startHandshake()
                socket.inputStream.read()
            }
        }
        assertThrows(Exception::class.java) { Tls(alice).connect(loopback(server.localPort)) }
        server.close()
    }

    @Test
    fun `a client presenting another device's certificate is refused`() {
        val server = Tls(bob).listen(loopback(0))
        val accepted = Executors.newSingleThreadExecutor().submit<String> { Tls(bob).handshake(server.accept() as SSLSocket).second }
        // Mallory signs with her own key but presents Alice's certificate.
        val impostor = Identity(testIdentity("mallory").privateKey, alice.certificate)
        runCatching { Tls(impostor).connect(loopback(server.localPort)) }
        assertThrows(Exception::class.java) { accepted.get(10, TimeUnit.SECONDS) }
        server.close()
    }
}

package io.github.josevini.clipsync.session

import java.io.File
import java.security.KeyStore
import java.security.PrivateKey
import java.security.cert.X509Certificate
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.io.path.createTempDirectory

/** Identities generated once with keytool (EC P-256, self-signed): see src/test/resources/identities. */
fun testIdentity(name: String): Identity {
    val store = KeyStore.getInstance("PKCS12")
    val password = "testing".toCharArray()
    Identity::class.java.getResourceAsStream("/identities/$name.p12")!!.use { store.load(it, password) }
    val key = store.getKey("clipsync", password) as PrivateKey
    return Identity(key, store.getCertificate("clipsync") as X509Certificate)
}

const val WAIT_SECONDS = 20L

/** Collects what a node reports, so tests can wait for it. */
class Recorder : NodeListener {
    val events = LinkedBlockingQueue<NodeEvent>()
    val clipboard = LinkedBlockingQueue<String>()

    override fun onEvent(event: NodeEvent) {
        events.add(event)
    }

    fun setClipboard(text: String) {
        clipboard.add(text)
    }

    /** The first event matching [predicate], skipping the others. */
    fun <T : Any> await(
        what: String,
        predicate: (NodeEvent) -> T?,
    ): T {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_SECONDS)
        while (true) {
            val left = deadline - System.nanoTime()
            val event = events.poll(left.coerceAtLeast(0), TimeUnit.NANOSECONDS) ?: error("timed out waiting for $what")
            predicate(event)?.let { return it }
        }
    }

    fun awaitClipboard(): String = clipboard.poll(WAIT_SECONDS, TimeUnit.SECONDS) ?: error("nothing reached the clipboard")
}

/** A node on an ephemeral loopback port, with its state in a scratch directory. */
class TestNode(
    val name: String,
    identityName: String,
    val dir: File = createTempDirectory("clipsync-node").toFile(),
) : AutoCloseable {
    val identity = testIdentity(identityName)
    val recorder = Recorder()
    val node =
        Node(
            identity = identity,
            config = NodeConfig(name = name, platform = "test", port = 0),
            store = FileStateStore(File(dir, "state.json")),
            clipboard = recorder::setClipboard,
            listener = recorder,
        ).also { it.start() }

    fun uri(token: String) = "clipsync://pair?v=1&id=${identity.id}&name=$name&addr=127.0.0.1:${node.port}&token=$token"

    override fun close() = node.close()
}

/** Polls [condition] until it holds. */
fun waitUntil(
    what: String,
    condition: () -> Boolean,
) {
    val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(WAIT_SECONDS)
    while (!condition()) {
        check(System.nanoTime() < deadline) { "timed out waiting for $what" }
        Thread.sleep(20)
    }
}

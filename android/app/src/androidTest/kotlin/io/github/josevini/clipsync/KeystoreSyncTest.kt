package io.github.josevini.clipsync

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.LocalChange
import io.github.josevini.clipsync.session.FileStateStore
import io.github.josevini.clipsync.session.Node
import io.github.josevini.clipsync.session.NodeConfig
import io.github.josevini.clipsync.session.NodeEvent
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * On a device: identities in the Android Keystore, TLS through the platform's Conscrypt, and the core through its
 * native library — two nodes pair over loopback and sync.
 */
@RunWith(AndroidJUnit4::class)
class KeystoreSyncTest {
    private val aliases = listOf("clipsync-test-a", "clipsync-test-b")
    private val dir = File(InstrumentationRegistry.getInstrumentation().targetContext.cacheDir, "keystore-sync-test")
    private val nodes = mutableListOf<Node>()

    @After
    fun cleanUp() {
        nodes.forEach { it.close() }
        aliases.forEach { KeystoreIdentity.delete(it) }
        dir.deleteRecursively()
    }

    private class Peer {
        val events = LinkedBlockingQueue<NodeEvent>()
        val clipboard = LinkedBlockingQueue<String>()

        fun <T : Any> await(predicate: (NodeEvent) -> T?): T {
            while (true) {
                val event = events.poll(20, TimeUnit.SECONDS) ?: error("timed out")
                predicate(event)?.let { return it }
            }
        }
    }

    private fun start(
        alias: String,
        name: String,
        peer: Peer,
    ): Node =
        Node(
            identity = KeystoreIdentity.loadOrCreate(alias),
            config = NodeConfig(name = name, platform = "android", port = 0),
            store = FileStateStore(File(dir, "$name.json")),
            clipboard = { peer.clipboard.add(it) },
            listener = { peer.events.add(it) },
        ).also {
            it.start()
            nodes += it
        }

    @Test
    fun keystoreIdentitiesPairAndSyncOverTls() {
        val (a, b) = Peer() to Peer()
        val nodeA = start(aliases[0], "a", a)
        val nodeB = start(aliases[1], "b", b)
        val idB = nodeB.status().id
        assertEquals(idB, KeystoreIdentity.loadOrCreate(aliases[1]).id)

        val uri = "clipsync://pair?v=1&id=$idB&name=b&addr=127.0.0.1:${nodeB.port}&token=${nodeB.startPairing()}"
        nodeA.pairWithUri(uri)
        a.await { ((it as? NodeEvent.Engine)?.event as? EngineEvent.PeerConnected) }
        assertTrue(
            nodeA
                .status()
                .devices
                .single()
                .connected,
        )

        assertEquals(LocalChange.Sent(1u, 1u), nodeB.sendText("from the Keystore"))
        assertEquals("from the Keystore", a.clipboard.poll(20, TimeUnit.SECONDS))
    }
}

package io.github.josevini.clipsync.session

import io.github.josevini.clipsync.core.CloseReason
import io.github.josevini.clipsync.core.CoreException
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.LocalChange
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** Two nodes on loopback, as two phones would be. */
class NodeTest {
    private val nodes = mutableListOf<TestNode>()

    private fun node(
        name: String,
        identity: String,
    ) = TestNode(name, identity).also { nodes += it }

    @After
    fun close() = nodes.forEach { it.close() }

    private fun paired(event: NodeEvent) = ((event as? NodeEvent.Engine)?.event as? EngineEvent.Paired)?.device

    private fun connected(event: NodeEvent) = ((event as? NodeEvent.Engine)?.event as? EngineEvent.PeerConnected)?.peer

    /** Alice scans Bob's QR code. */
    private fun pair(
        alice: TestNode,
        bob: TestNode,
    ) {
        alice.node.pairWithUri(bob.uri(bob.node.startPairing()))
        assertEquals("bob", alice.recorder.await("alice to pair", ::paired).name)
        assertEquals("alice", bob.recorder.await("bob to pair", ::paired).name)
    }

    @Test
    fun `pairing with a QR code then syncing both ways`() {
        val (alice, bob) = node("alice", "alice") to node("bob", "bob")
        pair(alice, bob)
        waitUntil("both connected") {
            alice.node
                .status()
                .devices
                .all { it.connected } &&
                bob.node
                    .status()
                    .devices
                    .all { it.connected }
        }

        assertEquals(LocalChange.Sent(1u, 1u), bob.node.sendText("olá 👋"))
        assertEquals("olá 👋", alice.recorder.awaitClipboard())
        assertEquals(LocalChange.Sent(2u, 1u), alice.node.sendText("from alice"))
        assertEquals("from alice", bob.recorder.awaitClipboard())

        val status = alice.node.status()
        assertEquals(alice.identity.id, status.id)
        assertEquals("alice", status.name)
        assertEquals(listOf(DeviceStatus(bob.identity.id, "bob", connected = true)), status.devices)
        assertFalse(bob.node.status().pairing)
    }

    @Test
    fun `a paired device is dialed again after a restart, at the address it was reached on`() {
        val (alice, bob) = node("alice", "alice") to node("bob", "bob")
        pair(alice, bob)
        alice.close()
        val again = TestNode("alice", "alice", alice.dir).also { nodes += it }
        assertEquals(bob.identity.id, again.recorder.await("alice to reconnect", ::connected))
        assertEquals(LocalChange.Sent(1u, 1u), again.node.sendText("after restart"))
        assertEquals("after restart", bob.recorder.awaitClipboard())
    }

    @Test
    fun `unpairing reaches the other device`() {
        val (alice, bob) = node("alice", "alice") to node("bob", "bob")
        pair(alice, bob)
        assertTrue(alice.node.unpair(bob.identity.id))
        bob.recorder.await("bob to learn") { (it as? NodeEvent.Engine)?.event as? EngineEvent.Unpaired }
        waitUntil("bob forgot alice") {
            bob.node
                .status()
                .devices
                .isEmpty()
        }
        assertEquals(emptyList<DeviceStatus>(), alice.node.status().devices)
        assertFalse(alice.node.unpair(bob.identity.id))
    }

    @Test
    fun `a wrong token is refused`() {
        val (alice, bob) = node("alice", "alice") to node("bob", "bob")
        bob.node.startPairing()
        alice.node.pairWithUri(bob.uri("00".repeat(16)))
        val reason =
            alice.recorder.await("alice to be refused") {
                ((it as? NodeEvent.Engine)?.event as? EngineEvent.ConnectionClosed)?.reason
            }
        assertEquals(CloseReason.RemoteError("bad_token"), reason)
    }

    @Test
    fun `a QR code pointing at another device is refused before pairing`() {
        val (alice, bob) = node("alice", "alice") to node("bob", "bob")
        val mallory = testIdentity("mallory")
        val forged = bob.uri(bob.node.startPairing()).replace(bob.identity.id, mallory.id)
        alice.node.pairWithUri(forged)
        val failed = alice.recorder.await("the dial to fail") { it as? NodeEvent.DialFailed }
        assertTrue(failed.error, failed.error.contains("not the expected"))
    }

    @Test
    fun `an invalid QR code is an error`() {
        val alice = node("alice", "alice")
        assertThrows(CoreException.InvalidUri::class.java) { alice.node.pairWithUri("https://example.com") }
    }
}

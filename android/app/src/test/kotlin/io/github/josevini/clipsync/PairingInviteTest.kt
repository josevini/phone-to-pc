package io.github.josevini.clipsync

import io.github.josevini.clipsync.core.CloseReason
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.PairedDevice
import io.github.josevini.clipsync.core.SocketAddress
import io.github.josevini.clipsync.session.NodeEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.net.InetAddress

class PairingInviteTest {
    private val phone = "ab".repeat(32)
    private val known = "cd".repeat(32)

    @Test
    fun `the code offers the addresses other devices can reach, IPv4 first`() {
        val ips =
            listOf("fd00::2", "127.0.0.1", "::1", "fe80::1", "169.254.1.2", "192.168.0.20", "0.0.0.0", "192.168.0.20")
                .map(InetAddress::getByName)
        // The JVM spells IPv6 addresses in full and Android compresses them; the core reads both.
        val ipv6 = InetAddress.getByName("fd00::2").hostAddress!!
        val expected = listOf(SocketAddress("192.168.0.20", 47823u), SocketAddress(ipv6, 47823u))
        assertEquals(expected, pairingAddresses(ips, 47823))
    }

    @Test
    fun `no reachable address is none`() {
        assertEquals(emptyList<SocketAddress>(), pairingAddresses(listOf(InetAddress.getByName("127.0.0.1")), 47823))
    }

    @Test
    fun `the code drawn from its modules is read back`() {
        val uri =
            "clipsync://pair?v=1&id=$phone&name=Pixel+8&addr=192.168.0.20%3A47823&token=000102030405060708090a0b0c0d0e0f"
        val modules = qrModules(uri)
        // Drawn dark on light, 8 pixels per module, with a quiet zone of 4 modules.
        val scale = 8
        val size = (modules.width + 8) * scale
        val plane = ByteArray(size * size) { 255.toByte() }
        for (y in 0 until modules.height) {
            for (x in 0 until modules.width) {
                if (!modules[x, y]) continue
                for (dy in 0 until scale) {
                    for (dx in 0 until scale) plane[((y + 4) * scale + dy) * size + (x + 4) * scale + dx] = 0
                }
            }
        }
        assertEquals(uri, decodeQr(plane, size, size, size))
    }

    @Test
    fun `a device that pairs with the code is reported by name`() {
        val tracker = InviteTracker(alreadyPaired = setOf(known))
        assertEquals(InviteOutcome.Paired("Pixel 8"), tracker.on(paired(phone, "Pixel 8")))
    }

    @Test
    fun `a paired device changing its name is not a pairing`() {
        val tracker = InviteTracker(alreadyPaired = setOf(known))
        assertNull(tracker.on(paired(known, "book2")))
    }

    @Test
    fun `the code ends when pairing mode does`() {
        // Expired, stopped, or too many wrong tokens.
        val tracker = InviteTracker(alreadyPaired = emptySet())
        assertNull(tracker.on(NodeEvent.Engine(EngineEvent.ConnectionClosed(3u, phone, CloseReason.BadToken))))
        assertEquals(InviteOutcome.Ended, tracker.on(NodeEvent.Engine(EngineEvent.PairingModeEnded)))
    }

    private fun paired(
        id: String,
        name: String,
    ) = NodeEvent.Engine(EngineEvent.Paired(PairedDevice(id, name)))
}

package io.github.josevini.clipsync

import io.github.josevini.clipsync.core.CloseReason
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.PairedDevice
import io.github.josevini.clipsync.core.SocketAddress
import io.github.josevini.clipsync.session.NodeEvent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PairingTrackerTest {
    private val pc = "ab".repeat(32)
    private val other = "cd".repeat(32)
    private val addrs = listOf(SocketAddress("192.168.0.10", 47823u))
    private val tracker = PairingTracker(peer = pc, addrs = addrs)

    private fun closed(
        conn: ULong,
        reason: CloseReason,
    ) = NodeEvent.Engine(EngineEvent.ConnectionClosed(conn, pc, reason))

    @Test
    fun `pairing succeeds when that device is paired`() {
        assertNull(tracker.on(NodeEvent.PairingConnection(7u)))
        assertNull(tracker.on(NodeEvent.Engine(EngineEvent.Paired(PairedDevice(other, "someone else")))))
        assertEquals(PairingOutcome.Paired("book2"), tracker.on(NodeEvent.Engine(EngineEvent.Paired(PairedDevice(pc, "book2")))))
    }

    @Test
    fun `a refusal on the pairing connection says why`() {
        tracker.on(NodeEvent.PairingConnection(7u))
        // Another connection closing is not about this pairing.
        assertNull(tracker.on(closed(3u, CloseReason.Timeout)))
        assertEquals(PairingOutcome.Failed(PairingFailure.CODE_EXPIRED), tracker.on(closed(7u, CloseReason.RemoteError("bad_token"))))
    }

    @Test
    fun `each refusal maps to what the user can do about it`() {
        val cases =
            mapOf(
                CloseReason.RemoteError("bad_token") to PairingFailure.CODE_EXPIRED,
                CloseReason.RemoteError("pairing_closed") to PairingFailure.NOT_IN_PAIRING_MODE,
                CloseReason.RemoteError("not_paired") to PairingFailure.NOT_IN_PAIRING_MODE,
                CloseReason.RejectedByPeer to PairingFailure.REJECTED,
                CloseReason.RemoteError("unsupported_version") to PairingFailure.INCOMPATIBLE,
                CloseReason.UnsupportedVersion to PairingFailure.INCOMPATIBLE,
                CloseReason.Timeout to PairingFailure.CONNECTION_LOST,
                CloseReason.Closed to PairingFailure.CONNECTION_LOST,
            )
        for ((reason, failure) in cases) {
            val tracker = PairingTracker(pc, addrs)
            tracker.on(NodeEvent.PairingConnection(1u))
            assertEquals("$reason", PairingOutcome.Failed(failure), tracker.on(closed(1u, reason)))
        }
    }

    @Test
    fun `a dial to the code's addresses that fails means the device is unreachable`() {
        assertNull(tracker.on(NodeEvent.DialFailed(listOf(SocketAddress("10.0.0.9", 47823u)), "timed out")))
        assertEquals(PairingOutcome.Failed(PairingFailure.UNREACHABLE), tracker.on(NodeEvent.DialFailed(addrs, "timed out")))
    }
}

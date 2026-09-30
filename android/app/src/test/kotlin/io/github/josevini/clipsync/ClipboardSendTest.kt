package io.github.josevini.clipsync

import android.service.quicksettings.Tile
import io.github.josevini.clipsync.session.DeviceStatus
import io.github.josevini.clipsync.session.NodeStatus
import org.junit.Assert.assertEquals
import org.junit.Test

class ClipboardSendTest {
    @Test
    fun `copied text is sent`() {
        var sent: String? = null
        val outcome =
            sendClip("copied", sensitive = false) {
                sent = it
                SendOutcome.Sent(1)
            }
        assertEquals(SendOutcome.Sent(1), outcome)
        assertEquals("copied", sent)
    }

    @Test
    fun `text marked sensitive is not sent`() {
        assertEquals(SendOutcome.Sensitive, sendClip("hunter2", sensitive = true) { throw AssertionError("sent") })
    }

    @Test
    fun `a clipboard without text sends empty text`() {
        // The core then reports it as empty.
        var sent: String? = null
        sendClip(null, sensitive = false) {
            sent = it
            SendOutcome.Empty
        }
        assertEquals("", sent)
    }

    @Test
    fun `the clipboard is read when the phone is unlocked soon after the tap`() {
        assertEquals(true, stillWanted(requestedAtMs = 1_000, nowMs = 1_000))
        assertEquals(true, stillWanted(requestedAtMs = 1_000, nowMs = 61_000))
    }

    @Test
    fun `a tap on a locked phone that was never unlocked sends nothing later`() {
        // Unlocking hours later must not send whatever the clipboard holds then.
        assertEquals(false, stillWanted(requestedAtMs = 1_000, nowMs = 61_001))
    }

    @Test
    fun `the tile is active while a paired device is connected`() {
        val pc = DeviceStatus(id = "a".repeat(64), name = "pc", connected = true)
        assertEquals(Tile.STATE_ACTIVE, tileState(status(pc, pc.copy(id = "b".repeat(64), connected = false))))
    }

    @Test
    fun `the tile is inactive with no device connected or sharing stopped`() {
        assertEquals(Tile.STATE_INACTIVE, tileState(status(DeviceStatus(id = "a".repeat(64), name = "pc", connected = false))))
        assertEquals(Tile.STATE_INACTIVE, tileState(status()))
        assertEquals(Tile.STATE_INACTIVE, tileState(null))
    }

    @Test
    fun `the tile is inactive while sharing is paused`() {
        val pc = DeviceStatus(id = "a".repeat(64), name = "pc", connected = true)
        assertEquals(Tile.STATE_INACTIVE, tileState(status(pc).copy(paused = true)))
    }

    private fun status(vararg devices: DeviceStatus) =
        NodeStatus(id = "c".repeat(64), name = "phone", port = 47823, pairing = false, devices = devices.toList())
}

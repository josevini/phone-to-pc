package io.github.josevini.clipsync

import io.github.josevini.clipsync.core.LocalChange
import org.junit.Assert.assertEquals
import org.junit.Test

class SendOutcomeTest {
    @Test
    fun `text sent to connected devices says how many`() {
        assertEquals(SendOutcome.Sent(2), sendIfConnected(connected = true) { LocalChange.Sent(5u, 2u) })
    }

    @Test
    fun `nothing is sent while no paired device is connected`() {
        // Sending would reach nobody, and make the same text Unchanged once a device connects.
        assertEquals(SendOutcome.NoDevice, sendIfConnected(connected = false) { throw AssertionError("sent") })
    }

    @Test
    fun `a device that disconnects just before the send counts as none connected`() {
        assertEquals(SendOutcome.NoDevice, sendIfConnected(connected = true) { LocalChange.Sent(5u, 0u) })
    }

    @Test
    fun `text the core does not send says why`() {
        val cases =
            mapOf(
                LocalChange.Unchanged to SendOutcome.Unchanged,
                LocalChange.Empty to SendOutcome.Empty,
                LocalChange.TooLarge to SendOutcome.TooLarge,
            )
        for ((change, outcome) in cases) assertEquals(outcome, sendIfConnected(connected = true) { change })
    }
}

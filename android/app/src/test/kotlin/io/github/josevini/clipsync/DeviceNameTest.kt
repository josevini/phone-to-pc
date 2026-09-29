package io.github.josevini.clipsync

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceNameTest {
    @Test
    fun `the phone's model is the default name`() {
        assertEquals("Pixel 8", defaultDeviceName("Pixel 8"))
        assertEquals("SM-S911B", defaultDeviceName("  SM-S911B "))
    }

    @Test
    fun `a missing model falls back to a generic name`() {
        assertEquals("Android", defaultDeviceName(""))
        assertEquals("Android", defaultDeviceName("   "))
    }

    @Test
    fun `a long model is cut to 64 bytes without splitting a character`() {
        val name = defaultDeviceName("é".repeat(40))
        assertTrue(name.encodeToByteArray().size <= 64)
        assertEquals("é".repeat(32), name)
    }
}

package io.github.josevini.clipsync.ui

import androidx.compose.ui.text.input.KeyboardType
import org.junit.Assert.assertEquals
import org.junit.Test

class PairingLinkTest {
    @Test
    fun `the pairing link is typed on a URL keyboard that does not correct it`() {
        // Autocorrection turned "id" into "I'd" on an Android 10 emulator, and the link no longer parsed.
        assertEquals(KeyboardType.Uri, PairingLinkKeyboard.keyboardType)
        assertEquals(false, PairingLinkKeyboard.autoCorrectEnabled)
    }
}

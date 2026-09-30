package io.github.josevini.clipsync

import android.content.Intent
import org.junit.Assert.assertEquals
import org.junit.Test

class SendIntentTest {
    @Test
    fun `the selection menu sends the selected text`() {
        assertEquals("selected", textToSend(Intent.ACTION_PROCESS_TEXT, processText = "selected", text = "ignored", subject = null))
    }

    @Test
    fun `the share sheet sends the shared text`() {
        val shared = textToSend(Intent.ACTION_SEND, processText = null, text = "https://example.org", subject = "Example")
        assertEquals("https://example.org", shared)
    }

    @Test
    fun `a share with only a subject sends the subject`() {
        assertEquals("Example", textToSend(Intent.ACTION_SEND, processText = null, text = null, subject = "Example"))
        assertEquals("Example", textToSend(Intent.ACTION_SEND, processText = null, text = "", subject = "Example"))
    }

    @Test
    fun `nothing to read is empty text`() {
        // The core then reports it as empty.
        assertEquals("", textToSend(Intent.ACTION_SEND, processText = null, text = null, subject = null))
        assertEquals("", textToSend(Intent.ACTION_PROCESS_TEXT, processText = null, text = null, subject = null))
        assertEquals("", textToSend(null, processText = "selected", text = "shared", subject = null))
    }
}

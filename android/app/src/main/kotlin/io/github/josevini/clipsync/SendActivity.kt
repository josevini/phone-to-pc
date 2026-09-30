package io.github.josevini.clipsync

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import kotlin.concurrent.thread

/**
 * "Send to devices" in the text-selection menu (`ACTION_PROCESS_TEXT`) and in the share sheet (`ACTION_SEND` of
 * text): sends the text to the connected devices and says in a toast what became of it. It has no window: the app
 * the text came from stays on screen.
 */
class SendActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A theme without a window requires finishing before onResume; the send outlives the activity.
        if (savedInstanceState == null) {
            send(
                textToSend(
                    intent.action,
                    processText = intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT),
                    text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT),
                    subject = intent.getCharSequenceExtra(Intent.EXTRA_SUBJECT),
                ),
            )
        }
        finish()
    }

    private fun send(text: String) {
        val app = applicationContext
        val node = Sync.node
        thread(name = "clipsync-send") {
            val message = send(node, text).message(app.resources)
            Handler(Looper.getMainLooper()).post { Toast.makeText(app, message, Toast.LENGTH_SHORT).show() }
        }
    }
}

/**
 * The text an intent of [action] asks to send, empty when there is none. A share carries the text itself, or only a
 * subject (e.g. a title with nothing else).
 */
internal fun textToSend(
    action: String?,
    processText: CharSequence?,
    text: CharSequence?,
    subject: CharSequence?,
): String =
    when (action) {
        Intent.ACTION_PROCESS_TEXT -> processText
        Intent.ACTION_SEND -> text?.takeIf { it.isNotEmpty() } ?: subject
        else -> null
    }?.toString().orEmpty()

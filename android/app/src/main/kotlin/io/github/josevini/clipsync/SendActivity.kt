package io.github.josevini.clipsync

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import kotlin.concurrent.thread

/**
 * "Send to devices" in the text-selection menu (`ACTION_PROCESS_TEXT`): sends the selected text to the connected
 * devices and says in a toast what became of it. It has no window: the app the text was selected in stays on screen.
 */
class SendActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A theme without a window requires finishing before onResume; the send outlives the activity.
        if (savedInstanceState == null) {
            send(intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString().orEmpty())
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

package io.github.josevini.clipsync

import android.app.Activity
import android.content.ClipboardManager
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock

/** How long the window may be on screen without getting focus, so that it never lingers there catching touches. */
private const val FOCUS_TIMEOUT_MS = 3_000L

/** How long after the tap a phone may be unlocked and still send; later, the clipboard may hold something else. */
private const val UNLOCK_WAIT_MS = 60_000L

private const val REQUESTED_AT = "requestedAt"

/** `ClipDescription.EXTRA_IS_SENSITIVE`, spelled out because the constant is API 33+; apps set it on older versions too. */
private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"

/**
 * "Send clipboard" from the Quick Settings tile and the notification: sends the text on the phone's clipboard and says
 * in a toast what became of it. Android lets only the app with focus read the clipboard (D7), so this activity has a
 * transparent window, reads the clipboard once that window has focus, and finishes. On a locked phone it starts behind
 * the lock screen and gets focus once the phone is unlocked.
 */
class ClipboardSendActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private val giveUp = Runnable { finish() }
    private var read = false
    private var requestedAt = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestedAt = savedInstanceState?.getLong(REQUESTED_AT) ?: SystemClock.elapsedRealtime()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putLong(REQUESTED_AT, requestedAt)
    }

    override fun onResume() {
        super.onResume()
        main.postDelayed(giveUp, FOCUS_TIMEOUT_MS)
    }

    override fun onPause() {
        main.removeCallbacks(giveUp)
        super.onPause()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || read) return
        read = true
        if (!stillWanted(requestedAt, SystemClock.elapsedRealtime())) return finish()
        if (Sync.status.value?.paused == true) {
            // Nothing would be sent: don't read the clipboard.
            reportInBackground(this) { SendOutcome.Paused }
            return finish()
        }
        val clip = getSystemService(ClipboardManager::class.java).primaryClip
        val text = clip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
        val sensitive = clip?.description?.extras?.getBoolean(EXTRA_IS_SENSITIVE) == true
        val node = Sync.node
        reportInBackground(this) { sendClip(text, sensitive) { send(node, it) } }
        finish()
    }
}

/** Whether a send asked for at [requestedAtMs] should still happen at [nowMs] (both since boot). */
internal fun stillWanted(
    requestedAtMs: Long,
    nowMs: Long,
): Boolean = nowMs - requestedAtMs <= UNLOCK_WAIT_MS

/** Sends the clipboard's [text], or nothing when an app marked it [sensitive]. No text is sent as empty text. */
internal fun sendClip(
    text: CharSequence?,
    sensitive: Boolean,
    send: (String) -> SendOutcome,
): SendOutcome = if (sensitive) SendOutcome.Sensitive else send(text?.toString().orEmpty())

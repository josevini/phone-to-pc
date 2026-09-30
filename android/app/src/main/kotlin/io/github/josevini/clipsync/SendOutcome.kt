package io.github.josevini.clipsync

import android.content.Context
import android.content.res.Resources
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import io.github.josevini.clipsync.core.LocalChange
import io.github.josevini.clipsync.session.Node
import java.util.concurrent.RejectedExecutionException
import kotlin.concurrent.thread

/** What became of text the user chose to send, as they are told. */
sealed interface SendOutcome {
    data class Sent(
        val devices: Int,
    ) : SendOutcome

    /** The service is stopped: there is no node to send with. */
    data object NotRunning : SendOutcome

    data object NoDevice : SendOutcome

    /** The text is the one last sent or received (spec §6). */
    data object Unchanged : SendOutcome

    data object Empty : SendOutcome

    /** Over 1 MiB. */
    data object TooLarge : SendOutcome

    /** Copied text an app marked as sensitive, such as a password (`ClipDescription.EXTRA_IS_SENSITIVE`). */
    data object Sensitive : SendOutcome

    /** Sharing is paused. */
    data object Paused : SendOutcome
}

/** Works out [outcome] on a worker thread, since sending blocks, and tells it in a toast. */
fun reportInBackground(
    context: Context,
    outcome: () -> SendOutcome,
) {
    val app = context.applicationContext
    thread(name = "clipsync-send") {
        val message = outcome().message(app.resources)
        Handler(Looper.getMainLooper()).post { Toast.makeText(app, message, Toast.LENGTH_SHORT).show() }
    }
}

/** Sends [text] to the devices [node] is connected to. Blocks until the node's thread has taken it. */
fun send(
    node: Node?,
    text: String,
): SendOutcome {
    if (node == null) return SendOutcome.NotRunning
    return try {
        val status = node.status()
        sendIfConnected(status.devices.any { it.connected }, status.paused) { node.sendText(text) }
    } catch (_: RejectedExecutionException) {
        // The service stopped the node meanwhile.
        SendOutcome.NotRunning
    }
}

/**
 * Calls [send] only when a paired device is [connected] and sharing is not [paused]: sending to nobody would still
 * make the text the current clip, so sending it again once a device connects would be [LocalChange.Unchanged].
 */
internal fun sendIfConnected(
    connected: Boolean,
    paused: Boolean = false,
    send: () -> LocalChange,
): SendOutcome {
    if (paused) return SendOutcome.Paused
    if (!connected) return SendOutcome.NoDevice
    return when (val change = send()) {
        is LocalChange.Sent -> if (change.peers == 0u) SendOutcome.NoDevice else SendOutcome.Sent(change.peers.toInt())
        LocalChange.Unchanged -> SendOutcome.Unchanged
        LocalChange.Empty -> SendOutcome.Empty
        LocalChange.TooLarge -> SendOutcome.TooLarge
        LocalChange.Paused -> SendOutcome.Paused
    }
}

fun SendOutcome.message(resources: Resources): String =
    when (this) {
        is SendOutcome.Sent -> resources.getQuantityString(R.plurals.sent_to, devices, devices)
        SendOutcome.NotRunning -> resources.getString(R.string.send_not_running)
        SendOutcome.NoDevice -> resources.getString(R.string.send_no_device)
        SendOutcome.Unchanged -> resources.getString(R.string.send_unchanged)
        SendOutcome.Empty -> resources.getString(R.string.send_empty)
        SendOutcome.TooLarge -> resources.getString(R.string.send_too_large)
        SendOutcome.Sensitive -> resources.getString(R.string.send_sensitive)
        SendOutcome.Paused -> resources.getString(R.string.send_paused)
    }

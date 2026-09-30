package io.github.josevini.clipsync

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import io.github.josevini.clipsync.session.NodeStatus
import kotlinx.coroutines.Job
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/** The "Send clipboard" Quick Settings tile: lit while a paired device is connected; tapping it sends the clipboard. */
class ClipboardTileService : TileService() {
    private val scope = MainScope()
    private var watching: Job? = null

    override fun onStartListening() {
        super.onStartListening()
        watching = scope.launch { Sync.status.collect(::show) }
    }

    override fun onStopListening() {
        watching?.cancel()
        watching = null
        super.onStopListening()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    // On a locked phone the system asks to unlock and shows the activity afterwards. unlockAndRun would start it while
    // the lock screen is still leaving, which takes its focus away before it can read the clipboard.
    override fun onClick() {
        super.onClick()
        sendClipboard()
    }

    // Below Android 14 the Intent overload is the only one; it throws only for apps targeting 14 on 14+.
    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun sendClipboard() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(sendClipboardIntent(this))
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(Intent(this, ClipboardSendActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    private fun show(status: NodeStatus?) {
        val tile = qsTile ?: return
        tile.state = tileState(status)
        tile.updateTile()
    }
}

/** Opens [ClipboardSendActivity], from the tile or the notification. */
fun sendClipboardIntent(context: Context): PendingIntent =
    PendingIntent.getActivity(
        context,
        0,
        Intent(context, ClipboardSendActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        PendingIntent.FLAG_IMMUTABLE,
    )

/** Active while a paired device is connected, so a tap would send; inactive otherwise, though a tap still says why. */
internal fun tileState(status: NodeStatus?): Int =
    if (status?.devices.orEmpty().any { it.connected }) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE

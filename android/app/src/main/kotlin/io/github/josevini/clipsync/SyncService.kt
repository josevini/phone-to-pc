package io.github.josevini.clipsync

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.session.FileStateStore
import io.github.josevini.clipsync.session.Node
import io.github.josevini.clipsync.session.NodeConfig
import io.github.josevini.clipsync.session.NodeEvent
import java.io.File
import java.net.BindException

private const val CHANNEL = "sync"
private const val NOTIFICATION_ID = 1
private const val ACTION_STOP = "io.github.josevini.clipsync.STOP"
private const val ACTION_RESTART = "io.github.josevini.clipsync.RESTART"

/**
 * Keeps the node running while the app is in the background: connections to paired devices stay open, and what
 * they copy is written to this phone's clipboard. Runs as a `connectedDevice` foreground service.
 */
class SyncService : LifecycleService() {
    private val main = Handler(Looper.getMainLooper())
    private var node: Node? = null
    private var discovery: Discovery? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
        startForeground(NOTIFICATION_ID, notification(connected = 0), ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        try {
            startNode()
        } catch (e: Exception) {
            Log.e("clipsync", "could not start", e)
            stopSelf()
        }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        super.onStartCommand(intent, flags, startId)
        when (intent?.action) {
            ACTION_STOP -> {
                stopSelf()
            }

            ACTION_RESTART -> {
                stopNode()
                startNode()
            }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        stopNode()
        super.onDestroy()
    }

    private fun stopNode() {
        discovery?.stop()
        discovery = null
        node?.close()
        node = null
        Sync.detach()
    }

    private fun startNode() {
        val identity = KeystoreIdentity.loadOrCreate()
        val name = DeviceName.get(this)
        val store = FileStateStore(File(filesDir, "state.json"))

        fun create(port: Int): Node {
            lateinit var node: Node
            node =
                Node(
                    identity = identity,
                    config = NodeConfig(name = name, platform = "android", port = port),
                    store = store,
                    clipboard = ::setClipboard,
                    listener = { event -> onEvent(node, event) },
                )
            return node
        }
        // The default port may be taken; any port works, since discovery advertises it.
        val node =
            try {
                create(NodeConfig(name, "android").port).also { it.start() }
            } catch (_: BindException) {
                create(0).also { it.start() }
            }
        this.node = node
        Sync.attach(node)
        discovery = Discovery(this, node, identity.id, name, node.port).also { it.start() }
    }

    private fun onEvent(
        node: Node,
        event: NodeEvent,
    ) {
        Sync.publish(node, event)
        val engine = (event as? NodeEvent.Engine)?.event
        if (engine is EngineEvent.PeerConnected || engine is EngineEvent.PeerDisconnected) {
            val connected = node.status().devices.count { it.connected }
            getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, notification(connected))
        }
    }

    /** Clips from other devices; writing the clipboard in the background is allowed, reading it is not. */
    private fun setClipboard(text: String) =
        main.post {
            getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText(getString(R.string.app_name), text))
        }

    private fun createChannel() {
        val channel = NotificationChannel(CHANNEL, getString(R.string.channel_sync), NotificationManager.IMPORTANCE_LOW)
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    private fun notification(connected: Int): Notification {
        val open =
            PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val stop =
            PendingIntent.getService(
                this,
                0,
                Intent(this, SyncService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val text =
            if (connected == 0) {
                getString(R.string.notification_waiting)
            } else {
                resources.getQuantityString(R.plurals.notification_connected, connected, connected)
            }
        return Notification
            .Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.notification_title))
            .setContentText(text)
            .setContentIntent(open)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, getString(R.string.send_clipboard), sendClipboardIntent(this)).build())
            .addAction(Notification.Action.Builder(null, getString(R.string.notification_stop), stop).build())
            .build()
    }

    companion object {
        fun start(context: Context) = ContextCompat.startForegroundService(context, Intent(context, SyncService::class.java))

        /** Restarts the node, e.g. with a new device name. */
        fun restart(context: Context) =
            ContextCompat.startForegroundService(context, Intent(context, SyncService::class.java).setAction(ACTION_RESTART))
    }
}

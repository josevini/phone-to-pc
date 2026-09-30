package io.github.josevini.clipsync

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.os.ParcelFileDescriptor
import android.os.PersistableBundle
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.session.FileStateStore
import io.github.josevini.clipsync.session.Node
import io.github.josevini.clipsync.session.NodeConfig
import io.github.josevini.clipsync.session.NodeEvent
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

/**
 * On a device: the activities that send, as the user reaches them — the text-selection menu, the share sheet and
 * "Send clipboard" — send through the running node to a paired device, which here is a second node on loopback.
 * "Send clipboard" reads the device's real clipboard, so this test replaces what it holds.
 */
@RunWith(AndroidJUnit4::class)
class SendFromDeviceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val aliases = listOf("clipsync-send-test-phone", "clipsync-send-test-pc")
    private val dir = File(context.cacheDir, "send-from-device-test")
    private val nodes = mutableListOf<Node>()

    /** What the paired device receives. */
    private val received = LinkedBlockingQueue<String>()

    @Before
    fun pairWithAPeer() {
        // "Send clipboard" reads only once its window has focus, which a locked screen withholds (it then waits).
        shell("input keyevent KEYCODE_WAKEUP")
        shell("wm dismiss-keyguard")
        val phoneEvents = LinkedBlockingQueue<NodeEvent>()
        val phone = start(aliases[0], "phone", clipboard = {}, listener = { phoneEvents.add(it) })
        val pc = start(aliases[1], "pc", clipboard = { received.add(it) }, listener = {})
        val id = pc.status().id
        phone.pairWithUri("clipsync://pair?v=1&id=$id&name=pc&addr=127.0.0.1:${pc.port}&token=${pc.startPairing()}")
        while (true) {
            val event = phoneEvents.poll(20, TimeUnit.SECONDS) ?: error("the nodes did not connect")
            if ((event as? NodeEvent.Engine)?.event is EngineEvent.PeerConnected) break
        }
        // What SyncService does with its node.
        Sync.attach(phone)
    }

    @After
    fun cleanUp() {
        Sync.detach()
        nodes.forEach { it.close() }
        aliases.forEach { KeystoreIdentity.delete(it) }
        dir.deleteRecursively()
    }

    private fun start(
        alias: String,
        name: String,
        clipboard: (String) -> Unit,
        listener: (NodeEvent) -> Unit,
    ): Node =
        Node(
            identity = KeystoreIdentity.loadOrCreate(alias),
            config = NodeConfig(name = name, platform = "android", port = 0),
            store = FileStateStore(File(dir, "$name.json")),
            clipboard = clipboard,
            listener = listener,
        ).also {
            it.start()
            nodes += it
        }

    /** Runs [command] as the shell user and waits for it to finish. */
    private fun shell(command: String) {
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }
    }

    private fun open(intent: Intent) = context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))

    private fun copy(
        text: String,
        sensitive: Boolean = false,
    ) {
        val clip = ClipData.newPlainText("test", text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
        }
        instrumentation.runOnMainSync { context.getSystemService(ClipboardManager::class.java).setPrimaryClip(clip) }
    }

    private fun next(): String? = received.poll(20, TimeUnit.SECONDS)

    @Test
    fun selectedTextIsSent() {
        open(
            Intent(context, SendActivity::class.java)
                .setAction(Intent.ACTION_PROCESS_TEXT)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_PROCESS_TEXT, "selected"),
        )
        assertEquals("selected", next())
    }

    @Test
    fun sharedTextIsSent() {
        open(
            Intent(context, SendActivity::class.java)
                .setAction(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_TEXT, "shared"),
        )
        assertEquals("shared", next())
    }

    @Test
    fun theClipboardIsSentButNotWhatIsMarkedSensitive() {
        copy("hunter2", sensitive = true)
        open(Intent(context, ClipboardSendActivity::class.java))
        // Give it the time a send takes, then show that the pipeline works: only the second copy arrives.
        Thread.sleep(3_000)
        copy("copied")
        open(Intent(context, ClipboardSendActivity::class.java))
        assertEquals("copied", next())
    }
}

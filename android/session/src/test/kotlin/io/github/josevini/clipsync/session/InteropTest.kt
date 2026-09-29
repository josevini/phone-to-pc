package io.github.josevini.clipsync.session

import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.LocalChange
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.nio.file.attribute.PosixFilePermissions
import java.util.concurrent.TimeUnit
import kotlin.io.path.createTempDirectory

/**
 * The Kotlin host, driving the core through its bindings, against a real `clipsyncd` on a private headless Sway:
 * what the Android app does with a PC, minus the Android platform code.
 */
class InteropTest {
    private val binDir = File(System.getProperty("clipsync.bin.dir") ?: error("run through Gradle: clipsync.bin.dir"))
    private val sway = HeadlessSway()
    private val home = createTempDirectory("clipsyncd-home").toFile()
    private val daemonLog = File(home, "daemon.log")
    private var daemon: Process? = null
    private val phone = TestNode("phone", "alice")

    @After
    fun stop() {
        phone.close()
        daemon?.destroy()
        daemon?.waitFor(5, TimeUnit.SECONDS)
        sway.close()
    }

    private fun command(vararg args: String): ProcessBuilder {
        val builder = sway.command(*args)
        builder.environment().apply {
            put("XDG_DATA_HOME", File(home, "data").path)
            put("XDG_CONFIG_HOME", File(home, "config").path)
            put("XDG_RUNTIME_DIR", File(home, "run").path)
            put("RUST_LOG", "info,clipsyncd=debug")
        }
        return builder
    }

    private fun startDaemon() {
        File(home, "config/clipsync").mkdirs()
        File(home, "config/clipsync/config.toml").writeText("name = \"pc\"\nport = 0\n")
        val run = File(home, "run").apply { mkdirs() }
        Files.setPosixFilePermissions(run.toPath(), PosixFilePermissions.fromString("rwx------"))
        daemon =
            command(File(binDir, "clipsyncd").path)
                .redirectErrorStream(true)
                .redirectOutput(daemonLog)
                .start()
        waitUntil("the daemon's control socket") {
            File(run, "clipsync.sock").exists() && command(File(binDir, "clipsync").path, "status").start().waitFor() == 0
        }
    }

    private fun fail(message: String): Nothing = throw AssertionError("$message\n--- daemon log ---\n${daemonLog.readText()}")

    @Test
    fun `the phone pairs with a PC's QR code and they sync both ways`() {
        startDaemon()
        val pairing =
            command(File(binDir, "clipsync").path, "pair")
                .redirectErrorStream(true)
                .start()
        val uri =
            pairing.inputStream
                .bufferedReader()
                .lineSequence()
                .firstOrNull { it.startsWith("clipsync://") } ?: fail("clipsync pair printed no URI")

        phone.node.pairWithUri(uri)
        val paired = phone.recorder.await("pairing") { ((it as? NodeEvent.Engine)?.event as? EngineEvent.Paired)?.device }
        assertEquals("pc", paired.name)
        if (!pairing.waitFor(WAIT_SECONDS, TimeUnit.SECONDS) || pairing.exitValue() != 0) fail("clipsync pair did not finish")

        sway.copy("copied on the PC ✓")
        assertEquals("copied on the PC ✓", phone.recorder.awaitClipboard())

        assertEquals(LocalChange.Sent(2u, 1u), phone.node.sendText("sent from the phone"))
        waitUntil("the PC's clipboard") { sway.paste() == "sent from the phone" }
    }
}

/** A headless Sway with its own runtime directory: a private compositor and clipboard. */
class HeadlessSway : AutoCloseable {
    private val dir = createTempDirectory("sway").toFile()
    private val process: Process
    val socket: File

    init {
        Files.setPosixFilePermissions(dir.toPath(), PosixFilePermissions.fromString("rwx------"))
        process =
            // Sway switches to realtime scheduling when it may (CAP_SYS_NICE on its binary, or RLIMIT_RTPRIO), and
            // Gradle's test workers run with a realtime CPU budget (RLIMIT_RTTIME) of 0, so the kernel would kill it
            // at once. No new privileges drops the file capability; a zero RLIMIT_RTPRIO covers the rest.
            ProcessBuilder("setpriv", "--no-new-privs", "prlimit", "--rtprio=0", "sway", "-c", "/dev/null")
                .also {
                    it.environment().apply {
                        clear()
                        put("PATH", System.getenv("PATH"))
                        put("HOME", dir.path)
                        put("XDG_RUNTIME_DIR", dir.path)
                        put("WLR_BACKENDS", "headless")
                        // Software rendering: CI runners have no GPU.
                        put("WLR_RENDERER", "pixman")
                        put("WLR_LIBINPUT_NO_DEVICES", "1")
                    }
                }.redirectOutput(ProcessBuilder.Redirect.DISCARD)
                .redirectError(ProcessBuilder.Redirect.DISCARD)
                .start()
        var found: File? = null
        waitUntil("sway's Wayland socket") {
            found = dir.listFiles()!!.firstOrNull { it.name.startsWith("wayland-") && !it.name.endsWith(".lock") }
            found != null
        }
        socket = found!!
    }

    /** A command that talks to this compositor only. */
    fun command(vararg args: String): ProcessBuilder =
        ProcessBuilder(*args).also {
            it.environment().apply {
                clear()
                put("PATH", System.getenv("PATH"))
                put("HOME", dir.path)
                put("WAYLAND_DISPLAY", socket.path)
            }
        }

    /** Copies [text] as another client would (wl-copy stays behind serving it). */
    fun copy(text: String) {
        val copy = command("wl-copy").redirectError(ProcessBuilder.Redirect.DISCARD).start()
        copy.outputStream.use { it.write(text.encodeToByteArray()) }
        check(copy.waitFor() == 0) { "wl-copy failed" }
    }

    fun paste(): String {
        val paste = command("wl-paste", "--no-newline").redirectError(ProcessBuilder.Redirect.DISCARD).start()
        return paste.inputStream
            .readBytes()
            .decodeToString()
            .also { paste.waitFor() }
    }

    override fun close() {
        process.destroy()
        process.waitFor(5, TimeUnit.SECONDS)
        dir.deleteRecursively()
    }
}

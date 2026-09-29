package io.github.josevini.clipsync.session

import io.github.josevini.clipsync.core.DiscoveredPeer
import io.github.josevini.clipsync.core.Engine
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.Intent
import io.github.josevini.clipsync.core.LocalChange
import io.github.josevini.clipsync.core.LocalDevice
import io.github.josevini.clipsync.core.Output
import io.github.josevini.clipsync.core.PairedDevice
import io.github.josevini.clipsync.core.Role
import io.github.josevini.clipsync.core.SocketAddress
import io.github.josevini.clipsync.core.defaultPort
import io.github.josevini.clipsync.core.parsePairUri
import io.github.josevini.clipsync.core.shortId
import java.io.IOException
import java.net.InetAddress
import java.net.InetSocketAddress
import java.util.concurrent.Callable
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadFactory
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket

/** How often the engine's timers are driven and failed dials retried. */
private const val TICK_MS = 1_000L
private const val RETRY_MIN_MS = 1_000L
private const val RETRY_MAX_MS = 60_000L

data class NodeConfig(
    /** Name shown to other devices (1–64 bytes). */
    val name: String,
    /** `hello.platform`. */
    val platform: String,
    /** TCP port to listen on; 0 lets the system choose. */
    val port: Int = defaultPort().toInt(),
)

/** What a node reports to its host. Delivered on the node's own thread: don't block, and don't call back in. */
sealed interface NodeEvent {
    data class Engine(
        val event: EngineEvent,
    ) : NodeEvent

    data class DialFailed(
        val addrs: List<SocketAddress>,
        val error: String,
    ) : NodeEvent

    /** A connection this device dialed to pair is up; later events name it by [conn]. */
    data class PairingConnection(
        val conn: ULong,
    ) : NodeEvent
}

fun interface NodeListener {
    fun onEvent(event: NodeEvent)
}

data class NodeStatus(
    val id: String,
    val name: String,
    val port: Int,
    val pairing: Boolean,
    val devices: List<DeviceStatus>,
)

data class DeviceStatus(
    val id: String,
    val name: String,
    val connected: Boolean,
)

/**
 * One device on the network: owns the protocol [Engine] and everything it drives — connections, dialing, the
 * clipboard and the saved state. The Android counterpart of the Linux daemon's actor.
 *
 * All engine calls and state changes run on one thread, the actor; sockets are read and written on other threads
 * that hand their work to it. [clipboard] receives the text of clips to apply.
 */
class Node(
    private val identity: Identity,
    private val config: NodeConfig,
    private val store: StateStore,
    private val clipboard: (String) -> Unit,
    private val listener: NodeListener,
    private val clock: () -> Long = System::currentTimeMillis,
) : AutoCloseable {
    private val tls = Tls(identity)
    private val actor = Executors.newSingleThreadScheduledExecutor(named("clipsync-node"))
    private val io = Executors.newCachedThreadPool(named("clipsync-io"))
    private val nextConn = AtomicLong()

    // Owned by the actor thread.
    private var state = store.load()
    private val engine =
        Engine(
            LocalDevice(identity.id, config.name, config.platform),
            state.paired.map { PairedDevice(it.id, it.name) },
            state.lamport.toULong(),
        )
    private val connections = HashMap<ULong, Connection>()

    /** Addresses to reconnect to: where paired devices were reached or found. */
    private val targets = LinkedHashMap<InetSocketAddress, Target>()

    private lateinit var server: SSLServerSocket

    @Volatile private var closed = false

    @Volatile private var actorThread: Thread? = null

    /** The port this node listens on. */
    val port: Int get() = server.localPort

    /** Binds the listener and starts accepting, dialing and driving the engine's timers. */
    fun start() {
        server = tls.listen(InetSocketAddress(config.port))
        post {
            for (device in state.paired) {
                for (addr in device.addrs) targets[addr.toInet()] = Target(device.id)
            }
        }
        io.execute(::acceptLoop)
        actor.scheduleWithFixedDelay({ run { tick() } }, 0, TICK_MS, TimeUnit.MILLISECONDS)
    }

    fun status(): NodeStatus =
        ask {
            val devices = state.paired.map { DeviceStatus(it.id, it.name, engine.isConnected(it.id)) }
            NodeStatus(identity.id, config.name, port, engine.pairingActive(now()), devices)
        }

    /** Opens pairing mode and returns the token for a pairing URI. */
    fun startPairing(): String = ask { engine.startPairing(now()) }

    fun stopPairing() = post { engine.stopPairing() }

    /** Pairs with the device whose QR code holds [uri] (spec §7.2); throws `CoreException` for a bad URI. */
    fun pairWithUri(uri: String) {
        val parsed = parsePairUri(uri)
        post { dial(parsed.addrs.map { it.toInet() }, parsed.id, Intent.PairToken(parsed.token)) }
    }

    /** The user's decision on a pairing code. */
    fun confirmPairing(
        conn: ULong,
        accept: Boolean,
    ) = post { engine.confirmPairing(conn, accept, now()) }

    /** Sends [text] to the connected devices, as if it had been copied here. */
    fun sendText(text: String): LocalChange = ask { engine.localClipboardChanged(text, now()) }

    /** Unpairs device [id]; false if it was not paired. */
    fun unpair(id: String): Boolean =
        ask {
            val known = state.paired.any { it.id == id }
            if (known) engine.unpair(id)
            known
        }

    /** A device found by discovery: dialed there while it is paired and not connected. */
    fun discovered(peer: DiscoveredPeer) =
        post {
            for (addr in peer.addrs) targets.getOrPut(addr.toInet()) { Target(peer.id) }.device = peer.id
            redial()
        }

    override fun close() {
        if (closed) return
        closed = true
        runCatching { server.close() }
        runCatching {
            actor
                .submit {
                    connections.values.forEach { it.close() }
                    connections.clear()
                }.get(5, TimeUnit.SECONDS)
        }
        actor.shutdown()
        actor.awaitTermination(5, TimeUnit.SECONDS)
        io.shutdownNow()
    }

    // ------------------------------------------------------------ actor

    private fun now() = clock().toULong()

    /** Runs [block] on the actor, then carries out what the engine queued. */
    private fun run(block: () -> Unit) {
        actorThread = Thread.currentThread()
        try {
            block()
            drain()
        } catch (e: Exception) {
            System.err.println("clipsync: ${e.stackTraceToString()}")
        }
    }

    private fun post(block: () -> Unit) {
        try {
            actor.execute { run(block) }
        } catch (_: RejectedExecutionException) {
            // Closed: nothing left to do.
        }
    }

    private fun <T> ask(block: () -> T): T {
        if (Thread.currentThread() == actorThread) return block().also { drain() }
        return actor
            .submit(
                Callable {
                    actorThread = Thread.currentThread()
                    block().also { drain() }
                },
            ).get()
    }

    private fun tick() {
        engine.tick(now())
        redial()
    }

    private fun drain() {
        for (output in engine.pollOutputs()) {
            when (output) {
                is Output.Send -> connections[output.conn]?.send(output.bytes)
                is Output.Close -> connections.remove(output.conn)?.close()
                is Output.SetClipboard -> clipboard(output.text)
                is Output.Event -> onEvent(output.event)
            }
        }
        val lamport = engine.lamport().toLong()
        if (lamport != state.lamport) {
            state = state.copy(lamport = lamport)
            save()
        }
    }

    private fun onEvent(event: EngineEvent) {
        when (event) {
            is EngineEvent.Paired -> {
                val device = event.device
                val rest = state.paired.filter { it.id != device.id }
                state = state.copy(paired = rest + SavedDevice(device.id, device.name))
                save()
            }

            is EngineEvent.Unpaired -> {
                state = state.copy(paired = state.paired.filter { it.id != event.peer })
                targets.values.removeAll { it.device == event.peer }
                save()
            }

            is EngineEvent.PeerDisconnected -> {
                // Reconnect soon, starting the backoff over.
                for (target in targets.values.filter { it.device == event.peer }) {
                    target.retryAt = monotonicMs() + RETRY_MIN_MS
                    target.delay = RETRY_MIN_MS
                }
            }

            else -> {}
        }
        listener.onEvent(NodeEvent.Engine(event))
    }

    /** Saves the state, with the addresses each paired device can be dialed at. */
    private fun save() {
        val paired =
            state.paired.map { device ->
                val addrs = targets.filter { it.value.device == device.id }.keys.map { SavedAddress(it.address.hostAddress, it.port) }
                device.copy(addrs = addrs)
            }
        state = state.copy(paired = paired)
        try {
            store.save(state)
        } catch (e: IOException) {
            System.err.println("clipsync: could not save the state: $e")
        }
    }

    /** Dials targets whose device is paired but not connected, when their retry time has come. */
    private fun redial() {
        val now = monotonicMs()
        for ((addr, target) in targets) {
            val device = target.device
            val wanted = device == null || (state.paired.any { it.id == device } && !engine.isConnected(device))
            if (wanted && !target.dialing && now >= target.retryAt) {
                target.dialing = true
                // Pushed back until the dial reports; a successful one resets the schedule.
                target.retryAt = now + target.delay
                dial(listOf(addr), device, Intent.Session)
            }
        }
    }

    private fun opened(
        socket: SSLSocket,
        role: Role,
        peer: String,
        intent: Intent,
        dialed: InetSocketAddress?,
    ) {
        if (closed) return socket.close()
        if (dialed != null) {
            val target = targets.getOrPut(dialed) { Target(peer) }
            target.dialing = false
            target.device = peer
            target.delay = RETRY_MIN_MS
            if (state.paired.any { it.id == peer }) save()
        }
        val conn = nextConn.incrementAndGet().toULong()
        val connection = Connection(conn, socket)
        connections[conn] = connection
        if (intent != Intent.Session) listener.onEvent(NodeEvent.PairingConnection(conn))
        engine.connectionOpened(conn, role, peer, intent, now())
        // Started after the engine knows the connection, so its bytes arrive after the hello.
        io.execute { readLoop(connection) }
    }

    private fun dialFailed(
        addrs: List<InetSocketAddress>,
        error: String,
    ) {
        for (addr in addrs) {
            val target = targets[addr] ?: continue
            target.dialing = false
            target.retryAt = monotonicMs() + target.delay
            target.delay = (target.delay * 2).coerceAtMost(RETRY_MAX_MS)
        }
        listener.onEvent(NodeEvent.DialFailed(addrs.map { SocketAddress(it.address.hostAddress, it.port.toUShort()) }, error))
    }

    // ------------------------------------------------------------ sockets

    private fun acceptLoop() {
        while (!closed) {
            val socket =
                try {
                    server.accept() as SSLSocket
                } catch (_: IOException) {
                    continue
                }
            io.execute {
                try {
                    val (tlsSocket, peer) = tls.handshake(socket)
                    post { opened(tlsSocket, Role.ACCEPTOR, peer, Intent.Session, null) }
                } catch (_: Exception) {
                    socket.close()
                }
            }
        }
    }

    /** Dials [addrs] in order until one works; reports the outcome to the actor. */
    private fun dial(
        addrs: List<InetSocketAddress>,
        expected: String?,
        intent: Intent,
    ) {
        io.execute {
            var error = "no address to dial"
            for (addr in addrs) {
                try {
                    val (socket, peer) = tls.connect(addr)
                    if (expected != null && peer != expected) {
                        socket.close()
                        throw IOException(
                            "${addr.hostString}:${addr.port} is device ${shortId(peer)}, not the expected ${shortId(expected)}",
                        )
                    }
                    return@execute post { opened(socket, Role.DIALER, peer, intent, addr) }
                } catch (e: Exception) {
                    error = e.message ?: e.toString()
                }
            }
            post { dialFailed(addrs, error) }
        }
    }

    private fun readLoop(connection: Connection) {
        val input = connection.socket.inputStream
        val buffer = ByteArray(16 * 1024)
        try {
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                val bytes = buffer.copyOf(n)
                post { engine.bytesReceived(connection.id, bytes, now()) }
            }
        } catch (_: IOException) {
            // Closed by either side.
        }
        post {
            // Absent if the engine closed it already.
            if (connections.remove(connection.id) != null) {
                connection.close()
                engine.connectionClosed(connection.id)
            }
        }
    }

    /** One TLS connection; writes happen in order on its own thread, so the actor never blocks on a socket. */
    private class Connection(
        val id: ULong,
        val socket: SSLSocket,
    ) {
        private val writer: ExecutorService = Executors.newSingleThreadExecutor(named("clipsync-write"))

        fun send(bytes: ByteArray) =
            writer.execute {
                try {
                    socket.outputStream.write(bytes)
                    socket.outputStream.flush()
                } catch (_: IOException) {
                    runCatching { socket.close() }
                }
            }

        /** Closes the socket once what was queued before has been written. */
        fun close() {
            writer.execute { runCatching { socket.close() } }
            writer.shutdown()
        }
    }

    /** An address to keep dialing, with its retry schedule. */
    private class Target(
        /** The device found there, once known. */
        var device: String?,
    ) {
        var dialing = false
        var retryAt = 0L
        var delay = RETRY_MIN_MS
    }
}

private fun monotonicMs() = System.nanoTime() / 1_000_000

private fun SavedAddress.toInet() = InetSocketAddress(InetAddress.getByName(ip), port)

private fun SocketAddress.toInet() = InetSocketAddress(InetAddress.getByName(ip), port.toInt())

private fun named(name: String) =
    ThreadFactory { runnable ->
        Thread(runnable, name).apply { isDaemon = true }
    }

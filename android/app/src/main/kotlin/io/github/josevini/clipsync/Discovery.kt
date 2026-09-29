package io.github.josevini.clipsync

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log
import io.github.josevini.clipsync.core.instanceName
import io.github.josevini.clipsync.core.peerFromService
import io.github.josevini.clipsync.core.serviceType
import io.github.josevini.clipsync.core.txtProperties
import io.github.josevini.clipsync.session.Node
import java.util.concurrent.Executor
import java.util.concurrent.Executors

private const val TAG = "clipsync"

/**
 * DNS-SD with [NsdManager] (spec §3): advertises this device and reports the clipsync devices it finds to [node],
 * which dials the paired ones.
 */
class Discovery(
    context: Context,
    private val node: Node,
    private val id: String,
    private val name: String,
    private val port: Int,
) {
    private val nsd = context.getSystemService(NsdManager::class.java)
    private val executor: Executor = Executors.newSingleThreadExecutor()

    // Before Android 14 NsdManager resolves one service at a time: the others wait here. Guarded by `this`.
    private val pending = ArrayDeque<NsdServiceInfo>()
    private var resolving = false
    private val watching = mutableMapOf<String, NsdManager.ServiceInfoCallback>()

    private val registration =
        object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(info: NsdServiceInfo) {
                Log.i(TAG, "advertised as ${info.serviceName}")
            }

            override fun onRegistrationFailed(
                info: NsdServiceInfo,
                error: Int,
            ) {
                Log.w(TAG, "could not advertise: error $error")
            }

            override fun onServiceUnregistered(info: NsdServiceInfo) {}

            override fun onUnregistrationFailed(
                info: NsdServiceInfo,
                error: Int,
            ) {}
        }

    private val browsing =
        object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {}

            override fun onDiscoveryStopped(serviceType: String) {}

            override fun onStartDiscoveryFailed(
                serviceType: String,
                error: Int,
            ) {
                Log.w(TAG, "could not browse: error $error")
            }

            override fun onStopDiscoveryFailed(
                serviceType: String,
                error: Int,
            ) {}

            override fun onServiceFound(info: NsdServiceInfo) = resolve(info)

            override fun onServiceLost(info: NsdServiceInfo) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                    synchronized(this@Discovery) { watching.remove(info.serviceName) }?.let { nsd.unregisterServiceInfoCallback(it) }
                }
            }
        }

    fun start() {
        val info =
            NsdServiceInfo().apply {
                serviceName = instanceName(name, id)
                serviceType = serviceType()
                port = this@Discovery.port
                for ((key, value) in txtProperties(id)) setAttribute(key, value)
            }
        nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, registration)
        nsd.discoverServices(serviceType(), NsdManager.PROTOCOL_DNS_SD, browsing)
    }

    fun stop() {
        runCatching { nsd.unregisterService(registration) }
        runCatching { nsd.stopServiceDiscovery(browsing) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val callbacks = synchronized(this) { watching.values.toList().also { watching.clear() } }
            callbacks.forEach { runCatching { nsd.unregisterServiceInfoCallback(it) } }
        }
    }

    private fun resolve(info: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            watch(info)
        } else {
            synchronized(this) {
                pending.addLast(info)
                if (resolving) return
                resolving = true
            }
            resolveNext()
        }
    }

    /** Android 14+: follows the service, so a device that changes address is reported again. */
    private fun watch(info: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return
        val callback =
            object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(error: Int) {}

                override fun onServiceUpdated(info: NsdServiceInfo) = report(info, info.hostAddresses.map { it.hostAddress.orEmpty() })

                override fun onServiceLost() {}

                override fun onServiceInfoCallbackUnregistered() {}
            }
        synchronized(this) {
            if (info.serviceName in watching) return
            watching[info.serviceName] = callback
        }
        nsd.registerServiceInfoCallback(info, executor, callback)
    }

    @Suppress("DEPRECATION") // resolveService is the only way before Android 14.
    private fun resolveNext() {
        val next = synchronized(this) { pending.removeFirstOrNull().also { if (it == null) resolving = false } } ?: return
        nsd.resolveService(
            next,
            object : NsdManager.ResolveListener {
                override fun onResolveFailed(
                    info: NsdServiceInfo,
                    error: Int,
                ) = resolveNext()

                override fun onServiceResolved(info: NsdServiceInfo) {
                    report(info, listOfNotNull(info.host?.hostAddress))
                    resolveNext()
                }
            },
        )
    }

    private fun report(
        info: NsdServiceInfo,
        ips: List<String>,
    ) {
        val txt = info.attributes.mapValues { (_, value) -> value?.decodeToString().orEmpty() }
        // Scoped IPv6 addresses ("fe80::1%wlan0") do not parse and are skipped, as the core skips link-local ones.
        val peer = peerFromService(id, txt, info.port.toUShort(), ips) ?: return
        node.discovered(peer)
    }
}

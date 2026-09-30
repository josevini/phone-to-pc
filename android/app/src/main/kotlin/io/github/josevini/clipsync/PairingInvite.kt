package io.github.josevini.clipsync

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.SocketAddress
import io.github.josevini.clipsync.session.NodeEvent
import java.net.Inet4Address
import java.net.InetAddress

/** How this phone's own pairing code ended. */
sealed interface InviteOutcome {
    data class Paired(
        val name: String,
    ) : InviteOutcome

    /** Pairing mode ended without a pairing: the code expired, or was guessed wrong too often (spec §7.2). */
    data object Ended : InviteOutcome
}

/** Follows the pairing code this phone shows (it is the acceptor) through the node's events. */
class InviteTracker(
    private val alreadyPaired: Set<String>,
) {
    /** The outcome once [event] settles it, null until then. */
    fun on(event: NodeEvent): InviteOutcome? =
        when (val e = (event as? NodeEvent.Engine)?.event) {
            // A paired device changing its name is reported as Paired too.
            is EngineEvent.Paired -> if (e.device.id in alreadyPaired) null else InviteOutcome.Paired(e.device.name)

            is EngineEvent.PairingModeEnded -> InviteOutcome.Ended

            else -> null
        }
}

/**
 * The addresses to put in the pairing code (spec §8): [ips] other devices can reach this one on, IPv4 first, as the
 * dialer tries them in order.
 */
fun pairingAddresses(
    ips: List<InetAddress>,
    port: Int,
): List<SocketAddress> =
    ips
        .filterNot { it.isLoopbackAddress || it.isLinkLocalAddress || it.isAnyLocalAddress }
        .distinct()
        .sortedBy { it !is Inet4Address }
        .map { SocketAddress(it.hostAddress!!.substringBefore('%'), port.toUShort()) }

/** This phone's addresses on local networks (Wi-Fi and Ethernet); mobile data does not reach other devices. */
fun localNetworkAddresses(context: Context): List<InetAddress> {
    val connectivity = context.getSystemService(ConnectivityManager::class.java)

    // One read when the code is shown; the replacement, a NetworkCallback, would follow every later change.
    @Suppress("DEPRECATION")
    val networks = connectivity.allNetworks
    return networks
        .filter { network ->
            val capabilities = connectivity.getNetworkCapabilities(network) ?: return@filter false
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
        }.flatMap { connectivity.getLinkProperties(it)?.linkAddresses.orEmpty() }
        .map { it.address }
}

/** The modules of a QR code holding [text], one per matrix cell, without the quiet zone. */
fun qrModules(text: String): BitMatrix =
    QRCodeWriter().encode(
        text,
        BarcodeFormat.QR_CODE,
        0,
        0,
        mapOf(EncodeHintType.MARGIN to 0, EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M),
    )

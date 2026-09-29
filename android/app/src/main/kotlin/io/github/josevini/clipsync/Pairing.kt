package io.github.josevini.clipsync

import io.github.josevini.clipsync.core.CloseReason
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.SocketAddress
import io.github.josevini.clipsync.session.NodeEvent

/** Why a pairing did not complete, as the user can act on it. */
enum class PairingFailure {
    /** The QR code was already used or has expired: show a new one. */
    CODE_EXPIRED,

    /** The other device closed pairing mode. */
    NOT_IN_PAIRING_MODE,

    /** The other device declined. */
    REJECTED,

    /** The other device speaks another protocol version. */
    INCOMPATIBLE,

    /** No address in the QR code could be reached: another network, or a firewall. */
    UNREACHABLE,

    CONNECTION_LOST,
}

sealed interface PairingOutcome {
    data class Paired(
        val name: String,
    ) : PairingOutcome

    data class Failed(
        val failure: PairingFailure,
    ) : PairingOutcome
}

/** Follows one pairing through a node's events: the device [peer] from a QR code with [addrs]. */
class PairingTracker(
    private val peer: String,
    private val addrs: List<SocketAddress>,
) {
    private var conn: ULong? = null

    /** The outcome once [event] settles it, null until then. */
    fun on(event: NodeEvent): PairingOutcome? =
        when (event) {
            is NodeEvent.PairingConnection -> {
                conn = event.conn
                null
            }

            is NodeEvent.DialFailed -> {
                if (event.addrs == addrs) PairingOutcome.Failed(PairingFailure.UNREACHABLE) else null
            }

            is NodeEvent.Engine -> {
                when (val e = event.event) {
                    is EngineEvent.Paired -> if (e.device.id == peer) PairingOutcome.Paired(e.device.name) else null
                    is EngineEvent.ConnectionClosed -> if (e.conn == conn) PairingOutcome.Failed(failure(e.reason)) else null
                    else -> null
                }
            }
        }

    private fun failure(reason: CloseReason): PairingFailure =
        when (reason) {
            is CloseReason.RemoteError -> {
                when (reason.code) {
                    "bad_token" -> PairingFailure.CODE_EXPIRED
                    "pairing_closed", "not_paired" -> PairingFailure.NOT_IN_PAIRING_MODE
                    "unsupported_version" -> PairingFailure.INCOMPATIBLE
                    else -> PairingFailure.CONNECTION_LOST
                }
            }

            CloseReason.RejectedByPeer -> {
                PairingFailure.REJECTED
            }

            CloseReason.UnsupportedVersion -> {
                PairingFailure.INCOMPATIBLE
            }

            else -> {
                PairingFailure.CONNECTION_LOST
            }
        }
}

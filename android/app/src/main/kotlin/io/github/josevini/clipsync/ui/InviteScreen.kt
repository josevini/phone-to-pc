package io.github.josevini.clipsync.ui

import android.os.SystemClock
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.josevini.clipsync.InviteOutcome
import io.github.josevini.clipsync.InviteTracker
import io.github.josevini.clipsync.R
import io.github.josevini.clipsync.Sync
import io.github.josevini.clipsync.core.EngineEvent
import io.github.josevini.clipsync.core.PairUri
import io.github.josevini.clipsync.core.formatPairUri
import io.github.josevini.clipsync.core.pairingWindowMs
import io.github.josevini.clipsync.countdown
import io.github.josevini.clipsync.localNetworkAddresses
import io.github.josevini.clipsync.pairingAddresses
import io.github.josevini.clipsync.qrModules
import io.github.josevini.clipsync.session.NodeEvent
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.withContext

private sealed interface InviteState {
    data object Starting : InviteState

    /** No Wi-Fi or Ethernet address to put in the code. */
    data object NoNetwork : InviteState

    data class Showing(
        val uri: String,
        /** [SystemClock.elapsedRealtime] when the code stops working. */
        val expiresAt: Long,
    ) : InviteState

    data class Paired(
        val name: String,
    ) : InviteState

    data object Expired : InviteState
}

/**
 * Shows this phone's pairing code, for another phone to scan (spec §7.2: this phone is the acceptor). Leaving the
 * screen closes pairing mode.
 */
@Composable
fun InviteScreen(onDone: () -> Unit) {
    val context = LocalContext.current
    var attempt by rememberSaveable { mutableIntStateOf(0) }
    var state by remember { mutableStateOf<InviteState>(InviteState.Starting) }

    LaunchedEffect(attempt) {
        state = InviteState.Starting
        val node = Sync.node ?: return@LaunchedEffect
        val status = withContext(Dispatchers.IO) { node.status() }
        val addrs = pairingAddresses(localNetworkAddresses(context), status.port)
        if (addrs.isEmpty()) {
            state = InviteState.NoNetwork
            return@LaunchedEffect
        }
        val tracker = InviteTracker(status.devices.map { it.id }.toSet())
        // Subscribe before opening pairing mode, so no event is missed.
        val outcome =
            async(start = CoroutineStart.UNDISPATCHED) {
                Sync.events
                    .mapNotNull { event ->
                        // Comparing codes (a PC's `clipsync pair <address>`) is not offered here: decline it.
                        val code = (event as? NodeEvent.Engine)?.event as? EngineEvent.PairingCode
                        if (code != null) node.confirmPairing(code.conn, false)
                        tracker.on(event)
                    }.first()
            }
        val token = withContext(Dispatchers.IO) { node.startPairing() }
        val expiresAt = SystemClock.elapsedRealtime() + pairingWindowMs().toLong()
        state = InviteState.Showing(formatPairUri(PairUri(status.id, status.name, addrs, token)), expiresAt)
        state =
            when (val result = outcome.await()) {
                is InviteOutcome.Paired -> InviteState.Paired(result.name)
                InviteOutcome.Ended -> InviteState.Expired
            }
    }
    DisposableEffect(Unit) { onDispose { Sync.node?.stopPairing() } }

    CollapsingScaffold(
        title = stringResource(R.string.show_code),
        expandable = false,
        navigation = { BackButton(onDone, stringResource(R.string.cancel)) },
    ) {
        item {
            when (val current = state) {
                InviteState.Starting -> {
                    Hero(
                        title = stringResource(R.string.starting),
                        visual = { CircularProgressIndicator(Modifier.size(72.dp)) },
                    ) { OutlinedButton(onClick = onDone) { Text(stringResource(R.string.cancel)) } }
                }

                InviteState.NoNetwork -> {
                    Hero(
                        title = stringResource(R.string.invite_no_network),
                        text = stringResource(R.string.invite_no_network_text),
                        visual = { LargeIcon(R.drawable.ic_link_off, tint = MaterialTheme.colorScheme.error) },
                    ) {
                        Button(onClick = { attempt++ }, Modifier.widthIn(min = 160.dp)) { Text(stringResource(R.string.try_again)) }
                        TextButton(onClick = onDone) { Text(stringResource(R.string.cancel)) }
                    }
                }

                is InviteState.Showing -> {
                    Showing(current, onCancel = onDone)
                }

                is InviteState.Paired -> {
                    Hero(
                        title = stringResource(R.string.paired_with, current.name),
                        text = stringResource(R.string.invite_paired_hint),
                        visual = { LargeIcon(R.drawable.ic_check_circle_filled) },
                    ) { Button(onClick = onDone, Modifier.widthIn(min = 160.dp)) { Text(stringResource(R.string.done)) } }
                }

                InviteState.Expired -> {
                    Hero(
                        title = stringResource(R.string.invite_expired),
                        text = stringResource(R.string.invite_expired_text),
                        visual = { LargeIcon(R.drawable.ic_error_filled, tint = MaterialTheme.colorScheme.error) },
                    ) {
                        Button(onClick = { attempt++ }, Modifier.widthIn(min = 160.dp)) { Text(stringResource(R.string.invite_new_code)) }
                        TextButton(onClick = onDone) { Text(stringResource(R.string.cancel)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun Showing(
    state: InviteState.Showing,
    onCancel: () -> Unit,
) {
    // The other phone may take a while to scan: keep the screen on meanwhile.
    val view = LocalView.current
    DisposableEffect(view) {
        view.keepScreenOn = true
        onDispose { view.keepScreenOn = false }
    }
    var remaining by remember { mutableLongStateOf(state.expiresAt - SystemClock.elapsedRealtime()) }
    LaunchedEffect(state) {
        while (true) {
            remaining = (state.expiresAt - SystemClock.elapsedRealtime()).coerceAtLeast(0)
            delay(1_000 - remaining % 1_000)
        }
    }
    Column(
        Modifier.padding(horizontal = 12.dp).fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            stringResource(R.string.invite_instructions),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        QrCode(state.uri, Modifier.widthIn(max = 320.dp).fillMaxWidth())
        val seconds = (remaining + 999) / 1_000
        Text(
            stringResource(R.string.invite_expires_in, countdown(seconds)),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        OutlinedButton(onClick = onCancel) { Text(stringResource(R.string.cancel)) }
    }
}

/** [text] as a QR code, dark on white in both themes, as scanners read best. */
@Composable
private fun QrCode(
    text: String,
    modifier: Modifier,
) {
    val modules = remember(text) { qrModules(text) }
    Surface(modifier.aspectRatio(1f), shape = MaterialTheme.shapes.extraLarge, color = Color.White) {
        // The quiet zone around the code.
        Box(Modifier.padding(24.dp)) {
            Canvas(Modifier.fillMaxSize()) {
                val cell = size.minDimension / modules.width
                for (y in 0 until modules.height) {
                    for (x in 0 until modules.width) {
                        // Slightly larger cells, so that no hairline shows between neighbours.
                        if (modules[x, y]) drawRect(Color.Black, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
                    }
                }
            }
        }
    }
}

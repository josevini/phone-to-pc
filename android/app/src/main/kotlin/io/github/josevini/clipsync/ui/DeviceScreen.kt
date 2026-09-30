package io.github.josevini.clipsync.ui

import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import io.github.josevini.clipsync.R
import io.github.josevini.clipsync.Sync
import io.github.josevini.clipsync.core.shortId
import io.github.josevini.clipsync.session.DeviceStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A paired device: what it is, whether it is connected, and unpairing it. */
@Composable
fun DeviceScreen(
    id: String,
    device: DeviceStatus?,
    onBack: () -> Unit,
) {
    // Unpaired, here or by the other device.
    if (device == null) {
        LaunchedEffect(Unit) { onBack() }
        return
    }
    var confirming by rememberSaveable { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val colors = MaterialTheme.colorScheme
    val status = stringResource(if (device.connected) R.string.connected else R.string.not_connected)
    CollapsingScaffold(
        title = device.name,
        header = {
            LargeIcon(R.drawable.ic_computer, tint = if (device.connected) colors.primary else colors.onSurfaceVariant)
            Text(device.name, style = MaterialTheme.typography.displaySmall, textAlign = TextAlign.Center)
            Text(
                status,
                style = MaterialTheme.typography.bodyLarge,
                color = if (device.connected) colors.primary else colors.onSurfaceVariant,
            )
        },
        navigation = { BackButton(onBack) },
        bottomBar = { BottomAction(R.drawable.ic_link_off, stringResource(R.string.unpair), onClick = { confirming = true }) },
    ) {
        item {
            Group(footer = stringResource(R.string.device_id_hint, shortId(id))) {
                GroupRow(
                    title = stringResource(R.string.device_id),
                    icon = R.drawable.ic_fingerprint,
                    below = {
                        SelectionContainer {
                            Text(
                                id,
                                style = MaterialTheme.typography.bodyMedium,
                                fontFamily = FontFamily.Monospace,
                                color = colors.onSurfaceVariant,
                            )
                        }
                    },
                )
            }
        }
    }
    if (confirming) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(stringResource(R.string.unpair_title, device.name)) },
            text = { Text(stringResource(R.string.unpair_text)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirming = false
                        scope.launch { withContext(Dispatchers.IO) { Sync.node?.unpair(id) } }
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = colors.error),
                ) { Text(stringResource(R.string.unpair)) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

package io.github.josevini.clipsync.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import io.github.josevini.clipsync.R
import io.github.josevini.clipsync.Sync
import io.github.josevini.clipsync.core.shortId
import io.github.josevini.clipsync.session.DeviceStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** A paired device: what it is, whether it is connected, and unpairing it. */
@OptIn(ExperimentalMaterial3Api::class)
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
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(device.name) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.back)) }
                },
            )
        },
    ) { padding ->
        Readable(Modifier.padding(padding)) {
            Column(
                Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(
                    stringResource(if (device.connected) R.string.connected else R.string.not_connected),
                    style = MaterialTheme.typography.titleMedium,
                    color = if (device.connected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Column {
                    Text(stringResource(R.string.device_id), style = MaterialTheme.typography.labelLarge)
                    SelectionContainer {
                        Text(id, style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace)
                    }
                    Text(
                        stringResource(R.string.device_id_hint, shortId(id)),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                OutlinedButton(
                    onClick = { confirming = true },
                    colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(R.string.unpair)) }
            }
        }
    }
    if (confirming) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            title = { Text(stringResource(R.string.unpair_title, device.name)) },
            text = { Text(stringResource(R.string.unpair_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirming = false
                    scope.launch { withContext(Dispatchers.IO) { Sync.node?.unpair(id) } }
                }) { Text(stringResource(R.string.unpair)) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

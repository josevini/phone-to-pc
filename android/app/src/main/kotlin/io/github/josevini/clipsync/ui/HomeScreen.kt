package io.github.josevini.clipsync.ui

import android.annotation.SuppressLint
import android.content.Intent
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import io.github.josevini.clipsync.DeviceName
import io.github.josevini.clipsync.R
import io.github.josevini.clipsync.SyncService
import io.github.josevini.clipsync.core.isValidName
import io.github.josevini.clipsync.core.shortId
import io.github.josevini.clipsync.session.DeviceStatus
import io.github.josevini.clipsync.session.NodeStatus

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    status: NodeStatus?,
    onPair: () -> Unit,
    onDevice: (String) -> Unit,
    onAbout: () -> Unit,
) {
    var renaming by rememberSaveable { mutableStateOf(false) }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.app_name)) },
                actions = {
                    IconButton(onClick = onAbout) { Icon(Icons.Outlined.Info, stringResource(R.string.about)) }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = onPair,
                icon = { Icon(Icons.Filled.Add, null) },
                text = { Text(stringResource(R.string.pair_with_pc)) },
            )
        },
    ) { padding ->
        Readable(Modifier.padding(padding)) {
            LazyColumn(
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 96.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item { ThisDevice(status, onRename = { renaming = true }) }
                item { BatteryOptimization() }
                item {
                    Text(
                        stringResource(R.string.paired_devices),
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
                val devices = status?.devices.orEmpty()
                if (devices.isEmpty()) {
                    item {
                        Text(
                            stringResource(R.string.no_devices),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(devices, key = { it.id }) { device -> DeviceRow(device, onClick = { onDevice(device.id) }) }
            }
        }
    }
    if (renaming) RenameDialog(current = status?.name, onDismiss = { renaming = false })
}

@Composable
private fun ThisDevice(
    status: NodeStatus?,
    onRename: () -> Unit,
) {
    Card(Modifier.fillMaxWidth()) {
        ListItem(
            overlineContent = { Text(stringResource(R.string.this_device)) },
            headlineContent = { Text(status?.name ?: stringResource(R.string.starting)) },
            supportingContent = { status?.let { Text(stringResource(R.string.short_id, shortId(it.id))) } },
            trailingContent = {
                IconButton(onClick = onRename, enabled = status != null) {
                    Icon(Icons.Outlined.Edit, stringResource(R.string.rename))
                }
            },
            colors =
                androidx.compose.material3.ListItemDefaults
                    .colors(containerColor = CardDefaults.cardColors().containerColor),
        )
    }
}

@Composable
private fun DeviceRow(
    device: DeviceStatus,
    onClick: () -> Unit,
) {
    Card(onClick = onClick, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Surface(
                shape = CircleShape,
                color = MaterialTheme.colorScheme.primaryContainer,
                modifier = Modifier.size(40.dp),
            ) {
                Column(verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        device.name.take(1).uppercase(),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                        textAlign = TextAlign.Center,
                    )
                }
            }
            Spacer(Modifier.size(16.dp))
            Column(Modifier.weight(1f)) {
                Text(device.name, style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(if (device.connected) R.string.connected else R.string.not_connected),
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (device.connected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Offers to exempt the app from battery optimisation, which would otherwise cut its connections while the phone sleeps. */
@SuppressLint("BatteryLife") // Keeping connections to paired devices open is the app's purpose.
@Composable
private fun BatteryOptimization() {
    val context = LocalContext.current
    val power = remember { context.getSystemService(PowerManager::class.java) }
    var exempt by remember { mutableStateOf(true) }
    LifecycleResumeEffect(Unit) {
        exempt = power.isIgnoringBatteryOptimizations(context.packageName)
        onPauseOrDispose {}
    }
    if (exempt) return
    Card(
        Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.secondaryContainer),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(stringResource(R.string.battery_title), style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.battery_text), style = MaterialTheme.typography.bodyMedium)
            TextButton(
                onClick = {
                    val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri())
                    context.startActivity(intent)
                },
                modifier = Modifier.align(Alignment.End),
            ) { Text(stringResource(R.string.battery_allow)) }
        }
    }
}

@Composable
private fun RenameDialog(
    current: String?,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var name by rememberSaveable { mutableStateOf(current ?: DeviceName.get(context)) }
    val valid = isValidName(name.trim())
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.rename_title)) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                singleLine = true,
                isError = !valid,
                supportingText = { Text(stringResource(if (valid) R.string.rename_hint else R.string.rename_invalid)) },
            )
        },
        confirmButton = {
            TextButton(
                enabled = valid,
                onClick = {
                    if (DeviceName.set(context, name)) SyncService.restart(context)
                    onDismiss()
                },
            ) { Text(stringResource(R.string.save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

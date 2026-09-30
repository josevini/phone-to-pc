package io.github.josevini.clipsync.ui

import android.annotation.SuppressLint
import android.content.Intent
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
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
import io.github.josevini.clipsync.session.NodeStatus

@Composable
fun HomeScreen(
    status: NodeStatus?,
    onPair: () -> Unit,
    onDevice: (String) -> Unit,
    onAbout: () -> Unit,
) {
    var renaming by rememberSaveable { mutableStateOf(false) }
    val exempt = batteryExempt()
    val devices = status?.devices.orEmpty()
    val colors = MaterialTheme.colorScheme
    CollapsingScaffold(
        title = stringResource(R.string.app_name),
        subtitle = summary(status),
        actions = { MoreMenu(onAbout) },
    ) {
        if (!exempt) item { BatteryOptimization() }
        item {
            Group(title = stringResource(R.string.this_device)) {
                GroupRow(
                    title = status?.name ?: stringResource(R.string.starting),
                    icon = R.drawable.ic_smartphone,
                    summary = status?.let { stringResource(R.string.short_id, shortId(it.id)) },
                    onClickLabel = stringResource(R.string.rename),
                    onClick = if (status != null) ({ renaming = true }) else null,
                )
            }
        }
        item {
            Group(title = stringResource(R.string.paired_devices)) {
                devices.forEach { device ->
                    GroupRow(
                        title = device.name,
                        icon = R.drawable.ic_computer,
                        iconTint = if (device.connected) colors.primary else colors.onSurfaceVariant,
                        summary = stringResource(if (device.connected) R.string.connected else R.string.not_connected),
                        summaryColor = if (device.connected) colors.primary else colors.onSurfaceVariant,
                        onClick = { onDevice(device.id) },
                    )
                    GroupDivider()
                }
                if (devices.isEmpty()) {
                    NoDevices()
                    GroupDivider(afterIcon = false)
                }
                GroupRow(
                    title = stringResource(R.string.pair_with_pc),
                    icon = R.drawable.ic_qr_code_scanner,
                    titleColor = colors.primary,
                    onClick = onPair,
                )
            }
        }
        if (devices.isNotEmpty()) {
            item {
                SuggestionCard(
                    title = stringResource(R.string.tip_send_title),
                    text = stringResource(R.string.tip_send_text),
                    icon = R.drawable.ic_send,
                )
            }
        }
    }
    if (renaming) RenameDialog(current = status?.name, onDismiss = { renaming = false })
}

/** What the phone is doing, under the large title. */
@Composable
private fun summary(status: NodeStatus?): String {
    val devices = status?.devices ?: return stringResource(R.string.starting)
    val connected = devices.count { it.connected }
    return when {
        devices.isEmpty() -> stringResource(R.string.no_devices_title)
        connected == 0 -> stringResource(R.string.notification_waiting)
        else -> pluralStringResource(R.plurals.notification_connected, connected, connected)
    }
}

@Composable
private fun MoreMenu(onAbout: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) { Icon(painterResource(R.drawable.ic_more_vert), stringResource(R.string.more_options)) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.about)) },
                onClick = {
                    open = false
                    onAbout()
                },
            )
        }
    }
}

@Composable
private fun NoDevices() {
    Column(
        Modifier.fillMaxWidth().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        LargeIcon(R.drawable.ic_devices, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            stringResource(R.string.no_devices),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Command("clipsync pair")
    }
}

/** Whether the app is exempt from battery optimisation, checked again each time the screen comes back. */
@Composable
private fun batteryExempt(): Boolean {
    val context = LocalContext.current
    val power = remember { context.getSystemService(PowerManager::class.java) }
    var exempt by remember { mutableStateOf(true) }
    LifecycleResumeEffect(Unit) {
        exempt = power.isIgnoringBatteryOptimizations(context.packageName)
        onPauseOrDispose {}
    }
    return exempt
}

/** Offers to exempt the app from battery optimisation, which would otherwise cut its connections while the phone sleeps. */
@SuppressLint("BatteryLife") // Keeping connections to paired devices open is the app's purpose.
@Composable
private fun BatteryOptimization() {
    val context = LocalContext.current
    SuggestionCard(
        title = stringResource(R.string.battery_title),
        text = stringResource(R.string.battery_text),
        icon = R.drawable.ic_battery_alert,
        action = stringResource(R.string.battery_allow),
        onAction = {
            val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri())
            context.startActivity(intent)
        },
    )
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

package io.github.josevini.clipsync.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.github.josevini.clipsync.Sync

private const val HOME = "home"
private const val PAIR = "pair"
private const val ABOUT = "about"
private const val DEVICE = "device:"

@Composable
fun App() {
    var route by rememberSaveable { mutableStateOf(HOME) }
    val status by Sync.status.collectAsStateWithLifecycle()
    val home = { route = HOME }
    BackHandler(enabled = route != HOME, onBack = home)
    RequestNotifications()
    when {
        route == PAIR -> {
            PairScreen(onDone = home)
        }

        route == ABOUT -> {
            AboutScreen(onBack = home)
        }

        route.startsWith(DEVICE) -> {
            val id = route.removePrefix(DEVICE)
            DeviceScreen(id = id, device = status?.devices?.find { it.id == id }, onBack = home)
        }

        else -> {
            HomeScreen(status, onPair = { route = PAIR }, onDevice = { route = DEVICE + it }, onAbout = { route = ABOUT })
        }
    }
}

/** The foreground service's notification needs this permission on Android 13+; the service runs either way. */
@Composable
private fun RequestNotifications() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    LaunchedEffect(Unit) {
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS)
        if (granted != PackageManager.PERMISSION_GRANTED) launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
}

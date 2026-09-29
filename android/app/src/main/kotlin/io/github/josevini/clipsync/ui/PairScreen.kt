package io.github.josevini.clipsync.ui

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.josevini.clipsync.PairingFailure
import io.github.josevini.clipsync.PairingOutcome
import io.github.josevini.clipsync.PairingTracker
import io.github.josevini.clipsync.R
import io.github.josevini.clipsync.Sync
import io.github.josevini.clipsync.core.CoreException
import io.github.josevini.clipsync.core.parsePairUri
import io.github.josevini.clipsync.decodeQr
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.mapNotNull
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.time.Duration.Companion.seconds

private sealed interface PairState {
    data object Scanning : PairState

    data object InvalidCode : PairState

    data class Pairing(
        val name: String,
    ) : PairState

    data class Paired(
        val name: String,
    ) : PairState

    data class Failed(
        val failure: PairingFailure,
    ) : PairState
}

/** Pairs with a PC by scanning the QR code `clipsync pair` shows, or by pasting its link (spec §7.2). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairScreen(onDone: () -> Unit) {
    var state by remember { mutableStateOf<PairState>(PairState.Scanning) }
    val scope = rememberCoroutineScope()

    fun pair(link: String) {
        val node = Sync.node ?: return
        val uri = link.trim()
        val parsed =
            try {
                parsePairUri(uri)
            } catch (_: CoreException) {
                state = PairState.InvalidCode
                return
            }
        state = PairState.Pairing(parsed.name)
        val tracker = PairingTracker(parsed.id, parsed.addrs)
        // Subscribe before dialing, so no event is missed.
        scope.launch(start = CoroutineStart.UNDISPATCHED) {
            val outcome = withTimeoutOrNull(30.seconds) { Sync.events.mapNotNull { tracker.on(it) }.first() }
            state =
                when (outcome) {
                    is PairingOutcome.Paired -> PairState.Paired(outcome.name)
                    is PairingOutcome.Failed -> PairState.Failed(outcome.failure)
                    null -> PairState.Failed(PairingFailure.UNREACHABLE)
                }
        }
        node.pairWithUri(uri)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.pair_with_pc)) },
                navigationIcon = {
                    IconButton(onClick = onDone) { Icon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.cancel)) }
                },
            )
        },
    ) { padding ->
        Readable(Modifier.padding(padding)) {
            Column(
                Modifier.verticalScroll(rememberScrollState()).padding(16.dp).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                when (val current = state) {
                    PairState.Scanning, PairState.InvalidCode -> {
                        Scan(invalid = current == PairState.InvalidCode, onCode = ::pair)
                    }

                    is PairState.Pairing -> {
                        CircularProgressIndicator(Modifier.padding(top = 32.dp))
                        Text(stringResource(R.string.pairing_with, current.name), style = MaterialTheme.typography.titleMedium)
                        OutlinedButton(onClick = onDone) { Text(stringResource(R.string.cancel)) }
                    }

                    is PairState.Paired -> {
                        Icon(
                            Icons.Filled.CheckCircle,
                            null,
                            Modifier.size(64.dp).padding(top = 16.dp),
                            tint = MaterialTheme.colorScheme.primary,
                        )
                        Text(
                            stringResource(R.string.paired_with, current.name),
                            style = MaterialTheme.typography.titleMedium,
                            textAlign = TextAlign.Center,
                        )
                        Text(stringResource(R.string.paired_hint), textAlign = TextAlign.Center)
                        Button(onClick = onDone) { Text(stringResource(R.string.done)) }
                    }

                    is PairState.Failed -> {
                        Icon(Icons.Filled.Warning, null, Modifier.size(64.dp).padding(top = 16.dp), tint = MaterialTheme.colorScheme.error)
                        Text(stringResource(message(current.failure)), textAlign = TextAlign.Center)
                        Button(onClick = { state = PairState.Scanning }) { Text(stringResource(R.string.try_again)) }
                        OutlinedButton(onClick = onDone) { Text(stringResource(R.string.cancel)) }
                    }
                }
            }
        }
    }
}

private fun message(failure: PairingFailure) =
    when (failure) {
        PairingFailure.CODE_EXPIRED -> R.string.failure_code_expired
        PairingFailure.NOT_IN_PAIRING_MODE -> R.string.failure_not_in_pairing_mode
        PairingFailure.REJECTED -> R.string.failure_rejected
        PairingFailure.INCOMPATIBLE -> R.string.failure_incompatible
        PairingFailure.UNREACHABLE -> R.string.failure_unreachable
        PairingFailure.CONNECTION_LOST -> R.string.failure_connection_lost
    }

@Composable
private fun Scan(
    invalid: Boolean,
    onCode: (String) -> Unit,
) {
    val context = LocalContext.current
    var cameraAllowed by remember {
        mutableStateOf(ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { cameraAllowed = it }
    var link by rememberSaveable { mutableStateOf("") }

    Text(stringResource(R.string.pair_instructions), textAlign = TextAlign.Center)
    val frame =
        Modifier
            .widthIn(max = 360.dp)
            .fillMaxWidth()
            .aspectRatio(1f)
            .clip(RoundedCornerShape(16.dp))
    if (cameraAllowed) {
        QrScanner(onCode = onCode, modifier = frame)
    } else {
        Button(onClick = { request.launch(Manifest.permission.CAMERA) }) { Text(stringResource(R.string.allow_camera)) }
    }
    if (invalid) {
        Text(stringResource(R.string.invalid_code), color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center)
    }
    OutlinedTextField(
        value = link,
        onValueChange = { link = it },
        label = { Text(stringResource(R.string.pairing_link)) },
        placeholder = { Text("clipsync://pair?…") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
    Button(onClick = { onCode(link) }, enabled = link.isNotBlank()) { Text(stringResource(R.string.pair)) }
}

/** The back camera, reporting the first clipsync QR code it sees. */
@Composable
private fun QrScanner(
    onCode: (String) -> Unit,
    modifier: Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val currentOnCode by rememberUpdatedState(onCode)
    val analyzer = remember { Executors.newSingleThreadExecutor() }
    val found = remember { AtomicBoolean(false) }
    AndroidView(
        factory = { viewContext ->
            val view = PreviewView(viewContext)
            val future = ProcessCameraProvider.getInstance(viewContext)
            future.addListener({
                val provider = future.get()
                val preview = Preview.Builder().build().also { it.setSurfaceProvider(view.surfaceProvider) }
                val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                analysis.setAnalyzer(analyzer) { image ->
                    val plane = image.planes[0]
                    val bytes = ByteArray(plane.buffer.remaining()).also { plane.buffer.get(it) }
                    val text = decodeQr(bytes, image.width, image.height, plane.rowStride)
                    image.close()
                    if (text != null && text.startsWith("clipsync://") && found.compareAndSet(false, true)) {
                        view.post { currentOnCode(text) }
                    }
                }
                provider.unbindAll()
                provider.bindToLifecycle(lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
            }, ContextCompat.getMainExecutor(viewContext))
            view
        },
        modifier = modifier,
    )
    DisposableEffect(Unit) {
        onDispose {
            ProcessCameraProvider.getInstance(context).addListener({
                ProcessCameraProvider.getInstance(context).get().unbindAll()
            }, ContextCompat.getMainExecutor(context))
            analyzer.shutdown()
        }
    }
}

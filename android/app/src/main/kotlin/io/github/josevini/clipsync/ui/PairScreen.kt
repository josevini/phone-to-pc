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
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
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

/**
 * Pairs by scanning the QR code another device shows (`clipsync pair` on a PC, [InviteScreen] on a phone), or by pasting
 * its link (spec §7.2).
 */
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

    CollapsingScaffold(
        title = stringResource(R.string.scan_code),
        expandable = false,
        navigation = { BackButton(onDone, stringResource(R.string.cancel)) },
    ) {
        item {
            Column(
                Modifier.padding(horizontal = 12.dp).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                when (val current = state) {
                    PairState.Scanning, PairState.InvalidCode -> {
                        Scan(invalid = current == PairState.InvalidCode, onCode = ::pair)
                    }

                    is PairState.Pairing -> {
                        Hero(
                            title = stringResource(R.string.pairing_with, current.name),
                            visual = {
                                Box(Modifier.size(96.dp), contentAlignment = Alignment.Center) {
                                    CircularProgressIndicator(Modifier.fillMaxSize(), strokeWidth = 6.dp)
                                    Icon(
                                        painterResource(R.drawable.ic_computer),
                                        null,
                                        Modifier.size(40.dp),
                                        tint = MaterialTheme.colorScheme.primary,
                                    )
                                }
                            },
                        ) { OutlinedButton(onClick = onDone) { Text(stringResource(R.string.cancel)) } }
                    }

                    is PairState.Paired -> {
                        Hero(
                            title = stringResource(R.string.paired_with, current.name),
                            text = stringResource(R.string.paired_hint),
                            visual = { LargeIcon(R.drawable.ic_check_circle_filled) },
                        ) { Button(onClick = onDone, Modifier.widthIn(min = 160.dp)) { Text(stringResource(R.string.done)) } }
                    }

                    is PairState.Failed -> {
                        Hero(
                            title = stringResource(R.string.pair_failed),
                            text = stringResource(message(current.failure)),
                            visual = { LargeIcon(R.drawable.ic_error_filled, tint = MaterialTheme.colorScheme.error) },
                        ) {
                            Button(onClick = { state = PairState.Scanning }, Modifier.widthIn(min = 160.dp)) {
                                Text(stringResource(R.string.try_again))
                            }
                            TextButton(onClick = onDone) { Text(stringResource(R.string.cancel)) }
                        }
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

    Text(
        stringResource(R.string.pair_instructions),
        style = MaterialTheme.typography.bodyLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        textAlign = TextAlign.Center,
    )
    Command("clipsync pair")
    val shape = MaterialTheme.shapes.extraLarge
    val frame =
        Modifier
            .widthIn(max = 360.dp)
            .fillMaxWidth()
            .aspectRatio(1f)
    if (cameraAllowed) {
        QrScanner(onCode = onCode, modifier = frame.clip(shape).viewfinder(MaterialTheme.colorScheme.primary))
    } else {
        Surface(frame, shape = shape, color = LocalGroupColor.current) {
            Column(
                Modifier.padding(24.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                LargeIcon(R.drawable.ic_photo_camera)
                FilledTonalButton(onClick = { request.launch(Manifest.permission.CAMERA) }) { Text(stringResource(R.string.allow_camera)) }
            }
        }
    }
    if (invalid) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(painterResource(R.drawable.ic_error_filled), null, tint = MaterialTheme.colorScheme.error)
            Text(stringResource(R.string.invalid_code), color = MaterialTheme.colorScheme.error)
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        HorizontalDivider(Modifier.weight(1f))
        Text(stringResource(R.string.or), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        HorizontalDivider(Modifier.weight(1f))
    }
    OutlinedTextField(
        value = link,
        onValueChange = { link = it },
        label = { Text(stringResource(R.string.pairing_link)) },
        placeholder = { Text("clipsync://pair?…") },
        leadingIcon = { Icon(painterResource(R.drawable.ic_link), null) },
        singleLine = true,
        keyboardOptions = PairingLinkKeyboard,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth(),
    )
    Button(onClick = { onCode(link) }, enabled = link.isNotBlank(), modifier = Modifier.fillMaxWidth()) {
        Text(stringResource(R.string.pair))
    }
}

/** A link is typed as it is: autocorrection would turn its parameters into words. */
internal val PairingLinkKeyboard = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false)

/** Corner brackets over the camera preview, the familiar sign of a code scanner. */
private fun Modifier.viewfinder(color: Color) =
    drawWithContent {
        drawContent()
        val stroke = 4.dp.toPx()
        val radius = 36.dp.toPx()
        val arm = radius + 24.dp.toPx()
        val inset = 20.dp.toPx()
        val box = Size(size.width - 2 * inset, size.height - 2 * inset)
        for ((x, y) in listOf(0f to 0f, size.width - arm to 0f, 0f to size.height - arm, size.width - arm to size.height - arm)) {
            clipRect(x, y, x + arm, y + arm) {
                drawRoundRect(
                    color,
                    Offset(inset, inset),
                    box,
                    CornerRadius(radius - inset / 2),
                    style = Stroke(stroke, cap = StrokeCap.Round),
                )
            }
        }
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

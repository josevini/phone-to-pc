package io.github.josevini.clipsync.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

/** Material 3 with the wallpaper's colours where the system offers them (Android 12+), light or dark. */
@Composable
fun ClipsyncTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val context = LocalContext.current
    val colors =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
                if (dark) {
                    dynamicDarkColorScheme(
                        context,
                    )
                } else {
                    dynamicLightColorScheme(context)
                }
            }

            dark -> {
                darkColorScheme()
            }

            else -> {
                lightColorScheme()
            }
        }
    MaterialTheme(colorScheme = colors, content = content)
}

/** Keeps content readable on tablets and in landscape: centred, at most 640 dp wide. */
@Composable
fun Readable(
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        Box(Modifier.widthIn(max = 640.dp), content = content)
    }
}

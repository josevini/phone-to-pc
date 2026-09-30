package io.github.josevini.clipsync.ui

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

// Tonal-spot schemes generated with Material Color Utilities from the launcher icon's indigo, #3F51B5, for Android
// 10 and 11, which have no wallpaper colours.
private val light =
    lightColorScheme(
        primary = Color(0xFF515B92),
        onPrimary = Color(0xFFFFFFFF),
        primaryContainer = Color(0xFFDEE0FF),
        onPrimaryContainer = Color(0xFF394379),
        inversePrimary = Color(0xFFBAC3FF),
        secondary = Color(0xFF5B5D72),
        onSecondary = Color(0xFFFFFFFF),
        secondaryContainer = Color(0xFFE0E1F9),
        onSecondaryContainer = Color(0xFF434659),
        tertiary = Color(0xFF77536D),
        onTertiary = Color(0xFFFFFFFF),
        tertiaryContainer = Color(0xFFFFD7F1),
        onTertiaryContainer = Color(0xFF5D3C55),
        background = Color(0xFFFBF8FF),
        onBackground = Color(0xFF1B1B21),
        surface = Color(0xFFFBF8FF),
        onSurface = Color(0xFF1B1B21),
        surfaceVariant = Color(0xFFE3E1EC),
        onSurfaceVariant = Color(0xFF46464F),
        surfaceTint = Color(0xFF515B92),
        inverseSurface = Color(0xFF303036),
        inverseOnSurface = Color(0xFFF2EFF7),
        error = Color(0xFFBA1A1A),
        onError = Color(0xFFFFFFFF),
        errorContainer = Color(0xFFFFDAD6),
        onErrorContainer = Color(0xFF93000A),
        outline = Color(0xFF767680),
        outlineVariant = Color(0xFFC7C5D0),
        scrim = Color(0xFF000000),
        surfaceBright = Color(0xFFFBF8FF),
        surfaceContainer = Color(0xFFEFEDF4),
        surfaceContainerHigh = Color(0xFFE9E7EF),
        surfaceContainerHighest = Color(0xFFE4E1E9),
        surfaceContainerLow = Color(0xFFF5F2FA),
        surfaceContainerLowest = Color(0xFFFFFFFF),
        surfaceDim = Color(0xFFDBD9E0),
    )

private val dark =
    darkColorScheme(
        primary = Color(0xFFBAC3FF),
        onPrimary = Color(0xFF222C61),
        primaryContainer = Color(0xFF394379),
        onPrimaryContainer = Color(0xFFDEE0FF),
        inversePrimary = Color(0xFF515B92),
        secondary = Color(0xFFC3C5DD),
        onSecondary = Color(0xFF2D2F42),
        secondaryContainer = Color(0xFF434659),
        onSecondaryContainer = Color(0xFFE0E1F9),
        tertiary = Color(0xFFE6BAD7),
        onTertiary = Color(0xFF44263D),
        tertiaryContainer = Color(0xFF5D3C55),
        onTertiaryContainer = Color(0xFFFFD7F1),
        background = Color(0xFF121318),
        onBackground = Color(0xFFE4E1E9),
        surface = Color(0xFF121318),
        onSurface = Color(0xFFE4E1E9),
        surfaceVariant = Color(0xFF46464F),
        onSurfaceVariant = Color(0xFFC7C5D0),
        surfaceTint = Color(0xFFBAC3FF),
        inverseSurface = Color(0xFFE4E1E9),
        inverseOnSurface = Color(0xFF303036),
        error = Color(0xFFFFB4AB),
        onError = Color(0xFF690005),
        errorContainer = Color(0xFF93000A),
        onErrorContainer = Color(0xFFFFDAD6),
        outline = Color(0xFF90909A),
        outlineVariant = Color(0xFF46464F),
        scrim = Color(0xFF000000),
        surfaceBright = Color(0xFF39393F),
        surfaceContainer = Color(0xFF1F1F25),
        surfaceContainerHigh = Color(0xFF29292F),
        surfaceContainerHighest = Color(0xFF34343A),
        surfaceContainerLow = Color(0xFF1B1B21),
        surfaceContainerLowest = Color(0xFF0D0E13),
        surfaceDim = Color(0xFF121318),
    )

/** Rounder than Material's defaults, like the grouped lists of Samsung's One UI. */
private val shapes =
    Shapes(
        extraSmall = RoundedCornerShape(8.dp),
        small = RoundedCornerShape(12.dp),
        medium = RoundedCornerShape(20.dp),
        large = RoundedCornerShape(26.dp),
        extraLarge = RoundedCornerShape(28.dp),
    )

/** The background of a [Group]: it stands out from the screen, which is black in the dark theme. */
val LocalGroupColor = staticCompositionLocalOf { Color.Unspecified }

/**
 * Material 3 with the wallpaper's colours where the system offers them (Android 12+), light or dark, laid out like
 * One UI: a black screen in the dark theme and a grey one in the light theme, with lists grouped on raised blocks.
 */
@Composable
fun ClipsyncTheme(content: @Composable () -> Unit) {
    val isDark = isSystemInDarkTheme()
    val context = LocalContext.current
    val base =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
                if (isDark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            }

            isDark -> {
                dark
            }

            else -> {
                light
            }
        }
    val screen = if (isDark) Color.Black else base.surfaceContainer
    val colors = base.copy(background = screen, surface = screen)
    val group = if (isDark) base.surfaceContainer else base.surfaceContainerLowest
    MaterialTheme(colorScheme = colors, shapes = shapes) {
        CompositionLocalProvider(LocalGroupColor provides group, content = content)
    }
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

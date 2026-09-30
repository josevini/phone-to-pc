package io.github.josevini.clipsync.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import io.github.josevini.clipsync.R

private const val SOURCE = "https://github.com/josevini/phone-to-pc"

/** Version, licence and the notices of the open-source components the app ships. */
@Composable
fun AboutScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val uris = LocalUriHandler.current
    val version =
        context.packageManager
            .getPackageInfo(context.packageName, 0)
            .versionName
            .orEmpty()
    val colors = MaterialTheme.colorScheme
    CollapsingScaffold(
        title = stringResource(R.string.about),
        header = {
            Surface(Modifier.size(88.dp), shape = RoundedCornerShape(28.dp), color = colors.primary) {
                Icon(painterResource(R.drawable.ic_launcher_foreground), null, Modifier.fillMaxSize(), tint = colors.onPrimary)
            }
            Text(stringResource(R.string.app_name), style = MaterialTheme.typography.displaySmall)
            Text(stringResource(R.string.about_version, version), color = colors.onSurfaceVariant)
        },
        navigation = { BackButton(onBack) },
    ) {
        item {
            Text(
                stringResource(R.string.about_text),
                style = MaterialTheme.typography.bodyLarge,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
        }
        item {
            Group {
                GroupRow(
                    title = stringResource(R.string.about_source),
                    icon = R.drawable.ic_code,
                    summary = SOURCE.removePrefix("https://"),
                    summaryColor = colors.primary,
                    onClick = { uris.openUri(SOURCE) },
                )
            }
        }
        item {
            Group(title = stringResource(R.string.about_notices_title)) {
                Text(
                    stringResource(R.string.about_notices),
                    style = MaterialTheme.typography.bodyMedium,
                    color = colors.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 16.dp),
                )
            }
        }
    }
}

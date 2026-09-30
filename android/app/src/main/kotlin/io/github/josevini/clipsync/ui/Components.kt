package io.github.josevini.clipsync.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.github.josevini.clipsync.R

private val GroupShape = RoundedCornerShape(26.dp)

/**
 * A screen laid out like One UI: its title starts large and centred in the top third, where it is easy to read, and
 * moves into the top bar as the content scrolls. Short windows (landscape phones) start with the title in the bar.
 *
 * [header] replaces the large title and [subtitle] when given.
 */
@Composable
fun CollapsingScaffold(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    expandable: Boolean = true,
    header: (@Composable ColumnScope.() -> Unit)? = null,
    navigation: (@Composable () -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
    bottomBar: @Composable () -> Unit = {},
    content: LazyListScope.() -> Unit,
) {
    BoxWithConstraints(modifier.fillMaxSize()) {
        val expanded = expandable && maxHeight >= 560.dp
        val headerHeight = maxHeight * 0.38f - 64.dp
        val list = rememberLazyListState()
        val density = LocalDensity.current
        // 0 while the large title shows in full, 1 once it has scrolled under the top bar.
        val collapsed by remember(expanded, headerHeight) {
            derivedStateOf {
                when {
                    !expanded || list.firstVisibleItemIndex > 0 -> 1f
                    else -> (list.firstVisibleItemScrollOffset / with(density) { (headerHeight * 0.6f).toPx() }).coerceIn(0f, 1f)
                }
            }
        }
        Scaffold(
            topBar = { TopBar(title, titleAlpha = collapsed, navigation, actions) },
            bottomBar = bottomBar,
        ) { padding ->
            Readable(Modifier.padding(padding)) {
                LazyColumn(
                    state = list,
                    contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(20.dp),
                ) {
                    if (expanded) {
                        item(key = "header") {
                            Column(
                                Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = headerHeight)
                                    .graphicsLayer { alpha = 1 - collapsed }
                                    .padding(horizontal = 24.dp, vertical = 16.dp),
                                verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterVertically),
                                horizontalAlignment = Alignment.CenterHorizontally,
                            ) {
                                if (header != null) {
                                    header()
                                } else {
                                    Text(title, style = MaterialTheme.typography.displaySmall, textAlign = TextAlign.Center)
                                    subtitle?.let {
                                        Text(
                                            it,
                                            style = MaterialTheme.typography.bodyLarge,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            textAlign = TextAlign.Center,
                                        )
                                    }
                                }
                            }
                        }
                    }
                    content()
                }
            }
        }
    }
}

@Composable
private fun TopBar(
    title: String,
    titleAlpha: Float,
    navigation: (@Composable () -> Unit)?,
    actions: @Composable RowScope.() -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal))
            .heightIn(min = 64.dp)
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (navigation != null) navigation() else Spacer(Modifier.width(20.dp))
        Text(
            title,
            Modifier.weight(1f).graphicsLayer { alpha = titleAlpha },
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.Bold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        actions()
    }
}

/** The back chevron of the top bar. */
@Composable
fun BackButton(
    onClick: () -> Unit,
    description: String = stringResource(R.string.back),
) {
    IconButton(onClick = onClick) { Icon(painterResource(R.drawable.ic_arrow_back_ios_new), description, Modifier.size(20.dp)) }
}

/** Rows that belong together, on one rounded block, with an optional [title] above and [footer] below. */
@Composable
fun Group(
    modifier: Modifier = Modifier,
    title: String? = null,
    footer: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier.fillMaxWidth()) {
        if (title != null) {
            Text(
                title,
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 24.dp, end = 24.dp, bottom = 8.dp),
            )
        }
        Surface(Modifier.fillMaxWidth(), shape = GroupShape, color = LocalGroupColor.current) {
            Column(Modifier.padding(vertical = 4.dp), content = content)
        }
        if (footer != null) {
            Text(
                footer,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 8.dp),
            )
        }
    }
}

/** A row of a [Group]: a coloured line icon, a title, and a summary below it. */
@Composable
fun GroupRow(
    title: String,
    modifier: Modifier = Modifier,
    @DrawableRes icon: Int? = null,
    iconTint: Color = MaterialTheme.colorScheme.primary,
    titleColor: Color = MaterialTheme.colorScheme.onSurface,
    summary: String? = null,
    summaryColor: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    onClickLabel: String? = null,
    onClick: (() -> Unit)? = null,
    below: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val clickable =
        if (onClick != null) Modifier.clickable(onClickLabel = onClickLabel, role = Role.Button, onClick = onClick) else Modifier
    Row(
        modifier
            .fillMaxWidth()
            .then(clickable)
            .heightIn(min = 64.dp)
            .padding(horizontal = 24.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(painterResource(icon), null, Modifier.size(24.dp), tint = iconTint)
            Spacer(Modifier.width(20.dp))
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = titleColor)
            summary?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = summaryColor) }
            below?.invoke()
        }
        if (trailing != null) {
            Spacer(Modifier.width(16.dp))
            trailing()
        }
    }
}

/** A [GroupRow] that turns a setting on or off with a switch; tapping anywhere on the row toggles it. */
@Composable
fun SwitchRow(
    title: String,
    checked: Boolean,
    onCheckedChange: ((Boolean) -> Unit)?,
    @DrawableRes icon: Int? = null,
    iconTint: Color = MaterialTheme.colorScheme.primary,
    summary: String? = null,
) {
    val enabled = onCheckedChange != null
    GroupRow(
        title = title,
        modifier = Modifier.toggleable(value = checked, enabled = enabled, role = Role.Switch) { onCheckedChange?.invoke(it) },
        icon = icon,
        iconTint = iconTint,
        summary = summary,
        trailing = { Switch(checked = checked, onCheckedChange = null, enabled = enabled) },
    )
}

/** The line between two rows of a [Group], lined up with the rows' text. */
@Composable
fun GroupDivider(afterIcon: Boolean = true) {
    HorizontalDivider(
        Modifier.padding(start = if (afterIcon) 68.dp else 24.dp, end = 24.dp),
        color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.6f),
    )
}

/** A suggestion on a tinted block, with an optional link-style [action]. */
@Composable
fun SuggestionCard(
    title: String,
    text: String,
    modifier: Modifier = Modifier,
    @DrawableRes icon: Int? = null,
    action: String? = null,
    onAction: () -> Unit = {},
) {
    val colors = MaterialTheme.colorScheme
    Surface(modifier.fillMaxWidth(), shape = GroupShape, color = colors.primary.copy(alpha = 0.12f)) {
        Row(Modifier.padding(start = 24.dp, top = 20.dp, end = 24.dp, bottom = if (action == null) 20.dp else 8.dp)) {
            if (icon != null) {
                Icon(painterResource(icon), null, Modifier.size(24.dp), tint = colors.primary)
                Spacer(Modifier.width(20.dp))
            }
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium)
                Text(text, style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant)
                if (action != null) {
                    Text(
                        action,
                        style = MaterialTheme.typography.titleSmall,
                        color = colors.primary,
                        modifier =
                            Modifier
                                .clip(RoundedCornerShape(8.dp))
                                .clickable(role = Role.Button, onClick = onAction)
                                .heightIn(min = 48.dp)
                                .padding(vertical = 14.dp),
                    )
                }
            }
        }
    }
}

/** The action bar at the bottom of a page, where One UI puts actions such as unpairing. */
@Composable
fun BottomAction(
    @DrawableRes icon: Int,
    label: String,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().windowInsetsPadding(WindowInsets.navigationBars).padding(8.dp),
        horizontalArrangement = Arrangement.Center,
    ) {
        Column(
            Modifier
                .clip(RoundedCornerShape(16.dp))
                .clickable(role = Role.Button, onClick = onClick)
                .padding(horizontal = 32.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Icon(painterResource(icon), null)
            Text(label, style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** A command to run on the PC, set in monospace so it reads as something to type. */
@Composable
fun Command(
    command: String,
    modifier: Modifier = Modifier,
) {
    Surface(modifier, shape = MaterialTheme.shapes.small, color = MaterialTheme.colorScheme.surfaceContainerHighest) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(painterResource(R.drawable.ic_terminal), null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
            Text(command, style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace)
        }
    }
}

/** A moment of a flow: a large [visual], a title, an explanation and the actions below. */
@Composable
fun Hero(
    title: String,
    modifier: Modifier = Modifier,
    text: String? = null,
    visual: @Composable () -> Unit,
    actions: @Composable ColumnScope.() -> Unit = {},
) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        visual()
        Text(title, style = MaterialTheme.typography.headlineSmall, textAlign = TextAlign.Center)
        if (text != null) {
            Text(
                text,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
        Column(
            Modifier.padding(top = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
            content = actions,
        )
    }
}

/** The large line icon of a [Hero] or a page header. */
@Composable
fun LargeIcon(
    @DrawableRes icon: Int,
    tint: Color = MaterialTheme.colorScheme.primary,
) {
    Icon(painterResource(icon), null, Modifier.size(72.dp), tint = tint)
}

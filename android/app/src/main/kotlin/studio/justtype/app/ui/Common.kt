@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CloudOff
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.text.NumberFormat

/** A top bar with a back arrow, for every screen below a tab. */
@Composable
fun BackBar(title: String, onBack: () -> Unit, actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {}) {
    TopAppBar(
        title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        navigationIcon = {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
        },
        actions = actions,
    )
}

@Composable
fun TabBar(title: String, actions: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit = {}) {
    TopAppBar(title = { Text(title) }, actions = actions)
}

/**
 * What the phone knows that the server does not. Shown rather than hidden: an app that quietly
 * swallows a failed save is telling the writer their words are safe when they are on one device.
 */
@Composable
fun OfflineNotice(offline: Boolean, pending: Int) {
    val message = when {
        offline && pending == 0 -> "Offline. This is the copy on your phone."
        offline -> "Offline. $pending ${if (pending == 1) "scene" else "scenes"} waiting to go up."
        pending > 0 -> "$pending ${if (pending == 1) "scene" else "scenes"} still going up."
        else -> return
    }
    Banner(message, if (offline) Icons.Default.CloudOff else Icons.Default.Sync, Color(0x1FE0A100))
}

@Composable
fun Banner(text: String, icon: ImageVector = Icons.Default.Warning, tint: Color = Color(0x26E0A100)) {
    Row(
        Modifier.fillMaxWidth().background(tint).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
fun ReadOnlyBanner(text: String) = Banner(text, Icons.Default.Lock, MaterialTheme.colorScheme.surfaceVariant)

@Composable
fun Tag(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        modifier = Modifier
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(50))
            .padding(horizontal = 8.dp, vertical = 2.dp),
    )
}

@Composable
fun EmptyState(title: String, detail: String? = null, icon: ImageVector? = null, modifier: Modifier = Modifier) {
    Column(
        modifier.fillMaxWidth().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (icon != null) Icon(icon, contentDescription = null, modifier = Modifier.size(40.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(title, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
        if (detail != null) {
            Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
        }
    }
}

@Composable
fun Loading(modifier: Modifier = Modifier.fillMaxSize()) {
    Box(modifier, contentAlignment = Alignment.Center) { CircularProgressIndicator() }
}

@Composable
fun SectionHeader(text: String, trailing: @Composable () -> Unit = {}) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.size(8.dp))
        trailing()
    }
}

@Composable
fun Footnote(text: String, modifier: Modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

@Composable
fun ErrorText(text: String?) {
    if (text != null) Text(text, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
}

@Composable
fun Divider() = HorizontalDivider(color = MaterialTheme.colorScheme.surfaceVariant)

@Composable
fun Quote(text: String, maxLines: Int = 2) {
    Row(Modifier.padding(vertical = 2.dp)) {
        Box(Modifier.size(width = 2.dp, height = 32.dp).background(MaterialTheme.colorScheme.primary))
        Text(
            text, style = MaterialTheme.typography.bodySmall.copy(fontFamily = Serif, fontStyle = FontStyle.Italic),
            color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = maxLines, overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 8.dp),
        )
    }
}

val ListPadding = PaddingValues(bottom = 24.dp)

fun Number.grouped(): String = NumberFormat.getIntegerInstance().format(this)

fun plural(count: Number, one: String, many: String = one + "s") = "${count.grouped()} ${if (count.toLong() == 1L) one else many}"

@Composable
fun VSpace(dp: Int) = Spacer(Modifier.height(dp.dp))

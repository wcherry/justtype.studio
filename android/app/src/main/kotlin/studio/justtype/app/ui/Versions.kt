@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.WifiOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.label
import java.util.Locale

@Composable
fun VersionsScreen(workId: String, onBack: () -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var versions by remember { mutableStateOf<List<API.Version>?>(null) }
    var busy by remember { mutableStateOf(false) }
    var offering by remember { mutableStateOf<API.Version?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }

    suspend fun load() {
        runCatching { client.versions(workId) }
            .onSuccess { versions = it; problem = null }
            .onFailure { problem = it.message; if (versions == null) versions = emptyList() }
    }
    fun act(work: suspend () -> Unit) = scope.launch {
        busy = true
        runCatching { work() }.onFailure { problem = it.message }
        load()
        busy = false
    }
    LaunchedEffect(workId) { load() }

    Scaffold(topBar = { BackBar("Versions", onBack) }) { padding ->
        PullToRefreshBox(refreshing, { scope.launch { refreshing = true; load(); refreshing = false } }, Modifier.padding(padding).fillMaxSize()) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
                item {
                    OutlinedButton(
                        onClick = { act { client.snapshot(workId, null) } },
                        enabled = !busy,
                        modifier = Modifier.fillMaxWidth().padding(16.dp),
                    ) {
                        Icon(Icons.Default.CameraAlt, contentDescription = null)
                        Text("Snapshot the current draft", Modifier.padding(start = 8.dp))
                    }
                }
                item { ErrorText(problem) }
                if (versions == null) item { Loading(Modifier.fillMaxWidth().padding(32.dp)) }
                items(versions.orEmpty(), key = { it.id }) { version ->
                    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(version.label, style = MaterialTheme.typography.titleMedium)
                                Spacer(Modifier.weight(1f))
                                Tag(version.state.raw.replaceFirstChar { it.titlecase(Locale.getDefault()) })
                            }
                            Text(plural(version.wordCount, "word"), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            when (version.state) {
                                API.VersionState.draft ->
                                    TextButton(onClick = { act { client.publish(version.id) } }, enabled = !busy) { Text("Publish this version") }
                                API.VersionState.published -> Row {
                                    TextButton(onClick = { offering = version }) { Text("Offer for review") }
                                    TextButton(onClick = { act { client.finalize(version.id) } }, enabled = !busy) { Text("Mark final") }
                                }
                                API.VersionState.`final` -> {}
                            }
                        }
                    }
                }
            }
        }
    }

    offering?.let { version ->
        OfferDialog(version, onDismiss = { offering = null }) { visibility, prompt ->
            offering = null
            act { client.offerForReview(version.id, visibility, prompt.ifBlank { null }) }
        }
    }
}

@Composable
private fun OfferDialog(version: API.Version, onDismiss: () -> Unit, onSubmit: (API.Visibility, String) -> Unit) {
    var visibility by remember { mutableStateOf(API.Visibility.platform) }
    var prompt by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Offer ${version.label}") },
        confirmButton = { TextButton(onClick = { onSubmit(visibility, prompt.trim()) }) { Text("Offer") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Picker("Who can read it", API.Visibility.entries, visibility, { it.label }) { visibility = it }
                VSpace(12)
                OutlinedTextField(
                    prompt, { prompt = it }, minLines = 3, maxLines = 6, modifier = Modifier.fillMaxWidth(),
                    label = { Text("What do you want feedback on?") },
                    placeholder = { Text("Does the opening earn the next page?") },
                )
            }
        },
    )
}

/**
 * What the book says about itself in an export. Read-only here, as on iOS: they are edited on the
 * web, under Front matter.
 */
@Composable
fun BookDetailsScreen(workId: String, onBack: () -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var details by remember { mutableStateOf<API.MetadataResponse?>(null) }
    var failed by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }

    suspend fun load() {
        runCatching { client.metadata(workId) }.onSuccess { details = it; failed = false }.onFailure { failed = details == null }
    }
    LaunchedEffect(workId) { load() }

    Scaffold(topBar = { BackBar("Publishing details", onBack) }) { padding ->
        PullToRefreshBox(refreshing, { scope.launch { refreshing = true; load(); refreshing = false } }, Modifier.padding(padding).fillMaxSize()) {
            val d = details
            when {
                d != null -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
                    Row("Language", d.language?.let(::languageName) ?: "Not set (exports say English)")
                    Row("Reads", direction(d.textDirection))
                    Footnote("The language sets hyphenation and the reading voice.")
                    SectionHeader("Description")
                    Text(
                        d.description?.takeIf { it.isNotBlank() } ?: "None",
                        Modifier.padding(horizontal = 16.dp),
                        color = if (d.description.isNullOrBlank()) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                    )
                    SectionHeader("Publishing")
                    Row("Publisher", d.publisher)
                    Row("Published", d.published)
                    Row("Identifier", d.identifier)
                    Row("Keywords", d.keywords.joinToString(", ").ifEmpty { null })
                    if (d.contributors.isNotEmpty()) {
                        SectionHeader("Others who worked on it")
                        d.contributors.forEach { Row(it.name, it.role.replaceFirstChar { c -> c.titlecase(Locale.getDefault()) }) }
                    }
                    SectionHeader("Cover")
                    Row(
                        "Cover",
                        if (d.hasCover && d.coverWidth != null && d.coverHeight != null) "${d.coverWidth} × ${d.coverHeight}"
                        else if (d.hasCover) "Added" else "None",
                    )
                    d.coverWarning?.let { Footnote(it) }
                    VSpace(12)
                    Footnote("Edit these on the web, under Front matter.")
                }
                failed -> EmptyState("Could not load the details", "Pull to try again.", Icons.Default.WifiOff)
                else -> Loading()
            }
        }
    }
}

@Composable
private fun Row(label: String, value: String?) {
    androidx.compose.foundation.layout.Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp)) {
        Text(label, Modifier.weight(1f))
        Text(
            value?.takeIf { it.isNotBlank() } ?: "—",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = androidx.compose.ui.text.style.TextAlign.End,
            modifier = Modifier.weight(1.4f),
        )
    }
}

/** "fr-CA" as "French (Canada) · fr-CA", in the reader's own language. */
private fun languageName(tag: String): String {
    val name = Locale.forLanguageTag(tag).getDisplayName(Locale.getDefault())
    return if (name.isBlank() || name == tag) tag else "$name · $tag"
}

private fun direction(value: String?) = when (value) {
    "rtl" -> "Right to left"
    "ltr" -> "Left to right"
    else -> "As the language does"
}

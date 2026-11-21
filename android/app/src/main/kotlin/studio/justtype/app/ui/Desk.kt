@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.label
import studio.justtype.core.sync.Freshness

/** Your desk: every book you are writing. */
@Composable
fun DeskScreen(openWork: (String, String) -> Unit) {
    val graph = LocalContext.current.graph
    val session = graph.session
    val scope = rememberCoroutineScope()
    val pending by session.pending.collectAsState()
    val synced by session.synced.collectAsState()

    var works by remember { mutableStateOf<List<API.Work>?>(null) }
    var templates by remember { mutableStateOf<List<API.TemplateView>>(emptyList()) }
    var offline by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    var creating by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }

    suspend fun load() {
        // The first thing a writer sees, so it comes from the phone when there is no network.
        graph.sync.works()?.let {
            works = it.value
            offline = it.freshness == Freshness.Local
        } ?: run { if (works == null) works = emptyList() }
        templates = runCatching { graph.client.templates() }.getOrDefault(templates)
        session.refreshPendingCount()
    }

    LaunchedEffect(synced) { load() }

    Scaffold(
        topBar = { TabBar("Your desk") },
        floatingActionButton = {
            FloatingActionButton(onClick = { creating = true }) { Icon(Icons.Default.Add, contentDescription = "New work") }
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = {
                scope.launch {
                    refreshing = true
                    // Catching up bumps `synced`, which reloads.
                    session.catchUp().join()
                    refreshing = false
                }
            },
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            val list = works
            if (list == null) {
                Loading()
            } else {
                LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
                    item { OfflineNotice(offline, pending) }
                    item { ErrorText(problem) }
                    if (list.isEmpty()) {
                        item {
                            EmptyState(
                                "Nothing here yet",
                                "Start something — a template gives you a spine to hang it on.",
                                Icons.AutoMirrored.Filled.MenuBook,
                            )
                        }
                    }
                    items(list, key = { it.id }) { work ->
                        ListItem(
                            headlineContent = {
                                Text(work.title, style = MaterialTheme.typography.titleMedium.copy(fontFamily = Serif), maxLines = 2, overflow = TextOverflow.Ellipsis)
                            },
                            supportingContent = {
                                Text("${work.kind.label} · ${plural(work.wordCount, "word")} · ${work.status.raw}")
                            },
                            modifier = Modifier.clickable { openWork(work.id, work.title) },
                        )
                    }
                }
            }
        }
    }

    if (creating) {
        NewWorkDialog(templates, onDismiss = { creating = false }) { title, kind, templateId ->
            creating = false
            scope.launch {
                try {
                    val made = graph.client.createWork(title, kind, templateId)
                    load()
                    openWork(made.id, made.title)
                } catch (e: Exception) {
                    problem = e.message
                }
            }
        }
    }
}

@Composable
private fun NewWorkDialog(
    templates: List<API.TemplateView>,
    onDismiss: () -> Unit,
    onCreate: (String, API.WorkKind, String?) -> Unit,
) {
    var title by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf(API.WorkKind.novel) }
    var templateId by remember { mutableStateOf(templates.firstOrNull { it.id == "tpl-blank" }?.id ?: templates.firstOrNull()?.id) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("New work") },
        confirmButton = {
            TextButton(onClick = { onCreate(title.trim(), kind, templateId) }, enabled = title.isNotBlank()) { Text("Create") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                OutlinedTextField(title, { title = it }, label = { Text("Title") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                VSpace(8)
                Picker("Form", API.WorkKind.entries, kind, { it.label }) { kind = it }
                if (templates.isNotEmpty()) {
                    VSpace(8)
                    Picker("Template", templates.map { it.id }, templateId, { id -> templates.first { it.id == id }.name }) { templateId = it }
                    templates.firstOrNull { it.id == templateId }?.let {
                        Footnote(it.description, Modifier.padding(top = 6.dp))
                    }
                }
            }
        },
    )
}

/** A labelled drop-down, the Android form of iOS's `Picker`. */
@Composable
fun <T> Picker(label: String, options: List<T>, selected: T?, name: (T) -> String, modifier: Modifier = Modifier.fillMaxWidth(), enabled: Boolean = true, onSelect: (T) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { if (enabled) expanded = it }, modifier = modifier) {
        OutlinedTextField(
            value = selected?.let(name).orEmpty(),
            onValueChange = {},
            readOnly = true,
            enabled = enabled,
            label = { Text(label) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
            modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled).fillMaxWidth(),
        )
        ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            options.forEach { option ->
                DropdownMenuItem(text = { Text(name(option)) }, onClick = { onSelect(option); expanded = false })
            }
        }
    }
}

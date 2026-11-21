@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import studio.justtype.app.AppGraph
import studio.justtype.app.editor.EditorCommands
import studio.justtype.app.editor.ProseEditor
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.DocNodes
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.wordCount
import studio.justtype.core.sync.EntrySaveOutcome
import studio.justtype.core.sync.PendingSave

/** One answer open for writing: saved to the phone first, up when it can, as a scene is. */
private class AnswerSession(private val graph: AppGraph, val day: String) {
    var prompt by mutableStateOf<API.PromptView?>(null)
    var entry by mutableStateOf<API.PromptEntryView?>(null)
    var enrolled by mutableStateOf(true)
    var document by mutableStateOf(DocNodes.empty)
    var loaded by mutableStateOf(false)
    var words by mutableIntStateOf(0)
    var marks by mutableStateOf(emptySet<String>())
    var state by mutableStateOf(SaveState.Idle)
    var problem by mutableStateOf<String?>(null)
    var conflict by mutableStateOf(false)
    private var baseHash: String? = null
    private var timer: Job? = null
    private val saving = Mutex()

    suspend fun load() {
        prompt = runCatching { graph.client.prompt(day) }.getOrNull()
        enrolled = runCatching { graph.client.enrollment().enrolled }.getOrDefault(enrolled)
        val found = graph.sync.entry(day)
        entry = found.entry
        baseHash = found.entry?.contentHash
        document = found.document ?: DocNodes.empty
        words = document.wordCount
        state = if (graph.sync.pending(PendingSave.entryKey(day)) == null) SaveState.Idle else SaveState.Queued
        loaded = true
    }

    fun changed(doc: API.DocNode, count: Int) {
        document = doc
        words = count
        if (!enrolled) return
        state = SaveState.Dirty
        timer?.cancel()
        timer = graph.scope.launch {
            delay(1200)
            save()
        }
    }

    suspend fun save() = saving.withLock {
        if (!enrolled || (state == SaveState.Idle && entry != null)) return@withLock
        state = SaveState.Saving
        when (val outcome = graph.sync.saveEntry(day, document, baseHash, PromptDays.today())) {
            is EntrySaveOutcome.Saved -> {
                entry = outcome.entry
                baseHash = outcome.entry.contentHash
                state = SaveState.Saved
                graph.session.publishWritingSnapshot()
            }
            EntrySaveOutcome.Queued -> state = SaveState.Queued
            is EntrySaveOutcome.Conflict -> { state = SaveState.Conflict; conflict = true }
            is EntrySaveOutcome.Refused -> { state = SaveState.Idle; problem = outcome.message }
        }
        graph.session.refreshPendingCount()
    }

    fun leave() {
        val wasDirty = state == SaveState.Dirty
        timer?.cancel()
        if (wasDirty) graph.scope.launch { save() }
    }

    suspend fun act(work: suspend () -> API.PromptEntryView) {
        timer?.cancel()
        if (state == SaveState.Dirty) save()
        try {
            entry = work()
            problem = null
        } catch (e: Exception) {
            problem = e.message
        }
    }

    suspend fun keepMine() {
        conflict = false
        val outcome = graph.sync.keepMyEntry(day)
        if (outcome is EntrySaveOutcome.Saved) {
            entry = outcome.entry
            baseHash = outcome.entry.contentHash
            state = SaveState.Saved
        }
        graph.session.refreshPendingCount()
    }

    suspend fun reload() {
        conflict = false
        graph.sync.discardMyEntry(day)
        load()
    }
}

@Composable
fun PromptWriteScreen(day: String, onBack: () -> Unit) {
    val graph = LocalContext.current.graph
    val client = graph.client
    val scope = rememberCoroutineScope()
    val session = remember(day) { AnswerSession(graph, day) }
    val commands = remember { EditorCommands() }
    var menu by remember { mutableStateOf(false) }
    var converting by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }

    LaunchedEffect(day) { session.load() }
    DisposableEffect(session) { onDispose { session.leave() } }

    val entry = session.entry
    Scaffold(
        topBar = {
            BackBar(PromptDays.name(day), onBack) {
                SaveStatus(session.words, session.state)
                IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "Answer") }
                DropdownMenu(menu, { menu = false }) {
                    if (session.enrolled && entry?.state != API.EntryState.submitted) {
                        DropdownMenuItem(
                            text = { Text(if (entry?.state == API.EntryState.withdrawn) "Submit again" else "Submit") },
                            enabled = session.words > 0,
                            onClick = { menu = false; scope.launch { session.act { client.submitEntry(day) } } },
                        )
                    }
                    if (session.enrolled && entry?.state == API.EntryState.submitted) {
                        DropdownMenuItem(text = { Text("Withdraw") }, onClick = { menu = false; scope.launch { session.act { client.withdrawEntry(day) } } })
                    }
                    if (entry != null) {
                        DropdownMenuItem(text = { Text("Turn into a project") }, onClick = {
                            menu = false
                            scope.launch { session.save(); converting = true }
                        })
                        DropdownMenuItem(text = { Text("Delete", color = MaterialTheme.colorScheme.error) }, onClick = { menu = false; confirmDelete = true })
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize().imePadding()) {
            session.prompt?.let {
                Surface(color = MaterialTheme.colorScheme.surfaceVariant) {
                    Text(it.body, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = Serif), modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp))
                }
            }
            session.problem?.let { Banner(it) }
            if (!session.enrolled) ReadOnlyBanner("Join Daily Prompts to write an answer.")
            when (entry?.state) {
                API.EntryState.draft -> "A private draft: only you can see it."
                API.EntryState.submitted -> "Submitted: everyone in Daily Prompts can read it. Changes show as edited."
                API.EntryState.withdrawn -> "Withdrawn: private again, its critiques kept."
                null -> null
            }?.let { Footnote(it, Modifier.padding(horizontal = 16.dp, vertical = 6.dp)) }
            if (session.loaded) {
                ProseEditor(
                    document = session.document,
                    editable = session.enrolled,
                    commands = commands,
                    onChange = session::changed,
                    onSelection = { session.marks = it.marks },
                    onProblem = { session.problem = it },
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                )
                if (session.enrolled) FormatBar(session.marks, FormatBar.answer, commands::send)
            } else {
                Loading(Modifier.weight(1f).fillMaxWidth())
            }
        }
    }

    if (session.conflict) {
        AlertDialog(
            onDismissRequest = {},
            title = { Text("This answer changed somewhere else") },
            text = { Text("Another device saved this answer while you were writing. Your text is still here until you choose.") },
            confirmButton = { TextButton(onClick = { scope.launch { session.keepMine() } }) { Text("Keep mine") } },
            dismissButton = { TextButton(onClick = { scope.launch { session.reload() } }) { Text("Reload it") } },
        )
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete this answer and every critique on it?") },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    scope.launch {
                        try {
                            client.deleteEntry(day)
                            graph.sync.discardMyEntry(day)
                            onBack()
                        } catch (e: Exception) {
                            session.problem = e.message
                        }
                    }
                }) { Text("Delete", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancel") } },
        )
    }
    if (converting && entry != null) ToWorkDialog(entry.id) { converting = false }
}

/**
 * Turning an answer into a project. The scenes shown are the server's split, the same rules the
 * web uses, so this is exactly what will be made.
 */
@Composable
fun ToWorkDialog(entryId: String, onDismiss: () -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var preview by remember { mutableStateOf<API.ToWorkPreview?>(null) }
    var title by remember { mutableStateOf("") }
    var made by remember { mutableStateOf<API.OutlineResponse?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    LaunchedEffect(entryId) {
        runCatching { client.previewWork(entryId) }
            .onSuccess { preview = it; title = it.title }
            .onFailure { problem = it.message }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Turn into a project") },
        confirmButton = {
            if (made == null) {
                TextButton(
                    enabled = !busy && preview?.scenes?.isNotEmpty() == true && title.isNotBlank(),
                    onClick = {
                        busy = true
                        scope.launch {
                            runCatching { client.makeWork(entryId, title.trim()) }
                                .onSuccess { made = it }.onFailure { problem = it.message }
                            busy = false
                        }
                    },
                ) { Text("Make it") }
            } else {
                TextButton(onClick = onDismiss) { Text("Done") }
            }
        },
        dismissButton = { if (made == null) TextButton(onClick = onDismiss) { Text("Cancel") } },
        text = {
            val done = made
            if (done != null) {
                ListItem(
                    leadingContent = { Icon(Icons.Default.CheckCircle, contentDescription = null) },
                    headlineContent = { Text("Made “${done.title}”. It is on your desk.") },
                )
            } else {
                LazyColumn {
                    item {
                        OutlinedTextField(title, { title = it }, label = { Text("Title") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                        Footnote("A new book of yours, with this answer copied into one chapter and the prompt in its notebook. The answer stays as it is.", Modifier.padding(vertical = 6.dp))
                    }
                    preview?.let { p ->
                        item { SectionHeader(plural(p.scenes.size, "scene")) }
                        itemsIndexed(p.scenes) { i, scene ->
                            ListItem(
                                headlineContent = { Text(scene.firstLine.ifEmpty { "No text" }, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = Serif)) },
                                supportingContent = { Text("Scene ${i + 1} · ${plural(scene.wordCount, "word")}") },
                            )
                        }
                        item {
                            Footnote(
                                if (p.blankParagraphSplits) "Split at each line that is only -- and at each blank line."
                                else "Split at each line that is only --. A blank line can separate scenes too: turn that on in You → Settings.",
                                Modifier.padding(vertical = 6.dp),
                            )
                        }
                    }
                    item { ErrorText(problem) }
                }
            }
        },
    )
}

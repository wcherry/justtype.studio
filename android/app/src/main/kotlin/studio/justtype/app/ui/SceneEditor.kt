@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.FormatListBulleted
import androidx.compose.material.icons.automirrored.filled.Redo
import androidx.compose.material.icons.automirrored.filled.Undo
import androidx.compose.material.icons.filled.FormatBold
import androidx.compose.material.icons.filled.FormatItalic
import androidx.compose.material.icons.filled.FormatQuote
import androidx.compose.material.icons.filled.FormatUnderlined
import androidx.compose.material.icons.filled.Title
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
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
import studio.justtype.core.model.Naming
import studio.justtype.core.model.wordCount
import studio.justtype.core.sync.SaveOutcome

enum class SaveState(val label: String) {
    Idle(""), Dirty("Unsaved"), Saving("Saving…"), Saved("Saved"),
    /** Not a failure. The words are on the phone and they will go up. */
    Queued("Kept on this phone"),
    Conflict("Changed elsewhere"),
}

/**
 * One scene open for writing. Saves run on the app's scope, not the screen's, so leaving a scene
 * never cancels the save that carries its last sentence; and one at a time, so each carries the
 * hash the one before it came back with.
 */
class SceneSession(private val graph: AppGraph, val sceneId: String, val editable: Boolean) {
    var scene by mutableStateOf<API.SceneWithContent?>(null)
    var document by mutableStateOf(DocNodes.empty)
    var words by mutableIntStateOf(0)
    var marks by mutableStateOf(emptySet<String>())
    var problem by mutableStateOf<String?>(null)
    var state by mutableStateOf(SaveState.Idle)
    var conflict by mutableStateOf(false)
    /** The hash this document was built on, sent with every save so a moved base is refused. */
    private var baseHash: String? = null
    private var timer: Job? = null
    private val saving = Mutex()

    suspend fun load(displayTitle: String) {
        // Through the sync engine, so an unsent edit wins over the server's older copy.
        val loaded = graph.sync.scene(sceneId)
        if (loaded == null) {
            problem = "This scene is not on this phone yet, and there is no connection to fetch it."
            return
        }
        scene = loaded.value
        baseHash = loaded.value.contentHash
        document = loaded.value.content
        words = loaded.value.content.wordCount
        state = if (graph.sync.pending(sceneId) == null) SaveState.Idle else SaveState.Queued
        graph.session.refreshPendingCount()
        // So "Start writing" comes back here, and the widget knows whose words it counts.
        graph.session.publishWritingSnapshot(
            Triple(loaded.value.id, if (Naming.isUntitled(loaded.value.title)) displayTitle else loaded.value.title, loaded.value.workId),
        )
    }

    fun changed(doc: API.DocNode, count: Int) {
        document = doc
        words = count
        if (scene == null || !editable) return
        state = SaveState.Dirty
        timer?.cancel()
        // Debounced autosave, with a checkpoint revision once the writer pauses.
        timer = graph.scope.launch {
            delay(1200)
            save(createRevision = false)
            delay(20_000)
            save(createRevision = true)
        }
    }

    suspend fun save(createRevision: Boolean) = saving.withLock {
        val current = scene ?: return@withLock
        if (!editable) return@withLock
        state = SaveState.Saving
        when (val outcome = graph.sync.save(current.id, document, baseHash, createRevision)) {
            is SaveOutcome.Saved -> {
                baseHash = outcome.hash
                state = SaveState.Saved
                graph.session.publishWritingSnapshot()
            }
            SaveOutcome.Queued -> state = SaveState.Queued
            // Not a failure to retry: retrying unchanged can only fail again.
            is SaveOutcome.Conflict -> {
                state = SaveState.Conflict
                conflict = true
            }
        }
        graph.session.refreshPendingCount()
    }

    /** Leaving: anything not yet saved goes now, on the app's scope. */
    fun leave() {
        val wasDirty = state == SaveState.Dirty
        timer?.cancel()
        if (wasDirty) graph.scope.launch { save(createRevision = true) }
    }

    suspend fun reload(displayTitle: String) {
        graph.sync.discardMine(sceneId)
        conflict = false
        load(displayTitle)
    }

    /** Rebase onto whatever the server has now: "I have seen theirs and still mean this". */
    suspend fun keepMine() {
        conflict = false
        val outcome = graph.sync.keepMine(sceneId)
        if (outcome is SaveOutcome.Saved) {
            baseHash = outcome.hash
            state = SaveState.Saved
        }
        graph.session.refreshPendingCount()
    }
}

@Composable
fun SceneEditorScreen(sceneId: String, title: String, editable: Boolean, onBack: () -> Unit) {
    val graph = LocalContext.current.graph
    val session = remember(sceneId) { SceneSession(graph, sceneId, editable) }
    Scaffold(
        topBar = {
            val shown = session.scene?.title?.takeUnless { Naming.isUntitled(it) } ?: title
            BackBar(shown, onBack) {
                SaveStatus(session.words, session.state, Modifier.padding(end = 12.dp))
            }
        },
    ) { padding ->
        SceneEditor(session, title, Modifier.padding(padding).consumeWindowInsets(padding).fillMaxSize())
    }
}

@Composable
fun SaveStatus(words: Int, state: SaveState, modifier: Modifier = Modifier) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Text(plural(words, "word"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (state.label.isNotEmpty()) {
            Text(
                "  ${state.label}", style = MaterialTheme.typography.labelMedium,
                color = if (state == SaveState.Conflict) MaterialTheme.colorScheme.warning else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** The page, its banners and its format bar — on its own screen on a phone, beside the outline on a tablet. */
@Composable
fun SceneEditor(session: SceneSession, title: String, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    val commands = remember { EditorCommands() }

    LaunchedEffect(session) { session.load(title) }
    DisposableEffect(session) { onDispose { session.leave() } }

    Column(modifier.imePadding()) {
        session.problem?.let { Banner(it) }
        if (!session.editable) ReadOnlyBanner("Read only — this chapter is not yours to change.")
        if (session.scene != null) {
            ProseEditor(
                document = session.document,
                editable = session.editable,
                commands = commands,
                onChange = session::changed,
                onSelection = { session.marks = it.marks },
                onProblem = { session.problem = it },
                modifier = Modifier.weight(1f).fillMaxWidth(),
            )
        } else if (session.problem == null) {
            Loading(Modifier.weight(1f).fillMaxWidth())
        }
        if (session.editable && session.scene != null) FormatBar(session.marks, FormatBar.scene, commands::send)
    }

    if (session.conflict) {
        AlertDialog(
            onDismissRequest = {},
            title = { Text("This scene changed somewhere else") },
            text = { Text("Another device saved this scene while you were editing. Your text is still here until you choose.") },
            confirmButton = { TextButton(onClick = { scope.launch { session.keepMine() } }) { Text("Keep mine") } },
            dismissButton = { TextButton(onClick = { scope.launch { session.reload(title) } }) { Text("Reload it") } },
        )
    }
}

/**
 * The formatting a thumb can reach. Deliberately short: a phone is where a writer adds a line on a
 * train, not where they lay out a book.
 */
object FormatBar {
    data class Button(val name: String, val icon: ImageVector, val label: String)

    val scene = listOf(
        Button("bold", Icons.Default.FormatBold, "Bold"),
        Button("italic", Icons.Default.FormatItalic, "Italic"),
        Button("underline", Icons.Default.FormatUnderlined, "Underline"),
        Button("h2", Icons.Default.Title, "Heading"),
        Button("quote", Icons.Default.FormatQuote, "Quote"),
        Button("bullet", Icons.AutoMirrored.Filled.FormatListBulleted, "List"),
        // A phone has no Ctrl-Z. These are the buttons instead.
        Button("undo", Icons.AutoMirrored.Filled.Undo, "Undo"),
        Button("redo", Icons.AutoMirrored.Filled.Redo, "Redo"),
    )

    val answer = scene.filter { it.name !in setOf("h2", "bullet") }
}

@Composable
fun FormatBar(active: Set<String>, buttons: List<FormatBar.Button>, send: (String) -> Unit) {
    Surface(tonalElevation = 3.dp) {
        Row(Modifier.fillMaxWidth().height(48.dp)) {
            buttons.forEach { button ->
                val on = button.name in active
                IconButton(
                    onClick = { send(button.name) },
                    modifier = Modifier.weight(1f).fillMaxSize()
                        .background(if (on) MaterialTheme.colorScheme.primary.copy(alpha = 0.18f) else androidx.compose.ui.graphics.Color.Transparent),
                ) {
                    Icon(button.icon, contentDescription = button.label)
                }
            }
        }
    }
}

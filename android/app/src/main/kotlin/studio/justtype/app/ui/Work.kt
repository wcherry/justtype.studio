@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.automirrored.filled.Notes
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.Naming
import studio.justtype.core.model.canWrite
import studio.justtype.core.sync.Freshness

/**
 * A book: its outline, and the scene you open from it. A phone pushes the scene onto its own
 * screen; a tablet keeps the outline beside the page, so a writer can see where they are in the
 * book while they work on a paragraph of it.
 */
@Composable
fun WorkScreen(
    workId: String,
    title: String,
    onBack: () -> Unit,
    openScene: (String, String, Boolean) -> Unit,
    openVersions: () -> Unit,
    openDetails: () -> Unit,
) {
    val graph = LocalContext.current.graph
    val scope = rememberCoroutineScope()
    val pending by graph.session.pending.collectAsState()
    val synced by graph.session.synced.collectAsState()
    var outline by remember { mutableStateOf<API.OutlineResponse?>(null) }
    var offline by remember { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }

    suspend fun load() {
        val loaded = graph.sync.outline(workId)
        if (loaded == null) {
            problem = "This book is not on this phone yet, and there is no connection to fetch it."
            return
        }
        outline = loaded.value
        offline = loaded.freshness == Freshness.Local
    }
    LaunchedEffect(workId, synced) { load() }

    /** A scene's title, or "Scene 2" for where it sits in its chapter. */
    fun name(scene: API.Scene): String {
        val chapter = outline?.chapters?.firstOrNull { it.id == scene.chapterId }
        val siblings = chapter?.scenes.orEmpty()
        val number = siblings.indexOfFirst { it.id == scene.id }.let { if (it < 0) siblings.size else it } + 1
        return Naming.scene(scene.title, number, chapter?.role ?: "chapter")
    }

    Scaffold(
        topBar = {
            BackBar(outline?.title ?: title, onBack) {
                IconButton(onClick = openDetails) { Icon(Icons.Default.Info, contentDescription = "Publishing details") }
                IconButton(onClick = openVersions) { Icon(Icons.Default.History, contentDescription = "Versions") }
            }
        },
    ) { padding ->
        BoxWithConstraints(Modifier.padding(padding).fillMaxSize()) {
            val wide = maxWidth >= 840.dp
            val list = @androidx.compose.runtime.Composable {
                PullToRefreshBox(
                    isRefreshing = refreshing,
                    onRefresh = { scope.launch { refreshing = true; load(); refreshing = false } },
                ) {
                    val current = outline
                    if (current == null) {
                        if (problem != null) EmptyState("Could not open this book", problem) else Loading()
                    } else {
                        OutlineList(
                            outline = current,
                            offline = offline,
                            pending = pending,
                            selected = if (wide) selected else null,
                            onScene = { scene ->
                                if (wide) selected = scene.id
                                else openScene(scene.id, name(scene), current.canWrite(scene.chapterId))
                            },
                            onAddScene = { chapter ->
                                // Untitled — shown as "Scene 3" — so adding one never stops the writing.
                                scope.launch {
                                    val created = runCatching { graph.client.createScene(chapter.id, "") }
                                        .onFailure { problem = it.message }.getOrNull() ?: return@launch
                                    load()
                                    val scene = outline?.chapters?.flatMap { it.scenes }?.firstOrNull { it.id == created.id } ?: created
                                    if (wide) selected = scene.id
                                    else openScene(scene.id, name(scene), outline?.canWrite(scene.chapterId) ?: true)
                                }
                            },
                        )
                    }
                }
            }
            if (wide) {
                Row(Modifier.fillMaxSize()) {
                    Column(Modifier.width(320.dp).fillMaxHeight()) { list() }
                    VerticalDivider()
                    val scene = outline?.chapters?.flatMap { it.scenes }?.firstOrNull { it.id == selected }
                    if (scene == null) {
                        EmptyState("Pick a scene", "Or add one. A chapter with nothing in it is still a plan.", Icons.AutoMirrored.Filled.Notes, Modifier.weight(1f))
                    } else {
                        key(scene.id) {
                            val editable = outline?.canWrite(scene.chapterId) ?: true
                            val session = remember { SceneSession(graph, scene.id, editable) }
                            Column(Modifier.weight(1f)) {
                                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Text(name(scene), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                                    SaveStatus(session.words, session.state)
                                }
                                SceneEditor(session, name(scene), Modifier.fillMaxSize())
                            }
                        }
                    }
                }
            } else {
                list()
            }
        }
    }
}

@Composable
private fun OutlineList(
    outline: API.OutlineResponse,
    offline: Boolean,
    pending: Int,
    selected: String?,
    onScene: (API.Scene) -> Unit,
    onAddScene: (API.OutlineChapter) -> Unit,
) {
    val names = Naming.chapters(outline.chapters.map { it.title to it.role })
    LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
        item { OfflineNotice(offline, pending) }
        outline.chapters.forEachIndexed { chapterIndex, chapter ->
            item(key = "chapter-${chapter.id}") {
                SectionHeader(names[chapterIndex]) { Naming.roleBadge(chapter.role)?.let { Tag(it) } }
            }
            items(chapter.scenes.withIndex().toList(), key = { it.value.id }) { (index, scene) ->
                val untitled = Naming.isUntitled(scene.title)
                ListItem(
                    headlineContent = {
                        Text(
                            Naming.scene(scene.title, index + 1, chapter.role),
                            fontStyle = if (untitled) FontStyle.Italic else FontStyle.Normal,
                            color = if (untitled) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                        )
                    },
                    trailingContent = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(scene.wordCount.grouped(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            if (selected == null) Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                    colors = ListItemDefaults.colors(
                        containerColor = if (selected == scene.id) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
                    ),
                    modifier = Modifier.clickable { onScene(scene) },
                )
            }
            item(key = "add-${chapter.id}") {
                TextButton(onClick = { onAddScene(chapter) }, modifier = Modifier.padding(start = 8.dp)) {
                    Icon(Icons.Default.Add, contentDescription = null)
                    Text("Add scene", Modifier.padding(start = 6.dp))
                }
            }
        }
    }
}

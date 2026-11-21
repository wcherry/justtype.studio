@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Comment
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.FormatSize
import androidx.compose.material.icons.filled.RateReview
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Badge
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.Typography
import studio.justtype.app.editor.EditorCommands
import studio.justtype.app.editor.EditorSelection
import studio.justtype.app.editor.ProseEditor
import studio.justtype.app.graph
import studio.justtype.core.api.API
import kotlin.math.roundToInt

/** Versions writers have offered for review. */
@Composable
fun FeedScreen(open: (String) -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var cards by remember { mutableStateOf<List<API.PublicationCard>?>(null) }
    var mine by rememberSaveable { mutableStateOf(false) }
    var refreshing by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }

    suspend fun load() {
        runCatching { client.feed(mine) }.onSuccess { cards = it; problem = null }
            .onFailure { problem = it.message; if (cards == null) cards = emptyList() }
    }
    LaunchedEffect(mine) { load() }

    Scaffold(
        topBar = {
            TabBar(if (mine) "Your publications" else "Looking for readers") {
                TextButton(onClick = { mine = !mine }) { Text(if (mine) "Everyone’s" else "Only mine") }
            }
        },
    ) { padding ->
        PullToRefreshBox(refreshing, { scope.launch { refreshing = true; load(); refreshing = false } }, Modifier.padding(padding).fillMaxSize()) {
            val list = cards
            if (list == null) Loading() else LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
                item { ErrorText(problem) }
                if (list.isEmpty()) {
                    item { EmptyState("Nothing on offer", "When writers publish a version for review, it shows up here.", Icons.Default.RateReview) }
                }
                items(list, key = { it.id }) { card ->
                    ListItem(
                        headlineContent = { Text(card.workTitle, style = MaterialTheme.typography.titleMedium.copy(fontFamily = Serif)) },
                        supportingContent = {
                            Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                                Text("${card.author.displayName} · ${plural(card.wordCount, "word")} · ${plural(card.commentCount, "comment")}")
                                card.reviewPrompt?.takeIf { it.isNotBlank() }?.let { Text("“$it”", fontStyle = FontStyle.Italic) }
                                if (card.excerpt.isNotBlank()) Text(card.excerpt, maxLines = 3, overflow = TextOverflow.Ellipsis)
                            }
                        },
                        modifier = Modifier.clickable { open(card.id) },
                    )
                }
            }
        }
    }
}

private data class PendingAnchor(val sceneId: String, val from: Int, val to: Int, val quote: String)

/**
 * The reading and reviewing surface: the editor's page with the caret turned off. Still a real
 * ProseMirror document, so a reader selects *words*, and the anchor that makes is the one the web
 * would make for the same passage.
 */
@Composable
fun ReviewScreen(publicationId: String, onBack: () -> Unit) {
    val graph = LocalContext.current.graph
    val client = graph.client
    val scope = rememberCoroutineScope()
    val me by graph.session.user.collectAsState()
    val typography by graph.prefs.typography.collectAsState()

    var card by remember { mutableStateOf<API.PublicationCard?>(null) }
    var content by remember { mutableStateOf<API.VersionContent?>(null) }
    var index by rememberSaveable { mutableIntStateOf(0) }
    var selection by remember { mutableStateOf<EditorSelection?>(null) }
    var comments by remember { mutableStateOf<List<API.CommentView>>(emptyList()) }
    var pending by remember { mutableStateOf<PendingAnchor?>(null) }
    var showComments by remember { mutableStateOf(false) }
    var showType by remember { mutableStateOf(false) }
    var page by remember { mutableStateOf(1 to 1) }
    var problem by remember { mutableStateOf<String?>(null) }
    val commands = remember { EditorCommands() }

    val scenes = content?.chapters.orEmpty().flatMap { it.scenes }
    val scene = scenes.getOrNull(index)

    suspend fun loadComments() {
        comments = runCatching { client.comments(publicationId) }.getOrDefault(comments)
    }
    /** Reading on a phone should count for as much as reading in a browser. */
    suspend fun recordProgress() {
        val current = content?.chapters.orEmpty().flatMap { it.scenes }.getOrNull(index) ?: return
        runCatching { client.recordProgress(publicationId, current.sceneId) }
    }
    LaunchedEffect(publicationId) {
        try {
            card = client.publication(publicationId)
            content = client.publicationContent(publicationId)
        } catch (e: Exception) {
            problem = e.message
        }
        loadComments()
        recordProgress()
    }
    fun move(by: Int) {
        val next = index + by
        if (next !in scenes.indices) return
        index = next
        selection = null
        scope.launch { recordProgress() }
    }

    Scaffold(
        topBar = {
            BackBar(card?.workTitle ?: "Reading", onBack) {
                IconButton(onClick = { showType = true }) { Icon(Icons.Default.FormatSize, contentDescription = "Reading settings") }
                IconButton(onClick = { showComments = true }) {
                    BadgedBox(badge = { if (comments.isNotEmpty()) Badge { Text(comments.size.toString()) } }) {
                        Icon(Icons.AutoMirrored.Filled.Comment, contentDescription = "Feedback")
                    }
                }
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().navigationBarsPadding()) {
            problem?.let { Banner(it) }
            card?.reviewPrompt?.takeIf { it.isNotBlank() && index == 0 }?.let {
                Surface(color = MaterialTheme.colorScheme.surfaceVariant) {
                    Text("The author asked: $it", style = MaterialTheme.typography.bodySmall, modifier = Modifier.fillMaxWidth().padding(12.dp))
                }
            }
            if (scene != null) {
                androidx.compose.runtime.key(scene.sceneId) {
                    ProseEditor(
                        document = scene.content,
                        editable = false,
                        typography = typography,
                        commands = commands,
                        onSelection = { selection = if (it.isEmpty) null else it },
                        onPages = { p, of -> page = p to of },
                        modifier = Modifier.weight(1f).fillMaxWidth(),
                    )
                }
            } else if (problem == null) {
                Loading(Modifier.weight(1f).fillMaxWidth())
            } else {
                Spacer(Modifier.weight(1f))
            }

            val chosen = selection
            if (chosen != null && scene != null) {
                // Only while there is a selection to act on, so the reading surface stays one.
                Surface(color = MaterialTheme.colorScheme.primaryContainer) {
                    TextButton(
                        onClick = { pending = PendingAnchor(scene.sceneId, chosen.from, chosen.to, chosen.quote) },
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                    ) {
                        Icon(Icons.AutoMirrored.Filled.Comment, contentDescription = null)
                        Text("Comment on “${chosen.quote.take(28)}”", Modifier.padding(start = 8.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            } else if (scenes.isNotEmpty()) {
                SceneBar(
                    title = scene?.title.orEmpty(),
                    position = index + 1,
                    count = scenes.size,
                    page = if (typography.paged) page else null,
                    onPrevious = { if (typography.paged && page.first > 1) commands.turn(-1) else move(-1) },
                    onNext = { if (typography.paged && page.first < page.second) commands.turn(1) else move(1) },
                )
            }
        }
    }

    if (showType) {
        TypographySheet(typography, onChange = graph.prefs::set, onDismiss = { showType = false })
    }
    pending?.let { anchor ->
        NewCommentDialog(anchor.quote, withCategory = true, onDismiss = { pending = null }) { body, category ->
            pending = null
            selection = null
            scope.launch {
                runCatching {
                    client.addComment(publicationId, body, sceneId = anchor.sceneId, anchor = API.Anchor(anchor.from, anchor.to), anchorQuote = anchor.quote, category = category)
                }.onFailure { problem = it.message }
                loadComments()
            }
        }
    }
    if (showComments) {
        CommentsSheet(
            publicationId = publicationId,
            comments = comments,
            isAuthor = me?.id != null && me?.id == card?.author?.id,
            onDismiss = { showComments = false },
            onChange = { loadComments() },
        )
    }
}

/** Where you are, and how to get on. */
@Composable
private fun SceneBar(title: String, position: Int, count: Int, page: Pair<Int, Int>?, onPrevious: () -> Unit, onNext: () -> Unit) {
    Surface(tonalElevation = 3.dp) {
        Row(Modifier.fillMaxWidth().height(52.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onPrevious, enabled = position > 1 || (page != null && page.first > 1)) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "Previous")
            }
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(title, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    page?.let { "page ${it.first} of ${it.second} · $position/$count" } ?: "$position of $count",
                    style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = onNext, enabled = position < count || (page != null && page.first < page.second)) {
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = "Next")
            }
        }
    }
}

/** How the book should read. */
@Composable
private fun TypographySheet(value: Typography, onChange: (Typography) -> Unit, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(horizontal = 24.dp).padding(bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Reading", style = MaterialTheme.typography.titleLarge)
            val faces = listOf("serif" to "Serif", "sans" to "Sans", "mono" to "Mono")
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                faces.forEachIndexed { i, (key, label) ->
                    SegmentedButton(
                        selected = value.family == key,
                        onClick = { onChange(value.copy(family = key)) },
                        shape = SegmentedButtonDefaults.itemShape(i, faces.size),
                    ) { Text(label) }
                }
            }
            Text("Size ${value.size.roundToInt()}", style = MaterialTheme.typography.labelMedium)
            Slider(value.size, { onChange(value.copy(size = it.roundToInt().toFloat())) }, valueRange = 13f..26f, steps = 12)
            Text("Leading ${"%.1f".format(value.leading)}", style = MaterialTheme.typography.labelMedium)
            Slider(value.leading, { onChange(value.copy(leading = (it * 10).roundToInt() / 10f)) }, valueRange = 1.2f..2.2f, steps = 9)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Turn pages")
                    Text("A novel in portrait is a stack of pages rather than a scroll.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Switch(value.paged, { onChange(value.copy(paged = it)) })
            }
        }
    }
}

@Composable
fun NewCommentDialog(
    quote: String,
    withCategory: Boolean,
    onDismiss: () -> Unit,
    onSubmit: (String, API.CommentCategory) -> Unit,
) {
    var body by remember { mutableStateOf("") }
    var category by remember { mutableStateOf(API.CommentCategory.general) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(if (withCategory) "Comment" else "Reply") },
        confirmButton = { TextButton(onClick = { onSubmit(body.trim(), category) }, enabled = body.isNotBlank()) { Text("Post") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Quote(quote, maxLines = 4)
                OutlinedTextField(
                    body, { body = it }, minLines = 3, maxLines = 8, modifier = Modifier.fillMaxWidth(),
                    placeholder = { Text(if (withCategory) "What about this passage?" else "Your reply") },
                )
                if (withCategory) Picker("Kind", API.CommentCategory.entries, category, { it.raw.replaceFirstChar { c -> c.uppercase() } }) { category = it }
            }
        },
    )
}

@Composable
private fun CommentsSheet(
    publicationId: String,
    comments: List<API.CommentView>,
    isAuthor: Boolean,
    onDismiss: () -> Unit,
    onChange: suspend () -> Unit,
) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var replyingTo by remember { mutableStateOf<API.CommentView?>(null) }
    fun act(work: suspend () -> Unit) = scope.launch { runCatching { work() }; onChange() }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Text("Feedback", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp))
        LazyColumn(Modifier.fillMaxWidth(), contentPadding = androidx.compose.foundation.layout.PaddingValues(bottom = 32.dp)) {
            if (comments.isEmpty()) {
                item { EmptyState("No feedback yet", "Select words in the manuscript to comment on them.") }
            }
            fun flatten(list: List<API.CommentView>, depth: Int): List<Pair<API.CommentView, Int>> =
                list.flatMap { listOf(it to depth) + flatten(it.replies, depth + 1) }
            items(flatten(comments, 0), key = { it.first.id }) { (comment, depth) ->
                CommentRow(
                    comment, depth, isAuthor,
                    onVote = { act { client.vote(comment.id, if (comment.viewerVote == 1) 0 else 1) } },
                    onReply = { replyingTo = comment },
                    onResolve = { act { client.resolve(comment.id) } },
                )
            }
        }
    }
    replyingTo?.let { parent ->
        NewCommentDialog(parent.body, withCategory = false, onDismiss = { replyingTo = null }) { text, _ ->
            replyingTo = null
            act { client.addComment(publicationId, text, parentId = parent.id) }
        }
    }
}

@Composable
private fun CommentRow(comment: API.CommentView, depth: Int, isAuthor: Boolean, onVote: () -> Unit, onReply: () -> Unit, onResolve: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp + (depth.coerceAtMost(4) * 16).dp, end = 16.dp, top = 8.dp, bottom = 8.dp)) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.width(36.dp)) {
            val voted = comment.viewerVote == 1
            IconButton(onClick = onVote) {
                Icon(Icons.Default.ArrowUpward, contentDescription = "Useful", tint = if (voted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Text(comment.score.toString(), style = MaterialTheme.typography.labelSmall)
        }
        Column(Modifier.weight(1f).padding(start = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(comment.author.displayName, fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodyMedium)
                Tag(comment.category.raw)
                if (comment.state == API.CommentState.resolved) Text("resolved", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            comment.anchorQuote?.takeIf { it.isNotBlank() }?.let { Quote(it) }
            if (comment.state == API.CommentState.deleted) {
                // Kept only because someone answered it.
                Text("This note was removed.", fontStyle = FontStyle.Italic, color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                Text(comment.body, style = MaterialTheme.typography.bodyMedium)
            }
            Row {
                TextButton(onClick = onReply) { Text("Reply") }
                if (isAuthor && comment.state == API.CommentState.open) TextButton(onClick = onResolve) { Text("Resolve") }
            }
        }
    }
}

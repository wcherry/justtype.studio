@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.HelpOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiError
import studio.justtype.core.model.CritiqueRow
import studio.justtype.core.model.Critiques
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.PromptRoute
import studio.justtype.core.model.document
import studio.justtype.core.model.plainParagraphs

/** One answer, whole, with its critiques below. */
@Composable
fun PromptEntryScreen(entryId: String, open: (PromptRoute) -> Unit, onBack: () -> Unit) {
    val client = LocalContext.current.graph.client
    var entry by remember { mutableStateOf<API.PromptEntryView?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var converting by remember { mutableStateOf(false) }

    LaunchedEffect(entryId) {
        try {
            entry = client.promptEntry(entryId)
        } catch (e: ApiError.Http) {
            problem = if (e.status == 403) "Join Daily Prompts to read the answers." else "It may have been withdrawn or deleted."
        } catch (e: Exception) {
            problem = "It may have been withdrawn or deleted."
        }
    }

    Scaffold(topBar = { BackBar(entry?.let { PromptDays.name(it.day) } ?: "Answer", onBack) }) { padding ->
        val e = entry
        when {
            problem != null -> EmptyState("Not here", problem, Icons.AutoMirrored.Filled.HelpOutline, Modifier.padding(padding))
            e == null -> Loading(Modifier.padding(padding).fillMaxSize())
            else -> Column(
                Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Text(e.prompt, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = Serif), color = MaterialTheme.colorScheme.onSurfaceVariant)
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(e.author.displayName, fontWeight = FontWeight.SemiBold)
                    Text("· ${plural(e.wordCount, "word")}", style = MaterialTheme.typography.bodySmall)
                    if (e.late) Tag("late")
                    if (e.editedAt != null) Tag("edited")
                    if (e.mine && e.state != API.EntryState.submitted) Tag(e.state.raw)
                }
                if (e.mine) {
                    Row {
                        TextButton(onClick = { open(PromptRoute.Write(e.day)) }) { Text("Edit") }
                        TextButton(onClick = { converting = true }) { Text("Turn into a project") }
                    }
                }
                // Read here, not edited, and short enough that the web view would be more weight than it is worth.
                SelectionContainer {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        e.document.plainParagraphs.forEach {
                            Text(it, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = Serif, lineHeight = 26.sp))
                        }
                    }
                }
                Divider()
                CritiquesSection(e.id, open = e.state == API.EntryState.submitted, isAuthor = e.mine)
            }
        }
    }
    if (converting) ToWorkDialog(entryId) { converting = false }
}

@Composable
private fun CritiquesSection(entryId: String, open: Boolean, isAuthor: Boolean) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var critiques by remember { mutableStateOf<List<API.CritiqueView>>(emptyList()) }
    var draft by remember { mutableStateOf("") }
    var problem by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var composing by remember { mutableStateOf<Composer?>(null) }
    var reporting by remember { mutableStateOf<API.CritiqueView?>(null) }
    var removing by remember { mutableStateOf<API.CritiqueView?>(null) }

    suspend fun load() {
        runCatching { client.critiques(entryId) }.onSuccess { critiques = it; problem = null }.onFailure { problem = it.message }
    }
    fun act(work: suspend () -> Unit) = scope.launch {
        try { work(); load() } catch (e: Exception) { problem = e.message }
    }
    LaunchedEffect(entryId) { load() }

    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(plural(critiques.count { !it.removed }, "critique"), style = MaterialTheme.typography.titleMedium)
        if (!open && isAuthor && critiques.isNotEmpty()) Footnote("Withdrawn: only you, and each critic their own, can see these.", Modifier)
        problem?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
        notice?.let { Footnote(it, Modifier) }
        Critiques.arrange(critiques).forEach { row ->
            CritiqueItem(
                row, open, isAuthor,
                reply = { composing = Composer("Reply to ${row.critique.author?.displayName ?: "this"}", "") { text -> client.critique(entryId, text, row.critique.id) } },
                edit = { composing = Composer("Edit your critique", row.critique.body) { text -> client.editCritique(row.critique.id, text) } },
                remove = { removing = row.critique },
                report = { reporting = row.critique },
            )
        }
        if (open && !isAuthor) {
            OutlinedTextField(draft, { draft = it }, minLines = 3, maxLines = 8, placeholder = { Text("Your critique") }, modifier = Modifier.fillMaxWidth())
            Button(onClick = { val text = draft.trim(); act { client.critique(entryId, text); draft = "" } }, enabled = draft.isNotBlank()) { Text("Post") }
            Footnote("Your name is shown with it. Everyone in Daily Prompts can read it.", Modifier)
        }
    }

    composing?.let { c ->
        var text by remember(c) { mutableStateOf(c.initial) }
        AlertDialog(
            onDismissRequest = { composing = null },
            title = { Text(c.title) },
            text = { OutlinedTextField(text, { text = it }, minLines = 4, maxLines = 12, modifier = Modifier.fillMaxWidth()) },
            confirmButton = { TextButton(enabled = text.isNotBlank(), onClick = { composing = null; act { c.send(text.trim()) } }) { Text("Send") } },
            dismissButton = { TextButton(onClick = { composing = null }) { Text("Cancel") } },
        )
    }
    removing?.let { c ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(if (c.mine) "Delete your critique?" else "Remove this critique from your answer?") },
            confirmButton = {
                TextButton(onClick = { removing = null; act { client.removeCritique(c.id) } }) {
                    Text(if (c.mine) "Delete" else "Remove", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }
    reporting?.let { c ->
        val reasons = listOf(API.ReportReason.abuse to "Abusive", API.ReportReason.spam to "Spam", API.ReportReason.off_topic to "Not about the writing", API.ReportReason.other to "Something else")
        AlertDialog(
            onDismissRequest = { reporting = null },
            title = { Text("Report this critique to the writer of the answer?") },
            text = {
                Column {
                    reasons.forEach { (reason, label) ->
                        TextButton(onClick = {
                            reporting = null
                            act { client.reportCritique(c.id, reason); notice = "Reported. The writer of this answer has been told." }
                        }) { Text(label) }
                    }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { reporting = null }) { Text("Cancel") } },
        )
    }
}

private data class Composer(val title: String, val initial: String, val send: suspend (String) -> Any)

@Composable
private fun CritiqueItem(row: CritiqueRow, open: Boolean, isAuthor: Boolean, reply: () -> Unit, edit: () -> Unit, remove: () -> Unit, report: () -> Unit) {
    val c = row.critique
    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min).padding(start = (row.depth * 14).dp)) {
        Box(Modifier.width(2.dp).fillMaxHeight().background(MaterialTheme.colorScheme.surfaceVariant))
        Column(Modifier.padding(start = 8.dp)) {
            if (c.removed) {
                Text("Removed", fontStyle = FontStyle.Italic, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(c.author?.displayName.orEmpty(), fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodySmall)
                    row.replyingTo?.let { Text("replying to $it", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    if (c.editedAt != null) Text("· edited", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Text(c.body, style = MaterialTheme.typography.bodyMedium)
                Row {
                    if (open) TextButton(onClick = reply) { Text("Reply") }
                    if (c.mine) TextButton(onClick = edit) { Text("Edit") }
                    if (c.canRemove) TextButton(onClick = remove) { Text(if (c.mine) "Delete" else "Remove", color = MaterialTheme.colorScheme.error) }
                    if (!c.mine && !isAuthor) TextButton(onClick = report) { Text("Report") }
                }
            }
        }
    }
}

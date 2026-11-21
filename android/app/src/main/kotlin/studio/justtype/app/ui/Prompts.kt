@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import android.app.TimePickerDialog
import android.text.format.DateFormat
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.WifiOff
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiClient
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.PromptRoute
import java.time.ZoneId

/** Daily Prompts: today's prompt, taking part, everyone's answers, the archive, and the email. */
@Composable
fun PromptsScreen(open: (PromptRoute) -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var today by remember { mutableStateOf<API.PromptView?>(null) }
    var enrollment by remember { mutableStateOf<API.EnrollmentView?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var feedKey by remember { mutableStateOf(0) }

    suspend fun load() {
        try {
            coroutineScope {
                val t = async { client.todayPrompt() }
                val e = async { client.enrollment() }
                today = t.await().prompt
                enrollment = e.await()
            }
            problem = null
            feedKey++
        } catch (e: Exception) {
            problem = e.message
        }
    }
    LaunchedEffect(Unit) { load() }

    Scaffold(topBar = { TabBar("Daily Prompts") }) { padding ->
        PullToRefreshBox(refreshing, { scope.launch { refreshing = true; load(); refreshing = false } }, Modifier.padding(padding).fillMaxSize()) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
                problem?.let { item { Banner(it, Icons.Default.WifiOff) } }
                val prompt = today
                if (prompt == null && problem == null) item { Loading(Modifier.fillMaxWidth().padding(32.dp)) }
                if (prompt != null) {
                    item { SectionHeader("Today") }
                    item { PromptCard(prompt, enrolled = enrollment?.enrolled == true, isToday = true, open = open) }
                }
                val e = enrollment
                if (e != null && !e.enrolled) {
                    item {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Text("A prompt every day. Write an answer, keep it to yourself or share it with everyone else taking part, and read and critique theirs. An answer that wants to keep going can become a project.")
                            Button(onClick = {
                                scope.launch {
                                    try {
                                        enrollment = client.joinPrompts(ZoneId.systemDefault().id)
                                        today?.let { today = client.prompt(it.day) }
                                        feedKey++
                                    } catch (ex: Exception) {
                                        problem = ex.message
                                    }
                                }
                            }) { Text("Join Daily Prompts") }
                            Footnote("Joining does not turn on the email; you can do that afterwards.", Modifier)
                        }
                    }
                }
                if (e?.enrolled == true && prompt != null) {
                    entryFeed(prompt.day, "Today's submissions", feedKey, client, open)
                    item {
                        ListItem(headlineContent = { Text("Archive") }, modifier = Modifier.clickable { open(PromptRoute.Archive) })
                    }
                    item { PromptPreferences(e) { enrollment = it } }
                }
            }
        }
    }
}

@Composable
fun PromptCard(prompt: API.PromptView, enrolled: Boolean, isToday: Boolean, open: (PromptRoute) -> Unit) {
    Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(prompt.body, style = MaterialTheme.typography.titleLarge)
            if (enrolled) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Tag(PromptDays.stateLabel(prompt.myState))
                    Spacer(Modifier.weight(1f))
                    Button(onClick = { open(PromptRoute.Write(prompt.day)) }) {
                        Text(if (prompt.myState == "none") "Write" else "Continue", fontWeight = FontWeight.SemiBold)
                    }
                }
                Text(
                    if (isToday) "Open until ${PromptDays.time(prompt.closesAt)} · ${prompt.submissions} submitted"
                    else "You can still write on it · ${prompt.submissions} submitted",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Everyone's submitted answers to one prompt, late ones too, as rows of a list. */
fun LazyListScope.entryFeed(day: String, heading: String, key: Any, client: ApiClient, open: (PromptRoute) -> Unit) {
    item(key = "feed-$day") { EntryFeed(day, heading, key, client, open) }
}

@Composable
private fun EntryFeed(day: String, heading: String, key: Any, client: ApiClient, open: (PromptRoute) -> Unit) {
    val scope = rememberCoroutineScope()
    var cards by remember { mutableStateOf<List<API.EntryCard>>(emptyList()) }
    var cursor by remember { mutableStateOf<String?>(null) }
    var fewest by remember { mutableStateOf(false) }
    var loaded by remember { mutableStateOf(false) }

    suspend fun load(more: Boolean) {
        val page = runCatching { client.entries(day, fewest, if (more) cursor else null) }.getOrNull()
        if (page != null) {
            cards = if (more) cards + page.entries else page.entries
            cursor = page.nextCursor
        }
        loaded = true
    }
    LaunchedEffect(day, fewest, key) { load(false) }

    Column {
        SectionHeader(heading)
        val orders = listOf(false to "Newest", true to "Fewest critiques")
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
            orders.forEachIndexed { i, (value, label) ->
                SegmentedButton(fewest == value, { fewest = value }, SegmentedButtonDefaults.itemShape(i, orders.size)) { Text(label) }
            }
        }
        if (loaded && cards.isEmpty()) Footnote("No one has submitted an answer yet.", Modifier.padding(16.dp))
        cards.forEach { card ->
            ListItem(
                headlineContent = {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(card.author.displayName, fontWeight = FontWeight.SemiBold)
                        if (card.late) Tag("late")
                        if (card.editedAt != null) Tag("edited")
                        if (card.mine) Tag("yours")
                    }
                },
                supportingContent = {
                    Column {
                        Text(card.excerpt, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = Serif), maxLines = 3, overflow = TextOverflow.Ellipsis)
                        Text("${plural(card.wordCount, "word")} · ${plural(card.critiqueCount, "critique")}", style = MaterialTheme.typography.bodySmall)
                    }
                },
                modifier = Modifier.clickable { open(PromptRoute.Entry(card.id)) },
            )
        }
        if (cursor != null) TextButton(onClick = { scope.launch { load(true) } }, modifier = Modifier.padding(start = 8.dp)) { Text("More") }
    }
}

/** The email, when it comes, and leaving. */
@Composable
private fun PromptPreferences(enrollment: API.EnrollmentView, onChange: (API.EnrollmentView) -> Unit) {
    val context = LocalContext.current
    val client = context.graph.client
    val scope = rememberCoroutineScope()
    var problem by remember { mutableStateOf<String?>(null) }
    var choosingZone by remember { mutableStateOf(false) }

    fun save(work: suspend () -> API.EnrollmentView) = scope.launch {
        try {
            onChange(work())
            problem = null
        } catch (e: Exception) {
            problem = e.message
        }
    }

    Column {
        SectionHeader("Preferences")
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text("Email me the prompt each day", Modifier.weight(1f))
            Switch(
                enrollment.emailEnabled,
                { on -> save { client.updateEnrollment(emailEnabled = on) } },
                enabled = enrollment.emailVerified || enrollment.emailEnabled,
            )
        }
        val time = PromptDays.parseTime(enrollment.emailLocalTime)
        ListItem(
            headlineContent = { Text("At") },
            trailingContent = { Text(enrollment.emailLocalTime) },
            modifier = Modifier.clickable {
                TimePickerDialog(context, { _, h, m ->
                    save { client.updateEnrollment(localTime = "%02d:%02d".format(h, m)) }
                }, time.hour, time.minute, DateFormat.is24HourFormat(context)).show()
            },
        )
        ListItem(
            headlineContent = { Text("Timezone") },
            trailingContent = { Text(enrollment.emailTimezone.replace('_', ' ')) },
            modifier = Modifier.clickable { choosingZone = true },
        )
        ErrorText(problem)
        TextButton(onClick = { save { client.leavePrompts() } }, modifier = Modifier.padding(start = 8.dp)) {
            Text("Leave Daily Prompts", color = MaterialTheme.colorScheme.error)
        }
        if (!enrollment.emailVerified) {
            Footnote("Confirm your address to turn the email on; the link is in the message we sent when you signed up.")
        }
        Footnote("Each prompt opens at ${enrollment.opensAtLocal} your time and stays open for a day.")
        if (enrollment.emailEnabled) PromptDays.closingNote(enrollment.opensAtLocal, enrollment.emailLocalTime)?.let { Footnote(it) }
    }

    if (choosingZone) {
        val zones = remember(enrollment.emailTimezone) {
            (listOf(enrollment.emailTimezone) + ZoneId.getAvailableZoneIds().sorted()).distinct()
        }
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { choosingZone = false },
            confirmButton = { TextButton(onClick = { choosingZone = false }) { Text("Cancel") } },
            title = { Text("Timezone") },
            text = {
                LazyColumn {
                    items(zones) { zone ->
                        Text(
                            zone.replace('_', ' '),
                            fontWeight = if (zone == enrollment.emailTimezone) FontWeight.SemiBold else FontWeight.Normal,
                            modifier = Modifier.fillMaxWidth().clickable {
                                choosingZone = false
                                save { client.updateEnrollment(timezone = zone) }
                            }.padding(vertical = 10.dp),
                        )
                    }
                }
            },
        )
    }
}

/** Every past day, newest first. */
@Composable
fun PromptArchiveScreen(open: (PromptRoute) -> Unit, onBack: () -> Unit) {
    val client = LocalContext.current.graph.client
    val scope = rememberCoroutineScope()
    var days by remember { mutableStateOf<List<API.PromptView>?>(null) }
    var next by remember { mutableStateOf<String?>(null) }

    suspend fun load(more: Boolean) {
        val page = runCatching { client.promptArchive(if (more) next else null, 20) }.getOrNull()
        if (page == null) { if (days == null) days = emptyList(); return }
        days = if (more) days.orEmpty() + page.prompts else page.prompts
        next = page.nextBefore
    }
    LaunchedEffect(Unit) { load(false) }

    Scaffold(topBar = { BackBar("Archive", onBack) }) { padding ->
        val list = days
        if (list == null) Loading(Modifier.padding(padding).fillMaxSize())
        else LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = ListPadding) {
            if (list.isEmpty()) item { EmptyState("No earlier prompts yet", icon = Icons.Default.CalendarMonth) }
            items(list, key = { it.day }) { prompt ->
                ListItem(
                    overlineContent = { Text(PromptDays.name(prompt.day, year = true)) },
                    headlineContent = { Text(prompt.body, style = MaterialTheme.typography.bodyLarge.copy(fontFamily = Serif), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    supportingContent = { Text(if (prompt.myState == "none") "Not answered" else PromptDays.stateLabel(prompt.myState)) },
                    modifier = Modifier.clickable { open(PromptRoute.Day(prompt.day)) },
                )
            }
            if (next != null) item { TextButton(onClick = { scope.launch { load(true) } }, modifier = Modifier.padding(start = 8.dp)) { Text("Earlier") } }
        }
    }
}

/** One past day: its prompt and everyone's answers to it. */
@Composable
fun PromptDayScreen(day: String, open: (PromptRoute) -> Unit, onBack: () -> Unit) {
    val client = LocalContext.current.graph.client
    var prompt by remember { mutableStateOf<API.PromptView?>(null) }
    var enrolled by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(day) {
        try {
            prompt = client.prompt(day)
            enrolled = runCatching { client.enrollment().enrolled }.getOrDefault(false)
        } catch (e: Exception) {
            problem = e.message
        }
    }

    Scaffold(topBar = { BackBar(PromptDays.name(day), onBack) }) { padding ->
        LazyColumn(Modifier.padding(padding).fillMaxSize(), contentPadding = ListPadding) {
            problem?.let { item { Footnote(it, Modifier.padding(16.dp)) } }
            val p = prompt
            if (p == null && problem == null) item { Loading(Modifier.fillMaxWidth().padding(32.dp)) }
            if (p != null) {
                item { SectionHeader(PromptDays.name(p.day, year = true)) }
                item { PromptCard(p, enrolled, isToday = false, open = open) }
                if (enrolled) entryFeed(p.day, "Submissions", Unit, client, open)
            }
        }
    }
}

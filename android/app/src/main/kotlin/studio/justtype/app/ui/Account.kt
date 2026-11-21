@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.BuildConfig
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.hint
import studio.justtype.core.model.label
import java.time.ZoneId

/**
 * The account and the settings, split as they are on the web: the account is who you are; the
 * settings are how the app behaves, read from the account so every device agrees.
 */
@Composable
fun AccountScreen() {
    val graph = LocalContext.current.graph
    val client = graph.client
    val session = graph.session
    val scope = rememberCoroutineScope()
    val user by session.user.collectAsState()
    var section by rememberSaveable { mutableIntStateOf(0) }
    var problem by remember { mutableStateOf<String?>(null) }
    var summary by remember { mutableStateOf<API.SummaryEmailView?>(null) }
    var summaryProblem by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        runCatching { client.me() }.onSuccess(session::replaceUser)
        summary = runCatching { client.summaryEmail() }.getOrNull()
    }
    fun setting(work: suspend () -> API.User) = scope.launch {
        runCatching { work() }.onSuccess { session.replaceUser(it); problem = null }.onFailure { problem = it.message }
    }
    fun setSummary(often: API.SummaryEmail) = scope.launch {
        runCatching { client.setSummaryEmail(often, ZoneId.systemDefault().id) }
            .onSuccess { summary = it; summaryProblem = null }.onFailure { summaryProblem = it.message }
    }

    Scaffold(topBar = { TabBar(if (section == 0) "Account" else "Settings") }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(16.dp)) {
                listOf("Account", "Settings").forEachIndexed { i, label ->
                    SegmentedButton(section == i, { section = i }, SegmentedButtonDefaults.itemShape(i, 2)) { Text(label) }
                }
            }
            val u = user
            if (section == 0) {
                if (u != null) {
                    SectionHeader("Signed in as")
                    Text(u.displayName, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 16.dp))
                    Text(u.email, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
                }
                VSpace(16)
                OutlinedButton(onClick = { scope.launch { session.signOut() } }, modifier = Modifier.padding(horizontal = 16.dp)) {
                    Text("Sign out", color = MaterialTheme.colorScheme.error)
                }
                Footnote("Signing out removes your books from this phone, and anything not yet sent with them.")
                Footnote("Your password, two-factor sign-in, sessions and your data are on the web, under Account.")
                VSpace(24)
                Footnote("Version ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})")
            } else {
                SectionHeader("Appearance")
                Picker("Theme", API.Theme.entries, u?.theme ?: API.Theme.system, { it.label }, Modifier.fillMaxWidth().padding(horizontal = 16.dp)) { theme ->
                    setting { client.updateSettings(theme) }
                }
                Footnote("Kept with your account, so the web looks the same.")

                SectionHeader("Writing")
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Treat a blank line (double Enter) as a scene break", Modifier.weight(1f))
                    Switch(u?.doubleEnterSplitsScenes ?: false, { on -> setting { client.updateSettings(doubleEnterSplitsScenes = on) } })
                }
                ErrorText(problem)
                Footnote("When turning an answer into a project, a line that is only -- always starts a new scene; this adds a blank line too. Kept with your account: on the web, a double Enter while writing a book also moves on to the next scene.")

                summary?.let { view ->
                    val here = ZoneId.systemDefault().id
                    SectionHeader("Summary email")
                    Picker("Summary email", API.SummaryEmail.entries, view.often, { it.label }, Modifier.fillMaxWidth().padding(horizontal = 16.dp), enabled = view.emailVerified) {
                        setSummary(it)
                    }
                    if (view.often != API.SummaryEmail.none && view.timezone != null && view.timezone != here) {
                        TextButton(onClick = { setSummary(view.often) }, modifier = Modifier.padding(start = 8.dp)) { Text("Use $here instead") }
                    }
                    ErrorText(summaryProblem)
                    Footnote(
                        when {
                            !view.emailVerified -> "Confirm your address first: the summary only goes to a confirmed one."
                            view.often == API.SummaryEmail.none -> "What you wrote, what is still on your to-do lists, and new comments from your reviewers. Nothing is sent when nothing happened."
                            else -> "${view.often.hint} Sent at ${view.sendTime}, ${view.timezone ?: here} time. Nothing is sent when nothing happened."
                        },
                    )
                }

                SectionHeader("Daily Prompts")
                Footnote("Taking part and the daily email are set on the Prompts tab.")
            }
        }
    }
}

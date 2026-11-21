@file:OptIn(ExperimentalMaterial3Api::class)

package studio.justtype.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import studio.justtype.app.graph
import studio.justtype.core.api.API
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.PromptLink
import studio.justtype.core.model.PromptRoute

/**
 * What happened that you should hear about, with yesterday's summary pinned at the top until read.
 * The tab's badge is the count the web's bell shows; there is no push yet, so it is refreshed on
 * opening and on coming to the front.
 */
@Composable
fun InboxScreen(open: (PromptRoute) -> Unit) {
    val graph = LocalContext.current.graph
    val client = graph.client
    val session = graph.session
    val scope = rememberCoroutineScope()
    val unread by session.unread.collectAsState()
    var page by remember { mutableStateOf<API.InboxPage?>(null) }
    var items by remember { mutableStateOf<List<API.InboxItemView>>(emptyList()) }
    var refreshing by remember { mutableStateOf(false) }

    suspend fun load(more: Boolean = false) {
        val next = runCatching { client.inbox(if (more) page?.nextCursor else null) }.getOrNull() ?: return
        items = if (more) items + next.items else next.items
        page = if (more) next.copy(digest = page?.digest, items = items) else next
        session.refreshUnread()
    }
    LaunchedEffect(Unit) { load() }

    Scaffold(
        topBar = {
            TabBar("Inbox") {
                TextButton(enabled = unread > 0, onClick = {
                    scope.launch {
                        runCatching { client.markAllRead() }.onSuccess(session::setUnread)
                        load()
                    }
                }) { Text("Mark all read") }
            }
        },
    ) { padding ->
        PullToRefreshBox(refreshing, { scope.launch { refreshing = true; load(); refreshing = false } }, Modifier.padding(padding).fillMaxSize()) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = ListPadding) {
                page?.digest?.let { digest ->
                    item {
                        Card(Modifier.fillMaxWidth().padding(16.dp)) {
                            Column(Modifier.padding(16.dp)) {
                                Text(digest.text)
                                TextButton(onClick = {
                                    scope.launch {
                                        runCatching { client.markDigestRead(digest.day) }.onSuccess(session::setUnread)
                                        load()
                                    }
                                }) { Text("Dismiss") }
                            }
                        }
                    }
                }
                if (page != null && items.isEmpty() && page?.digest == null) {
                    item { EmptyState("Nothing yet", "Critiques and replies to you show up here.", Icons.Default.Inbox) }
                }
                items(items, key = { it.id }) { item ->
                    val fresh = item.readAt == null
                    ListItem(
                        leadingContent = {
                            Box(Modifier.size(8.dp).background(if (fresh) MaterialTheme.colorScheme.primary else Color.Transparent, CircleShape))
                        },
                        headlineContent = { Text(item.text, fontWeight = if (fresh) FontWeight.SemiBold else FontWeight.Normal) },
                        supportingContent = { Text(PromptDays.time(item.createdAt)) },
                        modifier = Modifier.clickable {
                            scope.launch {
                                if (fresh) runCatching { client.markRead(item.id) }.onSuccess(session::setUnread)
                                // The item's link is a web path; the ones about answers open here.
                                PromptLink.route(item.link)?.let(open)
                                load()
                            }
                        },
                    )
                }
                if (page?.nextCursor != null) item { TextButton(onClick = { scope.launch { load(more = true) } }, modifier = Modifier.padding(start = 8.dp)) { Text("Older") } }
            }
        }
    }
}

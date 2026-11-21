package studio.justtype.app.ui

import android.net.Uri
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.MenuBook
import androidx.compose.material.icons.filled.Inbox
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.RateReview
import androidx.compose.material.icons.filled.WbSunny
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.navigation.NavController
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavGraphBuilder
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.navigation
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import kotlinx.coroutines.flow.MutableStateFlow
import studio.justtype.app.graph
import studio.justtype.core.model.PromptRoute

/** Something asked for from outside the app: a link from the daily email, or "Start writing". */
sealed interface Outside {
    data class Prompt(val route: PromptRoute) : Outside
    data class Scene(val sceneId: String, val title: String) : Outside
}

private enum class Tab(val route: String, val start: String, val label: String, val icon: ImageVector) {
    Desk("tab/desk", "desk", "Desk", Icons.AutoMirrored.Filled.MenuBook),
    Review("tab/review", "review", "Review", Icons.Default.RateReview),
    Prompts("tab/prompts", "prompts", "Prompts", Icons.Default.WbSunny),
    Inbox("tab/inbox", "inbox", "Inbox", Icons.Default.Inbox),
    You("tab/you", "you", "You", Icons.Default.Person),
}

@Composable
fun Root(outside: MutableStateFlow<Outside?>) {
    val session = LocalContext.current.graph.session
    val user by session.user.collectAsState()
    val loading by session.loading.collectAsState()

    LaunchedEffect(Unit) { session.restore() }

    JustTypeTheme(user?.theme) {
        when {
            loading -> Loading()
            user == null -> SignInScreen()
            else -> MainTabs(outside)
        }
    }
}

/** Screens that want the whole height: the page, and the keyboard under it. */
private fun isFullScreen(route: String?) =
    route != null && (route.contains("/scene/") || route.contains("/publication/") || route.contains("/write/"))

@Composable
private fun MainTabs(outside: MutableStateFlow<Outside?>) {
    val graph = LocalContext.current.graph
    val session = graph.session
    val nav = rememberNavController()
    val entry by nav.currentBackStackEntryAsState()
    val unread by session.unread.collectAsState()
    val asked by outside.collectAsState()

    LaunchedEffect(Unit) { session.catchUp() }

    LaunchedEffect(asked) {
        when (val request = asked) {
            is Outside.Prompt -> {
                nav.selectTab(Tab.Prompts)
                nav.navigate(promptPath("prompts", request.route))
            }
            is Outside.Scene -> nav.navigate(scenePath(request.sceneId, request.title, true))
            null -> return@LaunchedEffect
        }
        outside.value = null
    }

    Scaffold(
        bottomBar = {
            if (!isFullScreen(entry?.destination?.route)) {
                NavigationBar {
                    for (tab in Tab.entries) {
                        val selected = entry?.destination?.hierarchy?.any { it.route == tab.route } == true
                        NavigationBarItem(
                            selected = selected,
                            onClick = {
                                if (selected) nav.popBackStack(tab.start, inclusive = false) else nav.selectTab(tab)
                            },
                            icon = {
                                if (tab == Tab.Inbox && unread > 0) {
                                    BadgedBox(badge = { Badge { Text(if (unread > 99) "99+" else unread.toString()) } }) {
                                        Icon(tab.icon, contentDescription = null)
                                    }
                                } else {
                                    Icon(tab.icon, contentDescription = null)
                                }
                            },
                            label = { Text(tab.label) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        // Only the bottom is this Scaffold's to take: each screen's own bar handles the status bar.
        val bottom = PaddingValues(bottom = padding.calculateBottomPadding())
        Box(Modifier.fillMaxSize().padding(bottom).consumeWindowInsets(bottom)) {
            NavHost(nav, startDestination = Tab.Desk.route) {
                navigation(startDestination = "desk", route = Tab.Desk.route) {
                    composable("desk") {
                        DeskScreen(openWork = { id, title -> nav.navigate("desk/work/$id?title=${Uri.encode(title)}") })
                    }
                    composable(
                        "desk/work/{workId}?title={title}",
                        arguments = listOf(navArgument("title") { defaultValue = "" }),
                    ) { back ->
                        val workId = back.arguments!!.getString("workId")!!
                        WorkScreen(
                            workId = workId,
                            title = back.arguments!!.getString("title").orEmpty(),
                            onBack = { nav.popBackStack() },
                            openScene = { id, title, editable -> nav.navigate(scenePath(id, title, editable)) },
                            openVersions = { nav.navigate("desk/versions/$workId") },
                            openDetails = { nav.navigate("desk/details/$workId") },
                        )
                    }
                    composable(
                        "desk/scene/{sceneId}?title={title}&editable={editable}",
                        arguments = listOf(
                            navArgument("title") { defaultValue = "" },
                            navArgument("editable") { type = NavType.BoolType; defaultValue = true },
                        ),
                    ) { back ->
                        SceneEditorScreen(
                            sceneId = back.arguments!!.getString("sceneId")!!,
                            title = back.arguments!!.getString("title").orEmpty(),
                            editable = back.arguments!!.getBoolean("editable"),
                            onBack = { nav.popBackStack() },
                        )
                    }
                    composable("desk/versions/{workId}") { back ->
                        VersionsScreen(back.arguments!!.getString("workId")!!, onBack = { nav.popBackStack() })
                    }
                    composable("desk/details/{workId}") { back ->
                        BookDetailsScreen(back.arguments!!.getString("workId")!!, onBack = { nav.popBackStack() })
                    }
                }
                navigation(startDestination = "review", route = Tab.Review.route) {
                    composable("review") { FeedScreen(open = { nav.navigate("review/publication/$it") }) }
                    composable("review/publication/{id}") { back ->
                        ReviewScreen(back.arguments!!.getString("id")!!, onBack = { nav.popBackStack() })
                    }
                }
                navigation(startDestination = "prompts", route = Tab.Prompts.route) {
                    composable("prompts") { PromptsScreen(open = { nav.navigate(promptPath("prompts", it)) }) }
                    promptScreens("prompts", nav)
                }
                navigation(startDestination = "inbox", route = Tab.Inbox.route) {
                    composable("inbox") { InboxScreen(open = { nav.navigate(promptPath("inbox", it)) }) }
                    promptScreens("inbox", nav)
                }
                navigation(startDestination = "you", route = Tab.You.route) {
                    composable("you") { AccountScreen() }
                }
            }
        }
    }
}

private fun NavController.selectTab(tab: Tab) = navigate(tab.route) {
    popUpTo(graph.findStartDestination().id) { saveState = true }
    launchSingleTop = true
    restoreState = true
}

private fun scenePath(id: String, title: String, editable: Boolean) =
    "desk/scene/$id?title=${Uri.encode(title)}&editable=$editable"

private fun promptPath(prefix: String, route: PromptRoute) = when (route) {
    is PromptRoute.Day -> "$prefix/day/${route.day}"
    is PromptRoute.Write -> "$prefix/write/${route.day}"
    is PromptRoute.Entry -> "$prefix/entry/${route.id}"
    PromptRoute.Archive -> "$prefix/archive"
}

/** Daily Prompts' screens, reachable from the Prompts tab and from the inbox alike. */
private fun NavGraphBuilder.promptScreens(prefix: String, nav: NavController) {
    val open: (PromptRoute) -> Unit = { nav.navigate(promptPath(prefix, it)) }
    composable("$prefix/day/{day}") { back ->
        PromptDayScreen(back.arguments!!.getString("day")!!, open = open, onBack = { nav.popBackStack() })
    }
    composable("$prefix/write/{day}") { back ->
        PromptWriteScreen(back.arguments!!.getString("day")!!, onBack = { nav.popBackStack() })
    }
    composable("$prefix/entry/{id}") { back ->
        PromptEntryScreen(back.arguments!!.getString("id")!!, open = open, onBack = { nav.popBackStack() })
    }
    composable("$prefix/archive") { PromptArchiveScreen(open = open, onBack = { nav.popBackStack() }) }
}

package studio.justtype.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import kotlinx.coroutines.flow.MutableStateFlow
import studio.justtype.app.ui.Outside
import studio.justtype.app.ui.Root
import studio.justtype.core.model.PromptLink

/** The one activity; everything on screen is Compose. */
class MainActivity : ComponentActivity() {
    /** A request from outside, held until the app is signed in and showing. */
    private val outside = MutableStateFlow<Outside?>(null)

    private val foreground = object : DefaultLifecycleObserver {
        // Coming back to the front is when a phone usually has signal again: send what waited.
        override fun onStart(owner: LifecycleOwner) = graph.session.catchUp().let { }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) handle(intent)
        ProcessLifecycleOwner.get().lifecycle.addObserver(foreground)
        setContent { Root(outside) }
    }

    override fun onDestroy() {
        ProcessLifecycleOwner.get().lifecycle.removeObserver(foreground)
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        intent ?: return
        when (intent.action) {
            // The daily email's "Write your answer", and its other links into Daily Prompts.
            Intent.ACTION_VIEW -> intent.data?.path?.let(PromptLink::route)?.let { outside.value = Outside.Prompt(it) }
            // The launcher shortcut and the widget: the page in front, not three taps away.
            START_WRITING -> graph.session.lastScene()?.let { last ->
                last.sceneId?.let { outside.value = Outside.Scene(it, last.sceneTitle ?: "Writing") }
            }
        }
    }

    companion object {
        const val START_WRITING = "studio.justtype.app.START_WRITING"
    }
}

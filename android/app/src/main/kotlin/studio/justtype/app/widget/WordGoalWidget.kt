package studio.justtype.app.widget

import android.content.Context
import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.LinearProgressIndicator
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.provideContent
import androidx.glance.appwidget.updateAll
import androidx.glance.background
import androidx.glance.layout.Column
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.padding
import androidx.glance.text.FontFamily
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import studio.justtype.app.MainActivity
import studio.justtype.core.sync.WritingSnapshot
import java.io.File

/**
 * Today's words on the home screen. It reads what the app left behind rather than fetching
 * anything: a widget cannot sign in, and giving it a refresh token so it can draw a progress bar
 * would be a credential in a second place for no reason. Tapping it carries on writing.
 */
class WordGoalWidget : GlanceAppWidget() {
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        val snapshot = WritingSnapshot.load(File(context.filesDir, "writing-today.json"))
        provideContent { GlanceTheme { Content(snapshot) } }
    }

    @Composable
    private fun Content(snapshot: WritingSnapshot?) {
        val secondary = GlanceTheme.colors.onSurfaceVariant
        val open = actionStartActivity(
            Intent(androidx.glance.LocalContext.current, MainActivity::class.java).setAction(MainActivity.START_WRITING),
        )
        Column(GlanceModifier.fillMaxSize().background(GlanceTheme.colors.widgetBackground).padding(14.dp).clickable(open)) {
            Text("Today", style = TextStyle(fontSize = 12.sp, color = secondary))
            if (snapshot == null) {
                // Not an error. Nobody has opened the app yet, and saying so beats a zero that looks like a bad day.
                Text("Open Just Type Studio to start counting.", style = TextStyle(fontSize = 12.sp, color = secondary))
                return@Column
            }
            Text("${snapshot.words}", style = TextStyle(fontSize = 30.sp, fontFamily = FontFamily.Serif, color = GlanceTheme.colors.onSurface))
            val goal = snapshot.goal
            if (goal != null) {
                LinearProgressIndicator(
                    progress = snapshot.fraction,
                    modifier = GlanceModifier.fillMaxWidth().padding(vertical = 4.dp),
                    color = if (snapshot.fraction >= 1f) ColorProvider(Color(0xFF2E7D32)) else GlanceTheme.colors.primary,
                    backgroundColor = GlanceTheme.colors.surfaceVariant,
                )
                Text("of $goal", style = TextStyle(fontSize = 11.sp, color = secondary))
            } else {
                Text("words", style = TextStyle(fontSize = 11.sp, color = secondary))
            }
            // The week, when there is a target for it: a kinder measure for someone who writes three evenings, not seven.
            snapshot.weekGoal?.let { Text("${snapshot.weekWords ?: 0} of $it this week", style = TextStyle(fontSize = 11.sp, color = secondary)) }
            if (snapshot.streak > 1) Text("${snapshot.streak} days running", style = TextStyle(fontSize = 11.sp, color = secondary))
        }
    }

    companion object {
        /** Redraws every placed widget from the file the app just wrote. */
        fun refresh(context: Context) {
            CoroutineScope(Dispatchers.Default).launch { runCatching { WordGoalWidget().updateAll(context) } }
        }
    }
}

class WordGoalWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = WordGoalWidget()
}

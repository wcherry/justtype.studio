package studio.justtype.app

import android.app.Application
import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import studio.justtype.core.api.ApiClient
import studio.justtype.core.api.TokenManager
import studio.justtype.core.sync.LocalStore
import studio.justtype.core.sync.SaveQueue
import studio.justtype.core.sync.SyncEngine
import java.io.File

/** Everything the screens share, made once. */
class AppGraph(context: Context) {
    /** Outlives every screen, so leaving one never cancels the save it started. */
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    val client = ApiClient(BuildConfig.API_BASE_URL, TokenManager(SecureTokenStore(context)))
    private val offline = File(context.filesDir, "Offline")
    val sync = SyncEngine(client, LocalStore(offline), SaveQueue(offline))
    val snapshotFile = File(context.filesDir, "writing-today.json")
    val prefs = ReaderPrefs(context)
    val session = SessionStore(context.applicationContext, this)
}

class JustTypeApplication : Application() {
    lateinit var graph: AppGraph
        private set

    override fun onCreate() {
        super.onCreate()
        graph = AppGraph(this)
    }
}

val Context.graph: AppGraph get() = (applicationContext as JustTypeApplication).graph

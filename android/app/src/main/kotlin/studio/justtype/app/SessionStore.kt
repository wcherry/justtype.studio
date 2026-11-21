package studio.justtype.app

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import studio.justtype.app.widget.WordGoalWidget
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiError
import studio.justtype.core.api.ApiJson
import studio.justtype.core.model.PromptDays
import studio.justtype.core.sync.WritingSnapshot

/** The signed-in writer and what the whole app needs to know about them. Mirrors iOS's `SessionStore`. */
class SessionStore(private val context: Context, private val graph: AppGraph) {
    private val client get() = graph.client

    private val _user = MutableStateFlow<API.User?>(null)
    val user: StateFlow<API.User?> = _user.asStateFlow()

    private val _loading = MutableStateFlow(true)
    val loading: StateFlow<Boolean> = _loading.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    /** The password was right; the account wants a code as well. */
    private val _needsCode = MutableStateFlow(false)
    val needsCode: StateFlow<Boolean> = _needsCode.asStateFlow()

    /** Edits the server has not seen. Shown rather than hidden: a writer is owed the truth. */
    private val _pending = MutableStateFlow(0)
    val pending: StateFlow<Int> = _pending.asStateFlow()

    private val _unread = MutableStateFlow(0L)
    val unread: StateFlow<Long> = _unread.asStateFlow()

    /** Bumped each time a catch-up finishes, so lists drawn from an older copy redraw. */
    private val _synced = MutableStateFlow(0)
    val synced: StateFlow<Int> = _synced.asStateFlow()

    fun replaceUser(next: API.User) {
        _user.value = next
        remember(next)
    }
    fun setUnread(count: Long) { _unread.value = count }
    fun clearError() { _error.value = null }

    /**
     * A stored refresh token may be stale; asking the server is the only way to find out. But only
     * the server saying no signs the writer out: with no signal they get the account they last had
     * and the copy of their books on this phone, which is what offline writing is for.
     */
    suspend fun restore() {
        try {
            if (!client.tokens.isSignedIn) return
            _user.value = try {
                client.me().also(::remember)
            } catch (e: ApiError.NotAuthenticated) {
                null
            } catch (e: ApiError) {
                remembered()
            }
        } finally {
            _loading.value = false
        }
    }

    private val userFile = java.io.File(context.noBackupFilesDir, "user.json")

    private fun remember(user: API.User) {
        runCatching { userFile.writeText(ApiJson.encodeToString(API.User.serializer(), user)) }
    }

    private fun remembered(): API.User? =
        runCatching { ApiJson.decodeFromString(API.User.serializer(), userFile.readText()) }.getOrNull()

    suspend fun refreshUnread() {
        runCatching { client.unreadCount() }.onSuccess { _unread.value = it }
    }

    suspend fun refreshPendingCount() { _pending.value = graph.sync.pendingCount() }

    /** On coming back to the front, the moment a phone usually has signal again. */
    fun catchUp() = graph.scope.launch {
        if (_user.value == null) return@launch
        refreshUnread()
        graph.sync.flush()
        refreshPendingCount()
        _synced.value += 1
        publishWritingSnapshot()
    }

    suspend fun signIn(email: String, password: String, code: String?) = attempt { client.signIn(email, password, code).user }

    suspend fun register(email: String, password: String, displayName: String, acceptTerms: Boolean) =
        attempt { client.register(email, password, displayName, acceptTerms).user }

    suspend fun signOut() {
        client.signOut()
        // A shared phone should not keep somebody else's unpublished novel.
        graph.sync.signOut()
        graph.snapshotFile.delete()
        userFile.delete()
        WordGoalWidget.refresh(context)
        _pending.value = 0
        _user.value = null
    }

    private suspend fun attempt(work: suspend () -> API.User) {
        _error.value = null
        try {
            _user.value = work().also(::remember)
            _needsCode.value = false
        } catch (e: ApiError.Http) {
            // Not a failure: ask for the code and go again.
            if (e.code == "two_factor_required") _needsCode.value = true else _error.value = e.message
        } catch (e: ApiError) {
            _error.value = e.message
        }
    }

    /**
     * Leaves today's writing where the widget and the "Start writing" shortcut can find it. Written
     * by the app because the app is the only thing here that can sign in.
     */
    suspend fun publishWritingSnapshot(lastScene: Triple<String, String, String>? = null) {
        val summary = runCatching { client.writing(PromptDays.today()) }.getOrNull()
        val previous = WritingSnapshot.load(graph.snapshotFile)
        val next = WritingSnapshot(
            words = summary?.wordsToday ?: previous?.words ?: 0,
            goal = summary?.dailyWordGoal ?: previous?.goal,
            streak = summary?.streak ?: previous?.streak ?: 0,
            weekWords = summary?.wordsThisWeek ?: previous?.weekWords,
            weekGoal = summary?.weeklyWordGoal ?: previous?.weekGoal,
            sceneId = lastScene?.first ?: previous?.sceneId,
            sceneTitle = lastScene?.second ?: previous?.sceneTitle,
            workId = lastScene?.third ?: previous?.workId,
        )
        if (summary == null && lastScene == null) return
        WritingSnapshot.save(graph.snapshotFile, next)
        WordGoalWidget.refresh(context)
    }

    fun lastScene(): WritingSnapshot? = WritingSnapshot.load(graph.snapshotFile)
}

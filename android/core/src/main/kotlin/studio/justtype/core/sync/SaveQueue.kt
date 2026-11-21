package studio.justtype.core.sync

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson
import java.io.File

/** A save the writer has made and the server has not seen. */
@Serializable
data class PendingSave(
    val sceneId: String,
    val content: API.DocNode,
    /** The hash the edit was made on top of. The server refuses a write whose base has moved. */
    val baseHash: String? = null,
    val createRevision: Boolean,
    val queuedAt: Long = System.currentTimeMillis(),
    /** Set when the server refused it. The writer has to choose, so it stops being retried. */
    val conflicted: Boolean = false,
    /** For a Daily Prompts answer: the prompt's day. Its `sceneId` is then `prompt:<day>`. */
    val day: String? = null,
    /** For an answer: the writer's date when they wrote it, for their daily goal. */
    val localDay: String? = null,
) {
    companion object {
        fun entryKey(day: String) = "prompt:$day"
    }
}

/**
 * Saves waiting for a network, kept on disk.
 *
 * One entry per scene, because a writer who typed for an hour in a tunnel wants the paragraph as
 * it stands, not a hundred versions of it on the way there. The newest edit supersedes the one
 * before it.
 */
class SaveQueue(directory: File) {
    private val file = File(directory, "pending.json")
    private val mutex = Mutex()
    private val serializer = MapSerializer(String.serializer(), PendingSave.serializer())
    private var entries: MutableMap<String, PendingSave> = run {
        directory.mkdirs()
        runCatching { ApiJson.decodeFromString(serializer, file.readText()).toMutableMap() }.getOrElse { mutableMapOf() }
    }

    suspend fun enqueue(save: PendingSave) = mutex.withLock {
        // The base of the *first* unsent edit is what the server must be measured against; later
        // edits in the same sitting were made on top of text the server has never seen.
        val existing = entries[save.sceneId]
        entries[save.sceneId] = if (existing != null && !existing.conflicted) save.copy(baseHash = existing.baseHash) else save
        persist()
    }

    suspend fun pending(sceneId: String): PendingSave? = mutex.withLock { entries[sceneId] }

    suspend fun all(): List<PendingSave> = mutex.withLock { entries.values.sortedBy { it.queuedAt } }

    suspend fun count(): Int = mutex.withLock { entries.size }

    suspend fun remove(sceneId: String) = mutex.withLock {
        entries.remove(sceneId)
        persist()
    }

    suspend fun markConflicted(sceneId: String) = mutex.withLock {
        entries[sceneId]?.let { entries[sceneId] = it.copy(conflicted = true) }
        persist()
    }

    /** The writer has chosen to keep their version: the base moves to the server's, and it goes back in the queue. */
    suspend fun rebase(sceneId: String, hash: String) = mutex.withLock {
        entries[sceneId]?.let { entries[sceneId] = it.copy(baseHash = hash, conflicted = false) }
        persist()
    }

    suspend fun clear() = mutex.withLock {
        entries.clear()
        persist()
    }

    private suspend fun persist() = withContext(Dispatchers.IO) {
        file.writeAtomically(ApiJson.encodeToString(serializer, entries))
    }
}

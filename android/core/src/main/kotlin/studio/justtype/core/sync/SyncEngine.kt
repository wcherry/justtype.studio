package studio.justtype.core.sync

import studio.justtype.core.api.API
import studio.justtype.core.api.ApiClient
import studio.justtype.core.api.ApiError
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.document

/** What became of a save. */
sealed interface SaveOutcome {
    /** The server has it. Carries the new content hash to build the next edit on. */
    data class Saved(val hash: String) : SaveOutcome
    /** No network. It is on disk and will go when there is one. */
    data object Queued : SaveOutcome
    /** The server refused it: the scene moved under the edit. */
    data class Conflict(val message: String) : SaveOutcome
}

/** What became of saving a Daily Prompts answer. */
sealed interface EntrySaveOutcome {
    data class Saved(val entry: API.PromptEntryView) : EntrySaveOutcome
    data object Queued : EntrySaveOutcome
    data class Conflict(val message: String) : EntrySaveOutcome
    /** Refused for good (not enrolled, say). Not queued: it would fail again. */
    data class Refused(val message: String) : EntrySaveOutcome
}

/** What a read came from, so the writer can be told. */
enum class Freshness { Server, Local }

data class Loaded<T>(val value: T, val freshness: Freshness)

/**
 * The layer between the screens and the network — iOS's `SyncEngine`, rule for rule.
 *
 * Every read prefers the server and falls back to the phone. Every write goes to the phone first
 * and to the server if it can. The rule that matters most is in [scene]: a queued edit beats the
 * server's copy, always. Handing a writer the server's older text because a save had not gone
 * through yet would silently undo their work in front of them.
 */
class SyncEngine(private val client: ApiClient, private val store: LocalStore, private val queue: SaveQueue) {

    // region Reading

    suspend fun works(): Loaded<List<API.Work>>? {
        val fresh = runCatching { client.works() }.getOrNull()
        if (fresh != null) {
            store.putWorks(fresh)
            return Loaded(fresh, Freshness.Server)
        }
        return store.works()?.let { Loaded(it, Freshness.Local) }
    }

    suspend fun outline(workId: String): Loaded<API.OutlineResponse>? {
        val fresh = runCatching { client.outline(workId) }.getOrNull()
        if (fresh != null) {
            store.putOutline(fresh)
            return Loaded(fresh, Freshness.Server)
        }
        return store.outline(workId)?.let { Loaded(it, Freshness.Local) }
    }

    suspend fun scene(id: String): Loaded<API.SceneWithContent>? {
        val fresh = runCatching { client.scene(id) }.getOrNull()
        if (fresh != null) store.putScene(fresh)

        // A pending edit is newer than anything the server can offer, and the hash it was made on
        // top of is the one the next save must carry.
        queue.pending(id)?.let { pending ->
            val base = fresh ?: store.scene(id) ?: return null
            return Loaded(
                base.copy(content = pending.content, contentHash = pending.baseHash ?: base.contentHash),
                Freshness.Local,
            )
        }
        if (fresh != null) return Loaded(fresh, Freshness.Server)
        return store.scene(id)?.let { Loaded(it, Freshness.Local) }
    }

    // endregion

    // region Writing

    suspend fun save(sceneId: String, content: API.DocNode, baseHash: String?, createRevision: Boolean): SaveOutcome {
        // Disk first. Whatever happens to the network, the words are kept.
        store.scene(sceneId)?.let { store.putScene(it.copy(content = content)) }

        return try {
            val result = client.saveScene(sceneId, content, createRevision, baseHash)
            queue.remove(sceneId)
            // Built from the row the server sent rather than from what was cached, so a rename on
            // another device does not come back.
            store.scene(sceneId)?.let {
                store.putScene(
                    it.copy(
                        content = content,
                        contentHash = result.scene.contentHash,
                        wordCount = result.scene.wordCount,
                        updatedAt = result.scene.updatedAt,
                    ),
                )
            }
            SaveOutcome.Saved(result.scene.contentHash)
        } catch (e: ApiError.Conflict) {
            queue.enqueue(PendingSave(sceneId, content, baseHash, createRevision, conflicted = true))
            SaveOutcome.Conflict(e.message ?: "Changed elsewhere")
        } catch (e: Exception) {
            if (e is kotlinx.coroutines.CancellationException) {
                // Still keep the words: cancellation must not be the thing that loses them.
                queue.enqueue(PendingSave(sceneId, content, baseHash, createRevision))
                throw e
            }
            // Anything else — no network, a dead server, a tunnel — is a reason to wait, not a
            // reason to lose the paragraph.
            queue.enqueue(PendingSave(sceneId, content, baseHash, createRevision))
            SaveOutcome.Queued
        }
    }

    // endregion

    // region Daily Prompts answers

    data class EntryLoad(val entry: API.PromptEntryView?, val document: API.DocNode?, val freshness: Freshness)

    /** The writer's answer to `day`, and the document to open: a queued edit wins, as for a scene. */
    suspend fun entry(day: String): EntryLoad {
        val key = PendingSave.entryKey(day)
        var reached: Boolean
        val entry: API.PromptEntryView? = try {
            client.entry(day).also { reached = true; if (it != null) store.putEntry(it) }
        } catch (e: Exception) {
            reached = false
            store.entry(day)
        }
        queue.pending(key)?.let { return EntryLoad(entry, it.content, Freshness.Local) }
        return EntryLoad(entry, entry?.document, if (reached) Freshness.Server else Freshness.Local)
    }

    suspend fun saveEntry(day: String, content: API.DocNode, baseHash: String?, localDay: String): EntrySaveOutcome {
        val key = PendingSave.entryKey(day)
        return try {
            val saved = client.saveEntry(day, content, baseHash, localDay)
            queue.remove(key)
            store.putEntry(saved)
            EntrySaveOutcome.Saved(saved)
        } catch (e: ApiError.Conflict) {
            queue.enqueue(PendingSave(key, content, baseHash, false, conflicted = true, day = day, localDay = localDay))
            EntrySaveOutcome.Conflict(e.message ?: "Changed elsewhere")
        } catch (e: ApiError.Http) {
            if (e.status in 400..499 && e.status != 408 && e.status != 429) {
                EntrySaveOutcome.Refused(e.message ?: "Refused")
            } else {
                queue.enqueue(PendingSave(key, content, baseHash, false, day = day, localDay = localDay))
                EntrySaveOutcome.Queued
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            queue.enqueue(PendingSave(key, content, baseHash, false, day = day, localDay = localDay))
            throw e
        } catch (e: Exception) {
            queue.enqueue(PendingSave(key, content, baseHash, false, day = day, localDay = localDay))
            EntrySaveOutcome.Queued
        }
    }

    /** The writer keeps their answer over the one saved elsewhere. */
    suspend fun keepMyEntry(day: String): EntrySaveOutcome {
        val key = PendingSave.entryKey(day)
        val pending = queue.pending(key) ?: return EntrySaveOutcome.Queued
        val latest = runCatching { client.entry(day) }.getOrNull() ?: return EntrySaveOutcome.Queued
        queue.rebase(key, latest.contentHash)
        return saveEntry(day, pending.content, latest.contentHash, pending.localDay ?: PromptDays.today())
    }

    suspend fun discardMyEntry(day: String) = queue.remove(PendingSave.entryKey(day))

    // endregion

    // region Catching up

    suspend fun pendingCount(): Int = queue.count()

    suspend fun pending(sceneId: String): PendingSave? = queue.pending(sceneId)

    /**
     * Send everything waiting. Called when the app comes back to the front. A conflicted entry is
     * left alone: it is waiting on a person, and retrying it can only fail the same way.
     */
    suspend fun flush(): Map<String, SaveOutcome> {
        val outcomes = mutableMapOf<String, SaveOutcome>()
        for (entry in queue.all()) {
            if (entry.conflicted) continue
            val outcome = if (entry.day != null) {
                when (val sent = saveEntry(entry.day, entry.content, entry.baseHash, entry.localDay ?: PromptDays.today())) {
                    is EntrySaveOutcome.Saved -> SaveOutcome.Saved(sent.entry.contentHash)
                    EntrySaveOutcome.Queued -> SaveOutcome.Queued
                    is EntrySaveOutcome.Conflict -> SaveOutcome.Conflict(sent.message)
                    is EntrySaveOutcome.Refused -> SaveOutcome.Conflict(sent.message)
                }
            } else {
                save(entry.sceneId, entry.content, entry.baseHash, entry.createRevision)
            }
            outcomes[entry.sceneId] = outcome
            // Still no network: the rest will fare no better, and hammering a dead connection is
            // how a phone gets warm on a plane.
            if (outcome == SaveOutcome.Queued) break
        }
        return outcomes
    }

    /** The writer chose their version over the server's. */
    suspend fun keepMine(sceneId: String): SaveOutcome {
        val pending = queue.pending(sceneId) ?: return SaveOutcome.Queued
        val latest = runCatching { client.scene(sceneId) }.getOrNull() ?: return SaveOutcome.Queued
        queue.rebase(sceneId, latest.contentHash)
        return save(sceneId, pending.content, latest.contentHash, createRevision = true)
    }

    /** The writer chose the server's version. Their queued edit goes. */
    suspend fun discardMine(sceneId: String) = queue.remove(sceneId)

    suspend fun signOut() {
        queue.clear()
        store.clear()
    }

    // endregion
}

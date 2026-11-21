package studio.justtype.core.sync

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson
import java.io.File

/**
 * The copy of the manuscript that lives on the phone.
 *
 * Not a cache in the "nice to have" sense. A writer on a plane has opened this app to write, and
 * an app that shows them a spinner has failed at the only thing it was for. Everything the writer
 * has looked at is on disk, and everything they type goes to disk before it goes anywhere else.
 * Files rather than a database, as on iOS and on the server: the shapes are small and read whole.
 */
class LocalStore(private val directory: File) {
    private val mutex = Mutex()

    init { directory.mkdirs() }

    suspend fun works(): List<API.Work>? = read("works", ListSerializer(API.Work.serializer()))
    suspend fun putWorks(works: List<API.Work>) = write("works", ListSerializer(API.Work.serializer()), works)

    suspend fun outline(workId: String): API.OutlineResponse? = read("outline-${safe(workId)}", API.OutlineResponse.serializer())
    suspend fun putOutline(outline: API.OutlineResponse) =
        write("outline-${safe(outline.id)}", API.OutlineResponse.serializer(), outline)

    suspend fun scene(id: String): API.SceneWithContent? = read("scene-${safe(id)}", API.SceneWithContent.serializer())
    suspend fun putScene(scene: API.SceneWithContent) = write("scene-${safe(scene.id)}", API.SceneWithContent.serializer(), scene)

    /** A Daily Prompts answer, so the writing screen opens without a network. */
    suspend fun entry(day: String): API.PromptEntryView? = read("entry-${safe(day)}", API.PromptEntryView.serializer())
    suspend fun putEntry(entry: API.PromptEntryView) = write("entry-${safe(entry.day)}", API.PromptEntryView.serializer(), entry)

    /** Everything, for signing out. A shared phone should not keep somebody else's unpublished novel. */
    suspend fun clear() = mutex.withLock {
        withContext(Dispatchers.IO) {
            directory.deleteRecursively()
            directory.mkdirs()
        }
    }

    private fun file(name: String) = File(directory, "$name.json")

    private suspend fun <T> read(name: String, serializer: KSerializer<T>): T? = mutex.withLock {
        withContext(Dispatchers.IO) {
            val file = file(name)
            if (!file.exists()) null
            else runCatching { ApiJson.decodeFromString(serializer, file.readText()) }.getOrNull()
        }
    }

    private suspend fun <T> write(name: String, serializer: KSerializer<T>, value: T) = mutex.withLock {
        withContext(Dispatchers.IO) { file(name).writeAtomically(ApiJson.encodeToString(serializer, value)) }
    }
}

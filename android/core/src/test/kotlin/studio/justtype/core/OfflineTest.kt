package studio.justtype.core

import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiClient
import studio.justtype.core.api.ApiJson
import studio.justtype.core.api.MemoryTokenStore
import studio.justtype.core.api.TokenManager
import studio.justtype.core.model.DocNodes
import studio.justtype.core.model.plainText
import studio.justtype.core.sync.Freshness
import studio.justtype.core.sync.LocalStore
import studio.justtype.core.sync.SaveOutcome
import studio.justtype.core.sync.SaveQueue
import studio.justtype.core.sync.SyncEngine
import java.io.File
import java.nio.file.Files

/**
 * Writing on a plane — iOS's `OfflineTests`, against a real HTTP server that can be switched off,
 * so "no signal" is a real connection failure rather than a mocked method call.
 */
class OfflineTest {
    private lateinit var directory: File
    private lateinit var server: MockWebServer
    private lateinit var store: LocalStore
    private lateinit var engine: SyncEngine
    private val saves = mutableListOf<String>()

    @Volatile private var online = true
    @Volatile private var conflict = false
    @Volatile private var serverText = "The field went on."

    @Before fun setUp() {
        directory = Files.createTempDirectory("justtype").toFile()
        server = MockWebServer()
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                if (!online) return MockResponse().setSocketPolicy(okhttp3.mockwebserver.SocketPolicy.DISCONNECT_AT_START)
                val path = request.path!!
                return when {
                    request.method == "GET" && path.endsWith("/scenes/scene-1") ->
                        json(ApiJson.encodeToString(API.SceneWithContent.serializer(), scene(serverText, "hash-server")))
                    request.method == "PUT" && path.endsWith("/scenes/scene-1/content") -> {
                        saves += request.body.readUtf8()
                        if (conflict) MockResponse().setResponseCode(409)
                            .setBody("""{"error":{"code":"conflict","message":"Changed elsewhere"}}""")
                        else json("""{"scene":${ApiJson.encodeToString(API.Scene.serializer(), plain("hash-new"))},"workWordCount":5}""")
                    }
                    else -> MockResponse().setResponseCode(404)
                }
            }
        }
        server.start()
        val client = ApiClient(server.url("/api").toString(), TokenManager(MemoryTokenStore()))
        store = LocalStore(directory)
        engine = SyncEngine(client, store, SaveQueue(directory))
    }

    @After fun tearDown() {
        server.shutdown()
        directory.deleteRecursively()
    }

    private fun json(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)

    private fun plain(hash: String) = API.Scene(
        chapterId = "ch-1", contentHash = hash, createdAt = "2026-01-01T00:00:00Z", id = "scene-1", position = 0,
        status = "draft", title = "Opening", updatedAt = "2026-01-01T00:00:00Z", wordCount = 4, workId = "work-1",
    )

    private fun scene(text: String, hash: String = "hash-1") = API.SceneWithContent(
        chapterId = "ch-1", contentHash = hash, createdAt = "2026-01-01T00:00:00Z", id = "scene-1", position = 0,
        status = "draft", title = "Opening", updatedAt = "2026-01-01T00:00:00Z", wordCount = 4, workId = "work-1",
        content = DocNodes.fromPlainText(text),
    )

    @Test fun aSaveWithNoNetworkIsKeptRatherThanLost() = runBlocking {
        store.putScene(scene("The field went on."))
        online = false
        val outcome = engine.save("scene-1", DocNodes.fromPlainText("Past where the light gave out."), "hash-1", false)
        assertEquals(SaveOutcome.Queued, outcome)
        assertEquals("Past where the light gave out.", engine.pending("scene-1")?.content?.plainText)
    }

    /** The bug this exists to prevent: reopening offline must not show the server's older text. */
    @Test fun reopeningShowsTheWritersOwnWordsEvenOnline() = runBlocking {
        store.putScene(scene("The field went on."))
        online = false
        engine.save("scene-1", DocNodes.fromPlainText("Her own newer sentence."), "hash-1", false)

        val offline = engine.scene("scene-1")!!
        assertEquals("Her own newer sentence.", offline.value.content.plainText)
        assertEquals(Freshness.Local, offline.freshness)

        online = true
        val back = engine.scene("scene-1")!!
        assertEquals("Her own newer sentence.", back.value.content.plainText)
        assertEquals("hash-1", back.value.contentHash)
    }

    @Test fun aSittingOfEditsBecomesOneSaveOnTheOriginalBase() = runBlocking {
        store.putScene(scene("The field went on."))
        online = false
        engine.save("scene-1", DocNodes.fromPlainText("One."), "hash-1", false)
        engine.save("scene-1", DocNodes.fromPlainText("One. Two."), "hash-2", false)
        engine.save("scene-1", DocNodes.fromPlainText("One. Two. Three."), "hash-3", false)
        assertEquals(1, engine.pendingCount())
        assertEquals("hash-1", engine.pending("scene-1")?.baseHash)

        online = true
        val outcomes = engine.flush()
        assertEquals(SaveOutcome.Saved("hash-new"), outcomes["scene-1"])
        assertEquals(0, engine.pendingCount())
        assertTrue(saves.single().contains("\"baseHash\":\"hash-1\""))
        assertTrue(saves.single().contains("One. Two. Three."))
    }

    @Test fun aConflictWaitsForTheWriterAndIsNotRetried() = runBlocking {
        store.putScene(scene("The field went on."))
        conflict = true
        val outcome = engine.save("scene-1", DocNodes.fromPlainText("Mine."), "hash-1", false)
        assertTrue(outcome is SaveOutcome.Conflict)
        assertTrue(engine.pending("scene-1")!!.conflicted)

        saves.clear()
        engine.flush()
        assertTrue("a conflicted save is not retried", saves.isEmpty())

        // Keeping mine rebases onto the server's hash and sends again.
        conflict = false
        assertEquals(SaveOutcome.Saved("hash-new"), engine.keepMine("scene-1"))
        assertTrue(saves.single().contains("\"baseHash\":\"hash-server\""))
        assertNull(engine.pending("scene-1"))
    }

    @Test fun theQueueSurvivesARestart() = runBlocking {
        online = false
        engine.save("scene-1", DocNodes.fromPlainText("Kept."), "hash-1", true)
        val reopened = SaveQueue(directory)
        assertEquals("Kept.", reopened.pending("scene-1")?.content?.plainText)
    }

    @Test fun signingOutLeavesNothingBehind() = runBlocking {
        store.putScene(scene("Private."))
        online = false
        engine.save("scene-1", DocNodes.fromPlainText("Private, newer."), "hash-1", true)
        engine.signOut()
        assertEquals(0, engine.pendingCount())
        assertNull(store.scene("scene-1"))
    }
}

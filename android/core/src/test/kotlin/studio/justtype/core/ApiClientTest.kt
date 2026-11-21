package studio.justtype.core

import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import studio.justtype.core.api.ApiClient
import studio.justtype.core.api.ApiError
import studio.justtype.core.api.MemoryTokenStore
import studio.justtype.core.api.TokenManager
import java.util.concurrent.atomic.AtomicInteger

class ApiClientTest {
    private lateinit var server: MockWebServer
    private val refreshes = AtomicInteger()
    private val user = """{"createdAt":"x","displayName":"Ada","doubleEnterSplitsScenes":false,"email":"a@b.c","id":"u1",
        "isAdmin":false,"isModerator":false,"theme":"sepia","updatedAt":"x"}"""

    @Before fun setUp() {
        server = MockWebServer()
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = when (request.path) {
                "/api/auth/me" ->
                    if (request.getHeader("Authorization") == "Bearer fresh") MockResponse().setBody(user)
                    else MockResponse().setResponseCode(401).setBody("""{"error":{"code":"unauthorized","message":"expired"}}""")
                "/api/auth/refresh" -> {
                    refreshes.incrementAndGet()
                    Thread.sleep(50)
                    MockResponse().setBody("""{"accessToken":"fresh","refreshToken":"r2","expiresIn":1800,"user":$user}""")
                }
                "/api/auth/login" -> MockResponse().setResponseCode(401)
                    .setBody("""{"error":{"code":"two_factor_required","message":"Enter the code"}}""")
                else -> MockResponse().setResponseCode(404)
            }
        }
        server.start()
    }

    @After fun tearDown() = server.shutdown()

    private fun client(): ApiClient {
        val tokens = TokenManager(MemoryTokenStore())
        tokens.save(studio.justtype.core.api.ApiJson.decodeFromString(
            """{"accessToken":"stale","refreshToken":"r1","expiresIn":1,"user":$user}""",
        ))
        return ApiClient(server.url("/api/").toString(), tokens)
    }

    @Test fun anExpiredSessionRefreshesOnceForABurst() = runBlocking {
        val client = client()
        val users = (1..5).map { async { client.me() } }.awaitAll()
        assertTrue(users.all { it.displayName == "Ada" })
        assertEquals(1, refreshes.get())
        assertEquals("r2", client.tokens.refreshToken)
    }

    /** A 401 while signing in is the server asking for a code, not an expired session. */
    @Test fun signInPassesTheServersAnswerThrough() = runBlocking {
        try {
            client().signIn("a@b.c", "password1")
            fail("expected two_factor_required")
        } catch (e: ApiError.Http) {
            assertEquals("two_factor_required", e.code)
        }
        assertEquals(0, refreshes.get())
    }

    /** No signal while the access token is stale: an error, but still signed in. */
    @Test fun aFailedRefreshWithNoNetworkKeepsTheSession() = runBlocking {
        val client = client()
        server.shutdown()
        try {
            client.me()
            fail("expected a transport error")
        } catch (e: ApiError.Transport) {
            // expected
        }
        assertEquals("r1", client.tokens.refreshToken)
        assertTrue(client.tokens.isSignedIn)
    }

    @Test fun aRefusedRefreshSignsOut() = runBlocking {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest) =
                MockResponse().setResponseCode(401).setBody("""{"error":{"code":"unauthorized","message":"no"}}""")
        }
        val client = client()
        try {
            client.me()
            fail("expected to be signed out")
        } catch (e: ApiError.NotAuthenticated) {
            // expected
        }
        assertEquals(null, client.tokens.refreshToken)
    }

    @Test fun queriesStayQueries() {
        val client = ApiClient("https://example.test/api", TokenManager(MemoryTokenStore()))
        assertEquals("https://example.test/api/me/writing?today=2026-10-09", client.url("me/writing?today=2026-10-09"))
    }
}

package studio.justtype.core.api

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.encodeToJsonElement
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.net.URLEncoder

/** JSON as the server speaks it: unknown fields ignored, absent ones left out of requests. */
val ApiJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = false
}

/**
 * The server's `/api`, one call per endpoint the phone uses. Mirrors iOS's `APIClient`: the rules
 * are the server's; this only asks.
 *
 * [baseUrl] ends at `/api` (`https://www.justtype.studio/api`).
 */
class ApiClient(
    baseUrl: String,
    val tokens: TokenManager,
    private val http: OkHttpClient = OkHttpClient(),
) {
    private val base = baseUrl.trimEnd('/')

    @PublishedApi internal val json = ApiJson

    // region Transport

    /** `path` may carry a query; it is appended as it is. */
    internal fun url(path: String) = "$base/${path.trimStart('/')}"

    /**
     * The response body as text. A 401 refreshes the session once and goes again — except while
     * signing in, where a 401 is a wrong password or a request for a code, not an expired session.
     */
    @PublishedApi
    internal suspend fun raw(method: String, path: String, body: String?, retryOn401: Boolean = true): String {
        val sentWith = tokens.accessToken
        val (status, text) = exchange(method, path, body, sentWith)

        if (status == 401 && retryOn401) {
            val refreshed = tokens.refresh(sentWith) { token -> refreshSession(token) }
            if (refreshed) return raw(method, path, body, retryOn401 = false)
            throw ApiError.NotAuthenticated()
        }
        if (status !in 200..299) {
            val envelope = runCatching { json.decodeFromString(API.ErrorEnvelope.serializer(), text) }.getOrNull()
            if (status == 409 && envelope != null) throw ApiError.Conflict(envelope.error.message)
            if (envelope != null) throw ApiError.Http(status, envelope.error.code, envelope.error.message)
            throw ApiError.Http(status, "unknown", "Request failed")
        }
        return text
    }

    private suspend fun exchange(method: String, path: String, body: String?, token: String?): Pair<Int, String> =
        withContext(Dispatchers.IO) {
            val builder = Request.Builder().url(url(path))
            val payload = body?.toRequestBody(JSON_TYPE)
                ?: if (method == "GET" || method == "DELETE") null else "".toRequestBody(JSON_TYPE)
            builder.method(method, payload)
            if (token != null) builder.header("Authorization", "Bearer $token")
            try {
                http.newCall(builder.build()).execute().use { response ->
                    response.code to (response.body?.string() ?: "")
                }
            } catch (e: IOException) {
                throw ApiError.Transport(e.message ?: "no connection")
            } catch (e: IllegalArgumentException) {
                throw ApiError.Transport(e.message ?: "bad address")
            }
        }

    @PublishedApi
    internal suspend inline fun <reified T> send(
        method: String,
        path: String,
        body: JsonObject? = null,
        retryOn401: Boolean = true,
    ): T {
        val text = raw(method, path, body?.toString(), retryOn401)
        if (T::class == Unit::class) return Unit as T
        return try {
            json.decodeFromString<T>(text)
        } catch (e: SerializationException) {
            throw ApiError.Decoding(e.message ?: "bad JSON")
        } catch (e: IllegalArgumentException) {
            throw ApiError.Decoding(e.message ?: "bad JSON")
        }
    }

    @PublishedApi
    internal inline fun <reified T> JsonObjectBuilder.field(name: String, value: T?) {
        if (value != null) put(name, json.encodeToJsonElement(value))
    }

    /**
     * A new pair, or null when the server refuses the refresh token — the one answer that signs the
     * writer out. No answer at all (a tunnel, a dead server) throws [ApiError.Transport] instead and
     * keeps the tokens: losing signal must never cost someone their session.
     */
    private suspend fun refreshSession(refreshToken: String): API.SessionResponse? {
        val body = buildJsonObject { put("refreshToken", JsonPrimitive(refreshToken)) }
        val (status, text) = exchange("POST", "auth/refresh", body.toString(), null)
        if (status == 400 || status == 401 || status == 403) return null
        if (status !in 200..299) throw ApiError.Http(status, "refresh_failed", "Could not renew the session")
        return try {
            json.decodeFromString(API.SessionResponse.serializer(), text)
        } catch (e: SerializationException) {
            throw ApiError.Decoding(e.message ?: "bad JSON")
        }
    }

    // endregion

    // region Auth

    /** `acceptTerms` is the person agreeing to [terms]; the server refuses an account without it. */
    suspend fun register(email: String, password: String, displayName: String, acceptTerms: Boolean): API.SessionResponse {
        val session: API.SessionResponse = send("POST", "auth/register", buildJsonObject {
            field("email", email); field("password", password)
            field("displayName", displayName); field("acceptTerms", acceptTerms)
        }, retryOn401 = false)
        tokens.save(session)
        return session
    }

    /** `code` only once the server has asked for one with `two_factor_required`. */
    suspend fun signIn(email: String, password: String, code: String? = null): API.SessionResponse {
        val session: API.SessionResponse = send("POST", "auth/login", buildJsonObject {
            field("email", email); field("password", password); field("code", code)
        }, retryOn401 = false)
        tokens.save(session)
        return session
    }

    suspend fun terms(): API.Terms = send("GET", "terms")

    suspend fun signOut() {
        tokens.refreshToken?.let { token ->
            runCatching {
                send<Unit>("POST", "auth/logout", buildJsonObject { field("refreshToken", token) }, retryOn401 = false)
            }
        }
        tokens.clear()
    }

    suspend fun me(): API.User = send("GET", "auth/me")

    // endregion

    // region Writing

    /** `today` is the writer's local date: only the client knows which day it is where they are. */
    suspend fun writing(today: String): API.WritingSummary = send("GET", "me/writing?today=${q(today)}")

    suspend fun templates(): List<API.TemplateView> = send("GET", "templates")

    suspend fun works(): List<API.Work> = send("GET", "works")

    suspend fun createWork(title: String, kind: API.WorkKind, templateId: String?): API.OutlineResponse =
        send("POST", "works", buildJsonObject {
            field("title", title); field("kind", kind); field("templateId", templateId)
        })

    suspend fun outline(workId: String): API.OutlineResponse = send("GET", "works/$workId/outline")

    /** What an export says about the book. Read-only here; edited on the web. */
    suspend fun metadata(workId: String): API.MetadataResponse = send("GET", "works/$workId/metadata")

    suspend fun scene(id: String): API.SceneWithContent = send("GET", "scenes/$id")

    suspend fun createScene(chapterId: String, title: String): API.Scene =
        send("POST", "chapters/$chapterId/scenes", buildJsonObject { field("title", title) })

    suspend fun createChapter(workId: String, title: String): API.Chapter =
        send("POST", "works/$workId/chapters", buildJsonObject { field("title", title) })

    /**
     * `baseHash` is the `contentHash` this edit was made on top of. A mismatch comes back as
     * [ApiError.Conflict], which the caller resolves by reloading — never by retrying blindly.
     */
    suspend fun saveScene(id: String, content: API.DocNode, createRevision: Boolean = true, baseHash: String? = null): API.SaveResult =
        send("PUT", "scenes/$id/content", buildJsonObject {
            field("content", content); field("createRevision", createRevision); field("baseHash", baseHash)
        })

    suspend fun updateScene(id: String, title: String? = null, status: String? = null): API.Scene =
        send("PATCH", "scenes/$id", buildJsonObject { field("title", title); field("status", status) })

    // endregion

    // region Versions

    suspend fun versions(workId: String): List<API.Version> = send("GET", "works/$workId/versions")

    suspend fun snapshot(workId: String, label: String?): API.VersionDetail =
        send("POST", "works/$workId/versions", buildJsonObject { field("label", label) })

    suspend fun publish(versionId: String): API.Version = send("POST", "versions/$versionId/publish", JsonObject(emptyMap()))

    suspend fun finalize(versionId: String): API.Version = send("POST", "versions/$versionId/finalize", JsonObject(emptyMap()))

    // endregion

    // region Publishing and review

    suspend fun offerForReview(versionId: String, visibility: API.Visibility, reviewPrompt: String?): API.PublicationCard =
        send("POST", "versions/$versionId/publications", buildJsonObject {
            field("visibility", visibility); field("scope", "work"); field("reviewPrompt", reviewPrompt)
        })

    suspend fun feed(mine: Boolean = false): List<API.PublicationCard> =
        send("GET", if (mine) "publications?mine=true" else "publications")

    suspend fun publication(id: String): API.PublicationCard = send("GET", "publications/$id")

    suspend fun publicationContent(id: String): API.VersionContent = send("GET", "publications/$id/content")

    suspend fun comments(publicationId: String, sceneId: String? = null): List<API.CommentView> =
        send("GET", "publications/$publicationId/comments" + (sceneId?.let { "?sceneId=${q(it)}" } ?: ""))

    suspend fun addComment(
        publicationId: String,
        body: String,
        sceneId: String? = null,
        parentId: String? = null,
        anchor: API.Anchor? = null,
        anchorQuote: String? = null,
        category: API.CommentCategory = API.CommentCategory.general,
    ): API.CommentView = send("POST", "publications/$publicationId/comments", buildJsonObject {
        field("body", body); field("sceneId", sceneId); field("parentId", parentId)
        field("anchor", anchor); field("anchorQuote", anchorQuote); field("category", category)
    })

    /** How far a reader has got: the signal authors cannot get any other way. */
    suspend fun recordProgress(publicationId: String, sceneId: String): API.ReaderProgressView =
        send("PUT", "publications/$publicationId/progress", buildJsonObject { field("sceneId", sceneId) })

    suspend fun vote(commentId: String, value: Int): API.VoteResponse =
        send("POST", "comments/$commentId/vote", buildJsonObject { field("value", value) })

    suspend fun resolve(commentId: String): API.Comment =
        send("PATCH", "comments/$commentId", buildJsonObject { field("state", "resolved") })

    // endregion

    // region Daily Prompts

    suspend fun todayPrompt(): API.TodayResponse = send("GET", "prompts/today")

    suspend fun prompt(day: String): API.PromptView = send("GET", "prompts/$day")

    suspend fun promptArchive(before: String? = null, limit: Int = 30): API.ArchivePage =
        send("GET", "prompts?limit=$limit" + (before?.let { "&before=${q(it)}" } ?: ""))

    suspend fun enrollment(): API.EnrollmentView = send("GET", "prompts/enrollment")

    suspend fun joinPrompts(timezone: String): API.EnrollmentView =
        send("POST", "prompts/enrollment", buildJsonObject { field("timezone", timezone) })

    suspend fun updateEnrollment(emailEnabled: Boolean? = null, localTime: String? = null, timezone: String? = null): API.EnrollmentView =
        send("PATCH", "prompts/enrollment", buildJsonObject {
            field("emailEnabled", emailEnabled); field("emailLocalTime", localTime); field("emailTimezone", timezone)
        })

    suspend fun leavePrompts(): API.EnrollmentView = send("DELETE", "prompts/enrollment")

    /** The writer's own answer to `day`, or null when they have not started one. */
    suspend fun entry(day: String): API.PromptEntryView? = try {
        send<API.PromptEntryView>("GET", "prompts/$day/entry")
    } catch (e: ApiError.Http) {
        if (e.status == 404) null else throw e
    }

    suspend fun saveEntry(day: String, content: API.DocNode, baseHash: String?, localDay: String): API.PromptEntryView =
        send("PUT", "prompts/$day/entry", buildJsonObject {
            field("content", content); field("baseHash", baseHash); field("localDay", localDay)
        })

    suspend fun submitEntry(day: String): API.PromptEntryView = send("POST", "prompts/$day/entry/submit")

    suspend fun withdrawEntry(day: String): API.PromptEntryView = send("POST", "prompts/$day/entry/withdraw")

    suspend fun deleteEntry(day: String) = send<Unit>("DELETE", "prompts/$day/entry")

    suspend fun entries(day: String, fewestCritiques: Boolean = false, cursor: String? = null): API.EntryPage =
        send("GET", "prompts/$day/entries?sort=${if (fewestCritiques) "fewest_critiques" else "newest"}" +
            (cursor?.let { "&cursor=${q(it)}" } ?: ""))

    suspend fun promptEntry(id: String): API.PromptEntryView = send("GET", "prompt-entries/$id")

    /** What turning the answer into a project would make. The split is the server's. */
    suspend fun previewWork(entryId: String): API.ToWorkPreview = send("POST", "prompt-entries/$entryId/to-work/preview")

    suspend fun makeWork(entryId: String, title: String): API.OutlineResponse =
        send("POST", "prompt-entries/$entryId/to-work", buildJsonObject { field("title", title) })

    suspend fun critiques(entryId: String): List<API.CritiqueView> = send("GET", "prompt-entries/$entryId/critiques")

    suspend fun critique(entryId: String, body: String, parentId: String? = null): API.CritiqueView =
        send("POST", "prompt-entries/$entryId/critiques", buildJsonObject { field("body", body); field("parentId", parentId) })

    suspend fun editCritique(id: String, body: String): API.CritiqueView =
        send("PATCH", "prompt-critiques/$id", buildJsonObject { field("body", body) })

    suspend fun removeCritique(id: String) = send<Unit>("DELETE", "prompt-critiques/$id")

    suspend fun reportCritique(id: String, reason: API.ReportReason) =
        send<Unit>("POST", "prompt-critiques/$id/report", buildJsonObject { field("reason", reason) })

    // endregion

    // region Inbox

    suspend fun inbox(cursor: String? = null): API.InboxPage =
        send("GET", cursor?.let { "inbox?cursor=${q(it)}" } ?: "inbox")

    suspend fun unreadCount(): Long = send<API.UnreadCount>("GET", "inbox/unread-count").count

    suspend fun markRead(itemId: String): Long = send<API.UnreadCount>("POST", "inbox/$itemId/read").count

    suspend fun markDigestRead(day: String): Long = send<API.UnreadCount>("POST", "inbox/digests/$day/read").count

    suspend fun markAllRead(): Long = send<API.UnreadCount>("POST", "inbox/read-all").count

    // endregion

    // region Settings

    suspend fun updateSettings(doubleEnterSplitsScenes: Boolean): API.User =
        send("PATCH", "me/settings", buildJsonObject { field("doubleEnterSplitsScenes", doubleEnterSplitsScenes) })

    suspend fun updateSettings(theme: API.Theme): API.User =
        send("PATCH", "me/settings", buildJsonObject { field("theme", theme) })

    suspend fun summaryEmail(): API.SummaryEmailView = send("GET", "me/summary-email")

    suspend fun setSummaryEmail(often: API.SummaryEmail, timezone: String): API.SummaryEmailView =
        send("PATCH", "me/summary-email", json.encodeToJsonElement(API.UpdateSummaryEmailRequest(often, timezone)) as JsonObject)

    // endregion

    private companion object {
        val JSON_TYPE = "application/json".toMediaType()
        fun q(value: String): String = URLEncoder.encode(value, Charsets.UTF_8)
    }
}

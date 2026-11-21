package studio.justtype.core.model

import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson

// Behaviour on the generated types, kept apart from them as on iOS (`Model+Helpers.swift`), so
// regenerating never clobbers hand-written code.

val API.WorkKind.label: String
    get() = when (this) {
        API.WorkKind.novel -> "Novel"
        API.WorkKind.novella -> "Novella"
        API.WorkKind.short_story -> "Short story"
        API.WorkKind.serial -> "Serial"
    }

val API.Visibility.label: String
    get() = when (this) {
        API.Visibility.unlisted -> "Anyone with the link"
        API.Visibility.platform -> "Public"
        API.Visibility.invited -> "Named readers only"
        API.Visibility.`public` -> "Open to all"
    }

val API.Theme.label: String get() = raw.replaceFirstChar { it.uppercase() }

val API.SummaryEmail.label: String get() = raw.replaceFirstChar { it.uppercase() }

val API.SummaryEmail.hint: String
    get() = when (this) {
        API.SummaryEmail.none -> "No summary email."
        API.SummaryEmail.daily -> "Each morning, about the day before."
        API.SummaryEmail.weekly -> "Monday mornings, about the week before."
    }

/**
 * Whether the caller may change this chapter's text. The server decides; this keeps the phone
 * from offering a caret it would refuse. An outline without access is from before collaboration,
 * when every work had one person on it — its owner.
 */
fun API.OutlineResponse.canWrite(chapterId: String): Boolean {
    val access = access ?: return true
    return when (access.role) {
        API.CollaboratorRole.owner -> true
        API.CollaboratorRole.writer -> access.chapterIds?.contains(chapterId) ?: true
        API.CollaboratorRole.editor, API.CollaboratorRole.reader -> false
    }
}

/** The `{from,to}` pair, when the comment is anchored to a passage. */
val API.CommentView.anchorRange: API.Anchor?
    get() {
        val raw = anchor ?: return null
        return runCatching {
            val obj = ApiJson.parseToJsonElement(raw).jsonObject
            val from = obj["from"]?.jsonPrimitive?.intOrNull
            val to = obj["to"]?.jsonPrimitive?.intOrNull
            if (from != null && to != null) API.Anchor(from, to) else null
        }.getOrNull()
    }

/** The answer as the editor's document. */
val API.PromptEntryView.document: API.DocNode get() = DocNodes.from(content)

/** A scene's body without its content: what the outline holds. */
fun API.SceneWithContent.withoutContent() = API.Scene(
    chapterId = chapterId, contentHash = contentHash, createdAt = createdAt, excluded = excluded, id = id,
    location = location, position = position, pov = pov, status = status, storyDate = storyDate,
    summary = summary, targetWords = targetWords, title = title, updatedAt = updatedAt,
    wordCount = wordCount, workId = workId,
)

package studio.justtype.core.sync

import kotlinx.serialization.Serializable
import studio.justtype.core.api.ApiJson
import java.io.File

/**
 * Today's writing, small enough to hand to the home-screen widget. A widget cannot sign in, and it
 * has no business holding a refresh token to try; so the app writes what it knows to a file and
 * the widget's whole job is to draw it. The same shape as iOS's.
 */
@Serializable
data class WritingSnapshot(
    val words: Int = 0,
    val goal: Int? = null,
    val streak: Int = 0,
    val weekWords: Int? = null,
    val weekGoal: Int? = null,
    /** What the writer would carry on with, for "start writing". */
    val sceneId: String? = null,
    val sceneTitle: String? = null,
    val workId: String? = null,
    val updatedAt: Long = System.currentTimeMillis(),
) {
    val fraction: Float get() = if (goal == null || goal <= 0) 0f else minOf(1f, words.toFloat() / goal)
    val weekFraction: Float get() = if (weekGoal == null || weekGoal <= 0) 0f else minOf(1f, maxOf(0, weekWords ?: 0).toFloat() / weekGoal)

    companion object {
        fun load(file: File): WritingSnapshot? =
            runCatching { ApiJson.decodeFromString(serializer(), file.readText()) }.getOrNull()

        fun save(file: File, snapshot: WritingSnapshot) = file.writeAtomically(ApiJson.encodeToString(serializer(), snapshot))
    }
}

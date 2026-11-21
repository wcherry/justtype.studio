package studio.justtype.core.model

import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import kotlin.math.roundToInt

/** Where a Daily Prompts screen goes next. */
sealed interface PromptRoute {
    data class Day(val day: String) : PromptRoute
    data class Write(val day: String) : PromptRoute
    data class Entry(val id: String) : PromptRoute
    data object Archive : PromptRoute
}

/** Paths in the web app that have a screen here: the inbox's links and the daily email's. */
object PromptLink {
    /** `/prompts/<day>/write`, `/prompts/<day>/entries/<id>`, `/prompts/<day>`. */
    fun route(path: String): PromptRoute? {
        val clean = path.substringBefore('#')
        val parts = clean.split('/').filter { it.isNotEmpty() }
        if (parts.firstOrNull() != "prompts" || parts.size < 2) return null
        val day = parts[1]
        if (day.length != 10) return null
        return when {
            parts.size == 3 && parts[2] == "write" -> PromptRoute.Write(day)
            parts.size == 4 && parts[2] == "entries" -> PromptRoute.Entry(parts[3])
            parts.size == 2 -> PromptRoute.Day(day)
            else -> null
        }
    }
}

/** Days and times for showing. */
object PromptDays {
    /** "Friday 9 October" (with the year if asked), from `YYYY-MM-DD`. */
    fun name(day: String, year: Boolean = false, locale: Locale = Locale.getDefault()): String {
        val date = runCatching { LocalDate.parse(day) }.getOrNull() ?: return day
        return date.format(DateTimeFormatter.ofPattern(if (year) "EEEE d MMMM yyyy" else "EEEE d MMMM", locale))
    }

    /** An ISO timestamp as a short local time. */
    fun time(iso: String, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
        val instant = runCatching { OffsetDateTime.parse(iso).toInstant() }.getOrNull()
            ?: runCatching { Instant.parse(iso) }.getOrNull() ?: return ""
        return instant.atZone(zone).toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale))
    }

    fun stateLabel(state: String) = when (state) {
        "draft" -> "Draft"
        "submitted" -> "Submitted"
        "withdrawn" -> "Withdrawn"
        else -> "Not started"
    }

    /** The writer's own day, not UTC's: the server takes the date the client hands it. */
    fun today(zone: ZoneId = ZoneId.systemDefault()): String = LocalDate.now(zone).toString()

    /** The note when the email would come in the prompt's last six hours. */
    fun closingNote(opensAt: String, chosen: String): String? {
        fun minutes(s: String): Int {
            val p = s.split(':').mapNotNull { it.toIntOrNull() }
            return (p.firstOrNull() ?: 0) * 60 + (if (p.size > 1) p.last() else 0)
        }
        val left = (minutes(opensAt) - minutes(chosen) + 24 * 60) % (24 * 60)
        if (left <= 0 || left > 6 * 60) return null
        val hours = (left / 60.0 * 10).roundToInt() / 10.0
        val said = if (hours == Math.rint(hours)) hours.toInt().toString() else String.format(Locale.ROOT, "%.1f", hours)
        return "Today's prompt closes at $opensAt. You'll have $said hour${if (hours == 1.0) "" else "s"}."
    }

    fun parseTime(value: String): LocalTime =
        runCatching { LocalTime.parse(value) }.getOrDefault(LocalTime.of(7, 0))
}

/** Critiques and replies flattened in reading order, each with its shown depth. */
data class CritiqueRow(val critique: studio.justtype.core.api.API.CritiqueView, val depth: Int, val replyingTo: String?)

object Critiques {
    /**
     * On a phone threads indent three levels; deeper replies carry on at that depth and say whom
     * they answer, so a long thread stays readable on the smallest screen.
     */
    fun arrange(list: List<studio.justtype.core.api.API.CritiqueView>, maxIndent: Int = 3): List<CritiqueRow> {
        val children = list.groupBy { it.parentId ?: "" }
        val byId = list.associateBy { it.id }
        val rows = mutableListOf<CritiqueRow>()
        fun walk(parent: String, depth: Int) {
            for (c in children[parent].orEmpty()) {
                val name = c.parentId?.let { byId[it]?.author?.displayName } ?: "a removed critique"
                rows += CritiqueRow(c, minOf(depth, maxIndent), if (depth > maxIndent) name else null)
                walk(c.id, depth + 1)
            }
        }
        walk("", 0)
        return rows
    }
}

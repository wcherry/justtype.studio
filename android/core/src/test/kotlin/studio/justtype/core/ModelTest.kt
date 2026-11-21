package studio.justtype.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import studio.justtype.core.api.API
import studio.justtype.core.model.Critiques
import studio.justtype.core.model.Naming
import studio.justtype.core.model.PromptDays
import studio.justtype.core.model.PromptLink
import studio.justtype.core.model.PromptRoute

class ModelTest {
    @Test fun chaptersAreNumberedPastFrontMatterAndThroughParts() {
        val names = Naming.chapters(listOf("" to "front", "" to "part", "" to "chapter", "Named" to "chapter", "" to "chapter", "" to "part", "" to "back"))
        assertEquals(listOf("Front matter", "Part 1", "Chapter 1", "Named", "Chapter 3", "Part 2", "Back matter"), names)
    }

    @Test fun untitledScenesAreNamedForWhereTheyFall() {
        assertEquals("Scene 2", Naming.scene(" ", 2))
        assertEquals("Part page", Naming.scene("", 1, "part"))
        assertEquals("The Barn", Naming.scene("The Barn", 2))
    }

    @Test fun promptLinksOpenTheirScreens() {
        assertEquals(PromptRoute.Write("2026-10-09"), PromptLink.route("/prompts/2026-10-09/write"))
        assertEquals(PromptRoute.Entry("abc"), PromptLink.route("/prompts/2026-10-09/entries/abc#critiques"))
        assertEquals(PromptRoute.Day("2026-10-09"), PromptLink.route("/prompts/2026-10-09"))
        assertNull(PromptLink.route("/works/1"))
        assertNull(PromptLink.route("/prompts/today"))
    }

    @Test fun theClosingNoteWarnsOnlyInTheLastSixHours() {
        assertEquals("Today's prompt closes at 06:00. You'll have 2 hours.", PromptDays.closingNote("06:00", "04:00"))
        assertEquals("Today's prompt closes at 06:00. You'll have 1 hour.", PromptDays.closingNote("06:00", "05:00"))
        assertEquals("Today's prompt closes at 06:00. You'll have 1.5 hours.", PromptDays.closingNote("06:00", "04:30"))
        assertNull(PromptDays.closingNote("06:00", "07:00"))
        assertNull(PromptDays.closingNote("06:00", "06:00"))
    }

    @Test fun deepCritiqueThreadsStopIndentingAndSayWhomTheyAnswer() {
        fun c(id: String, parent: String?, name: String) = API.CritiqueView(
            author = API.AuthorCard(name, "u-$id"), body = "", canRemove = false, createdAt = "",
            entryId = "e", id = id, mine = false, parentId = parent, removed = false,
        )
        val rows = Critiques.arrange(listOf(c("1", null, "Ann"), c("2", "1", "Bo"), c("3", "2", "Cy"), c("4", "3", "Di"), c("5", "4", "Ed")))
        assertEquals(listOf(0, 1, 2, 3, 3), rows.map { it.depth })
        assertEquals(listOf(null, null, null, null, "Di"), rows.map { it.replyingTo })
    }
}

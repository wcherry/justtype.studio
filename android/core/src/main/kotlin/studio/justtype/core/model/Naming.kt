package studio.justtype.core.model

/**
 * A new chapter or scene is untitled. Wherever it is shown it is called by where it falls —
 * "Chapter 3", "Scene 2" — so the name follows it when it moves. The web app, the server and iOS
 * name them the same way.
 */
object Naming {
    fun isUntitled(title: String) = title.isBlank()

    fun chapter(title: String, number: Int) = if (isUntitled(title)) "Chapter $number" else title

    /**
     * Every chapter's display name, in order. Front and back matter are not numbered and do not take
     * a number from the chapters around them; parts are counted on their own and chapter numbers run
     * on through them.
     */
    fun chapters(chapters: List<Pair<String, String>>): List<String> {
        var numbered = 0
        var parts = 0
        return chapters.map { (title, role) ->
            when (role) {
                "part" -> { parts += 1; if (isUntitled(title)) "Part $parts" else title }
                "front" -> if (isUntitled(title)) "Front matter" else title
                "back" -> if (isUntitled(title)) "Back matter" else title
                else -> { numbered += 1; chapter(title, numbered) }
            }
        }
    }

    fun roleBadge(role: String): String? = when (role) {
        "front" -> "Front matter"
        "back" -> "Back matter"
        "part" -> "Part"
        else -> null
    }

    /** `title`, or "Scene n" (1-based, within its chapter); in a part, its "Part page". */
    fun scene(title: String, number: Int, role: String = "chapter"): String = when {
        !isUntitled(title) -> title
        role == "part" -> if (number == 1) "Part page" else "Part page $number"
        else -> "Scene $number"
    }
}

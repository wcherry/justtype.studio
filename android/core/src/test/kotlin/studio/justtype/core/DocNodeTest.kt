package studio.justtype.core

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import org.junit.Assert.assertEquals
import org.junit.Test
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson
import studio.justtype.core.model.DocNodes
import studio.justtype.core.model.blocks
import studio.justtype.core.model.plainParagraphs
import studio.justtype.core.model.plainText
import studio.justtype.core.model.wordCount

/** The one piece of shared arithmetic that has to match the server and the web editor exactly. */
class DocNodeTest {
    private fun doc(json: String) = ApiJson.decodeFromString(API.DocNode.serializer(), json)

    @Test fun plainTextFlattensBlocksOntoLines() {
        val node = doc("""{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"One."}]},
          {"type":"paragraph","content":[{"type":"text","text":"Two."}]}]}""")
        assertEquals("One.\nTwo.", node.plainText)
        assertEquals(2, node.wordCount)
    }

    @Test fun blockPositionsMatchProseMirror() {
        val blocks = doc("""{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Hello"}]},
          {"type":"paragraph","content":[{"type":"text","text":"World"}]}]}""").blocks()
        assertEquals(listOf(1 to 6, 8 to 13), blocks.map { it.from to it.to })
    }

    @Test fun leafNodesOccupyOnePosition() {
        val blocks = doc("""{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Hi"}]},
          {"type":"horizontalRule"},
          {"type":"paragraph","content":[{"type":"text","text":"Ho"}]}]}""").blocks()
        assertEquals(4, blocks[1].from)
        assertEquals(5, blocks[1].to)
        assertEquals(6, blocks[2].from)
    }

    @Test fun textIsMeasuredInUtf16LikeTheWebEditor() {
        assertEquals(4, doc("""{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a🙂"}]}]}""").blocks()[0].to)
    }

    @Test fun plainTextRoundTripPreservesParagraphs() {
        val node = DocNodes.fromPlainText("First para.\n\nSecond para.")
        assertEquals(2, node.content?.size)
        assertEquals("First para.\nSecond para.", node.plainText)
    }

    @Test fun paragraphsForReadingDropBlanksAndShowRules() {
        val node = doc("""{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"A"}]},
          {"type":"paragraph"},
          {"type":"horizontalRule"},
          {"type":"paragraph","content":[{"type":"text","text":"B"},{"type":"hardBreak"},{"type":"text","text":"C"}]}]}""")
        assertEquals(listOf("A", "⁂", "B\nC"), node.plainParagraphs)
    }

    /**
     * The numbers iOS's `ProseMirrorContractTests` captured from `window.justtype.blocks()` in the
     * built editor — the same page this app ships. If Kotlin disagrees, a comment anchored on an
     * Android phone highlights the wrong words in a browser.
     */
    @Test fun kotlinAgreesWithTheRealEditor() {
        fun text(value: String, vararg marks: String) =
            API.DocNode(marks = marks.map { API.DocMark(type = it) }.ifEmpty { null }, text = value, type = "text")
        fun paragraph(vararg children: API.DocNode) = API.DocNode(content = children.toList(), type = "paragraph")
        val document = API.DocNode(type = "doc", content = listOf(
            API.DocNode(attrs = buildJsonObject { put("level", JsonPrimitive(2)) }, content = listOf(text("The Long Field")), type = "heading"),
            paragraph(text("The field went on "), text("past", "italic"), text(" where the light gave out — "), text("well", "bold", "italic"), text(" past. 🜂")),
            API.DocNode(content = listOf(paragraph(text("She counted fenceposts."))), type = "blockquote"),
            API.DocNode(type = "bulletList", content = listOf(
                API.DocNode(content = listOf(paragraph(text("First"))), type = "listItem"),
                API.DocNode(content = listOf(paragraph(text("Second"))), type = "listItem"),
            )),
            API.DocNode(type = "horizontalRule"),
            paragraph(text("The house was still a long way off.")),
        ))
        val fromProseMirror = listOf(
            Triple("heading", 1, 15), Triple("paragraph", 17, 80), Triple("blockquote", 82, 107),
            Triple("bulletList", 109, 128), Triple("horizontalRule", 129, 130), Triple("paragraph", 131, 166),
        )
        val blocks = document.blocks()
        assertEquals(fromProseMirror, blocks.map { Triple(it.type, it.from, it.to) })
        assertEquals(2, blocks[0].level)
        // The astral character costs two positions: one code point, two UTF-16 units.
        assertEquals(63, blocks[1].to - blocks[1].from)
        assertEquals(62, blocks[1].text.codePointCount(0, blocks[1].text.length))
    }

    /** iOS's fixture, decoded and re-encoded without losing a mark or an attribute. */
    @Test fun richSceneRoundTrips() {
        val raw = javaClass.classLoader.getResource("rich-scene.json")!!.readText()
        val node = doc(raw)
        val again = doc(ApiJson.encodeToString(API.DocNode.serializer(), node))
        assertEquals(node, again)
        assertEquals(ApiJson.parseToJsonElement(raw), ApiJson.encodeToJsonElement(API.DocNode.serializer(), node))
    }
}

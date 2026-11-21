package studio.justtype.core.model

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson

/**
 * Document arithmetic on the generated [API.DocNode] — the same as iOS's `DocNode.swift`, and the
 * one piece of shared arithmetic that has to agree exactly with the server and the web editor:
 * ProseMirror positions.
 */
object DocNodes {
    val empty = API.DocNode(content = listOf(API.DocNode(type = "paragraph")), type = "doc")

    /** Nodes that hold no content of their own and occupy exactly one position. */
    val leafTypes = setOf("horizontalRule", "hardBreak", "image")

    internal val blockTypes = setOf("paragraph", "heading", "blockquote", "listItem", "codeBlock", "horizontalRule")

    fun fromPlainText(text: String): API.DocNode {
        val paragraphs = text.split("\n\n").map { paragraph ->
            if (paragraph.isBlank()) API.DocNode(type = "paragraph")
            else API.DocNode(content = listOf(API.DocNode(text = paragraph, type = "text")), type = "paragraph")
        }
        return API.DocNode(content = paragraphs.ifEmpty { listOf(API.DocNode(type = "paragraph")) }, type = "doc")
    }

    /** A document carried as untyped JSON (a prompt answer's `content`), as the editor's document. */
    fun from(element: JsonElement): API.DocNode =
        runCatching { ApiJson.decodeFromJsonElement(API.DocNode.serializer(), element) }.getOrDefault(empty)
}

val API.DocNode.isLeaf: Boolean get() = type in DocNodes.leafTypes

/** ProseMirror node size. Text counts UTF-16 code units — what a JVM `String.length` already is. */
val API.DocNode.nodeSize: Int
    get() = when {
        type == "text" -> text?.length ?: 0
        isLeaf -> 1
        else -> contentSize + 2
    }

val API.DocNode.contentSize: Int get() = content.orEmpty().sumOf { it.nodeSize }

/** Flattened text, one line per block, matching the server's `domain::doc::plain_text`. */
val API.DocNode.plainText: String
    get() = buildString { appendPlainText(this@plainText, this) }.trim()

private fun appendPlainText(node: API.DocNode, out: StringBuilder) {
    when (node.type) {
        "text" -> { out.append(node.text.orEmpty()); return }
        "hardBreak" -> { out.append('\n'); return }
    }
    node.content.orEmpty().forEach { appendPlainText(it, out) }
    if (node.type in DocNodes.blockTypes && !out.endsWith('\n')) out.append('\n')
}

val API.DocNode.wordCount: Int get() = plainText.split(Regex("\\s+")).count { it.isNotEmpty() }

/** Each top-level block's text, in order, blank ones dropped; a rule is shown as ⁂. */
val API.DocNode.plainParagraphs: List<String>
    get() {
        fun text(node: API.DocNode): String = when {
            node.type == "hardBreak" -> "\n"
            node.text != null -> node.text!!
            else -> node.content.orEmpty().joinToString("") { text(it) }
        }
        return content.orEmpty()
            .map { if (it.type == "horizontalRule") "⁂" else text(it) }
            .filter { it.isNotBlank() }
    }

/** One top-level block, with the ProseMirror positions it spans (`from` inclusive, `to` exclusive). */
data class DocBlock(val id: Int, val type: String, val level: Int, val text: String, val from: Int, val to: Int) {
    val isEmpty: Boolean get() = text.isBlank()
}

/** Walks the top level of a document and records where each block sits. */
fun API.DocNode.blocks(): List<DocBlock> {
    var pos = 0
    return content.orEmpty().mapIndexed { index, child ->
        val start = if (child.isLeaf) pos else pos + 1
        val end = if (child.isLeaf) pos + 1 else start + child.contentSize
        val level = ((child.attrs as? JsonObject)?.get("level"))?.jsonPrimitive?.intOrNull ?: 0
        pos += child.nodeSize
        DocBlock(index, child.type, level, child.plainText, start, end)
    }
}

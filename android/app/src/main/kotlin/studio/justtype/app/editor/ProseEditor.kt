package studio.justtype.app.editor

import android.annotation.SuppressLint
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.webkit.JavascriptInterface
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewAssetLoader
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import studio.justtype.app.Typography
import studio.justtype.app.ui.LocalProseColors
import studio.justtype.app.ui.ProseColors
import studio.justtype.core.api.API
import studio.justtype.core.api.ApiJson

/** What the reader or writer has their finger on: positions, the words between them, their marks. */
data class EditorSelection(val from: Int, val to: Int, val quote: String, val marks: Set<String>) {
    val isEmpty: Boolean get() = from == to
}

/** Commands sent down to the page: formatting from the toolbar, and page turns. */
class EditorCommands {
    internal var webView: WebView? = null

    /** `bold`, `italic`, `underline`, `strike`, `h2`, `quote`, `bullet`, `undo`, `redo`. */
    fun send(name: String) {
        if (name.all { it.isLetterOrDigit() }) webView?.evaluateJavascript("window.justtype.toggle('$name')", null)
    }

    fun turn(by: Int) {
        webView?.evaluateJavascript("window.justtype.turn($by)", null)
    }
}

/** Called by the page, on a binder thread; hands each message to the main thread. */
class EditorBridge(private val receive: (JsonObject) -> Unit) {
    private val main = Handler(Looper.getMainLooper())

    @JavascriptInterface
    fun post(message: String) {
        val parsed = runCatching { ApiJson.parseToJsonElement(message).jsonObject }.getOrNull() ?: return
        main.post { receive(parsed) }
    }
}

private class EditorState {
    var loaded = false
    var pushed: API.DocNode? = null
    var editable: Boolean? = null
    var typography: Typography? = null
    var colors: ProseColors? = null
}

/**
 * The writing surface: the same TipTap the browser and the iOS app run, in a web view.
 *
 * The document the page hands back *is* a ProseMirror document, and the positions it reports are
 * the ones the server and the browser use — so a comment anchored on an Android phone highlights
 * the same words in a browser. The page ships in the app (`assets/editor`, copied from iOS's
 * bundle at build time) and is served from a local origin, never the network: a writer on a plane
 * has to be able to open a scene.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun ProseEditor(
    document: API.DocNode,
    modifier: Modifier = Modifier,
    editable: Boolean = true,
    typography: Typography? = null,
    commands: EditorCommands? = null,
    onChange: (API.DocNode, Int) -> Unit = { _, _ -> },
    onSelection: (EditorSelection) -> Unit = {},
    onPages: (Int, Int) -> Unit = { _, _ -> },
    onProblem: (String) -> Unit = {},
) {
    val state = remember { EditorState() }
    val colors = LocalProseColors.current
    val currentDocument by rememberUpdatedState(document)
    val currentEditable by rememberUpdatedState(editable)
    val currentTypography by rememberUpdatedState(typography)
    val currentColors by rememberUpdatedState(colors)
    val change by rememberUpdatedState(onChange)
    val select by rememberUpdatedState(onSelection)
    val pages by rememberUpdatedState(onPages)
    val problem by rememberUpdatedState(onProblem)

    fun WebView.push(doc: API.DocNode) {
        state.pushed = doc
        evaluateJavascript("window.justtype.load(${ApiJson.encodeToString(API.DocNode.serializer(), doc)})", null)
    }

    fun WebView.applySettings() {
        if (!state.loaded) return
        if (state.pushed != currentDocument) push(currentDocument)
        if (state.editable != currentEditable) {
            state.editable = currentEditable
            evaluateJavascript("window.justtype.setEditable($currentEditable)", null)
        }
        currentTypography?.let { t ->
            if (state.typography != t) {
                state.typography = t
                val family = kotlinx.serialization.json.JsonPrimitive(t.families).toString()
                evaluateJavascript(
                    "window.justtype.setTypography({size: ${t.size}, family: $family, leading: ${t.leading}, paged: ${t.paged}})",
                    null,
                )
            }
        }
        if (state.colors != currentColors) {
            state.colors = currentColors
            val ink = String.format("#%06X", currentColors.ink.toArgb() and 0xFFFFFF)
            val scheme = if (currentColors.dark) "dark" else "light"
            evaluateJavascript(
                "document.documentElement.style.colorScheme='$scheme';" +
                    "document.body.style.background='transparent';document.body.style.color='$ink';",
                null,
            )
        }
    }

    AndroidView(
        modifier = modifier,
        factory = { context ->
            val loader = WebViewAssetLoader.Builder()
                .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(context))
                .build()
            WebView(context).apply {
                setBackgroundColor(Color.TRANSPARENT)
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = false
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                isVerticalScrollBarEnabled = true
                commands?.webView = this
                val view = this
                addJavascriptInterface(
                    EditorBridge { message ->
                        when (message["kind"]?.jsonPrimitive?.contentOrNull) {
                            "ready" -> {
                                state.loaded = true
                                state.pushed = null
                                // The page starts editable. Saying so again is not free: TipTap's
                                // setEditable fires an update, which would read as an edit and save.
                                state.editable = true
                                view.applySettings()
                            }
                            "change" -> {
                                val doc = message["doc"]?.let {
                                    runCatching { ApiJson.decodeFromJsonElement(API.DocNode.serializer(), it) }.getOrNull()
                                } ?: return@EditorBridge
                                // Remember what the page holds, so it is not handed the writer's own edit back.
                                state.pushed = doc
                                change(doc, message["words"]?.jsonPrimitive?.intOrNull ?: 0)
                            }
                            "selection" -> select(
                                EditorSelection(
                                    from = message["from"]?.jsonPrimitive?.intOrNull ?: 0,
                                    to = message["to"]?.jsonPrimitive?.intOrNull ?: 0,
                                    quote = message["quote"]?.jsonPrimitive?.contentOrNull ?: "",
                                    marks = message["marks"]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }?.toSet().orEmpty(),
                                ),
                            )
                            "pages" -> pages(
                                message["page"]?.jsonPrimitive?.intOrNull ?: 1,
                                message["pages"]?.jsonPrimitive?.intOrNull ?: 1,
                            )
                        }
                    },
                    "JustTypeAndroid",
                )
                webViewClient = object : WebViewClient() {
                    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                        loader.shouldInterceptRequest(request.url)

                    // The page is the whole of what this view shows; a link in a scene opens nowhere.
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true

                    override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                        if (request.isForMainFrame) problem("the editor did not load: ${error.description}")
                    }

                    /** Up, but never said `ready`: the script threw. Say so rather than show a blank page. */
                    override fun onPageFinished(view: WebView, url: String?) {
                        view.postDelayed({
                            if (state.loaded) return@postDelayed
                            view.evaluateJavascript(
                                "JSON.stringify({error: window.__error || null, api: typeof window.justtype})",
                            ) { problem("the editor did not start: $it") }
                        }, 3000)
                    }
                }
                loadUrl("https://${WebViewAssetLoader.DEFAULT_DOMAIN}/assets/editor/index.html")
            }
        },
        update = { it.applySettings() },
        onRelease = {
            commands?.webView = null
            it.destroy()
        },
    )
}

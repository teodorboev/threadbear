package com.mikerosoft.threadbear.android.ui

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Color as AndroidColor
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.mikerosoft.threadbear.kit.MarkdownBlock
import java.io.ByteArrayInputStream
import kotlinx.serialization.json.JsonPrimitive

/**
 * A ```mermaid or ```html block in a reply, drawn as what it shows: a box
 * like a code block, with the language on the left, a Diagram / Source
 * switch (Preview / Source for a page) and a button that opens it full
 * screen, as the iPhone and the Mac draw it (Mike picked it, 6 Nov).
 *
 * Not native Compose: a diagram needs Mermaid's layout, which is Pele's
 * job (bundled, as on the iPhone), and a page is HTML. So both are drawn
 * in a web view with no way out (RichSandbox): no network, nothing kept,
 * nowhere to navigate. A diagram runs only the app's own script, with the
 * source handed over as a string; a page's own scripts never run.
 */
@Composable
fun RichBlockView(kind: MarkdownBlock.RichKind, source: String) {
    val colors = MaterialTheme.colorScheme
    var showSource by remember { mutableStateOf(false) }
    var expanded by remember { mutableStateOf(false) }
    // A page measures itself from small up, as a web view never reports its
    // content shorter than the view it's in; a diagram starts at a guess.
    var height by remember { mutableFloatStateOf(if (kind == MarkdownBlock.RichKind.MERMAID) 160f else 24f) }
    val shape = RoundedCornerShape(8.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerHigh).border(1.dp, colors.onSurface.copy(alpha = 0.12f), shape)) {
        Row(Modifier.fillMaxWidth().padding(start = 10.dp, end = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(kind.wire, style = MaterialTheme.typography.labelSmall.copy(fontFamily = Mono), color = colors.onSurfaceVariant)
            Spacer(Modifier.weight(1f))
            Switch2(if (kind == MarkdownBlock.RichKind.MERMAID) "Diagram" else "Preview", "Source", showSource) { showSource = it }
            IconButton(onClick = { expanded = true }, modifier = Modifier.size(36.dp)) {
                Icon(Line.Expand, if (kind == MarkdownBlock.RichKind.MERMAID) "Open the diagram full screen" else "Open the page full screen",
                    Modifier.size(15.dp), tint = colors.onSurfaceVariant)
            }
        }
        HorizontalDivider(color = colors.outlineVariant)
        if (showSource) {
            SelectionContainer {
                Box(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(10.dp)) {
                    Text(source, style = MaterialTheme.typography.bodySmall.copy(fontFamily = Mono, fontSize = 13.sp))
                }
            }
        } else {
            // Made only once the row is composed: the transcript is a lazy
            // list, so a long thread doesn't make one web view per block.
            Box(Modifier.fillMaxWidth().height(minOf(height, MAX_INLINE_HEIGHT).dp)) {
                RichWebView(kind, source, fullScreen = false) { height = it }
                // Over the web view, so a drag scrolls the transcript and never the
                // page, and a tap opens it full screen, where it can be zoomed.
                Box(Modifier.matchParentSize().pointerInput(Unit) { detectTapGestures { expanded = true } })
                if (height > MAX_INLINE_HEIGHT) {
                    // Says there's more below, which expanding shows.
                    Box(
                        Modifier.align(Alignment.BottomCenter).fillMaxWidth().height(36.dp)
                            .background(Brush.verticalGradient(listOf(Color.Transparent, colors.surfaceContainerHigh))),
                    )
                }
            }
        }
    }
    if (expanded) RichBlockSheet(kind, source) { expanded = false }
}

/** Taller than this, the inline view is cut off; expanding shows it all. */
private const val MAX_INLINE_HEIGHT = 420f

/** Two words, one picked: the iPhone's small segmented control. */
@Composable
private fun Switch2(first: String, second: String, onSecond: Boolean, pick: (Boolean) -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(Modifier.clip(RoundedCornerShape(7.dp)).background(colors.surfaceContainerHighest).padding(2.dp)) {
        for ((i, word) in listOf(first, second).withIndex()) {
            val on = (i == 1) == onSecond
            Text(
                word,
                style = MaterialTheme.typography.labelSmall,
                color = if (on) colors.onSurface else colors.onSurfaceVariant,
                modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(if (on) colors.surfaceContainerLow else Color.Transparent)
                    .clickable { pick(i == 1) }.padding(horizontal = 10.dp, vertical = 5.dp),
            )
        }
    }
}

/** The block full screen: pinch to zoom and scroll, with its source a tap away. */
@Composable
private fun RichBlockSheet(kind: MarkdownBlock.RichKind, source: String, close: () -> Unit) {
    var showSource by remember { mutableStateOf(false) }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background).statusBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = close) { Icon(Line.Close, "Close", Modifier.size(20.dp)) }
                Spacer(Modifier.weight(1f))
                Switch2(if (kind == MarkdownBlock.RichKind.MERMAID) "Diagram" else "Preview", "Source", showSource) { showSource = it }
                Spacer(Modifier.weight(1f))
                Spacer(Modifier.size(48.dp))
            }
            if (showSource) {
                SelectionContainer {
                    Box(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).horizontalScroll(rememberScrollState()).padding(16.dp)) {
                        Text(source, style = MaterialTheme.typography.bodySmall.copy(fontFamily = Mono, fontSize = 13.sp))
                    }
                }
            } else {
                Box(Modifier.fillMaxSize()) { RichWebView(kind, source, fullScreen = true) {} }
            }
        }
    }
}

/**
 * The web view under a rich block. Inline it doesn't scroll and reports its
 * content's height (a diagram is scaled to fit under the inline height; a
 * taller page is cut off there); full screen it scrolls and zooms.
 */
@SuppressLint("SetJavaScriptEnabled")
@Composable
private fun RichWebView(kind: MarkdownBlock.RichKind, source: String, fullScreen: Boolean, height: (Float) -> Unit) {
    val dark = androidx.compose.foundation.isSystemInDarkTheme()
    // A new view for a new kind, never one reused across kinds: a diagram's
    // view has scripts on and the diagram client, and a page must have
    // neither (Codex review, 11 Nov: a list row going from mermaid to html
    // kept them).
    key(kind) {
    AndroidView(
        // Scripts are set when it's made, and checked again on every update.
        factory = { context -> RichSandbox.webView(context, kind, fullScreen, height) },
        update = { view ->
            view.settings.javaScriptEnabled = kind == MarkdownBlock.RichKind.MERMAID
            // The same block with new source (a message edited): drawn again.
            val key = "$dark\u0000$source"
            if (view.tag != key) {
                view.tag = key
                RichSandbox.load(view, kind, source, fullScreen, dark)
            }
        },
        onRelease = { view ->
            view.stopLoading()
            view.destroy()
        },
        modifier = Modifier.fillMaxSize(),
    )
    }
}

/**
 * How rich blocks' web views are locked down, as the iPhone's are
 * (RichWebSandbox): every load but the page itself and data: pictures in it
 * answered with nothing (Android's web view has no per-view proxy, so the
 * request interception and blockNetworkLoads stand in for the iPhone's
 * dead-end proxy and rule list), no navigation, no new windows, no files,
 * no storage, and a CSP of inline styles and data: pictures only.
 */
object RichSandbox {
    /** The source handed to the draw function, kept per view, never put in markup. */
    private val sources = java.util.WeakHashMap<WebView, String>()

    @SuppressLint("SetJavaScriptEnabled")
    fun webView(context: Context, kind: MarkdownBlock.RichKind, fullScreen: Boolean, height: (Float) -> Unit): WebView {
        val view = WebView(context)
        view.setBackgroundColor(AndroidColor.TRANSPARENT)
        view.isVerticalScrollBarEnabled = fullScreen
        view.isHorizontalScrollBarEnabled = fullScreen
        with(view.settings) {
            // A page's own scripts never run; a diagram's page runs only Pele and the draw function.
            javaScriptEnabled = kind == MarkdownBlock.RichKind.MERMAID
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
            blockNetworkLoads = true
            blockNetworkImage = true
            allowFileAccess = false
            allowContentAccess = false
            domStorageEnabled = false
            databaseEnabled = false
            cacheMode = WebSettings.LOAD_NO_CACHE
            mediaPlaybackRequiresUserGesture = true
            setGeolocationEnabled(false)
            builtInZoomControls = fullScreen
            displayZoomControls = false
            setSupportZoom(fullScreen)
            saveFormData = false
        }
        view.webViewClient = object : WebViewClient() {
            /** Only the page itself, once: no link, form or reload goes anywhere. */
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true

            /** Nothing loads but the page and data: pictures in it. */
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? {
                val scheme = request.url.scheme?.lowercase()
                if (scheme == "data" || scheme == "about") return null
                return WebResourceResponse("text/plain", "utf-8", 403, "Blocked", emptyMap(), ByteArrayInputStream(ByteArray(0)))
            }

            override fun onPageFinished(view: WebView, url: String?) {
                if (kind == MarkdownBlock.RichKind.MERMAID) {
                    val src = sources[view] ?: return
                    // Pele, then the draw function, then the source as a JSON string argument.
                    view.evaluateJavascript(pele(view.context)) {
                        view.evaluateJavascript(DRAW_SCRIPT) {
                            view.evaluateJavascript("threadbearDraw(${JsonPrimitive(src)})") { measure(view, height) }
                        }
                    }
                } else {
                    // A page's scripts are off, and stay off: its height is the
                    // web view's own measure of the content, not a script's.
                    val report = { if (view.contentHeight > 0) height(view.contentHeight.toFloat()) }
                    view.post(report)
                    // Pictures in a page (data: only) can land after it has finished.
                    view.postDelayed(report, 300)
                }
            }
        }
        view.webChromeClient = null
        return view
    }

    fun load(view: WebView, kind: MarkdownBlock.RichKind, source: String, fullScreen: Boolean, dark: Boolean) {
        sources[view] = source
        val page = if (kind == MarkdownBlock.RichKind.MERMAID) diagramPage(fullScreen, dark) else htmlPage(source, fullScreen)
        view.loadDataWithBaseURL(null, page, "text/html", "utf-8", null)
    }

    /**
     * How far down a diagram reaches, not scrollHeight, which is never less
     * than the view's own height and so never let a block get shorter than
     * its first guess. Only for a diagram, whose page runs the app's scripts.
     */
    private fun measure(view: WebView, height: (Float) -> Unit) {
        view.evaluateJavascript(CONTENT_HEIGHT) { value -> value?.toFloatOrNull()?.takeIf { it > 0 }?.let(height) }
    }

    private var peleSource: String? = null

    /** Pele, read once from the app's assets, for every diagram after. */
    private fun pele(context: Context): String =
        peleSource ?: context.assets.open("pele.js").bufferedReader().use { it.readText() }.also { peleSource = it }

    /** Nothing but inline styles and data: pictures, whatever the markup asks for. */
    private const val POLICY = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; form-action 'none'; base-uri 'none'"

    private fun head(fullScreen: Boolean): String {
        val zoom = if (fullScreen) "maximum-scale=5, user-scalable=yes" else "maximum-scale=1, user-scalable=no"
        return """
            <meta http-equiv="Content-Security-Policy" content="$POLICY">
            <meta name="viewport" content="width=device-width, initial-scale=1, $zoom">
        """.trimIndent()
    }

    /** A page from an ```html block: the agent's markup after the app's policy and viewport. */
    private fun htmlPage(source: String, fullScreen: Boolean): String =
        // The doctype first, so it isn't drawn in quirks mode whatever the agent wrote.
        "<!doctype html>\n" + head(fullScreen) + "<style>html,body{margin:0;padding:0;background:transparent;-webkit-text-size-adjust:100%}</style>\n" + source

    /** The page a diagram is drawn into, themed with Pele's CSS variables to the app's greys. */
    private fun diagramPage(fullScreen: Boolean, dark: Boolean): String {
        val vars = if (dark) {
            "--pele-bg:#1c1c1e;--pele-fg:#ececec;--pele-muted:#9a9a9a;--pele-line:#8e8e93;--pele-surface:#333335;--pele-surface-alt:#3a3a3c;--pele-border:#5a5a5e;--pele-accent:#0a84ff;--pele-radius:6px"
        } else {
            "--pele-bg:#f2f2f7;--pele-fg:#1d1d1f;--pele-muted:#6e6e73;--pele-line:#86868b;--pele-surface:#ffffff;--pele-surface-alt:#f0f0f2;--pele-border:#c7c7cc;--pele-accent:#007aff;--pele-radius:6px"
        }
        val cap = if (fullScreen) "" else ";max-height:${MAX_INLINE_HEIGHT.toInt() - 28}px"
        return """
            <!doctype html><html><head>${head(fullScreen)}
            <style>
            html,body{margin:0;padding:0;background:transparent;font:14px sans-serif;color:${if (dark) "#ececec" else "#1d1d1f"}}
            #d{padding:14px;display:flex;justify-content:center}
            #d svg{max-width:100%;height:auto$cap}
            #d.err{display:block;color:${if (dark) "#ff6961" else "#d0453a"};font:13px monospace;white-space:pre-wrap}
            .pele{$vars}
            </style></head><body><div id="d"></div></body></html>
        """.trimIndent()
    }

    /**
     * Draws a diagram's source with Pele: no links, no pictures, so the SVG
     * names nothing outside itself. A diagram Pele can't read shows its
     * message as text.
     */
    private val DRAW_SCRIPT = """
        window.threadbearDraw = function (source) {
          const box = document.getElementById('d');
          try {
            box.className = '';
            box.innerHTML = pele.render(String(source), { links: false, images: false, fontFamily: 'sans-serif', fontSize: 14, idPrefix: 'd' }).svg;
          } catch (error) {
            box.className = 'err';
            box.textContent = "This diagram couldn't be drawn: " + (error && error.message ? error.message : String(error));
          }
          return true;
        };
    """.trimIndent()

    private val CONTENT_HEIGHT = """
        (() => {
          const range = document.createRange();
          range.selectNodeContents(document.body);
          const style = getComputedStyle(document.body);
          return Math.ceil(range.getBoundingClientRect().bottom + window.scrollY + parseFloat(style.paddingBottom || 0) + parseFloat(style.marginBottom || 0));
        })()
    """.trimIndent()
}

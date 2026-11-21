package studio.justtype.app

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** How a reader likes the page, kept: nobody wants to set the type size twice. Per device, as on iOS. */
data class Typography(val size: Float = 17f, val family: String = "serif", val leading: Float = 1.6f, val paged: Boolean = false) {
    val families: String
        get() = when (family) {
            "sans" -> "Roboto, 'Noto Sans', sans-serif"
            "mono" -> "'Roboto Mono', 'Droid Sans Mono', monospace"
            else -> "'Noto Serif', Georgia, serif"
        }
}

class ReaderPrefs(context: Context) {
    private val prefs = context.getSharedPreferences("reader", Context.MODE_PRIVATE)
    private val state = MutableStateFlow(
        Typography(
            size = prefs.getFloat("size", 17f),
            family = prefs.getString("family", "serif") ?: "serif",
            leading = prefs.getFloat("leading", 1.6f),
            paged = prefs.getBoolean("paged", false),
        ),
    )
    val typography: StateFlow<Typography> = state.asStateFlow()

    fun set(next: Typography) {
        state.value = next
        prefs.edit().putFloat("size", next.size).putString("family", next.family)
            .putFloat("leading", next.leading).putBoolean("paged", next.paged).apply()
    }
}

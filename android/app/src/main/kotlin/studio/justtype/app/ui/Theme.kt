package studio.justtype.app.ui

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import studio.justtype.core.api.API

/** What the editor page needs of the theme: the page's ink, and whether it is a dark page. */
data class ProseColors(val ink: Color, val dark: Boolean)

val LocalProseColors = staticCompositionLocalOf { ProseColors(Color(0xFF1F1B16), false) }

private val Light = lightColorScheme(
    primary = Color(0xFF6E5030), onPrimary = Color.White,
    primaryContainer = Color(0xFFF1E3CF), onPrimaryContainer = Color(0xFF2A1A08),
    secondary = Color(0xFF6B5D4F), surface = Color(0xFFFCFBF8), background = Color(0xFFFCFBF8),
    // The selected tab's pill and the chips: Material's default here is lilac.
    secondaryContainer = Color(0xFFEADBC6), onSecondaryContainer = Color(0xFF2A1A08),
    surfaceContainerLow = Color(0xFFF7F3ED), surfaceContainerHighest = Color(0xFFE9E3DA),
    onSurface = Color(0xFF1F1B16), onBackground = Color(0xFF1F1B16),
    surfaceVariant = Color(0xFFEFE9E0), onSurfaceVariant = Color(0xFF5E564C),
    surfaceContainer = Color(0xFFF4F0EA), surfaceContainerHigh = Color(0xFFEFEAE2),
)

private val Dark = darkColorScheme(
    primary = Color(0xFFDDBF97), onPrimary = Color(0xFF3D2A12),
    primaryContainer = Color(0xFF55402A), onPrimaryContainer = Color(0xFFF6E3CB),
    secondary = Color(0xFFD3C4B4), surface = Color(0xFF16140F), background = Color(0xFF16140F),
    secondaryContainer = Color(0xFF4A3D2E), onSecondaryContainer = Color(0xFFF1E1CC),
    surfaceContainerLow = Color(0xFF1B1914), surfaceContainerHighest = Color(0xFF34302A),
    onSurface = Color(0xFFEAE3D9), onBackground = Color(0xFFEAE3D9),
    surfaceVariant = Color(0xFF2B2721), onSurfaceVariant = Color(0xFFC9BFB2),
    surfaceContainer = Color(0xFF201D18), surfaceContainerHigh = Color(0xFF2A2620),
)

/** The web's warmer page: cream paper, brown ink. */
private val Sepia = Light.copy(
    surface = Color(0xFFF4ECD8), background = Color(0xFFF4ECD8),
    onSurface = Color(0xFF3B2F22), onBackground = Color(0xFF3B2F22),
    surfaceVariant = Color(0xFFE8DCC2), surfaceContainer = Color(0xFFEDE3CC), surfaceContainerHigh = Color(0xFFE6DBC2),
    surfaceContainerLow = Color(0xFFF0E7D2), surfaceContainerHighest = Color(0xFFE0D4B9),
)

/** The web's dimmer page, for writing in the dark: near-black, low-contrast warm ink. */
private val Night = Dark.copy(
    surface = Color(0xFF000000), background = Color(0xFF000000),
    onSurface = Color(0xFFB8AE9F), onBackground = Color(0xFFB8AE9F),
    surfaceContainer = Color(0xFF0E0D0B), surfaceContainerHigh = Color(0xFF161410),
    surfaceContainerLow = Color(0xFF080706), surfaceContainerHighest = Color(0xFF1E1B17),
)

val Serif = FontFamily.Serif

@Composable
fun JustTypeTheme(theme: API.Theme?, content: @Composable () -> Unit) {
    val system = isSystemInDarkTheme()
    val (scheme, dark) = when (theme) {
        API.Theme.light -> Light to false
        API.Theme.dark -> Dark to true
        API.Theme.sepia -> Sepia to false
        API.Theme.night -> Night to true
        API.Theme.system, null -> if (system) Dark to true else Light to false
    }
    val base = Typography()
    val typography = base.copy(
        headlineLarge = base.headlineLarge.serif(), headlineMedium = base.headlineMedium.serif(),
        headlineSmall = base.headlineSmall.serif(), titleLarge = base.titleLarge.serif(),
    )
    // Edge to edge picks the status bar's icons from the phone's setting; the account's theme can
    // differ (Night on a light phone), and dark icons on a black page are no icons at all.
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as? Activity)?.window ?: return@SideEffect
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !dark
                isAppearanceLightNavigationBars = !dark
            }
        }
    }
    CompositionLocalProvider(LocalProseColors provides ProseColors(scheme.onSurface, dark)) {
        MaterialTheme(colorScheme = scheme, typography = typography, content = content)
    }
}

private fun TextStyle.serif() = copy(fontFamily = Serif)

val ColorScheme.warning: Color get() = Color(0xFFE0A100)

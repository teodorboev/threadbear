package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * A quiet palette: greys, with the accent only where it means something
 * (working, links, the send button), orange for Needs you, red for Failed,
 * as the Mac sidebar colours them. The accent is the Mac's system blue.
 */
@Immutable
data class Signals(val accent: Color, val needsYou: Color, val failed: Color, val done: Color, val faint: Color)

val LocalSignals = staticCompositionLocalOf { Signals(Color(0xFF0A84FF), Color(0xFFFF9F0A), Color(0xFFFF453A), Color(0xFF30D158), Color.Gray) }

private val dark = darkColorScheme(
    primary = Color(0xFF0A84FF),
    onPrimary = Color.White,
    primaryContainer = Color(0xFF1C2A3D),
    onPrimaryContainer = Color(0xFFD6E6FF),
    secondary = Color(0xFF9A9AA0),
    onSecondary = Color.Black,
    secondaryContainer = Color(0xFF2A2A2E),
    onSecondaryContainer = Color(0xFFE6E6EA),
    background = Color(0xFF0E0E10),
    onBackground = Color(0xFFEDEDF0),
    surface = Color(0xFF0E0E10),
    onSurface = Color(0xFFEDEDF0),
    surfaceVariant = Color(0xFF1E1E21),
    onSurfaceVariant = Color(0xFF9C9CA3),
    surfaceContainerLowest = Color(0xFF0A0A0B),
    surfaceContainerLow = Color(0xFF151517),
    surfaceContainer = Color(0xFF19191C),
    surfaceContainerHigh = Color(0xFF212124),
    surfaceContainerHighest = Color(0xFF2A2A2E),
    outline = Color(0xFF3A3A3F),
    outlineVariant = Color(0xFF26262A),
    error = Color(0xFFFF453A),
    // Snackbars: Material's default inverse is near-white, a glaring bar on
    // this dark grey; a raised grey keeps them in the quiet palette.
    inverseSurface = Color(0xFF2E2E33),
    inverseOnSurface = Color(0xFFEDEDF0),
    inversePrimary = Color(0xFF0A84FF),
)

private val light = lightColorScheme(
    primary = Color(0xFF007AFF),
    onPrimary = Color.White,
    primaryContainer = Color(0xFFE3EEFF),
    onPrimaryContainer = Color(0xFF00285A),
    secondary = Color(0xFF6C6C72),
    secondaryContainer = Color(0xFFEDEDF0),
    onSecondaryContainer = Color(0xFF1C1C1E),
    background = Color(0xFFFBFBFC),
    surface = Color(0xFFFBFBFC),
    surfaceVariant = Color(0xFFF0F0F3),
    onSurfaceVariant = Color(0xFF6C6C72),
    surfaceContainerLow = Color(0xFFF5F5F7),
    surfaceContainer = Color(0xFFF0F0F2),
    surfaceContainerHigh = Color(0xFFEAEAED),
    surfaceContainerHighest = Color(0xFFE3E3E7),
    outline = Color(0xFFD0D0D5),
    outlineVariant = Color(0xFFE6E6EA),
    error = Color(0xFFFF3B30),
)

val Mono = FontFamily.Monospace

@Composable
fun ThreadbearTheme(content: @Composable () -> Unit) {
    val darkTheme = isSystemInDarkTheme()
    val base = Typography()
    val type = base.copy(
        titleLarge = base.titleLarge.copy(fontWeight = FontWeight.SemiBold, fontSize = 20.sp),
        titleMedium = base.titleMedium.copy(fontWeight = FontWeight.Medium, fontSize = 16.sp),
        bodyLarge = base.bodyLarge.copy(fontSize = 16.sp, lineHeight = 23.sp),
        bodyMedium = base.bodyMedium.copy(fontSize = 14.sp, lineHeight = 20.sp),
        labelSmall = TextStyle(fontSize = 12.sp, lineHeight = 16.sp, fontWeight = FontWeight.Medium),
    )
    val signals = if (darkTheme) {
        Signals(Color(0xFF0A84FF), Color(0xFFFF9F0A), Color(0xFFFF453A), Color(0xFF30D158), Color(0xFF6E6E75))
    } else {
        Signals(Color(0xFF007AFF), Color(0xFFE68600), Color(0xFFFF3B30), Color(0xFF28A745), Color(0xFF9A9AA0))
    }
    androidx.compose.runtime.CompositionLocalProvider(LocalSignals provides signals) {
        MaterialTheme(colorScheme = if (darkTheme) dark else light, typography = type, content = content)
    }
}

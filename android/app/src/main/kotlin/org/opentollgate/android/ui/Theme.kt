package org.opentollgate.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush

/**
 * TollGate Material3 dark color scheme.
 *
 * Maps the brand palette onto Material3 roles so every existing screen that
 * reads `MaterialTheme.colorScheme.*` picks up the new colours without code
 * changes:
 *
 *  - [primary] / [onPrimary]   → amber CTA (#FFB54C) with dark navy text.
 *  - [surface]                 → dark panel (#1D2144).
 *  - [surfaceVariant]          → darker panel (#242B51) — used by InfoCard.
 *  - [background] / [onBackground] → deep navy (#080E1D) with light text.
 *  - [outline]                 → muted secondary text (#959CB1).
 *  - [error]                   → brand red (#C32222).
 *  - [tertiary]                → success green (#4AA922).
 */
private val TollGateColorScheme = darkColorScheme(
    // Primary — amber CTA
    primary = TollGateAmber,
    onPrimary = TollGateSurface,
    primaryContainer = TollGateAmberTransparent,
    onPrimaryContainer = TollGateBackground,

    // Secondary — amber toned-down
    secondary = TollGateAmber,
    onSecondary = TollGateBackground,
    secondaryContainer = TollGateSurfaceDark,
    onSecondaryContainer = TollGateTextPrimary,

    // Tertiary — success green
    tertiary = TollGateGreen,
    onTertiary = TollGateTextPrimary,
    tertiaryContainer = TollGateGreen,
    onTertiaryContainer = TollGateBackground,

    // Background
    background = TollGateBackground,
    onBackground = TollGateTextPrimary,

    // Surface
    surface = TollGateSurface,
    onSurface = TollGateTextPrimary,
    surfaceVariant = TollGateSurfaceDark,
    onSurfaceVariant = TollGateTextPrimary,
    surfaceTint = TollGateAmber,
    inverseSurface = TollGateTextPrimary,
    inverseOnSurface = TollGateBackground,

    // Error
    error = TollGateRed,
    onError = TollGateTextPrimary,
    errorContainer = TollGateRed,
    onErrorContainer = TollGateTextPrimary,

    // Outline — used for secondary text + dividers throughout the screens
    outline = TollGateTextSecondary,
    outlineVariant = TollGateSurfaceDark,
)

/**
 * The radial-gradient brush used as the TollGate background: lighter navy
 * (#181836) at the centre, deep dark navy (#080E1D) at the edges — matching the
 * tollgate.me hero section.
 */
val TollGateBackgroundBrush: Brush
    @Composable
    get() = Brush.radialGradient(
        colors = listOf(TollGateBackgroundLight, TollGateBackground),
    )

/**
 * Root theme wrapper for the entire app.
 *
 * Supplies the TollGate [darkColorScheme] + [TollGateTypography], and paints the
 * radial-gradient background behind the [content] so every screen inherits the
 * branded backdrop automatically.
 *
 * Usage:
 * ```
 * TollGateTheme {
 *     // Scaffold, NavHost, etc.
 * }
 * ```
 */
@Composable
fun TollGateTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = TollGateColorScheme,
        typography = TollGateTypography,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(TollGateBackgroundBrush),
        ) {
            content()
        }
    }
}

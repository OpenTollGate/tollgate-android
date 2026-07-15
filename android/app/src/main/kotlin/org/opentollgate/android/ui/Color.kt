package org.opentollgate.android.ui

import androidx.compose.ui.graphics.Color

/**
 * TollGate brand color palette.
 *
 * Extracted from tollgate.me + the captive-portal SPA source. The app is a
 * dark-themed (deep navy) product with a warm amber CTA accent.
 */

// ---- Core brand -----------------------------------------------------------

/** Primary CTA / accent — warm amber (#FFB54C). */
val TollGateAmber = Color(0xFFFFB54C)

/** Amber at 80 % opacity — used for glows, borders, spinners. */
val TollGateAmberTransparent = Color(0xCCFFB54C)

// ---- Backgrounds ----------------------------------------------------------

/** Deep dark navy — the base background (#080e1d). */
val TollGateBackground = Color(0xFF080E1D)

/** Lighter navy — the radial-gradient centre (#181836). */
val TollGateBackgroundLight = Color(0xFF181836)

// ---- Text -----------------------------------------------------------------

/** Primary text on dark — light blue-white (#CED3F6). */
val TollGateTextPrimary = Color(0xFFCED3F6)

/** Secondary text — muted blue-gray (#959CB1). */
val TollGateTextSecondary = Color(0xFF959CB1)

// ---- Surfaces -------------------------------------------------------------

/** Dark surface — card / panel background (#1D2144). */
val TollGateSurface = Color(0xFF1D2144)

/** Darker surface — elevated card / surface variant (#242B51). */
val TollGateSurfaceDark = Color(0xFF242B51)

/** Semi-transparent white — used for cards over the gradient on the web. */
val TollGateCardWhite = Color(0xE6FFFFFF)

// ---- Status colors --------------------------------------------------------

/** Success green (#4AA922). */
val TollGateGreen = Color(0xFF4AA922)

/** Error red (#C32222). */
val TollGateRed = Color(0xFFC32222)

// ---- Neutrals -------------------------------------------------------------

/** Light gray (#DBDBDB). */
val TollGateGray = Color(0xFFDBDBDB)

/** Darkened gray (#C4C4C4). */
val TollGateGrayDark = Color(0xFFC4C4C4)

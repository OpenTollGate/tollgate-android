package org.opentollgate.android.ui

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import org.opentollgate.android.R

/**
 * TollGate typography — Quicksand for headings, Fredoka for labels, Inter for
 * body. All three are bundled as variable-font TTFs in `res/font/`.
 */

val Quicksand = FontFamily(
    Font(R.font.quicksand_variable, FontWeight.Light),
    Font(R.font.quicksand_variable, FontWeight.Normal),
    Font(R.font.quicksand_variable, FontWeight.Medium),
    Font(R.font.quicksand_variable, FontWeight.SemiBold),
    Font(R.font.quicksand_variable, FontWeight.Bold),
)

val Fredoka = FontFamily(
    Font(R.font.fredoka_variable, FontWeight.Light),
    Font(R.font.fredoka_variable, FontWeight.Normal),
    Font(R.font.fredoka_variable, FontWeight.Medium),
    Font(R.font.fredoka_variable, FontWeight.SemiBold),
    Font(R.font.fredoka_variable, FontWeight.Bold),
)

val Inter = FontFamily(
    Font(R.font.inter_variable, FontWeight.Light),
    Font(R.font.inter_variable, FontWeight.Normal),
    Font(R.font.inter_variable, FontWeight.Medium),
    Font(R.font.inter_variable, FontWeight.SemiBold),
    Font(R.font.inter_variable, FontWeight.Bold),
)

val TollGateTypography = Typography(
    displayLarge = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.Bold, fontSize = 57.sp, lineHeight = 64.sp),
    displayMedium = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.Bold, fontSize = 45.sp, lineHeight = 52.sp),
    displaySmall = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.SemiBold, fontSize = 36.sp, lineHeight = 44.sp),
    headlineLarge = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.SemiBold, fontSize = 32.sp, lineHeight = 40.sp),
    headlineMedium = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.SemiBold, fontSize = 28.sp, lineHeight = 36.sp),
    headlineSmall = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.SemiBold, fontSize = 24.sp, lineHeight = 32.sp),
    titleLarge = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.SemiBold, fontSize = 22.sp, lineHeight = 28.sp),
    titleMedium = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.Medium, fontSize = 16.sp, lineHeight = 24.sp),
    titleSmall = TextStyle(fontFamily = Quicksand, fontWeight = FontWeight.Medium, fontSize = 14.sp, lineHeight = 20.sp),
    bodyLarge = TextStyle(fontFamily = Inter, fontWeight = FontWeight.Normal, fontSize = 16.sp, lineHeight = 24.sp),
    bodyMedium = TextStyle(fontFamily = Inter, fontWeight = FontWeight.Normal, fontSize = 14.sp, lineHeight = 20.sp),
    bodySmall = TextStyle(fontFamily = Inter, fontWeight = FontWeight.Normal, fontSize = 12.sp, lineHeight = 16.sp),
    labelLarge = TextStyle(fontFamily = Fredoka, fontWeight = FontWeight.Medium, fontSize = 14.sp, lineHeight = 20.sp),
    labelMedium = TextStyle(fontFamily = Fredoka, fontWeight = FontWeight.Medium, fontSize = 12.sp, lineHeight = 16.sp),
    labelSmall = TextStyle(fontFamily = Fredoka, fontWeight = FontWeight.Normal, fontSize = 11.sp, lineHeight = 16.sp),
)

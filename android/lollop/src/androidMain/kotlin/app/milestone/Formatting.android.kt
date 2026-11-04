package app.milestone

import java.util.Locale

// "%.0f" … "%.3f", so a figure formatted every frame builds no format string.
private val FIXED_FORMATS = Array(4) { "%.${it}f" }

internal actual fun formatFixed(value: Double, decimals: Int): String =
    String.format(Locale.US, FIXED_FORMATS.getOrNull(decimals) ?: "%.${decimals}f", value)

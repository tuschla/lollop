package app.milestone

import java.util.Locale

private val FIXED_FORMATS = Array(4) { "%.${it}f" }

internal actual fun formatFixed(value: Double, decimals: Int): String =
    String.format(Locale.US, FIXED_FORMATS.getOrNull(decimals) ?: "%.${decimals}f", value)

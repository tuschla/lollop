package app.milestone

import androidx.compose.ui.graphics.Color

/** How far [AccentTint] walks from the card ground toward the accent in OKLab
 *  lightness, and how much of the accent's own chroma it keeps. */
private const val AccentTintStep = 0.16f
private const val AccentTintChroma = 0.5f

/**
 * The one soft accent tint: the selected nav-bar pill, leading icon tiles, the
 * [SecondaryButton] fill and the selected row of an [OptionList]. Opaque and
 * mixed perceptually ([accentMix]) from `bgElevated` toward the accent, never the
 * accent at low alpha: an alpha wash of a warm accent over a near-complementary
 * ground (tangerine on mint, yellow on indigo) cancels its chroma into beige or
 * grey-brown, while this keeps the accent's hue on every ground. Mixed from the
 * card ground, so it reads one step off both a card and the screen ground.
 * Bold `OnBgBody` text clears AA on it (ThemeContrastTest); Accent text does not
 * on every theme, so it never carries Accent text (an Accent icon is fine).
 */
internal fun Palette.accentTint(): Color = accentMix(accent, bgElevated, AccentTintStep, AccentTintChroma)

/** How far [accentFaint] walks toward the accent, and how much chroma it keeps. */
private const val AccentFaintStep = 0.45f
private const val AccentFaintChroma = 0.8f

/**
 * A quieter solid accent for a data mark that is the accent's lesser kind (a
 * partial split bar beside full ones): [accentMix] from `bgElevated` toward the
 * accent. Opaque for the same reason as [accentTint]: an alpha wash of the accent
 * goes muddy brown over the complementary dark grounds.
 */
internal fun Palette.accentFaint(): Color = accentMix(accent, bgElevated, AccentFaintStep, AccentFaintChroma)

/**
 * The route progression colour at [f] (0 = start, 1 = finish) along a run: the same
 * OKLab mix as [accentFaint], walked on from it to the full accent, so the route
 * opens in `AccentFaint` and ends in `Accent` in every theme (Paper's chroma-free
 * accent included, where only the lightness moves).
 */
internal fun Palette.routeRamp(f: Float): Color {
    val k = f.coerceIn(0f, 1f)
    return accentMix(
        accent, bgElevated,
        AccentFaintStep + k * (1f - AccentFaintStep),
        AccentFaintChroma + k * (1f - AccentFaintChroma),
    )
}

/**
 * Perceptual accent mix in OKLab: lightness walks [t] of the way from [ground] to
 * [accent], chroma is [chroma] × the accent's own, at the accent's hue, so the
 * result is a clean shade of the accent whatever the ground (a plain alpha blend
 * drifts toward the ground's hue and, on a complementary ground, through grey).
 * Chroma that falls outside sRGB at that lightness is reduced (hue and lightness
 * kept) until it fits. Pure; reads no composition state.
 */
internal fun accentMix(accent: Color, ground: Color, t: Float, chroma: Float): Color {
    val a = accent.toOklab()
    val lg = ground.toOklab()[0]
    val l = lg + t * (a[0] - lg)
    val ca = a[1] * chroma
    val cb = a[2] * chroma
    // Largest chroma scale in 0..1 that stays inside sRGB (0 = neutral grey, always inside).
    var k = 1.0
    if (!oklabFitsSrgb(l, ca, cb)) {
        var lo = 0.0
        var hi = 1.0
        repeat(20) {
            val mid = (lo + hi) / 2
            if (oklabFitsSrgb(l, ca * mid, cb * mid)) lo = mid else hi = mid
        }
        k = lo
    }
    val rgb = oklabToLinearSrgb(l, ca * k, cb * k)
    fun encode(v: Double): Float {
        val c = v.coerceIn(0.0, 1.0)
        return (if (c <= 0.0031308) 12.92 * c else 1.055 * Math.pow(c, 1 / 2.4) - 0.055).toFloat()
    }
    return Color(encode(rgb[0]), encode(rgb[1]), encode(rgb[2]))
}

/** sRGB [Color] → OKLab (L, a, b). Alpha is ignored. */
private fun Color.toOklab(): DoubleArray {
    fun lin(ch: Float): Double {
        val s = ch.toDouble()
        return if (s <= 0.04045) s / 12.92 else Math.pow((s + 0.055) / 1.055, 2.4)
    }
    val r = lin(red)
    val g = lin(green)
    val b = lin(blue)
    val l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
    val m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
    val s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
    return doubleArrayOf(
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )
}

/** OKLab → linear-light sRGB channels (unclamped; may fall outside 0..1). */
private fun oklabToLinearSrgb(lightness: Double, a: Double, b: Double): DoubleArray {
    val l = Math.pow(lightness + 0.3963377774 * a + 0.2158037573 * b, 3.0)
    val m = Math.pow(lightness - 0.1055613458 * a - 0.0638541728 * b, 3.0)
    val s = Math.pow(lightness - 0.0894841775 * a - 1.2914855480 * b, 3.0)
    return doubleArrayOf(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )
}

/** Whether an OKLab colour lies inside sRGB (with a rounding hair of slack). */
private fun oklabFitsSrgb(lightness: Double, a: Double, b: Double): Boolean =
    oklabToLinearSrgb(lightness, a, b).all { it >= -1e-4 && it <= 1 + 1e-4 }

// ── Dynamic-color WCAG-AA contrast guard ──────────────────────────────────────
// Material You's `dynamic.primary` is wallpaper-derived and carries no contrast
// guarantee, unlike the hand-tuned theme accents (ratios documented at each palette
// definition). These pure helpers keep a dynamic accent legible under the app's
// on-accent text (bgTop) by nudging only its lightness (hue preserved) until it
// clears AA, or falling back to the theme accent.

/** WCAG 2.x AA minimum contrast ratio for normal-size text. */
internal const val WCAG_AA_NORMAL = 4.5

/**
 * WCAG relative luminance of an sRGB color: linearize each channel (the 0.03928
 * knee), then weight 0.2126 R / 0.7152 G / 0.0722 B. Alpha is ignored (accents
 * are opaque fills).
 */
internal fun relativeLuminance(c: Color): Double {
    fun lin(ch: Float): Double {
        val s = ch.toDouble()
        return if (s <= 0.03928) s / 12.92 else Math.pow((s + 0.055) / 1.055, 2.4)
    }
    return 0.2126 * lin(c.red) + 0.7152 * lin(c.green) + 0.0722 * lin(c.blue)
}

/**
 * WCAG contrast ratio between two colors: (L_hi + 0.05) / (L_lo + 0.05), so it
 * runs 1.0 (identical luminance) … 21.0 (black vs white). Order-independent.
 */
internal fun contrastRatio(a: Color, b: Color): Double {
    val la = relativeLuminance(a)
    val lb = relativeLuminance(b)
    val hi = maxOf(la, lb)
    val lo = minOf(la, lb)
    return (hi + 0.05) / (lo + 0.05)
}

/**
 * Return `primary` if it already clears AA against `onAccent`; otherwise step its
 * HSL lightness AWAY from the on-accent tone (a dark on-accent needs a lighter
 * fill, a light one a darker fill) in small increments, keeping hue + saturation,
 * and return the first shade that passes. If none passes within the lightness
 * range, return `fallback` (the theme's own AA-verified accent).
 */
internal fun accentPassingContrast(primary: Color, onAccent: Color, fallback: Color): Color {
    if (contrastRatio(primary, onAccent) >= WCAG_AA_NORMAL) return primary
    val (h, s, l0) = rgbToHsl(primary)
    // A dark on-accent (dark-theme bgTop) → lighten toward 1.0; a light on-accent
    // (light-theme bgTop) → darken toward 0.0. This is the direction of increasing
    // separation from the fixed on-accent text tone.
    val towardLight = relativeLuminance(onAccent) < 0.5
    val step = 0.04
    var l = l0
    repeat(25) {
        l = if (towardLight) (l + step).coerceAtMost(1.0) else (l - step).coerceAtLeast(0.0)
        val candidate = hslToColor(h, s, l)
        if (contrastRatio(candidate, onAccent) >= WCAG_AA_NORMAL) return candidate
    }
    return fallback
}

/** sRGB [Color] → HSL (hue degrees 0…360, saturation + lightness 0…1). */
private fun rgbToHsl(c: Color): Triple<Double, Double, Double> {
    val r = c.red.toDouble()
    val g = c.green.toDouble()
    val b = c.blue.toDouble()
    val max = maxOf(r, g, b)
    val min = minOf(r, g, b)
    val l = (max + min) / 2.0
    if (max == min) return Triple(0.0, 0.0, l) // achromatic: hue undefined
    val d = max - min
    val s = if (l > 0.5) d / (2.0 - max - min) else d / (max + min)
    val h = when (max) {
        r -> (g - b) / d + (if (g < b) 6.0 else 0.0)
        g -> (b - r) / d + 2.0
        else -> (r - g) / d + 4.0
    } * 60.0
    return Triple(h, s, l)
}

/** HSL (hue degrees, saturation + lightness 0…1) → opaque sRGB [Color]. */
private fun hslToColor(h: Double, s: Double, l: Double): Color {
    if (s == 0.0) {
        val v = l.toFloat().coerceIn(0f, 1f)
        return Color(v, v, v)
    }
    val q = if (l < 0.5) l * (1.0 + s) else l + s - l * s
    val p = 2.0 * l - q
    val hk = (((h % 360.0) + 360.0) % 360.0) / 360.0
    fun channel(t0: Double): Float {
        var t = t0
        if (t < 0.0) t += 1.0
        if (t > 1.0) t -= 1.0
        val v = when {
            t < 1.0 / 6.0 -> p + (q - p) * 6.0 * t
            t < 1.0 / 2.0 -> q
            t < 2.0 / 3.0 -> p + (q - p) * (2.0 / 3.0 - t) * 6.0
            else -> p
        }
        return v.toFloat().coerceIn(0f, 1f)
    }
    return Color(channel(hk + 1.0 / 3.0), channel(hk), channel(hk - 1.0 / 3.0))
}

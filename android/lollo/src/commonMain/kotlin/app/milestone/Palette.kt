package app.milestone

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance

/**
 * The theme-varying decorative token set: ground/surface, on-ground text tones, and
 * the one interactive accent. Selecting a theme (visual-design-system.md §9) swaps
 * this whole map. Semantic [StatusColors] (evidence/danger/warn/HR-zone) are *not*
 * here: they stay fixed across every theme so safety and evidence signaling never
 * shift meaning with a cosmetic choice.
 *
 * Contrast contract (WCAG 2.2 SC 1.4.3, enforced by ThemeContrastTest): every
 * on-ground text tone ([onBgBody], [onBgMuted] AND [onBgFaint]) clears 4.5:1 on
 * [bgTop], [bgElevated] and [bgSunken], and on-accent text ([bgTop]) clears 4.5:1
 * on [accent]. [onBgFaint] is still used as text (overlines, captions), so it is
 * tuned to the AA floor rather than kept as a decorative-only tone. [warnText] is
 * the warn ochre for TEXT (the outlined CONTESTED marker): the fixed
 * [StatusColors.warn] fill only measures 3.0–4.4:1 as text, so each theme carries
 * a lighter (dark grounds) or deeper (light grounds) ochre that clears 4.5:1.
 * [dangerText] is the same idea for the danger red used AS text (a Delete text
 * action, a Discard dialog button): the fixed [StatusColors.danger] fill measures
 * under 3:1 on the dark grounds.
 */
data class Palette(
    val accent: Color,
    val bgTop: Color,
    val bgElevated: Color,
    // Recessed ground for surfaces that sit BELOW the base (keypad, bottom
    // sheets): a hair off bgTop in the dark themes; on light the plain ground
    // or a slightly deeper tint of it.
    val bgSunken: Color,
    val onBgMuted: Color,
    val onBgFaint: Color,
    val onBgBody: Color,
    val warnText: Color,
    val dangerText: Color,
)

/** A dark palette: its ground has low luminance (a forced Dark on a light OS too). */
internal val Palette.isDark: Boolean get() = bgTop.luminance() < 0.5f

/** Text and icons on the [Palette.accent] fill (spec "Accent fill + BgTop text"):
 *  the ground, the deep tinted one on the dark palettes (white fails AA on their
 *  light, bright accents: 2.2:1 on Rose-dark's accent vs 8.1:1 for its plum
 *  ground) and the soft one on the light palettes, whose accents are deepened for it. */
internal val Palette.onAccent: Color get() = bgTop

// Subtext tuned to pair with the theme-fixed danger surface (StatusColors.danger).
// It does not vary by theme: the danger ground is identical in every theme.
// It clears WCAG AA (≈4.9:1) on the danger ground at caption size.
internal val DangerOn = Color(0xFFFFE4E4)
// Labels and glyphs on a fixed status fill, the same in every theme: the danger
// surface (5.8:1), the warn fill (4.9:1) and the evidence fills
// (ThemeContrastTest).
internal val OnStatusFill = Color.White

// Warn ochre for text. The dark one clears ≥5.85:1 on every dark ground (worst
// Meadow-dark bgElevated), the light one ≥5.19:1 on every light ground (worst
// Iris-light bgSunken) (ThemeContrastTest).
private val WarnTextOnDark = Color(0xFFE0A33A)
private val WarnTextOnLight = Color(0xFF8A5300)

// Danger red for text. The dark one clears ≥4.69:1 on every dark ground (worst
// Meadow-dark bgElevated), the light one ≥5.36:1 on every light ground (worst
// Iris-light bgSunken) (ThemeContrastTest).
private val DangerTextOnDark = Color(0xFFF87171)
private val DangerTextOnLight = Color(0xFFB3261E)

/**
 * Rose, dark: a deep aubergine-plum ground with a bright rose-pink accent (hue
 * ≈338°, held apart from the salmon-red dangerText) and warm off-white text.
 * Cards lift one plum step.
 */
internal val RosePalette = Palette(
    accent = Color(0xFFFF8AB5),
    bgTop = Color(0xFF24111F),
    bgElevated = Color(0xFF3A2233),
    bgSunken = Color(0xFF2C1727),
    onBgMuted = Color(0xFFCDB2BD),
    onBgFaint = Color(0xFFA88A96),
    onBgBody = Color(0xFFF7EAE6),
    warnText = WarnTextOnDark,
    dangerText = DangerTextOnDark,
)

/**
 * Meadow, dark: a deep forest-teal ground with a warm peach-coral accent and warm
 * off-white text. Cards lift one teal step.
 */
internal val MeadowPalette = Palette(
    accent = Color(0xFFFF9473),
    bgTop = Color(0xFF0F2420),
    bgElevated = Color(0xFF1B3631),
    bgSunken = Color(0xFF142B26),
    onBgMuted = Color(0xFFBAC8BE),
    onBgFaint = Color(0xFF93A79C),
    onBgBody = Color(0xFFF4EEE3),
    warnText = WarnTextOnDark,
    dangerText = DangerTextOnDark,
)

/**
 * Iris, dark: a deep ink-indigo ground with a sunflower-yellow accent and warm
 * cream text. Cards lift one indigo step.
 */
internal val IrisPalette = Palette(
    accent = Color(0xFFFFD25E),
    bgTop = Color(0xFF17142F),
    bgElevated = Color(0xFF262249),
    bgSunken = Color(0xFF1D1A3B),
    onBgMuted = Color(0xFFCBC1D8),
    onBgFaint = Color(0xFFA79FC0),
    onBgBody = Color(0xFFFBF3E4),
    warnText = WarnTextOnDark,
    dangerText = DangerTextOnDark,
)

// Light variants. Soft tinted grounds (blush, mint, butter cream; Paper stays
// white) with lighter near-white cards, and each accent deepened so it clears
// text contrast on a light surface and under its on-accent tone (bgTop). The
// semantic [StatusColors] stay fixed, only these decorative grounds/accent flip.
// Chosen at the theme root by the resolved light/dark mode, not per theme.

/** Rose, light: a soft blush ground, near-white rose cards and a raspberry accent
 *  (hue ≈330°, on the magenta side of the danger red) with deep plum-ink text.
 *  bgSunken is the plain ground. */
internal val RoseLightPalette = Palette(
    accent = Color(0xFFAD1A64),
    bgTop = Color(0xFFFBE8ED),
    bgElevated = Color(0xFFFFF7F9),
    bgSunken = Color(0xFFFBE8ED),
    onBgMuted = Color(0xFF6B4655),
    onBgFaint = Color(0xFF7D5C69),
    onBgBody = Color(0xFF2A1520),
    warnText = WarnTextOnLight,
    dangerText = DangerTextOnLight,
)

/** Meadow, light: a mint ground, near-white mint cards and a tangerine accent (on
 *  the orange side of coral, apart from rose and the danger red) with deep
 *  green-ink text. bgSunken is the plain ground. */
internal val MeadowLightPalette = Palette(
    accent = Color(0xFFBF4100),
    bgTop = Color(0xFFE4F2E8),
    bgElevated = Color(0xFFF5FBF6),
    bgSunken = Color(0xFFE4F2E8),
    onBgMuted = Color(0xFF4A5E54),
    onBgFaint = Color(0xFF5B6E64),
    onBgBody = Color(0xFF16261F),
    warnText = WarnTextOnLight,
    dangerText = DangerTextOnLight,
)

/** Iris, light: a butter-cream ground, near-white cream cards, a deeper cream for
 *  sheets and the keypad, and a violet accent with ink-violet text. */
internal val IrisLightPalette = Palette(
    accent = Color(0xFF5A3FC0),
    bgTop = Color(0xFFF8EFD6),
    bgElevated = Color(0xFFFFFAEC),
    bgSunken = Color(0xFFF3E8CB),
    onBgMuted = Color(0xFF5E5470),
    onBgFaint = Color(0xFF6B6078),
    onBgBody = Color(0xFF231C3A),
    warnText = WarnTextOnLight,
    dangerText = DangerTextOnLight,
)

/**
 * Paper (default), minimal monochrome: white ground, black
 * foreground, no chroma at all. The accent is near-black (not a color), so the UI
 * reads as pure ink-on-paper; interactive/selected states use tone, not hue. A
 * fresh install pairs it with the system (Material You) accent, which replaces
 * only this ink accent; without dynamic colour it stays ink. Like the others it
 * inverts with system dark. Semantic [StatusColors] still apply, so
 * safety/evidence remain colored even in this monochrome theme.
 */
internal val PaperLightPalette = Palette(
    accent = Color(0xFF1A1A1A),
    bgTop = Color(0xFFFFFFFF),
    bgElevated = Color(0xFFF3F3F1),
    bgSunken = Color(0xFFFFFFFF),
    onBgMuted = Color(0xFF5C5C5A),
    onBgFaint = Color(0xFF6E6E6C),
    onBgBody = Color(0xFF0A0A0A),
    warnText = WarnTextOnLight,
    dangerText = DangerTextOnLight,
)

/** Paper, dark (inverted): black ground, near-white ink accent. */
internal val PaperDarkPalette = Palette(
    accent = Color(0xFFF0F0EE),
    bgTop = Color(0xFF0A0A0A),
    bgElevated = Color(0xFF1B1B1A),
    bgSunken = Color(0xFF141414),
    onBgMuted = Color(0xFFA0A09E),
    onBgFaint = Color(0xFF848481),
    onBgBody = Color(0xFFF5F5F3),
    warnText = WarnTextOnDark,
    dangerText = DangerTextOnDark,
)

/** Light/dark selection: follow the OS, or force one. */
enum class DarkMode {
    System, Light, Dark;

    /** Dark under this selection, where the OS is dark when [systemDark]. */
    fun isDark(systemDark: Boolean): Boolean = when (this) {
        System -> systemDark
        Light -> false
        Dark -> true
    }
}

/**
 * User-selectable visual themes (visual-design-system.md §1, §9). Each carries a
 * [dark] and [light] palette; the theme root picks between them by system setting.
 * Persisted by [name]; [Default] is what a fresh install, an unknown stored name
 * and a composition outside the theme root get.
 */
enum class AppTheme(
    val dark: Palette,
    val light: Palette,
    /** Picking the theme turns the system (wallpaper) accent on: Paper's ink accent
     *  is a blank slate for it, the tinted grounds clash with one. */
    val takesSystemAccent: Boolean = false,
) {
    Rose(RosePalette, RoseLightPalette),
    Meadow(MeadowPalette, MeadowLightPalette),
    Iris(IrisPalette, IrisLightPalette),
    Paper(PaperDarkPalette, PaperLightPalette, takesSystemAccent = true);

    companion object {
        val Default: AppTheme = Paper

        /** Theme names older builds persisted → the theme that took each one's
         *  place, so a stored choice never silently falls back. */
        private val retired = mapOf("Beton" to Rose, "Werkstatt" to Meadow, "Signal" to Iris)

        fun from(name: String?): AppTheme =
            entries.firstOrNull { it.name == name } ?: name?.let(retired::get) ?: Default
    }
}

package app.milestone

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance

/**
 * The theme-varying decorative token set: ground/surface, on-ground text tones, and
 * the one interactive accent. Selecting a theme (visual-design-system.md §9) swaps
 * this whole map. Semantic [StatusColors] (evidence/danger/warn/HR-zone) are *not*
 * here, they stay fixed across every theme so safety and evidence signaling never
 * shift meaning with a cosmetic choice.
 *
 * Contrast contract (WCAG 2.2 SC 1.4.3, enforced by ThemeContrastTest): every
 * on-ground text tone — [onBgBody], [onBgMuted] AND [onBgFaint] — clears 4.5:1 on
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

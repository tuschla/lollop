package app.milestone

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Subtext tuned to pair with the theme-fixed danger surface (StatusColors.danger).
// It does not vary by theme: the danger ground is identical in every theme.
// #FFE4E4 clears WCAG AA (≈4.9:1) on the #C1272D danger ground at caption size.
internal val DangerOn = Color(0xFFFFE4E4) // subtext on the danger surface

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

// Warn ochre for text. #E0A33A clears ≥5.85:1 on every dark ground (worst
// Meadow-dark bgElevated), #8A5300 ≥5.19:1 on every light ground (worst
// Iris-light bgSunken) (ThemeContrastTest).
private val WarnTextOnDark = Color(0xFFE0A33A)
private val WarnTextOnLight = Color(0xFF8A5300)

// Danger red for text. #F87171 clears ≥4.69:1 on every dark ground (worst
// Meadow-dark bgElevated), #B3261E ≥5.36:1 on every light ground (worst
// Iris-light bgSunken) (ThemeContrastTest).
private val DangerTextOnDark = Color(0xFFF87171)
private val DangerTextOnLight = Color(0xFFB3261E)

/**
 * Rose, dark: a deep aubergine-plum ground (wine-tinted, never grey-black)
 * with a bright rose-pink accent and warm off-white text. Cards lift one plum step.
 * The accent leans pink (hue ≈338°), held apart from the salmon-red dangerText.
 * Ratios (ThemeContrastTest, on bgTop / bgElevated / bgSunken): body 15.21 /
 * 12.27 / 14.19, muted 9.11 / 7.35 / 8.50, faint 5.74 / 4.63 / 5.35, accent as
 * text 8.13 / 6.56 / 7.59, warnText 8.06 / 6.50 / 7.52, dangerText 6.46 / 5.21 /
 * 6.03; on-accent (bgTop) on accent 8.13:1; body on 4% cell wash ≥11.00:1, on
 * AccentTint (#5D2D3F) 9.34:1. The ground stays deep so the fixed danger fill
 * still separates from it (3.06:1 on bgTop, red against plum).
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
 * Meadow, dark: a deep forest-teal ground (green-tinted, never grey-black) with a
 * warm peach-coral accent and warm off-white text, an evening run by the lake.
 * Cards lift one teal step. Ratios (ThemeContrastTest, on bgTop / bgElevated /
 * bgSunken): body 14.06 / 11.24 / 12.96, muted 9.35 / 7.47 / 8.62, faint 6.38 /
 * 5.10 / 5.88, accent as text 7.53 / 6.01 / 6.94, warnText 7.32 / 5.85 / 6.75,
 * dangerText 5.87 / 4.69 / 5.41; on-accent (bgTop) on accent 7.53:1; body on 4%
 * cell wash ≥10.11:1, on AccentTint (#623528) 8.83:1. The fixed danger fill sits
 * on the complementary hue (red on green), 2.78:1 luminance on bgTop.
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
 * Iris, dark: a deep ink-indigo night (blue-violet, never grey-black) with a
 * sunflower-yellow accent, the iris's violet petals and yellow crest. Cards lift
 * one indigo step; text is a warm cream. Ratios (ThemeContrastTest, on
 * bgTop / bgElevated / bgSunken): body 16.17 / 13.53 / 15.08, muted 10.33 /
 * 8.64 / 9.63, faint 7.10 / 5.94 / 6.62, accent as text 12.43 / 10.40 / 11.58,
 * warnText 8.04 / 6.73 / 7.50, dangerText 6.45 / 5.39 / 6.01; on-accent
 * (bgTop) on accent 12.43:1; body on 4% cell wash ≥12.17:1, on AccentTint
 * (#513E0A) 9.32:1. The ground stays deep enough that the fixed danger fill
 * still separates from it (3.05:1 on bgTop).
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

/** Rose, light: a soft blush ground, near-white rose cards and a
 *  raspberry-berry accent (hue ≈330°, on the magenta side so it never reads as
 *  the fire-engine danger red) with deep plum-ink text. bgSunken is the plain
 *  ground. Ratios (ThemeContrastTest, on bgTop / bgElevated / bgSunken): body
 *  14.57 / 16.24 / 14.57, muted 6.79 / 7.57 / 6.79, faint 4.95 / 5.52 / 4.95,
 *  accent as text 5.75 / 6.41 / 5.75, warnText 5.39 / 6.00 / 5.39, dangerText
 *  5.56 / 6.20 / 5.56; on-accent (bgTop) on accent 5.75:1; body on 4% cell wash
 *  ≥13.54:1, on AccentTint (#FFD1E0) 12.59:1. The blush stays light so the fixed
 *  danger fill still stands out (4.97:1 on bgTop). */
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

/** Meadow, light: fresh mint ground, near-white mint cards, and a juicy tangerine
 *  accent (kept on the orange side of coral so it never reads as rose/berry or the
 *  danger red) with deep green-ink text. The accent is pure orange at the most
 *  chroma sRGB holds at the AA lightness ceiling (an orange with less blue reads
 *  rusty brown at this depth); the mint is lifted a hair so that ceiling sits
 *  higher. Ratios (ThemeContrastTest, on bgTop / bgElevated / bgSunken): body
 *  13.65 / 15.03 / 13.65, muted 6.01 / 6.63 / 6.01, faint 4.71 / 5.19 / 4.71,
 *  accent as text 4.57 / 5.04 / 4.57, warnText 5.47 / 6.03 / 5.47, dangerText
 *  5.65 / 6.23 / 5.65; on-accent (bgTop) on accent 4.57:1; body on 4% cell wash
 *  ≥12.69:1, on AccentTint (#FFD9CB) 12.05:1. The danger fill reads 5.05:1
 *  against the mint ground. */
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

/** Iris, light: sunny butter-cream ground, near-white cream cards, a slightly
 *  deeper cream for sheets/keypad, and a vivid violet accent with ink-violet
 *  text. Ratios (ThemeContrastTest, on bgTop / bgElevated / bgSunken): body
 *  14.10 / 15.51 / 13.26, muted 6.15 / 6.76 / 5.78, faint 5.13 / 5.64 / 4.82,
 *  accent as text 6.30 / 6.93 / 5.92, warnText 5.52 / 6.07 / 5.19, dangerText
 *  5.70 / 6.27 / 5.36; on-accent (bgTop) on accent 6.30:1; body on 4% cell
 *  wash ≥12.32:1, on AccentTint (#DCDBFF) 12.03:1. The danger fill reads
 *  4.79–5.60:1 against these grounds. */
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
    // Faint #8C8C8A → #6E6E6C so it clears AA on bgElevated: 3.03:1 → 4.60:1.
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
    // Faint #6A6A68 → #848481 so it clears AA on bgElevated: 3.18:1 → 4.60:1.
    onBgFaint = Color(0xFF848481),
    onBgBody = Color(0xFFF5F5F3),
    warnText = WarnTextOnDark,
    dangerText = DangerTextOnDark,
)

/**
 * User-selectable visual themes (visual-design-system.md §1, §9). Each carries a
 * [dark] and [light] palette; the theme root picks between them by system setting.
 * Persisted by [name]; [Paper] is the default (AppSettings pins it, with the system
 * accent, on a fresh install).
 */
enum class AppTheme(val label: String, val dark: Palette, val light: Palette) {
    Rose("Rose", RosePalette, RoseLightPalette),
    Meadow("Meadow", MeadowPalette, MeadowLightPalette),
    Iris("Iris", IrisPalette, IrisLightPalette),
    Paper("Paper", PaperDarkPalette, PaperLightPalette);

    companion object {
        /** Names persisted before the 2026-10-10 theme redesign → the theme that
         *  took each one's place, so a stored choice never silently falls back. */
        private val retired = mapOf("Beton" to Rose, "Werkstatt" to Meadow, "Signal" to Iris)

        fun from(name: String?): AppTheme =
            entries.firstOrNull { it.name == name } ?: name?.let(retired::get) ?: Paper
    }
}

/** Active decorative palette, provided at the theme root by [MilestoneTheme]. */
val LocalPalette = staticCompositionLocalOf { RosePalette }

// Bare token accessors for the many call sites that read them directly. Each
// resolves against the active [LocalPalette], so switching themes recolors the
// whole tree. These are @Composable getters, usable anywhere in composition, but
// NOT from plain (non-composable) code; read [LocalPalette].current there instead.
val Accent: Color @Composable get() = LocalPalette.current.accent
val BgTop: Color @Composable get() = LocalPalette.current.bgTop
val BgElevated: Color @Composable get() = LocalPalette.current.bgElevated
val BgSunken: Color @Composable get() = LocalPalette.current.bgSunken
val OnBgMuted: Color @Composable get() = LocalPalette.current.onBgMuted
val OnBgFaint: Color @Composable get() = LocalPalette.current.onBgFaint
val OnBgBody: Color @Composable get() = LocalPalette.current.onBgBody
val WarnText: Color @Composable get() = LocalPalette.current.warnText

/** Danger red for TEXT on any ground (Delete / Discard actions); the fixed
 *  [StatusColors.danger] stays the fill of the danger surfaces. */
val DangerText: Color @Composable get() = LocalPalette.current.dangerText

/** Tone for text/icons sitting on the [Accent] fill (spec
 *  "Accent fill + BgTop text"): the active theme's `bgTop`, the deep tinted
 *  ground on the dark palettes (white fails AA on their light, bright accents,
 *  2.2:1 on Rose-dark's #FF8AB5 vs 8.1:1 for its plum #24111F) and the soft
 *  tinted ground on the light palettes, whose accents are deepened for contrast. */
internal val OnAccent: Color @Composable get() = LocalPalette.current.bgTop

/** The active theme's [accentTint]: the one soft accent tint (selected nav pill,
 *  leading icon tiles, [SecondaryButton] fill, selected [OptionList] row). */
val AccentTint: Color @Composable get() = LocalPalette.current.accentTint()

/** The active theme's [accentFaint]. */
val AccentFaint: Color @Composable get() = LocalPalette.current.accentFaint()

/**
 * Hairline opacities, shared by every theme. Each role keeps its own weight: a row
 * divider inside a card is quieter than a card outline, which is quieter than the
 * outline of a floating control.
 */
object HairlineAlpha {
    /** Row dividers inside a card or review list (OnBgBody). */
    const val Row = 0.05f
    /** Card outlines and chart baselines/tracks (OnBgBody). */
    const val Card = 0.06f
    /** Outlines of tappable cards, pills and floating round controls (OnBgBody). */
    const val Control = 0.07f
    /** Plain dividers between list rows and sections (OnBgBody). */
    const val Divider = 0.08f
    /** The visible bound of an unselected cell, idle field, gridline (OnBgFaint). */
    const val Strong = 0.3f
}

/** Plain divider between list rows and sections. */
val Hairline: Color @Composable get() = OnBgBody.copy(alpha = HairlineAlpha.Divider)

/** Hairline that must stay visible on any ground: an unselected cell's bound, an
 *  idle input field, a chart gridline. */
val HairlineStrong: Color @Composable get() = OnBgFaint.copy(alpha = HairlineAlpha.Strong)

/**
 * Semantic colors Material 3's [ColorScheme] has no role for: evidence grades,
 * safety/warn states, HR zones. These carry fixed meaning and never follow the
 * system/dynamic accent, only the decorative [ColorScheme.primary] does. Provided
 * app-wide via [LocalStatusColors] so any composable can read them without
 * threading params. See design/visual-design-system.md §7.
 */
data class StatusColors(
    val evidenceStrong: Color = Color(0xFF166534),
    val evidenceModerate: Color = Color(0xFF3F6212),
    val evidenceWeak: Color = Color(0xFF78350F),
    // Warm stone, not indigo: a bright violet badge sat off-palette next to the
    // clay/olive/green grades. White text on it clears 7.6:1.
    val evidenceExpert: Color = Color(0xFF57534E),
    val evidenceUnknown: Color = Color(0xFF334155),
    // Warn ochre, held apart from evidenceWeak and hrZone2 so a warn chip is
    // never read as an evidence grade or an HR zone (both render beside it).
    // White text clears 4.9:1 AA on this fill. It is a FILL/border colour only:
    // as text it measures 3.0–4.4:1, so text uses the per-theme [WarnText].
    val warn: Color = Color(0xFFA16207),
    // Signal red (spec §2A): white text clears 5.8:1 AA on this DANGER surface.
    val danger: Color = Color(0xFFC1272D),
    val dangerStrong: Color = Color(0xFFCE2A2E),
    // The done mark (a ✓ glyph, a week-strip dot) drawn straight on the ground:
    // a graphical object, so ≥3:1 against every ground of the mode. The deep
    // green clears it on the light grounds; [DarkStatusColors] lightens it for
    // the dark ones (ThemeContrastTest).
    val done: Color = Color(0xFF166534),
    // The warn mark drawn straight on a card ground (an `Adjusted` StatusCard's
    // ring and mark tile, an InlineNotice glyph): a graphical object, ≥3:1 against every ground of the mode.
    // The warn fill clears it on the light grounds; [DarkStatusColors] swaps in
    // the lighter ochre of the dark grounds' [WarnText] (ThemeContrastTest).
    val warnMark: Color = Color(0xFFA16207),
    // Three-zone lactate model (running.rs::ThreeZone). Fixed meaning: Z1 easy
    // aerobic, Z2 threshold, Z3 hard. Never follow the system accent.
    val hrZone1: Color = Color(0xFF0E7490),
    val hrZone2: Color = Color(0xFFB45309),
    val hrZone3: Color = Color(0xFFB91C1C),
)

/** [StatusColors] on the dark grounds: only the on-ground done and warn marks change. */
val DarkStatusColors = StatusColors(done = Color(0xFF4ADE80), warnMark = Color(0xFFE0A33A))

val LocalStatusColors = staticCompositionLocalOf { StatusColors() }

/** Tabular figures so numeric columns (pace, e1RM, confidence) don't jitter. */
val TabularFigures = TextStyle(fontFeatureSettings = "tnum")

/**
 * Type scale, shared by every theme (design spec §3, visual-design-system.md §3).
 * System font; the scale is what matters. Readable at the default font size:
 * running text is never below [Caption] (14sp), and no style is below 12sp.
 */
object Type {
    val Display = TextStyle(fontSize = 32.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
    val Title = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold)

    // Off-scale display figures, each owned by one surface.
    /** Do-not-train banner headline: 23sp Black. */
    val BannerTitle = TextStyle(fontSize = 23.sp, fontWeight = FontWeight.Black)
    /** Guided-run step countdown on the tracker: 28sp/32sp ExtraBold, tabular. */
    val Countdown = TextStyle(fontSize = 28.sp, lineHeight = 32.sp, fontWeight = FontWeight.ExtraBold, fontFeatureSettings = "tnum")
    /** Numeric keypad key face: 22sp Bold, tabular. */
    val KeypadKey = TextStyle(fontSize = 22.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
    /** History week-strip stat value: 19sp ExtraBold, tabular. */
    val StatValue = TextStyle(fontSize = 19.sp, fontWeight = FontWeight.ExtraBold, fontFeatureSettings = "tnum")
    /** Title of a history card or detail sheet (a run, a lift session): 17sp ExtraBold. */
    val CardTitle = TextStyle(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold)

    val Section = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp)
    // 22sp leading for multi-line running text, trimmed above the first and below
    // the last line so a single line keeps its font height and one-line rows don't grow.
    val Body = TextStyle(
        fontSize = 16.sp,
        lineHeight = 22.sp,
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both),
    )
    // Secondary running text. Explicit 20sp line height, untrimmed: the extra
    // leading is split above the first and below the last line, so a multi-line
    // caption's final-line descenders ("goes", "you're") never touch, and get cut
    // by, a clipped row edge.
    val Caption = TextStyle(
        fontSize = 14.sp,
        lineHeight = 20.sp,
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None),
    )
    /** Chip / control / short-label text (choice chips, segments, nav labels,
     *  chart legends): 13sp Bold. */
    val Chip = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold)

    /** Tile-scale overline (chrome §3): 12sp Bold, FieldLabel letter-spacing,
     *  UPPERCASE at the call site, color `OnBgFaint`. Top-left of every tile. */
    val Overline = TextStyle(fontSize = 12.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp)
}

/** Spacing scale in dp, shared by every theme (design spec §4). */
object Space {
    const val Xs = 2
    const val Sm = 4
    const val Md = 8
    const val Card = 14
    const val Screen = 16
    /** Horizontal gutter of a bottom sheet (editors, detail sheets, tracker sheet). */
    const val Sheet = 18
    /** Wider gutter of a text-heavy reference sheet (evidence legend, glossary,
     *  calculators), where the line length matters more than the width. */
    const val ReadingSheet = 20
    const val Lg = 24
}

/** Minimum height (= touch target) of every button and tappable row (WCAG 2.5.8
 *  / Material's 48dp minimum). */
val ActionMinHeight = 48.dp

/** Height of a large control: a pinned editor action, the tracker's control row,
 *  a tall tappable record row. */
val LargeControlHeight = 56.dp

/** Horizontal inset of a [TextAction]'s label inside its touch target: a
 *  start-aligned caller offsets by `-TextActionInset` so the label (not its
 *  padding) lines up with the content edge above it. */
val TextActionInset = Space.Sm.dp

/** Height of an embedded route map (run detail, trim sheet). */
val RouteMapHeight = 220.dp

/** Top-corner radius of the tracker's floating stat sheet. */
val SheetCornerRadius = 26.dp

/** Outline of a [StatusCard]: the ring that carries its tone. */
val StatusRingWidth = 1.5.dp

/** Opacity of the `BgTop` ground behind the full-screen map's top bar and stat sheet. */
const val MapChromeAlpha = 0.94f

/** Floor of a split bar's domain, as a fraction of the slowest split's pace. */
const val SplitBarBaseline = 0.85f

/** Medium motion, in ms: a content fade-in (the route map). */
const val MotionMedium = 200

/**
 * Route markers (map, trim, finish review). Start: an [StartSize] disc with an
 * [StartRing] `Accent` ring and a `BgElevated` centre. Finish: a solid `Accent`
 * [FinishSize] disc holding a [FinishGlyph] OnAccent flag
 * (`R.drawable.ic_route_finish_flag`). Distance: a [DistanceSize] `BgElevated`
 * disc with a [DistanceRing] `Accent` ring and a `Type.Chip` tnum `OnBgBody`
 * numeral at every whole km/mi. Every marker sits on a [Halo] casing in the
 * polyline's ground-casing colour so it reads on any tile.
 */
object RouteMarker {
    val StartSize = 18.dp
    val StartRing = 4.dp
    val FinishSize = 26.dp
    val FinishGlyph = 16.dp
    val DistanceSize = 22.dp
    val DistanceRing = 1.5.dp
    val Halo = 3.dp
}

/** Grade badge color from the theme-fixed status palette. See design spec §2. */
fun StatusColors.gradeColor(grade: String): Color = when (grade) {
    "Strong" -> evidenceStrong
    "Moderate" -> evidenceModerate
    "Weak" -> evidenceWeak
    "ExpertOpinion" -> evidenceExpert
    else -> evidenceUnknown
}

/**
 * HR-zone accent from the theme-fixed status palette. Zone strings mirror
 * app/mod.rs `RunResultView.zone` ("Z1"/"Z2"/"Z3"); unknown ("-") falls back to the
 * active theme's [accent] so a run with no HR sample isn't miscolored as a real
 * zone. The accent is passed in because this runs outside composition.
 */
fun StatusColors.hrZoneColor(zone: String, accent: Color): Color = when (zone) {
    "Z1" -> hrZone1
    "Z2" -> hrZone2
    "Z3" -> hrZone3
    else -> accent
}

/**
 * App theme. The selected [AppTheme] supplies the whole decorative palette (ground,
 * surface, text tones, accent). On Android 12+ with "system accent" enabled, only
 * the accent role follows the user's Material You wallpaper color; the rest of the
 * palette is still the chosen theme's. Semantic status colors stay fixed across all
 * themes so safety and evidence signaling remain unambiguous; only the on-ground
 * done mark lightens on the dark grounds ([DarkStatusColors]).
 */
@Composable
fun MilestoneTheme(
    content: @Composable () -> Unit,
) {
    val ctx = LocalContext.current
    val dynamicColor by AppSettings.dynamicAccent.collectAsState()
    val appTheme by AppSettings.theme.collectAsState()
    val darkMode by AppSettings.darkMode.collectAsState()
    // Light/dark source: respect the OS by default, or honor a forced override
    // (the "respect system dark mode" toggle). Each theme supplies both grounds;
    // only the decorative palette (and the done mark) flips, never the fills.
    val dark = when (darkMode) {
        DarkMode.System -> isSystemInDarkTheme()
        DarkMode.Light -> false
        DarkMode.Dark -> true
    }
    val base = if (dark) appTheme.dark else appTheme.light
    // The other ground of the same palette: M3 draws snackbars on the inverse
    // roles, so they take this theme's opposite ground instead of M3's baseline
    // lavender (ui-sweep §15).
    val inverse = if (dark) appTheme.light else appTheme.dark
    // The selected theme ALWAYS supplies the full scheme, dialogs, menus,
    // switches and every other M3 surface keep the chosen palette. When dynamic
    // color (Material You) is on, only the accent role (primary + its on-color)
    // is taken from the wallpaper-derived scheme (spec §8/§9, design-spec §2:
    // "only the accent role follows the wallpaper color; the rest of the
    // selected theme's palette still applies"). Swapping in the whole dynamic
    // scheme here would wallpaper-tint every dialog/menu/switch surface.
    val themed: ColorScheme = if (dark) {
        darkColorScheme(
            primary = base.accent,
            // Accent fill uses BgTop text: M3 Buttons read
            // onPrimary, so pin it to the ground tone (dark OnAccent here -
            // the mid-tone dark accents fail AA under light text).
            onPrimary = base.bgTop,
            background = base.bgTop,
            surface = base.bgElevated,
            onBackground = base.onBgBody,
            onSurface = base.onBgBody,
            // Pin M3's surface-container / variant / outline roles to the
            // palette's own grounds so dialogs, bottom sheets and dropdown
            // menus (which draw on surfaceContainer*/surfaceVariant, not
            // `surface`) render in the selected palette instead of M3's
            // baseline tones. Container tiers all map to the app's elevated
            // surface (dialogs read as raised cards); the lowest tier sits on
            // the ground.
            surfaceVariant = base.bgElevated,
            onSurfaceVariant = base.onBgMuted,
            surfaceContainerLowest = base.bgTop,
            surfaceContainerLow = base.bgElevated,
            surfaceContainer = base.bgElevated,
            surfaceContainerHigh = base.bgElevated,
            surfaceContainerHighest = base.bgElevated,
            outline = base.onBgFaint,
            // Also pin outlineVariant: M3 HorizontalDividers/menus draw on it, so
            // leaving it baseline let a stray divider tone leak through the palette.
            outlineVariant = base.onBgFaint.copy(alpha = HairlineAlpha.Strong),
            inverseSurface = inverse.bgElevated,
            inverseOnSurface = inverse.onBgBody,
            inversePrimary = inverse.accent,
        )
    } else {
        lightColorScheme(
            primary = base.accent,
            onPrimary = base.bgTop,
            background = base.bgTop,
            surface = base.bgElevated,
            onBackground = base.onBgBody,
            onSurface = base.onBgBody,
            // Same surface-container / variant / outline pinning as the dark
            // scheme: keep dialogs/sheets/menus in the selected palette rather
            // than M3's baseline tones (see dark branch for role rationale).
            surfaceVariant = base.bgElevated,
            onSurfaceVariant = base.onBgMuted,
            surfaceContainerLowest = base.bgTop,
            surfaceContainerLow = base.bgElevated,
            surfaceContainer = base.bgElevated,
            surfaceContainerHigh = base.bgElevated,
            surfaceContainerHighest = base.bgElevated,
            outline = base.onBgFaint,
            // Also pin outlineVariant: M3 HorizontalDividers/menus draw on it, so
            // leaving it baseline let a stray divider tone leak through the palette.
            outlineVariant = base.onBgFaint.copy(alpha = HairlineAlpha.Strong),
            inverseSurface = inverse.bgElevated,
            inverseOnSurface = inverse.onBgBody,
            inversePrimary = inverse.accent,
        )
    }
    val scheme: ColorScheme = if (dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val dynamic = if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        // AA guard: every hand-picked theme accent is WCAG-AA-verified against its
        // on-accent text (= bgTop, the accent-fill-on-bgTop pairing; the
        // per-palette ratios are documented at each palette definition). The Material You
        // `dynamic.primary` is NOT verified, so pin the on-accent to the theme's
        // bgTop (as the themed scheme already does) and nudge the dynamic accent's
        // lightness (hue kept) until it clears AA on that same pairing; if it can't,
        // fall back to the theme's own verified accent rather than ship a failing
        // contrast. This keeps Material You's hue whenever it is legible.
        val guardedPrimary = accentPassingContrast(dynamic.primary, base.bgTop, base.accent)
        themed.copy(primary = guardedPrimary, onPrimary = base.bgTop)
    } else {
        themed
    }
    // Accent follows the Material You primary when dynamic color is on; the rest of
    // the palette (grounds, text) is always the selected theme's.
    val palette = base.copy(accent = scheme.primary)
    CompositionLocalProvider(
        LocalStatusColors provides if (dark) DarkStatusColors else StatusColors(),
        LocalPalette provides palette,
    ) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}

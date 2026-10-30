package app.milestone

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
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
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Subtext tuned to pair with the theme-fixed danger surface (StatusColors.danger).
// It does not vary by theme: the danger ground is identical in every theme.
// #FFE4E4 clears WCAG AA (≈4.9:1) on the #C1272D danger ground at caption size.
internal val DangerOn = Color(0xFFFFE4E4) // subtext on the danger surface
// Labels and glyphs on the danger surface (white, 5.8:1 on #C1272D).
internal val DangerLabel = Color.White

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

/**
 * User-selectable visual themes (visual-design-system.md §1, §9). Each carries a
 * [dark] and [light] palette; the theme root picks between them by system setting.
 * Persisted by [name]; [Paper] is the default (AppSettings pins it, with the system
 * accent, on a fresh install).
 */
enum class AppTheme(val dark: Palette, val light: Palette) {
    Rose(RosePalette, RoseLightPalette),
    Meadow(MeadowPalette, MeadowLightPalette),
    Iris(IrisPalette, IrisLightPalette),
    Paper(PaperDarkPalette, PaperLightPalette);

    companion object {
        /** Theme names older builds persisted → the theme that took each one's
         *  place, so a stored choice never silently falls back. */
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

/**
 * Base direction of every paragraph: its first strong character, LTR when it has
 * none. The copy is English-only while the layout mirrors in RTL locales; taking
 * the base direction from the RTL layout (Compose's default) reordered the neutral
 * characters at a paragraph's edges ("5.5 km" → "km 5.5", a sentence's final '.'
 * jumping to the line start). LTR is the fallback, not RTL, so a neutral-only
 * figure ("−2.5", "4%") keeps its sign and symbol in place; user-entered RTL text
 * still starts with a strong RTL character and resolves RTL. Set on every style
 * because an explicit `style` replaces Material's LocalTextStyle and a
 * TextMeasurer never reads it.
 */
val AppTextDirection = TextDirection.ContentOrLtr

/** Tabular figures so numeric columns (pace, e1RM, confidence) don't jitter. */
val TabularFigures = TextStyle(fontFeatureSettings = "tnum", textDirection = AppTextDirection)

/**
 * Type scale, shared by every theme (design spec §3, visual-design-system.md §3).
 * System font; the scale is what matters. Readable at the default font size:
 * running text is never below [Caption] (14sp), and no style is below 12sp.
 */
object Type {
    val Display = TextStyle(fontSize = 32.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum", textDirection = AppTextDirection)
    val Title = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold, textDirection = AppTextDirection)

    // Off-scale display figures, each owned by one surface.
    /** Do-not-train banner headline: 23sp Black. */
    val BannerTitle = TextStyle(fontSize = 23.sp, fontWeight = FontWeight.Black, textDirection = AppTextDirection)
    /** Guided-run step countdown on the tracker: 28sp/32sp ExtraBold, tabular. */
    val Countdown = TextStyle(fontSize = 28.sp, lineHeight = 32.sp, fontWeight = FontWeight.ExtraBold, fontFeatureSettings = "tnum", textDirection = AppTextDirection)
    /** Numeric keypad key face: 22sp Bold, tabular. */
    val KeypadKey = TextStyle(fontSize = 22.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum", textDirection = AppTextDirection)
    /** History week-strip stat value: 19sp ExtraBold, tabular. */
    val StatValue = TextStyle(fontSize = 19.sp, fontWeight = FontWeight.ExtraBold, fontFeatureSettings = "tnum", textDirection = AppTextDirection)
    /** Title of a history card or detail sheet (a run, a lift session): 17sp ExtraBold. */
    val CardTitle = TextStyle(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold, textDirection = AppTextDirection)

    val Section = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp, textDirection = AppTextDirection)
    // 22sp leading for multi-line running text, trimmed above the first and below
    // the last line so a single line keeps its font height and one-line rows don't grow.
    val Body = TextStyle(
        fontSize = 16.sp,
        lineHeight = 22.sp,
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both),
        textDirection = AppTextDirection,
    )
    /** [Body] in Bold: a row title, a figure or a text action. */
    val BodyBold = Body.copy(fontWeight = FontWeight.Bold)
    // Secondary running text. Explicit 20sp line height, untrimmed: the extra
    // leading is split above the first and below the last line, so a multi-line
    // caption's final-line descenders ("goes", "you're") never touch, and get cut
    // by, a clipped row edge.
    val Caption = TextStyle(
        fontSize = 14.sp,
        lineHeight = 20.sp,
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None),
        textDirection = AppTextDirection,
    )
    /** Chip / control / short-label text (choice chips, segments, nav labels,
     *  chart legends): 13sp Bold. */
    val Chip = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, textDirection = AppTextDirection)
    /** [Chip] at Regular weight: an unselected nav label. */
    val ChipRegular = Chip.copy(fontWeight = FontWeight.Normal)

    /** Tile-scale overline (chrome §3): 12sp Bold, FieldLabel letter-spacing,
     *  UPPERCASE at the call site, color `OnBgFaint`. Top-left of every tile. */
    val Overline = TextStyle(fontSize = 12.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp, textDirection = AppTextDirection)
}

/** Material's type scale with [AppTextDirection], for the text M3 components draw
 *  themselves (button labels, text fields, nav labels) and for `Text` without a
 *  style, which reads LocalTextStyle (seeded from `bodyLarge`). */
private val AppTypography = Typography().run {
    fun TextStyle.directed() = copy(textDirection = AppTextDirection)
    Typography(
        displayLarge = displayLarge.directed(),
        displayMedium = displayMedium.directed(),
        displaySmall = displaySmall.directed(),
        headlineLarge = headlineLarge.directed(),
        headlineMedium = headlineMedium.directed(),
        headlineSmall = headlineSmall.directed(),
        titleLarge = titleLarge.directed(),
        titleMedium = titleMedium.directed(),
        titleSmall = titleSmall.directed(),
        bodyLarge = bodyLarge.directed(),
        bodyMedium = bodyMedium.directed(),
        bodySmall = bodySmall.directed(),
        labelLarge = labelLarge.directed(),
        labelMedium = labelMedium.directed(),
        labelSmall = labelSmall.directed(),
    )
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

/** Fully rounded ends: pills, discs, round touch targets. */
internal val PillShape = RoundedCornerShape(100)

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
    val dark = darkMode.isDark(isSystemInDarkTheme())
    val base = if (dark) appTheme.dark else appTheme.light
    // The other ground of the same palette: M3 draws snackbars on the inverse
    // roles, so they take this theme's opposite ground instead of M3's baseline
    // lavender.
    val inverse = if (dark) appTheme.light else appTheme.dark
    // The palette with the Material You accent when dynamic color is on (AA-guarded,
    // [withDynamicAccent]); only the accent follows the wallpaper.
    val palette = if (dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        base.withDynamicAccent((if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)).primary)
    } else {
        base
    }
    // The selected palette supplies every M3 role, so dialogs, menus and switches
    // keep it. Accent fills carry bgTop text (onPrimary). Dialogs, sheets and menus
    // draw on surfaceContainer*/surfaceVariant, not `surface`: every container tier
    // maps to the elevated ground, the lowest to the base ground. outlineVariant is
    // pinned too, M3 dividers and menus draw on it. The `*ColorScheme()` factories
    // default surfaceTint to primary; copy() does not, so it is set here.
    val scheme: ColorScheme = (if (dark) darkColorScheme() else lightColorScheme()).copy(
        primary = palette.accent,
        onPrimary = palette.bgTop,
        surfaceTint = palette.accent,
        background = palette.bgTop,
        surface = palette.bgElevated,
        onBackground = palette.onBgBody,
        onSurface = palette.onBgBody,
        surfaceVariant = palette.bgElevated,
        onSurfaceVariant = palette.onBgMuted,
        surfaceContainerLowest = palette.bgTop,
        surfaceContainerLow = palette.bgElevated,
        surfaceContainer = palette.bgElevated,
        surfaceContainerHigh = palette.bgElevated,
        surfaceContainerHighest = palette.bgElevated,
        outline = palette.onBgFaint,
        outlineVariant = palette.onBgFaint.copy(alpha = HairlineAlpha.Strong),
        inverseSurface = inverse.bgElevated,
        inverseOnSurface = inverse.onBgBody,
        inversePrimary = inverse.accent,
    )
    CompositionLocalProvider(
        LocalStatusColors provides if (dark) DarkStatusColors else StatusColors(),
        LocalPalette provides palette,
    ) {
        MaterialTheme(colorScheme = scheme, typography = AppTypography, content = content)
    }
}

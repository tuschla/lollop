package app.milestone

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Active decorative palette, provided at the theme root by [LolloTheme]. */
val LocalPalette = staticCompositionLocalOf { AppTheme.Default.light }

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

/** The active theme's [Palette.onAccent]: text and icons on the [Accent] fill. */
internal val OnAccent: Color @Composable get() = LocalPalette.current.onAccent

/** The active theme's [accentTint]: the one soft accent tint (selected nav pill,
 *  leading icon tiles, [SecondaryButton] fill, selected [OptionList] row). */
val AccentTint: Color @Composable get() = LocalPalette.current.accentTint()

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
    // Three-zone lactate model (schema.rs ThreeZone). Fixed meaning: Z1 easy
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
 * running text is never below [Caption], and no style is below 12sp.
 */
object Type {
    val Display = TextStyle(fontSize = 32.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum", textDirection = AppTextDirection)
    val Title = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold, textDirection = AppTextDirection)

    /** Title of a history card or detail sheet (a run, a lift session). */
    val CardTitle = TextStyle(fontSize = 17.sp, fontWeight = FontWeight.ExtraBold, textDirection = AppTextDirection)

    val Section = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp, textDirection = AppTextDirection)
    // Leading for multi-line running text, trimmed above the first and below the
    // last line so a single line keeps its font height and one-line rows don't grow.
    val Body = TextStyle(
        fontSize = 16.sp,
        lineHeight = 22.sp,
        lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.Both),
        textDirection = AppTextDirection,
    )
    /** [Body] in Bold: a row title, a figure or a text action. */
    val BodyBold = Body.copy(fontWeight = FontWeight.Bold)
    // Secondary running text. Explicit line height, untrimmed: the extra
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
     *  chart legends). */
    val Chip = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, textDirection = AppTextDirection)
    /** [Chip] at Regular weight: an unselected nav label. */
    val ChipRegular = Chip.copy(fontWeight = FontWeight.Normal)
    /** [Chip] at Medium weight: an unselected segment or scale value. */
    val ChipMedium = Chip.copy(fontWeight = FontWeight.Medium)

    /** Tile-scale overline (chrome §3): FieldLabel letter-spacing, UPPERCASE at
     *  the call site, color `OnBgFaint`. Top-left of every tile. */
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

/** Widest a single column of cards or a form grows on a wide window (a tablet,
 *  a landscape phone): centred beyond it, so lines stay readable. The same width
 *  as Material's bottom-sheet cap, so a sheet lines up with the page under it. */
val ContentMaxWidth = 640.dp

/** Widest one pane of a two-pane layout grows; the pair centres beyond it. */
val PaneMaxWidth = 560.dp

/** Fully rounded ends: pills, discs, round touch targets. */
internal val PillShape = RoundedCornerShape(100)

/** Outline of a [StatusCard]: the ring that carries its tone. */
val StatusRingWidth = 1.5.dp

/** Accent bar along the start edge of a list card whose entry is open in the
 *  detail pane beside the list ([PlainCard] `selected`). */
val SelectedBarWidth = Space.Sm.dp

/**
 * App theme. [appTheme] supplies the whole decorative palette (ground, surface,
 * text tones, accent); [darkMode] picks its light or dark ground. On Android 12+
 * with [dynamicColor] (the system accent), only the accent role follows the
 * user's Material You wallpaper color; the rest of the palette is still the
 * chosen theme's. Semantic status colors stay fixed across all themes so safety
 * and evidence signaling remain unambiguous; only the on-ground done and warn
 * marks lighten on the dark grounds ([DarkStatusColors]).
 */
@Composable
fun LolloTheme(
    appTheme: AppTheme,
    darkMode: DarkMode,
    dynamicColor: Boolean,
    content: @Composable () -> Unit,
) {
    // Light/dark source: respect the OS by default, or honor a forced override
    // (the "respect system dark mode" toggle). Each theme supplies both grounds;
    // only the decorative palette (and the on-ground marks) flips, never the fills.
    val dark = darkMode.isDark(isSystemInDarkTheme())
    val base = if (dark) appTheme.dark else appTheme.light
    // The other ground of the same palette: M3 draws snackbars on the inverse
    // roles, so they take this theme's opposite ground instead of M3's baseline
    // lavender.
    val inverse = if (dark) appTheme.light else appTheme.dark
    // The palette with the Material You accent when dynamic color is on (AA-guarded,
    // [withDynamicAccent]); only the accent follows the wallpaper.
    val accent = if (dynamicColor) platformAccent(dark) else null
    val palette = if (accent != null) base.withDynamicAccent(accent) else base
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

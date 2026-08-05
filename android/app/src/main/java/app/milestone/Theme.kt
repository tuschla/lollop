package app.milestone

import android.content.Context
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
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.flow.MutableStateFlow

// Subtext tuned to pair with the theme-fixed danger surface (StatusColors.danger).
// It does not vary by theme: the danger ground is identical in every theme.
internal val DangerOn = Color(0xFFFECACA) // subtext on the danger surface

/**
 * The theme-varying decorative token set: ground/surface, on-ground text tones, and
 * the one interactive accent. Selecting a theme (visual-design-system.md §9) swaps
 * this whole map. Semantic [StatusColors] (evidence/danger/warn/HR-zone) are *not*
 * here, they stay fixed across every theme so safety and evidence signaling never
 * shift meaning with a cosmetic choice.
 */
data class Palette(
    val accent: Color,
    val bgTop: Color,
    val bgElevated: Color,
    val onBgMuted: Color,
    val onBgFaint: Color,
    val onBgBody: Color,
)

/**
 * Beton (default, spec §2A): warm concrete base with almost no chroma so any color
 * reads as *information*, not decoration; a single faded signal-orange accent.
 */
internal val BetonPalette = Palette(
    accent = Color(0xFFE0733A),
    bgTop = Color(0xFF141210),
    bgElevated = Color(0xFF24211D),
    onBgMuted = Color(0xFFA7A199),
    onBgFaint = Color(0xFF9A9186),
    onBgBody = Color(0xFFE8E4DC),
)

/**
 * Werkstatt ("workshop", spec §1): warmer neutrals and denser, more visible
 * structure; a muted brass accent instead of orange. Reuses every token name.
 */
internal val WerkstattPalette = Palette(
    accent = Color(0xFFC8A24B),
    bgTop = Color(0xFF1A1611),
    bgElevated = Color(0xFF2E2820),
    onBgMuted = Color(0xFFB3A88F),
    onBgFaint = Color(0xFF998C74),
    onBgBody = Color(0xFFECE3D2),
)

/**
 * Signal (spec §1): high-contrast, near-black ground with one vivid saturated
 * accent and bolder chrome. Reuses every token name.
 */
internal val SignalPalette = Palette(
    accent = Color(0xFF38BDF8),
    bgTop = Color(0xFF0A0A0B),
    bgElevated = Color(0xFF18181B),
    onBgMuted = Color(0xFFA1A1AA),
    onBgFaint = Color(0xFF71717A),
    onBgBody = Color(0xFFFAFAFA),
)

// Light variants (design import: "Light theme added"). Same near-neutral grounds
// inverted to warm paper, with the accent darkened so it clears text contrast on a
// light surface (an accent tuned for a dark ground is too pale on paper). The
// semantic [StatusColors] stay fixed, only these decorative grounds/accent flip.
// Chosen at the theme root by system light/dark, not a separate user setting.

/** Beton, light (design import: bg #EDE7DD, surface #FBF7F0). Accent darkened
 *  #B85520 → #9A4318 so the on-accent text tone (onPrimary = bgTop #EDE7DD) clears
 *  WCAG AA on accent fills: 3.92:1 (fail) → 5.35:1 (pass). */
internal val BetonLightPalette = Palette(
    accent = Color(0xFF9A4318),
    bgTop = Color(0xFFEDE7DD),
    bgElevated = Color(0xFFFBF7F0),
    onBgMuted = Color(0xFF6E665B),
    onBgFaint = Color(0xFF8A8172),
    onBgBody = Color(0xFF221E18),
)

/** Werkstatt, light: warm paper, darkened brass accent. Accent darkened
 *  #9A7A2E → #7C5F22 so on-accent text (onPrimary = bgTop #ECE4D4) clears WCAG
 *  AA on accent fills: 3.20:1 (fail) → 4.72:1 (pass). */
internal val WerkstattLightPalette = Palette(
    accent = Color(0xFF7C5F22),
    bgTop = Color(0xFFECE4D4),
    bgElevated = Color(0xFFF7F1E3),
    onBgMuted = Color(0xFF6E6552),
    onBgFaint = Color(0xFF8A8064),
    onBgBody = Color(0xFF241E14),
)

/** Signal, light: cool near-white, darkened cyan accent. Accent darkened
 *  #0C7FB8 → #08618C so on-accent text (onPrimary = bgTop #F1F1F3) clears WCAG
 *  AA on accent fills: 3.92:1 (fail) → 6.01:1 (pass). */
internal val SignalLightPalette = Palette(
    accent = Color(0xFF08618C),
    bgTop = Color(0xFFF1F1F3),
    bgElevated = Color(0xFFFFFFFF),
    onBgMuted = Color(0xFF52525B),
    onBgFaint = Color(0xFF71717A),
    onBgBody = Color(0xFF0A0A0B),
)

/**
 * Paper, minimal monochrome (owner request 2026-07-28): white ground, black
 * foreground, no chroma at all. The accent is near-black (not a color), so the UI
 * reads as pure ink-on-paper; interactive/selected states use tone, not hue. Like
 * the others it inverts with system dark. Semantic [StatusColors] still apply, so
 * safety/evidence remain colored even in this monochrome theme.
 */
internal val PaperLightPalette = Palette(
    accent = Color(0xFF1A1A1A),
    bgTop = Color(0xFFFFFFFF),
    bgElevated = Color(0xFFF3F3F1),
    onBgMuted = Color(0xFF5C5C5A),
    onBgFaint = Color(0xFF8C8C8A),
    onBgBody = Color(0xFF0A0A0A),
)

/** Paper, dark (inverted): black ground, near-white ink accent. */
internal val PaperDarkPalette = Palette(
    accent = Color(0xFFF0F0EE),
    bgTop = Color(0xFF0A0A0A),
    bgElevated = Color(0xFF1B1B1A),
    onBgMuted = Color(0xFFA0A09E),
    onBgFaint = Color(0xFF6A6A68),
    onBgBody = Color(0xFFF5F5F3),
)

/**
 * User-selectable visual themes (visual-design-system.md §1, §9). Each carries a
 * [dark] and [light] palette; the theme root picks between them by system setting.
 */
enum class AppTheme(val label: String, val dark: Palette, val light: Palette) {
    Beton("Beton", BetonPalette, BetonLightPalette),
    Werkstatt("Werkstatt", WerkstattPalette, WerkstattLightPalette),
    Signal("Signal", SignalPalette, SignalLightPalette),
    Paper("Paper", PaperDarkPalette, PaperLightPalette);

    companion object {
        fun from(name: String?): AppTheme = entries.firstOrNull { it.name == name } ?: Beton
    }
}

/** Active decorative palette, provided at the theme root by [MilestoneTheme]. */
val LocalPalette = staticCompositionLocalOf { BetonPalette }

// Bare token accessors kept for the many call sites that read them directly. Each
// now resolves against the active [LocalPalette], so switching themes recolors the
// whole tree. These are @Composable getters, usable anywhere in composition, but
// NOT from plain (non-composable) code; read [LocalPalette].current there instead.
val Accent: Color @Composable get() = LocalPalette.current.accent
val BgTop: Color @Composable get() = LocalPalette.current.bgTop
val BgElevated: Color @Composable get() = LocalPalette.current.bgElevated
val OnBgMuted: Color @Composable get() = LocalPalette.current.onBgMuted
val OnBgFaint: Color @Composable get() = LocalPalette.current.onBgFaint
val OnBgBody: Color @Composable get() = LocalPalette.current.onBgBody

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
    val evidenceExpert: Color = Color(0xFF3730A3),
    val evidenceUnknown: Color = Color(0xFF334155),
    val warn: Color = Color(0xFF78350F),
    // Signal red (spec §2A): white text clears 5.8:1 AA on this DANGER surface.
    val danger: Color = Color(0xFFC1272D),
    val dangerStrong: Color = Color(0xFFCE2A2E),
    // Three-zone lactate model (running.rs::ThreeZone). Fixed meaning: Z1 easy
    // aerobic, Z2 threshold, Z3 hard. Never follow the system accent.
    val hrZone1: Color = Color(0xFF0E7490),
    val hrZone2: Color = Color(0xFFB45309),
    val hrZone3: Color = Color(0xFFB91C1C),
)

val LocalStatusColors = staticCompositionLocalOf { StatusColors() }

/** Tabular figures so numeric columns (pace, e1RM, confidence) don't jitter. */
val TabularFigures = TextStyle(fontFeatureSettings = "tnum")

/** Beton type scale (design spec §3). System font; the scale is what matters. */
object Type {
    val Display = TextStyle(fontSize = 32.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
    val Title = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold)
    val Section = TextStyle(fontSize = 13.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.5.sp)
    val Body = TextStyle(fontSize = 15.sp)
    val Caption = TextStyle(fontSize = 12.sp)
    val Chip = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.Bold)

    /** Tile-scale overline (chrome §3): 11sp Bold, FieldLabel letter-spacing,
     *  UPPERCASE at the call site, color `OnBgFaint`. Top-left of every tile. */
    val Overline = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.2.sp)
}

/** Beton spacing scale in dp (design spec §4). */
object Space {
    const val Xs = 2
    const val Sm = 4
    const val Md = 8
    const val Card = 14
    const val Screen = 16
    const val Lg = 24
}

/**
 * UI theme preferences persisted outside the crux core (they are shell chrome,
 * not coaching state, so they do not belong in the event log). Backed by
 * SharedPreferences; exposed as a flow so [MilestoneTheme] recomposes when
 * the user flips the toggle.
 */
/** Light/dark selection: follow the OS, or force one (owner request 2026-07-28). */
enum class DarkMode { System, Light, Dark }

object ThemeSettings {
    private const val PREFS = "theme"
    private const val KEY_DYNAMIC = "dynamic_accent"
    private const val KEY_THEME = "app_theme"
    private const val KEY_DARKMODE = "dark_mode"
    private const val KEY_PACE_BUCKET = "pace_bucket_minutes"
    private const val KEY_DISTANCE_UNIT = "distance_unit_override"

    // Dynamic (Material You) accent is OPT-IN, default OFF: first run boots
    // into Beton, the board identity, and the wallpaper accent only applies
    // once the user flips the Profile toggle (see DEVIATIONS).
    val dynamicAccent = MutableStateFlow(false)
    val theme = MutableStateFlow(AppTheme.Beton)

    // Dark-mode source: default System (respect the OS setting). Light/Dark force
    // a fixed appearance regardless of the OS, the "respect system dark mode"
    // toggle in Profile flips between System and a manual choice.
    val darkMode = MutableStateFlow(DarkMode.System)

    // Live-tracking pace-bucket size in minutes (consumed by the live run tracker):
    // the moving window over which instantaneous pace is smoothed/quantized. Default 4.
    val paceBucketMinutes = MutableStateFlow(4)

    // Distance/pace unit source: default System (defer to the device locale via
    // Units.localeDistanceUnit); Km/Mi force a fixed unit. Feeds Units.resolveDistanceUnit.
    val distanceUnitOverride = MutableStateFlow(DistanceUnitOverride.System)

    fun load(ctx: Context) {
        dynamicAccent.value = prefs(ctx).getBoolean(KEY_DYNAMIC, false)
        theme.value = AppTheme.from(prefs(ctx).getString(KEY_THEME, null))
        darkMode.value = runCatching {
            DarkMode.valueOf(prefs(ctx).getString(KEY_DARKMODE, null) ?: "System")
        }.getOrDefault(DarkMode.System)
        paceBucketMinutes.value = prefs(ctx).getInt(KEY_PACE_BUCKET, 4)
        distanceUnitOverride.value = runCatching {
            DistanceUnitOverride.valueOf(prefs(ctx).getString(KEY_DISTANCE_UNIT, null) ?: "System")
        }.getOrDefault(DistanceUnitOverride.System)
    }

    fun setDynamicAccent(ctx: Context, enabled: Boolean) {
        dynamicAccent.value = enabled
        prefs(ctx).edit().putBoolean(KEY_DYNAMIC, enabled).apply()
    }

    fun setTheme(ctx: Context, selected: AppTheme) {
        theme.value = selected
        prefs(ctx).edit().putString(KEY_THEME, selected.name).apply()
    }

    fun setDarkMode(ctx: Context, mode: DarkMode) {
        darkMode.value = mode
        prefs(ctx).edit().putString(KEY_DARKMODE, mode.name).apply()
    }

    fun setPaceBucketMinutes(ctx: Context, n: Int) {
        paceBucketMinutes.value = n
        prefs(ctx).edit().putInt(KEY_PACE_BUCKET, n).apply()
    }

    fun setDistanceUnitOverride(ctx: Context, override: DistanceUnitOverride) {
        distanceUnitOverride.value = override
        prefs(ctx).edit().putString(KEY_DISTANCE_UNIT, override.name).apply()
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
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
 * app.rs `RunResultView.zone` ("Z1"/"Z2"/"Z3"); unknown ("-") falls back to the
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
 * themes so safety and evidence signaling remain unambiguous.
 */
@Composable
fun MilestoneTheme(
    content: @Composable () -> Unit,
) {
    val ctx = LocalContext.current
    val dynamicColor by ThemeSettings.dynamicAccent.collectAsState()
    val appTheme by ThemeSettings.theme.collectAsState()
    val darkMode by ThemeSettings.darkMode.collectAsState()
    // Light/dark source: respect the OS by default, or honor a forced override
    // (owner "respect system dark mode" toggle). Each theme supplies both grounds;
    // only the decorative palette flips, never the fixed semantic status colors.
    val dark = when (darkMode) {
        DarkMode.System -> isSystemInDarkTheme()
        DarkMode.Light -> false
        DarkMode.Dark -> true
    }
    val base = if (dark) appTheme.dark else appTheme.light
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
            // Owner ruling / spec "Accent fill + BgTop text": M3 Buttons read
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
            outlineVariant = base.onBgFaint.copy(alpha = 0.3f),
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
            outlineVariant = base.onBgFaint.copy(alpha = 0.3f),
        )
    }
    val scheme: ColorScheme = if (dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        val dynamic = if (dark) dynamicDarkColorScheme(ctx) else dynamicLightColorScheme(ctx)
        themed.copy(primary = dynamic.primary, onPrimary = dynamic.onPrimary)
    } else {
        themed
    }
    // Accent follows the Material You primary when dynamic color is on; the rest of
    // the palette (grounds, text) is always the selected theme's.
    val palette = base.copy(accent = scheme.primary)
    CompositionLocalProvider(
        LocalStatusColors provides StatusColors(),
        LocalPalette provides palette,
    ) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}

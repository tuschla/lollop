package app.milestone

import android.content.Context
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow

/** Light/dark selection: follow the OS, or force one. */
enum class DarkMode { System, Light, Dark }

/**
 * Shell preferences persisted outside the core: appearance (theme, dark mode,
 * dynamic accent), units, route maps, the tracker's pace bucket and keep-screen-on,
 * and the rest alert. They are shell chrome, not coaching state, so they do not
 * belong in the event log. Backed by SharedPreferences; each one is exposed as a
 * flow so composables recompose when the user flips it.
 */
object AppSettings {
    private const val PREFS = "theme"
    private const val KEY_DYNAMIC = "dynamic_accent"
    private const val KEY_THEME = "app_theme"
    private const val KEY_DARKMODE = "dark_mode"
    private const val KEY_PACE_BUCKET = "pace_bucket_minutes"
    private const val KEY_DISTANCE_UNIT = "distance_unit_override"
    private const val KEY_SHOW_MAPS = "show_route_maps"
    private const val KEY_KEEP_SCREEN_ON = "keep_screen_on_tracking"
    private const val KEY_REST_ALERT = "rest_alert"

    /** The core's event log (Core.restore): present = an install already in use. */
    private const val EVENT_LOG = "event-log.ndjson"

    /** Default live-tracking pace bucket, in minutes. */
    private const val DEFAULT_PACE_BUCKET_MINUTES = 4

    // A fresh install boots into Paper with the dynamic (Material You) accent ON;
    // without dynamic colour (API < 31) Paper keeps its own ink accent.
    // [setTheme] couples the toggle to the theme choice.
    val dynamicAccent = MutableStateFlow(true)
    val theme = MutableStateFlow(AppTheme.Paper)

    // Dark-mode source: default System (respect the OS setting). Light/Dark force
    // a fixed appearance regardless of the OS, the "respect system dark mode"
    // toggle in Profile flips between System and a manual choice.
    val darkMode = MutableStateFlow(DarkMode.System)

    // Live-tracking pace-bucket size in minutes (consumed by the live run tracker):
    // the moving window over which instantaneous pace is smoothed/quantized.
    val paceBucketMinutes = MutableStateFlow(DEFAULT_PACE_BUCKET_MINUTES)

    // Distance/pace unit source: default System (defer to the device locale via
    // Units.localeDistanceUnit); Km/Mi force a fixed unit. Feeds Units.resolveDistanceUnit.
    val distanceUnitOverride = MutableStateFlow(DistanceUnitOverride.System)

    // Route maps (OSM tiles) are the app's only network egress; ON by default,
    // the Profile toggle flips it. When off, no MapView is attached or drawn, so
    // no tile request is made (RunMap/RunTracking gate on this).
    val showRouteMaps = MutableStateFlow(true)

    // Keep the screen awake on the run tracker while recording (a mounted or
    // handheld phone). Default OFF: an always-on screen costs battery.
    val keepScreenOn = MutableStateFlow(false)

    // Rest-timer alert: when the rest ends, the set loop buzzes and the rest-done
    // notification plays the channel's sound/vibration. Off = a silent cue only.
    val restAlert = MutableStateFlow(true)

    fun load(ctx: Context) {
        val prefs = prefs(ctx)
        if (!prefs.contains(KEY_THEME)) pinFirstTheme(ctx)
        dynamicAccent.value = prefs.getBoolean(KEY_DYNAMIC, false)
        theme.value = AppTheme.from(prefs.getString(KEY_THEME, null))
        darkMode.value = runCatching {
            DarkMode.valueOf(prefs.getString(KEY_DARKMODE, null) ?: DarkMode.System.name)
        }.getOrDefault(DarkMode.System)
        paceBucketMinutes.value = prefs.getInt(KEY_PACE_BUCKET, DEFAULT_PACE_BUCKET_MINUTES)
        distanceUnitOverride.value = runCatching {
            DistanceUnitOverride.valueOf(prefs.getString(KEY_DISTANCE_UNIT, null) ?: DistanceUnitOverride.System.name)
        }.getOrDefault(DistanceUnitOverride.System)
        showRouteMaps.value = prefs.getBoolean(KEY_SHOW_MAPS, true)
        keepScreenOn.value = prefs.getBoolean(KEY_KEEP_SCREEN_ON, false)
        restAlert.value = prefs.getBoolean(KEY_REST_ALERT, true)
    }

    /**
     * Persist the appearance an install starts from, once. A fresh install (no
     * event log yet) gets Paper with the system accent; an install already in use
     * from before Paper became the default, that never saved a theme, keeps what
     * it rendered: Rose with its saved (or the old default-off) accent.
     */
    private fun pinFirstTheme(ctx: Context) {
        val prefs = prefs(ctx)
        val existingInstall = File(ctx.filesDir, EVENT_LOG).exists()
        prefs.edit()
            .putString(KEY_THEME, (if (existingInstall) AppTheme.Rose else AppTheme.Paper).name)
            .putBoolean(KEY_DYNAMIC, prefs.getBoolean(KEY_DYNAMIC, !existingInstall))
            .apply()
    }

    fun setDynamicAccent(ctx: Context, enabled: Boolean) {
        dynamicAccent.value = enabled
        prefs(ctx).edit().putBoolean(KEY_DYNAMIC, enabled).apply()
    }

    /**
     * Select [selected]. Changing theme also sets the system accent: ON for Paper
     * (its ink accent is a blank slate for the wallpaper colour), OFF for Rose,
     * Meadow and Iris (a wallpaper accent clashes with their tinted grounds). The
     * user can still flip the toggle afterwards; re-picking the current theme
     * leaves it alone.
     */
    fun setTheme(ctx: Context, selected: AppTheme) {
        val prefs = prefs(ctx)
        val edit = prefs.edit().putString(KEY_THEME, selected.name)
        if (prefs.getString(KEY_THEME, null) != selected.name) {
            val dynamic = selected == AppTheme.Paper
            dynamicAccent.value = dynamic
            edit.putBoolean(KEY_DYNAMIC, dynamic)
        }
        theme.value = selected
        edit.apply()
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

    fun setShowRouteMaps(ctx: Context, enabled: Boolean) {
        showRouteMaps.value = enabled
        prefs(ctx).edit().putBoolean(KEY_SHOW_MAPS, enabled).apply()
    }

    fun setKeepScreenOn(ctx: Context, enabled: Boolean) {
        keepScreenOn.value = enabled
        prefs(ctx).edit().putBoolean(KEY_KEEP_SCREEN_ON, enabled).apply()
    }

    fun setRestAlert(ctx: Context, enabled: Boolean) {
        restAlert.value = enabled
        prefs(ctx).edit().putBoolean(KEY_REST_ALERT, enabled).apply()
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

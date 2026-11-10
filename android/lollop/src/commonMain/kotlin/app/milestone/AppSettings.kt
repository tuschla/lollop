package app.milestone

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** Default live-tracking pace bucket, in minutes (AppSettings and RunSession). */
internal const val DEFAULT_PACE_BUCKET_MINUTES = 4

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
 * Shell preferences persisted outside the core: appearance (theme, dark mode,
 * dynamic accent), units, route maps, the tracker's pace bucket and keep-screen-on,
 * and the rest alert. They are shell chrome, not coaching state, so they do not
 * belong in the event log. Backed by a [KeyValueStore] (SharedPreferences on
 * Android, NSUserDefaults on iOS); each one is exposed as a
 * flow so composables recompose when the user flips it. [AppRuntime.start] loads
 * them; the setters persist and publish together.
 */
object AppSettings {
    /** One preference: its key and the value an install without it reads. */
    private class Pref<T>(
        val key: String,
        val default: T,
        val read: KeyValueStore.(key: String, default: T) -> T,
        val write: KeyValueStore.Editor.(key: String, value: T) -> Unit,
    )

    private fun bool(key: String, default: Boolean) =
        Pref(key, default, { k, d -> getBoolean(k, d) }, { k, v -> putBoolean(k, v) })

    private fun int(key: String, default: Int) = Pref(key, default, { k, d -> getInt(k, d) }, { k, v -> putInt(k, v) })

    private fun <E : Enum<E>> enum(key: String, default: E, parse: (String) -> E?) = Pref(
        key,
        default,
        { k, d ->
            val stored = getString(k, null)
            stored?.let(parse) ?: d.also { if (stored != null) PlatformLog.w("AppSettings", "unknown $k \"$stored\", using $d") }
        },
        { k, v -> putString(k, v.name) },
    )

    // The defaults table. A fresh install pins its first theme ([pinFirstTheme]).
    private val DYNAMIC = bool("dynamic_accent", true)
    private val THEME = Pref("app_theme", AppTheme.Paper, { k, _ -> AppTheme.from(getString(k, null)) }, { k, v -> putString(k, v.name) })
    private val DARK_MODE = enum("dark_mode", DarkMode.System) { n -> DarkMode.entries.firstOrNull { it.name == n } }
    private val PACE_BUCKET = int("pace_bucket_minutes", DEFAULT_PACE_BUCKET_MINUTES)
    private val DISTANCE_UNIT = enum("distance_unit_override", DistanceUnitOverride.System) { n -> DistanceUnitOverride.entries.firstOrNull { it.name == n } }

    // Route maps (OSM tiles) are the app's only network egress; when off, no
    // MapView is attached or drawn, so no tile request is made.
    private val SHOW_MAPS = bool("show_route_maps", true)

    // Keep the screen awake on the run tracker while recording; an always-on
    // screen costs battery.
    private val KEEP_SCREEN_ON = bool("keep_screen_on_tracking", false)

    // When the rest ends, the set loop buzzes and the rest-done notification
    // plays the channel's sound/vibration. Off = a silent cue only.
    private val REST_ALERT = bool("rest_alert", true)

    private class Values {
        val dynamicAccent = MutableStateFlow(DYNAMIC.default)
        val theme = MutableStateFlow(THEME.default)
        val darkMode = MutableStateFlow(DARK_MODE.default)
        val paceBucketMinutes = MutableStateFlow(PACE_BUCKET.default)
        val distanceUnitOverride = MutableStateFlow(DISTANCE_UNIT.default)
        val showRouteMaps = MutableStateFlow(SHOW_MAPS.default)
        val keepScreenOn = MutableStateFlow(KEEP_SCREEN_ON.default)
        val restAlert = MutableStateFlow(REST_ALERT.default)
    }

    private val values by ProcessLocal { Values() }

    /** The wallpaper (Material You) accent; [setTheme] couples it to the theme. */
    val dynamicAccent: StateFlow<Boolean> get() = values.dynamicAccent
    val theme: StateFlow<AppTheme> get() = values.theme

    /** System follows the OS; Light/Dark force one appearance. */
    val darkMode: StateFlow<DarkMode> get() = values.darkMode

    /** The live tracker's pace window, in minutes. */
    val paceBucketMinutes: StateFlow<Int> get() = values.paceBucketMinutes

    /** System defers to the locale ([resolveDistanceUnit]); Km/Mi force one. */
    val distanceUnitOverride: StateFlow<DistanceUnitOverride> get() = values.distanceUnitOverride
    val showRouteMaps: StateFlow<Boolean> get() = values.showRouteMaps
    val keepScreenOn: StateFlow<Boolean> get() = values.keepScreenOn
    val restAlert: StateFlow<Boolean> get() = values.restAlert

    internal fun load(ctx: PlatformContext) {
        val prefs = prefs(ctx)
        if (!prefs.contains(THEME.key)) pinFirstTheme(ctx)
        val v = values
        v.dynamicAccent.value = prefs.read(DYNAMIC)
        v.theme.value = prefs.read(THEME)
        v.darkMode.value = prefs.read(DARK_MODE)
        v.paceBucketMinutes.value = prefs.read(PACE_BUCKET)
        v.distanceUnitOverride.value = prefs.read(DISTANCE_UNIT)
        v.showRouteMaps.value = prefs.read(SHOW_MAPS)
        v.keepScreenOn.value = prefs.read(KEEP_SCREEN_ON)
        v.restAlert.value = prefs.read(REST_ALERT)
    }

    private fun <T> KeyValueStore.read(pref: Pref<T>): T = pref.read(this, pref.key, pref.default)

    /**
     * Persist the appearance an install starts from, once. A fresh install (no
     * event log yet) gets Paper with the system accent; an install already in use
     * from before Paper became the default, that never saved a theme, keeps what
     * it rendered: Rose with its saved (or the old default-off) accent.
     */
    private fun pinFirstTheme(ctx: PlatformContext) {
        val prefs = prefs(ctx)
        val existingInstall = ctx.hasEventLog()
        prefs.edit {
            putString(THEME.key, (if (existingInstall) AppTheme.Rose else AppTheme.Paper).name)
            putBoolean(DYNAMIC.key, prefs.getBoolean(DYNAMIC.key, !existingInstall))
        }
    }

    /** Publish [value] on [flow] and persist it as [pref]. */
    private fun <T> set(ctx: PlatformContext, pref: Pref<T>, flow: MutableStateFlow<T>, value: T) {
        flow.value = value
        prefs(ctx).edit { pref.write(this, pref.key, value) }
    }

    fun setDynamicAccent(ctx: PlatformContext, enabled: Boolean) = set(ctx, DYNAMIC, values.dynamicAccent, enabled)

    /**
     * Select [selected]. Changing theme also sets the system accent to the theme's
     * [AppTheme.takesSystemAccent]. The user can still flip the toggle afterwards;
     * re-picking the current theme leaves it alone.
     */
    fun setTheme(ctx: PlatformContext, selected: AppTheme) {
        val prefs = prefs(ctx)
        val changed = prefs.getString(THEME.key, null) != selected.name
        val dynamic = selected.takesSystemAccent
        val v = values
        if (changed) v.dynamicAccent.value = dynamic
        v.theme.value = selected
        prefs.edit {
            putString(THEME.key, selected.name)
            if (changed) putBoolean(DYNAMIC.key, dynamic)
        }
    }

    fun setDarkMode(ctx: PlatformContext, mode: DarkMode) = set(ctx, DARK_MODE, values.darkMode, mode)

    fun setPaceBucketMinutes(ctx: PlatformContext, n: Int) = set(ctx, PACE_BUCKET, values.paceBucketMinutes, n)

    fun setDistanceUnitOverride(ctx: PlatformContext, override: DistanceUnitOverride) =
        set(ctx, DISTANCE_UNIT, values.distanceUnitOverride, override)

    fun setShowRouteMaps(ctx: PlatformContext, enabled: Boolean) = set(ctx, SHOW_MAPS, values.showRouteMaps, enabled)

    fun setKeepScreenOn(ctx: PlatformContext, enabled: Boolean) = set(ctx, KEEP_SCREEN_ON, values.keepScreenOn, enabled)

    fun setRestAlert(ctx: PlatformContext, enabled: Boolean) = set(ctx, REST_ALERT, values.restAlert, enabled)

    private fun prefs(ctx: PlatformContext) = ctx.keyValueStore(AppFileNames.SETTINGS_STORE)
}

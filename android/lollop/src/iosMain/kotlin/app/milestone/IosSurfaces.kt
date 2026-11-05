package app.milestone

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import app.milestone.resources.Res
import app.milestone.resources.live_chip_locating
import app.milestone.resources.live_chip_no_gps
import app.milestone.resources.live_chip_paused
import app.milestone.resources.live_chip_waiting
import app.milestone.resources.log_rest_notif_done_title
import app.milestone.resources.log_rest_notif_title
import app.milestone.resources.run_stat_distance
import app.milestone.resources.track_action_pause
import app.milestone.resources.track_action_resume
import app.milestone.resources.track_btn_stop
import app.milestone.resources.track_notif_armed_text
import app.milestone.resources.track_notif_armed_title
import app.milestone.resources.track_notif_locate_text
import app.milestone.resources.track_notif_locate_title
import app.milestone.resources.track_notif_paused_title
import app.milestone.resources.track_notif_recording_hint
import app.milestone.resources.track_notif_recording_title
import app.milestone.resources.track_notif_signal_lost
import app.milestone.resources.track_stat_elapsed
import app.milestone.resources.track_stat_pace_now
import app.milestone.resources.track_state_paused
import app.milestone.resources.widget_more_items
import app.milestone.widget.WidgetFace
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetTone
import app.milestone.widget.widgetFace
import app.milestone.widget.widgetSnapshotOf
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.launch
import kotlinx.datetime.LocalDate
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSOperationQueue
import platform.Foundation.NSSystemTimeZoneDidChangeNotification
import platform.UIKit.UIApplicationSignificantTimeChangeNotification

// The app's surfaces outside its window on iOS, drawn by the Swift widget
// extension (iosApp/LollopWidgets): the home and lock screen widgets and the
// Control Center control (Android: the Glance widget and the Quick Settings tile),
// and the Live Activities of a run and a rest (Android: the Live Update
// notifications). The extension runs no Kotlin and has no strings of its own:
// the app resolves every text here, from the same common code and resources as
// Android, and hands it over as JSON. The Swift side (`SurfaceHost` in the
// `Lollop` framework) stores the widget payload in the App Group container and
// starts, updates and ends the activities.

/** The Swift side of the system surfaces, installed by [IosSurfaces.install]. Main thread. */
public interface IosSurfaceHost {
    /** Store the widget payload ([IosWidgetPayload] JSON) where the widget
     *  extension reads it, and reload the widgets and controls. */
    public fun publishWidget(json: String)

    /** Start or update the run's Live Activity ([IosRunLive] JSON). */
    public fun updateRunActivity(json: String)

    /** End the run's Live Activity. */
    public fun endRunActivity()

    /** Start or update the rest's Live Activity ([IosRestLive] JSON). */
    public fun updateRestActivity(json: String)

    /** End the rest's Live Activity. */
    public fun endRestActivity()
}

/** Where the app's surfaces outside its window go. Main thread. */
public object IosSurfaces {
    private var host: IosSurfaceHost? = null

    /**
     * Hand the surfaces to [host]: the current widget payload, and the running
     * run's and rest's activities, at once; every change after.
     */
    public fun install(host: IosSurfaceHost) {
        this.host = host
        RunActivity.sink = HostRunSink
        IosWidgetSync.start()
        IosWidgetSync.republish()
        RestAlarm.resendActivity()
    }

    internal fun publishWidget(json: String) {
        host?.publishWidget(json)
    }

    /** Show [run] (and the [next] set) in the rest's activity. */
    internal fun showRest(run: RestRun, next: RestNextSet?) {
        val host = host ?: return
        host.updateRestActivity(surfaceJson.encodeToString(IosRestLive.serializer(), restLiveOf(run, next, systemResources())))
    }

    internal fun endRest() {
        host?.endRestActivity()
    }

    private object HostRunSink : RunActivitySink {
        override fun update(state: RunActivityState) {
            val host = host ?: return
            host.updateRunActivity(surfaceJson.encodeToString(IosRunLive.serializer(), runLiveOf(state, systemResources())))
        }

        override fun end() {
            host?.endRunActivity()
        }
    }
}

internal val surfaceJson = Json { encodeDefaults = true }

// ── Widget and control ──────────────────────────────────────────────────────

/** Bumped on any change an older widget extension would misread. */
internal const val IOS_WIDGET_PAYLOAD_VERSION = 1

/**
 * What the widgets and the control show, written on every change by [IosWidgetSync]
 * (Android: `widget-today.json` and what the Glance widget resolves from it).
 * [snapshot] is the common day snapshot; [face] is the widget for its day and
 * [staleFace] the one for any other day ("Open lollop to update"), which the
 * extension shows from local midnight, after a time zone change ([WidgetSnapshot.zone])
 * or when [WidgetSnapshot.stale]. [moreItems] holds "N more items" for N = 1, 2, …
 * (index N - 1) for the extension's line fitting (`visibleLines`). [control] and
 * [staleControl] are the Control Center control's faces for the same days.
 */
@Serializable
internal data class IosWidgetPayload(
    val version: Int = IOS_WIDGET_PAYLOAD_VERSION,
    val snapshot: WidgetSnapshot,
    val recording: Boolean,
    val face: IosWidgetFace,
    val staleFace: IosWidgetFace,
    val moreItems: List<String>,
    val control: IosControlFace,
    val staleControl: IosControlFace,
    val palette: IosWidgetPalette,
)

/** [WidgetFace] for the extension; [date] is the short date alone, the overline of a narrow card. */
@Serializable
internal data class IosWidgetFace(
    val overline: String,
    val date: String?,
    val title: String,
    /** normal | paused | muted ([WidgetTone]). */
    val tone: String,
    val status: String?,
    val brief: String?,
    val lead: List<String>,
    val items: List<String>,
    val hero: IosWidgetHero?,
    val heroLead: List<String>,
    val heroItems: List<String>,
    val week: String?,
    val days: List<IosWidgetDay>,
    val actions: List<IosWidgetAction>,
)

@Serializable
internal data class IosWidgetHero(val text: String, val note: String?, val figure: Boolean)

/** A week strip day: its two-letter weekday, its mark (done | next | planned |
 *  missed | rest | none) and whether it is the face's day. */
@Serializable
internal data class IosWidgetDay(val label: String, val mark: String, val today: Boolean)

/** A widget button: its label, the `lollop://open/<dest>` destination and the
 *  tile face whose icon it shows (recording | start_run | log_set | log). */
@Serializable
internal data class IosWidgetAction(val label: String, val dest: String, val face: String)

/** The Control Center control: Android's Quick Settings tile face. */
@Serializable
internal data class IosControlFace(val label: String, val subtitle: String?, val dest: String, val face: String, val active: Boolean)

/** The theme's colours as the widget uses them ("#AARRGGBB"), light and dark;
 *  [mode] is the app's dark mode choice (system | light | dark). */
@Serializable
internal data class IosWidgetPalette(val mode: String, val light: IosWidgetColors, val dark: IosWidgetColors)

@Serializable
internal data class IosWidgetColors(
    val ground: String,
    val body: String,
    val muted: String,
    val faint: String,
    val accent: String,
    val onAccent: String,
    val accentTint: String,
    val danger: String,
    val done: String,
)

/** The face's wire name. */
private val TileFace.wire: String
    get() = when (this) {
        TileFace.Recording -> "recording"
        TileFace.StartRun -> "start_run"
        TileFace.LogSet -> "log_set"
        TileFace.Log -> "log"
    }

internal fun Color.hex(): String = "#" + toArgb().toUInt().toString(16).padStart(8, '0').uppercase()

private fun colorsOf(p: Palette, done: Color) = IosWidgetColors(
    ground = p.bgTop.hex(),
    body = p.onBgBody.hex(),
    muted = p.onBgMuted.hex(),
    faint = p.onBgFaint.hex(),
    accent = p.accent.hex(),
    onAccent = p.bgTop.hex(),
    accentTint = p.accentTint().hex(),
    danger = p.dangerText.hex(),
    done = done.hex(),
)

/** [theme]'s widget colours, as Android's `widgetColors` (iOS has no system accent). */
internal fun widgetPaletteOf(theme: AppTheme, darkMode: DarkMode) = IosWidgetPalette(
    mode = darkMode.name.lowercase(),
    light = colorsOf(theme.light, StatusColors().done),
    dark = colorsOf(theme.dark, DarkStatusColors.done),
)

private fun iosFaceOf(face: WidgetFace): IosWidgetFace = IosWidgetFace(
    overline = face.overline,
    date = face.day?.let { planDateLabel(it) },
    title = face.title,
    tone = when (face.tone) {
        WidgetTone.Normal -> "normal"
        WidgetTone.Paused -> "paused"
        WidgetTone.Muted -> "muted"
    },
    status = face.status,
    brief = face.brief,
    lead = face.lead,
    items = face.items,
    hero = face.hero?.let { IosWidgetHero(it.text, it.note, it.figure) },
    heroLead = face.heroLead,
    heroItems = face.heroItems,
    week = face.week,
    days = face.days.map { d ->
        IosWidgetDay(
            label = weekdayName(LocalDate.fromEpochDays(d.epochDay).dayOfWeek, WeekdayWidth.ShortStandalone, appLocale()).take(2),
            mark = d.mark.name.lowercase(),
            today = d.epochDay == face.day,
        )
    },
    actions = face.actions.map { IosWidgetAction(it.label, it.dest.wire, it.face.wire) },
)

private fun controlOf(today: WidgetSnapshot?, recording: Boolean, res: PlatformResources): IosControlFace {
    val face = if (recording) TileFace.Recording else tileFace(today)
    return IosControlFace(res.getString(face.label), tileSubtitle(today, res), face.dest.wire, face.wire, face.active)
}

/**
 * The widget payload for [snapshot] while a run records ([recording]) or not, in
 * [theme] and [darkMode]. The faces are the common [widgetFace]s, so the widget
 * says what Android's says.
 */
internal fun iosWidgetPayload(
    snapshot: WidgetSnapshot,
    recording: Boolean,
    theme: AppTheme,
    darkMode: DarkMode,
    res: PlatformResources,
): IosWidgetPayload {
    val face = widgetFace(snapshot, snapshot.epochDay, res, recording)
    val current = snapshot.takeIf { !it.stale }
    val most = maxOf(face.items.size, face.heroItems.size, 1)
    return IosWidgetPayload(
        snapshot = snapshot,
        recording = recording,
        face = iosFaceOf(face),
        staleFace = iosFaceOf(widgetFace(null, snapshot.epochDay, res, recording)),
        moreItems = (1..most).map { n -> res.getQuantityString(Res.plurals.widget_more_items, n, n) },
        control = controlOf(current, recording, res),
        staleControl = controlOf(null, recording, res),
        palette = widgetPaletteOf(theme, darkMode),
    )
}

/**
 * Keeps the widget payload in step with the app, as Android's `WidgetSync`: every
 * published model, distance unit, theme, dark mode and recording change makes a
 * payload, handed to the Swift side when it differs from the last. A time zone or
 * significant time change derives it again (the snapshot records the zone it was
 * written in). Built on the main thread, as the notifications build their text.
 * Process-scoped; started by [IosSurfaces.install].
 */
internal object IosWidgetSync {
    private var started = false
    private var last: String? = null
    private val timeChanges = MutableStateFlow(0L)

    fun start() {
        if (started) return
        started = true
        for (name in listOf(NSSystemTimeZoneDidChangeNotification, UIApplicationSignificantTimeChangeNotification)) {
            NSNotificationCenter.defaultCenter.addObserverForName(name, `object` = null, queue = NSOperationQueue.mainQueue) { _ ->
                timeChanges.value += 1
            }
        }
        val look = combine(AppSettings.theme, AppSettings.darkMode, RunSession.tracking) { t, d, r -> Triple(t, d, r) }
        EventDispatcher.scope.launch {
            combine(EventDispatcher.model, AppSettings.distanceUnitOverride, look, timeChanges) { model, unit, (theme, dark, recording), _ ->
                model?.let {
                    surfaceJson.encodeToString(
                        IosWidgetPayload.serializer(),
                        iosWidgetPayload(widgetSnapshotOf(it, resolveDistanceUnit(unit)), recording, theme, dark, systemResources()),
                    )
                }
            }
                .filterNotNull()
                .distinctUntilChanged()
                .collect { json ->
                    last = json
                    IosSurfaces.publishWidget(json)
                }
        }
    }

    /** Hand the last payload over again (a new host). */
    fun republish() {
        last?.let(IosSurfaces::publishWidget)
    }
}

// ── Live Activities ─────────────────────────────────────────────────────────

/**
 * The run's Live Activity (Android's run notification and its chip): [phase]
 * (locating | armed | recording | paused | signal_lost), the notification's
 * [title], the chip ([chip]: a state word, or the moving time while recording),
 * [text] in place of the figures where Android's card shows a fixed line, a
 * guided run's [step] line, the moving time ([movingSec], [time]), [distance] and
 * the current [pace] (null where Android's line has none), the stat labels and the
 * button labels. [canPause]: Pause/Resume shows (a recording run). [accentLight] /
 * [accentDark]: the theme's accent ("#AARRGGBB").
 */
@Serializable
internal data class IosRunLive(
    val kind: String = "run",
    val phase: String,
    val title: String,
    val chip: String,
    val text: String?,
    val step: String?,
    val movingSec: Long,
    val time: String,
    val distance: String?,
    val pace: String?,
    val movingLabel: String,
    val distanceLabel: String,
    val paceLabel: String,
    val canPause: Boolean,
    val paused: Boolean,
    val pauseLabel: String,
    val resumeLabel: String,
    val stopLabel: String,
    val accentLight: String,
    val accentDark: String,
)

/**
 * The rest's Live Activity (Android's countdown notification): "Resting ·
 * <exercise>", the countdown from [startMs] to [endMs] (wall clock, epoch ms),
 * the next set's line ([next], "Next: Set 3 of 4 · 100 kg × 5") and [doneTitle]
 * for a countdown that ran out while the app could not end it.
 */
@Serializable
internal data class IosRestLive(
    val kind: String = "rest",
    val title: String,
    val next: String?,
    val startMs: Long,
    val endMs: Long,
    val doneTitle: String,
    val accentLight: String,
    val accentDark: String,
)

/** [state] as its activity shows it: the same titles, chip and line as Android's
 *  run notification (RunTrackingService.kt: `trackingNotifTitleRes`,
 *  `trackingChipRes`, `trackingNotifContentSpec`). */
internal fun runLiveOf(state: RunActivityState, res: PlatformResources): IosRunLive {
    val unit = if (state.miles) DistanceUnit.Mi else DistanceUnit.Km
    val time = formatElapsed(state.movingSec)
    val phase = state.phase
    val live = phase == RunActivityPhase.Recording || phase == RunActivityPhase.Paused || phase == RunActivityPhase.SignalLost
    val figures = live && state.hasTrack && phase != RunActivityPhase.SignalLost
    val theme = AppSettings.theme.value
    return IosRunLive(
        phase = when (phase) {
            RunActivityPhase.Locating -> "locating"
            RunActivityPhase.Armed -> "armed"
            RunActivityPhase.Recording -> "recording"
            RunActivityPhase.Paused -> "paused"
            RunActivityPhase.SignalLost -> "signal_lost"
        },
        title = res.getString(
            when (phase) {
                RunActivityPhase.Locating -> Res.string.track_notif_locate_title
                RunActivityPhase.Armed -> Res.string.track_notif_armed_title
                RunActivityPhase.Recording, RunActivityPhase.SignalLost -> Res.string.track_notif_recording_title
                RunActivityPhase.Paused -> Res.string.track_notif_paused_title
            },
        ),
        chip = when (phase) {
            RunActivityPhase.Locating -> res.getString(Res.string.live_chip_locating)
            RunActivityPhase.Armed -> res.getString(Res.string.live_chip_waiting)
            RunActivityPhase.Recording -> time
            RunActivityPhase.Paused -> res.getString(Res.string.live_chip_paused)
            RunActivityPhase.SignalLost -> res.getString(Res.string.live_chip_no_gps)
        },
        text = when {
            phase == RunActivityPhase.Locating -> res.getString(Res.string.track_notif_locate_text)
            phase == RunActivityPhase.Armed -> res.getString(Res.string.track_notif_armed_text)
            phase == RunActivityPhase.SignalLost -> res.getString(Res.string.track_notif_signal_lost)
            figures -> null
            phase == RunActivityPhase.Paused -> res.getString(Res.string.track_state_paused)
            else -> res.getString(Res.string.track_notif_recording_hint)
        },
        step = state.step?.takeIf { phase == RunActivityPhase.Recording }?.let { runStepLine(res, it) },
        movingSec = state.movingSec,
        time = time,
        distance = if (figures) runDistLabel(state.distanceKm, unit) else null,
        pace = state.paceMinPerKm.takeIf { figures && phase == RunActivityPhase.Recording && it.isFinite() }
            ?.let { "${formatPaceMinutes(paceInUnit(it, unit))} ${unit.paceSuffix}" },
        movingLabel = res.getString(Res.string.track_stat_elapsed),
        distanceLabel = res.getString(Res.string.run_stat_distance),
        paceLabel = res.getString(Res.string.track_stat_pace_now),
        canPause = live,
        paused = phase == RunActivityPhase.Paused,
        pauseLabel = res.getString(Res.string.track_action_pause),
        resumeLabel = res.getString(Res.string.track_action_resume),
        stopLabel = res.getString(Res.string.track_btn_stop),
        accentLight = theme.light.accent.hex(),
        accentDark = theme.dark.accent.hex(),
    )
}

/** [run] and the [next] set as the rest's activity shows them (Android's
 *  countdown notification: `log_rest_notif_title`, [countdownText]). */
internal fun restLiveOf(run: RestRun, next: RestNextSet?, res: PlatformResources): IosRestLive {
    val theme = AppSettings.theme.value
    return IosRestLive(
        title = res.getString(Res.string.log_rest_notif_title, run.exercise),
        next = countdownText(res, run.exercise, next),
        startMs = run.startMillis,
        endMs = run.endAtMillis,
        doneTitle = res.getString(Res.string.log_rest_notif_done_title),
        accentLight = theme.light.accent.hex(),
        accentDark = theme.dark.accent.hex(),
    )
}

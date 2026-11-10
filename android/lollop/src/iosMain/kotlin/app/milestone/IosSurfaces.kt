package app.milestone

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import app.milestone.resources.Res
import app.milestone.resources.log_rest_notif_done_title
import app.milestone.resources.log_rest_notif_title
import app.milestone.resources.run_stat_distance
import app.milestone.resources.track_action_pause
import app.milestone.resources.track_action_resume
import app.milestone.resources.track_btn_stop
import app.milestone.resources.track_stat_elapsed
import app.milestone.resources.track_stat_pace_now
import app.milestone.widget.IosWidgetSurface
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

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
    /** Store the widget payload ([app.milestone.widget.IosWidgetPayload] JSON) where the widget
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
        IosWidgetSurface.republish()
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
        override fun update(state: RunSurfaceState) {
            val host = host ?: return
            host.updateRunActivity(surfaceJson.encodeToString(IosRunLive.serializer(), runLiveOf(state, systemResources())))
        }

        override fun end() {
            host?.endRunActivity()
        }
    }
}

internal val surfaceJson = Json { encodeDefaults = true }

internal fun Color.hex(): String = "#" + toArgb().toUInt().toString(16).padStart(8, '0').uppercase()

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

/** [state] as its activity shows it: the text of Android's run notification
 *  ([runSurfaceText]) split into the activity's tiles. */
internal fun runLiveOf(state: RunSurfaceState, res: PlatformResources): IosRunLive {
    val text = runSurfaceText(state, res)
    val theme = AppSettings.theme.value
    return IosRunLive(
        phase = state.phase.wire,
        title = text.title,
        chip = text.chip,
        text = text.text,
        step = text.step,
        movingSec = state.movingSec,
        time = text.time,
        distance = text.distance,
        pace = text.pace,
        movingLabel = res.getString(Res.string.track_stat_elapsed),
        distanceLabel = res.getString(Res.string.run_stat_distance),
        paceLabel = res.getString(Res.string.track_stat_pace_now),
        canPause = state.phase.canPause,
        paused = state.phase == RunSurfacePhase.Paused,
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

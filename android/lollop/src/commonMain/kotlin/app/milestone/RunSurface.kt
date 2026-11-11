package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.live_chip_locating
import app.milestone.resources.live_chip_no_gps
import app.milestone.resources.live_chip_paused
import app.milestone.resources.live_chip_waiting
import app.milestone.resources.track_notif_armed_text
import app.milestone.resources.track_notif_armed_title
import app.milestone.resources.track_notif_locate_text
import app.milestone.resources.track_notif_locate_title
import app.milestone.resources.track_notif_paused_title
import app.milestone.resources.track_notif_progress
import app.milestone.resources.track_notif_progress_figures
import app.milestone.resources.track_notif_progress_pace
import app.milestone.resources.track_notif_recording_hint
import app.milestone.resources.track_notif_recording_title
import app.milestone.resources.track_notif_signal_lost
import app.milestone.resources.track_notif_step
import app.milestone.resources.track_state_paused
import org.jetbrains.compose.resources.StringResource

// What a run shows outside the app: Android's ongoing notification (and its Live
// Update chip), iOS's Live Activity. Both render the same text from here.

internal enum class RunSurfacePhase { Locating, Armed, Recording, Paused, SignalLost }

internal fun runSurfacePhase(recording: Boolean, armed: Boolean, paused: Boolean, signalLost: Boolean): RunSurfacePhase = when {
    recording && paused -> RunSurfacePhase.Paused
    recording && signalLost -> RunSurfacePhase.SignalLost
    recording -> RunSurfacePhase.Recording
    armed -> RunSurfacePhase.Armed
    else -> RunSurfacePhase.Locating
}

internal val RunSurfacePhase.titleRes: StringResource
    get() = when (this) {
        RunSurfacePhase.Paused -> Res.string.track_notif_paused_title
        RunSurfacePhase.Recording, RunSurfacePhase.SignalLost -> Res.string.track_notif_recording_title
        RunSurfacePhase.Armed -> Res.string.track_notif_armed_title
        RunSurfacePhase.Locating -> Res.string.track_notif_locate_title
    }

/**
 * The status chip's word (Android 16's Live Update chip, the Live Activity's
 * compact view); null while recording, where the chip shows the moving time
 * ([formatElapsed]). The chip shows text whole only up to 7 characters: moving time
 * fits to 9:59:59, every word here fits (LiveRunNotificationTest). Not a
 * chronometer: it would tick on the wall clock while moving time stands still.
 */
internal val RunSurfacePhase.chipRes: StringResource?
    get() = when (this) {
        RunSurfacePhase.Paused -> Res.string.live_chip_paused
        RunSurfacePhase.SignalLost -> Res.string.live_chip_no_gps
        RunSurfacePhase.Recording -> null
        RunSurfacePhase.Armed -> Res.string.live_chip_waiting
        RunSurfacePhase.Locating -> Res.string.live_chip_locating
    }

/** Pause/Resume shows only while a run records. Pausing a run that is merely armed
 *  would set the flag, so its first real fixes would be dropped the instant
 *  recording begins. */
internal val RunSurfacePhase.canPause: Boolean
    get() = this == RunSurfacePhase.Recording || this == RunSurfacePhase.Paused || this == RunSurfacePhase.SignalLost

internal sealed class RunSurfaceLine {
    data class Literal(val res: StringResource) : RunSurfaceLine()
    data class Progress(val text: String, val paceNow: String?) : RunSurfaceLine()
    data class PausedProgress(val res: StringResource, val progress: String) : RunSurfaceLine()
}

/** [progress] is the figures, null before the track has the
 *  two fixes a distance needs. A paused recording keeps its figures in the
 *  recording line's words without the pace, which stands still; the title ("Run
 *  paused") and the chip ("Paused") carry the pause, so the line does not say it a
 *  third time. A lost signal replaces the figures (frozen anyway) with its warning. */
internal fun runSurfaceLine(phase: RunSurfacePhase, progress: String?, paceNow: String?): RunSurfaceLine = when (phase) {
    RunSurfacePhase.Paused ->
        if (progress != null) RunSurfaceLine.PausedProgress(Res.string.track_notif_progress, progress) else RunSurfaceLine.Literal(Res.string.track_state_paused)
    RunSurfacePhase.SignalLost -> RunSurfaceLine.Literal(Res.string.track_notif_signal_lost)
    RunSurfacePhase.Recording ->
        if (progress != null) RunSurfaceLine.Progress(progress, paceNow) else RunSurfaceLine.Literal(Res.string.track_notif_recording_hint)
    RunSurfacePhase.Armed -> RunSurfaceLine.Literal(Res.string.track_notif_armed_text)
    RunSurfacePhase.Locating -> RunSurfaceLine.Literal(Res.string.track_notif_locate_text)
}

/**
 * One frame of a run for its system surface: the phase, moving time, distance, the
 * current pace (canonical min/km, NaN while there is none), the user's distance
 * unit, whether the track has the two fixes a distance needs ([hasTrack]) and, while
 * recording, a guided run's step. Equal frames show the same.
 */
internal data class RunSurfaceState(
    val phase: RunSurfacePhase,
    val movingSec: Long,
    val distanceKm: Double,
    val paceMinPerKm: Double,
    val miles: Boolean,
    val hasTrack: Boolean = true,
    val step: StepProgress? = null,
)

internal fun RunRecorder.surfaceState(): RunSurfaceState = RunSurfaceState(
    phase = runSurfacePhase(recording, armed, RunSession.paused.value, signalLost),
    movingSec = RunSession.elapsedSec.value,
    distanceKm = RunSession.distanceKm.value,
    paceMinPerKm = RunSession.currentPaceMinPerKm.value,
    miles = EventDispatcher.model.value.distanceUnit() == DistanceUnit.Mi,
    hasTrack = RunSession.points.value.size >= 2,
    step = if (recording) RunSession.stepProgress.value else null,
)

/**
 * [RunSurfaceState] in words: [title]; [chip]; [line], the notification's whole
 * content line; its parts for the Live Activity's tiles: [text] where the line is
 * fixed, else the moving [time], [distance] and current [pace] (null where the line
 * has none); a guided run's [step] line (it leads the line while recording).
 */
internal data class RunSurfaceText(
    val title: String,
    val chip: String,
    val line: String,
    val text: String?,
    val time: String,
    val distance: String?,
    val pace: String?,
    val step: String?,
)

/**
 * [state]'s text. The moving time is MOVING time, not wall-clock elapsed, matching
 * the tracker's overline; the distance honours the user's unit, as the rest of the
 * app does. The step line leads ("Rep 2 of 6 · 320 m left · Moving …"), so a glance
 * at the lock screen says what to do now.
 */
internal fun runSurfaceText(state: RunSurfaceState, res: PlatformResources): RunSurfaceText {
    val unit = if (state.miles) DistanceUnit.Mi else DistanceUnit.Km
    val time = formatElapsed(state.movingSec)
    val distance = if (state.hasTrack) runDistLabel(state.distanceKm, unit) else null
    val paceNow = state.paceMinPerKm.takeIf { it.isFinite() }?.let { "${formatPaceMinutes(paceInUnit(it, unit))} ${unit.paceSuffix}" }
    val spec = runSurfaceLine(state.phase, distance?.let { res.getString(Res.string.track_notif_progress_figures, time, it) }, paceNow)
    val step = state.step?.takeIf { state.phase == RunSurfacePhase.Recording }?.let { runStepLine(res, it) }
    val line = when (spec) {
        is RunSurfaceLine.Literal -> res.getString(spec.res)
        is RunSurfaceLine.Progress -> {
            val base = if (spec.paceNow != null) {
                res.getString(Res.string.track_notif_progress_pace, spec.text, spec.paceNow)
            } else {
                res.getString(Res.string.track_notif_progress, spec.text)
            }
            if (step != null) res.getString(Res.string.track_notif_step, step, base) else base
        }
        is RunSurfaceLine.PausedProgress -> res.getString(spec.res, spec.progress)
    }
    return RunSurfaceText(
        title = res.getString(state.phase.titleRes),
        chip = state.phase.chipRes?.let { res.getString(it) } ?: time,
        line = line,
        text = if (spec is RunSurfaceLine.Literal) line else null,
        time = time,
        distance = if (spec is RunSurfaceLine.Literal) null else distance,
        pace = (spec as? RunSurfaceLine.Progress)?.paceNow,
        step = step,
    )
}

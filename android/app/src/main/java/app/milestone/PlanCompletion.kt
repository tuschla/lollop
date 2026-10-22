package app.milestone

import android.content.res.Resources
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import java.util.Locale

/**
 * Per-set completion chrome shared by the Coach hero, the week-strip expander,
 * and the set editor's gym-floor loop. Everything here
 * renders wire fields the core owns (sets_done / logged_sets / ended_early /
 * run_logged_*); this file adds no coaching logic and invents no numbers.
 */

/** Ends [item] on [epochDay] and confirms it: "Squat ended at 4/6 · Undo" (or
 *  "Squat skipped · Undo" with nothing logged); Undo reopens it. */
internal fun endExerciseWithUndo(res: Resources, item: PrescriptionView, epochDay: Long, send: (Event) -> Unit) {
    if (item.exercise.isBlank()) return
    send(Event.SetExerciseEnded(item.exercise, epochDay, ended = true))
    val text = if (item.logged_sets.isEmpty()) {
        res.getString(R.string.plan_skipped_msg, item.exercise)
    } else {
        res.getString(R.string.plan_ended_msg, item.exercise, item.sets_done, plannedSetCount(item))
    }
    UiMessages.post(
        UiMessage(
            text,
            actionLabel = res.getString(R.string.log_undo),
            onAction = { EventDispatcher.send(Event.SetExerciseEnded(item.exercise, epochDay, ended = false)) },
        ),
    )
}

/** The prescribed target band for a plan item's set-editor header: a hold band
 *  ("20–45 s"), else a rep band + the effort as reps left ("5–8 reps · ~2 left"),
 *  else the intensity label. Unit-testable outside composition. */
internal fun setTargetBand(res: Resources, item: PrescriptionView): String {
    if (item.timed_hold) {
        return if (item.hold_low_sec > 0) {
            val hi = maxOf(item.hold_low_sec, item.hold_high_sec)
            if (hi > item.hold_low_sec) {
                res.getString(R.string.log_band_hold_range, item.hold_low_sec, hi)
            } else {
                res.getString(R.string.log_band_hold, item.hold_low_sec)
            }
        } else {
            item.rep_volume
        }
    }
    val reps = if (item.reps_low > 0) {
        val hi = maxOf(item.reps_low, item.reps_high)
        if (hi > item.reps_low) {
            res.getString(R.string.log_band_reps_range, item.reps_low, hi)
        } else {
            res.getQuantityString(R.plurals.log_band_reps, item.reps_low, item.reps_low)
        }
    } else {
        ""
    }
    // The effort reads in the face's one vocabulary, reps left ("3 reps · ~2
    // left"); a %e1RM/prose label is its own clause ("3 reps · 85% e1RM").
    val effort = item.target_rpe?.let { res.getString(R.string.log_band_effort, repsLeftText(res, it)) } ?: item.intensity_label
    return listOf(reps, effort).filter { it.isNotBlank() }.joinToString(" \u00b7 ")
}

/** Logged run totals for a run plan item in the user's distance unit ("4.1 km ·
 *  27:00" / "0.43 mi · 3:42"); blank when nothing is logged yet. Under one unit the
 *  distance keeps two decimals, as the run card and the Finish review show it. The
 *  time is a clock like every other run duration (m:ss, h:mm:ss from an hour). The
 *  prescribed target rides in the item summary. */
internal fun runProgressText(res: Resources, item: PrescriptionView, unit: DistanceUnit): String {
    val parts = buildList {
        if (item.run_logged_km > 0.0) {
            val shown = metersToDisplay(item.run_logged_km * 1000.0, unit)
            val figure = if (shown < 1.0) String.format(Locale.US, "%.2f", shown) else trimDecimal(shown)
            add(res.getString(R.string.plan_run_logged_distance, figure, unit.distanceLabel))
        }
        if (item.run_logged_min > 0.0) add(fmtRunTime(Math.round(item.run_logged_min * 60.0).toInt()))
    }
    return parts.joinToString(" \u00b7 ")
}

/** A run duration as a clock: "3:42", "27:00", "1:19:05". Pure. */
internal fun fmtRunTime(totalSec: Int): String {
    val s = totalSec.coerceAtLeast(0)
    if (s < 3600) return fmtClock(s)
    val m = (s % 3600) / 60
    return "${s / 3600}:${m.toString().padStart(2, '0')}:${(s % 60).toString().padStart(2, '0')}"
}

/** "m:ss" clock for a rest / elapsed readout. */
internal fun fmtClock(totalSec: Int): String {
    val s = totalSec.coerceAtLeast(0)
    return "${s / 60}:${(s % 60).toString().padStart(2, '0')}"
}

/** How long after a rest starts the loop keeps its readout. Past this the rest
 *  belongs to an earlier bout, not the one before the next set. */
internal const val RestReadoutWindowSec = 30 * 60

/** What the loop's rest readout shows. */
internal sealed interface RestFace {
    /** A prescribed rest still running: [leftSec] of [restSec] remain. */
    data class Counting(val leftSec: Int, val restSec: Int) : RestFace
    /** The prescribed rest ended [overSec] ago. */
    data class Over(val overSec: Int) : RestFace
    /** No prescribed rest: time since the newest set of this exercise. */
    data class Since(val elapsedSec: Int) : RestFace
}

/**
 * The rest readout at [nowMillis]. A rest the loop scheduled ([restStartMillis]
 * → [restEndMillis], [AppClock] millis, from [RestAlarm.running]) wins whatever
 * exercise is on screen, so the countdown carries into the next exercise. With
 * none running, an exercise without a prescribed rest ([itemRestSec] 0) counts up
 * from its newest set today ([lastSetSec], unix seconds). Null when nothing is in
 * its [RestReadoutWindowSec] window. Pure.
 */
internal fun restFace(
    restStartMillis: Long?,
    restEndMillis: Long?,
    lastSetSec: Long?,
    itemRestSec: Int,
    nowMillis: Long,
): RestFace? {
    if (restStartMillis != null && restEndMillis != null) {
        val sinceStart = nowMillis - restStartMillis
        if (sinceStart >= 0 && sinceStart <= RestReadoutWindowSec * 1000L) {
            val leftMs = restEndMillis - nowMillis
            return if (leftMs > 0) {
                // Rounded up: "0:01" shows until the rest is really over.
                RestFace.Counting(((leftMs + 999) / 1000).toInt(), ((restEndMillis - restStartMillis) / 1000).toInt())
            } else {
                RestFace.Over((-leftMs / 1000).toInt())
            }
        }
    }
    if (itemRestSec <= 0 && lastSetSec != null) {
        val elapsed = nowMillis / 1000 - lastSetSec
        if (elapsed in 0..RestReadoutWindowSec.toLong()) return RestFace.Since(elapsed.toInt())
    }
    return null
}

/** [AppClock] millis, re-emitted on every [periodMs] boundary, so every ticking
 *  readout steps together and a second never skips or repeats. */
@Composable
internal fun rememberTicker(periodMs: Long): Long {
    val now by produceState(AppClock.nowMillis(), periodMs) {
        while (true) {
            value = AppClock.nowMillis()
            kotlinx.coroutines.delay(periodMs - Math.floorMod(AppClock.nowMillis(), periodMs))
        }
    }
    return now
}

/** Tells [RestAlarm] the set loop is on a resumed screen, so its own rest cue
 *  covers the rest end and no notification is posted. Lives for the whole loop
 *  sheet, not one exercise's readout: switching exercise mid-rest never hands
 *  the countdown to a notification while the app is open. */
@Composable
internal fun RestAlarmInAppScope() {
    val ctx = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> RestAlarm.setInAppVisible(ctx, true)
                Lifecycle.Event.ON_PAUSE -> RestAlarm.setInAppVisible(ctx, false)
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            RestAlarm.setInAppVisible(ctx, false)
        }
    }
}

/**
 * The loop's rest readout ([restFace]): the running rest's countdown (a "Rest
 * left" caption over a bold "1:42"), flipping to "Rest done" / "+0:15" with one
 * haptic tick; without a prescribed rest it counts up since the last set. The
 * clock is the glanceable part. TalkBack reads the time left when it lands on
 * the readout and hears one polite announcement when the rest ends, never
 * per-tick chatter.
 */
@Composable
internal fun RestReadout(lastSetSec: Long?, itemRestSec: Int) {
    val run by RestAlarm.running.collectAsState()
    val now = rememberTicker(1000L)
    val face = restFace(run?.startMillis, run?.endAtMillis, lastSetSec, itemRestSec, now) ?: return
    when (face) {
        is RestFace.Counting, is RestFace.Over -> {
            val done = face is RestFace.Over
            val haptic = LocalHapticFeedback.current
            // Seeded with the state at first show, so reopening the sheet after the
            // rest already ended doesn't buzz; only the live transition does.
            var wasDone by remember(run?.endAtMillis) { mutableStateOf(done) }
            // The Profile "Rest timer alert" toggle silences the buzz too; the spoken
            // live-region announcement stays (it is an accessibility cue, not an alert).
            val alert by AppSettings.restAlert.collectAsState()
            LaunchedEffect(done) {
                if (done && !wasDone && alert) haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                wasDone = done
            }
            val clock = when (face) {
                is RestFace.Counting -> fmtClock(face.leftSec)
                is RestFace.Over -> "+" + fmtClock(face.overSec)
                else -> ""
            }
            // The countdown's description carries the time left (read on focus, not
            // announced). Once over it is static and a live region, so the change
            // to "done" is announced once and the overtime ticks stay silent.
            val cd = if (done) {
                stringResource(R.string.plan_rest_done_cd)
            } else {
                stringResource(R.string.log_rest_left_cd, clock)
            }
            RestClock(
                label = if (done) stringResource(R.string.plan_rest_done_label) else stringResource(R.string.plan_rest_left_label),
                clock = clock,
                color = if (done) Accent else OnBgBody,
                modifier = Modifier
                    .testTag("rest-readout")
                    .clearAndSetSemantics {
                        contentDescription = cd
                        if (done) liveRegion = LiveRegionMode.Polite
                    },
            )
        }
        is RestFace.Since -> {
            val cd = stringResource(R.string.plan_since_last_set_cd)
            RestClock(
                label = stringResource(R.string.plan_since_last_set_label),
                clock = fmtClock(face.elapsedSec),
                color = OnBgMuted,
                modifier = Modifier
                    .testTag("rest-readout")
                    .clearAndSetSemantics { contentDescription = cd },
            )
        }
    }
}

/** The rest readout's face: a quiet caption word over the bold clock,
 *  right-aligned so it closes the loop header's first row. */
@Composable
private fun RestClock(label: String, clock: String, color: Color, modifier: Modifier = Modifier) {
    Column(modifier = modifier, horizontalAlignment = Alignment.End) {
        Text(label, color = OnBgMuted, style = Type.Caption)
        Text(
            clock,
            color = color,
            style = Type.Title.merge(TabularFigures),
            maxLines = 1,
        )
    }
}

/** What each planned slot shows: null = still open, "" = a full set (✓), else
 *  what a short set that still counted reached: its reps ("4" of a 5-rep
 *  target) or its hold seconds. A set is short when the core flags it (`short`)
 *  or its reps sit under the rep target. Pure. */
internal fun setSlotMarks(item: PrescriptionView): List<String?> {
    val counted = item.logged_sets.filter { it.qualifies }
    return (0 until plannedSetCount(item)).map { i ->
        when {
            i >= item.sets_done -> null
            else -> counted.getOrNull(i)?.takeIf {
                it.short || (item.reps_low > 0 && it.hold_sec == null && it.reps in 1 until item.reps_low)
            }?.let { "${it.hold_sec ?: it.reps}" } ?: ""
        }
    }
}

/** The per-set slot row: one box per planned set, solid green with a white ✓
 *  when a counted set is logged (the reps instead for a short set), hollow
 *  otherwise. The solid fill keeps the mark legible on both themes' grounds; the
 *  ✓ is a dp-sized icon (the non-colour done cue) that fits its box at any font
 *  scale, and a box with reps grows with the text. TalkBack reads the row as ONE
 *  node ("1 of 4 sets done") instead of a run of identical per-slot nodes. */
@Composable
internal fun SetSlotsRow(item: PrescriptionView, modifier: Modifier = Modifier) {
    val status = LocalStatusColors.current
    val marks = setSlotMarks(item)
    val cd = stringResource(R.string.plan_sets_done_cd, item.sets_done, marks.size)
    Row(
        modifier = modifier
            .testTag("set-slots:${item.exercise}")
            .clearAndSetSemantics { contentDescription = cd },
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp + Space.Xs.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        marks.forEach { mark ->
            Box(
                modifier = Modifier
                    .defaultMinSize(minWidth = SlotSize, minHeight = SlotSize)
                    .clip(RoundedCornerShape(5.dp))
                    .then(
                        if (mark != null) {
                            Modifier.background(status.evidenceStrong)
                        } else {
                            Modifier.border(1.5.dp, OnBgFaint, RoundedCornerShape(5.dp))
                        },
                    ),
                contentAlignment = Alignment.Center,
            ) {
                when {
                    mark == null -> Unit
                    mark.isEmpty() -> Icon(
                        painterResource(R.drawable.ic_ui_check),
                        contentDescription = null,
                        tint = Color.White,
                        modifier = Modifier.size(SlotCheckSize),
                    )
                    else -> Text(
                        mark,
                        color = Color.White,
                        style = Type.Caption.merge(TabularFigures).copy(fontWeight = FontWeight.Bold),
                        modifier = Modifier.padding(horizontal = Space.Xs.dp),
                    )
                }
            }
        }
    }
}

private val SlotSize = 20.dp
private val SlotCheckSize = 14.dp

/** Completion detail INSIDE an open plan item's card: the set slots + "k/N"
 *  progress (or a run's logged totals), the not-counted marks on non-qualifying
 *  logged sets, and the "Skip exercise" / "End exercise" secondary action. A run
 *  with nothing logged shows nothing here (its Track / Log run action already
 *  says so); the caller drops the footer for it. */
@Composable
internal fun PlanItemCompletion(
    item: PrescriptionView,
    isRun: Boolean,
    actionable: Boolean,
    onEndExercise: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        if (isRun) {
            val logged = runProgressText(LocalContext.current.resources, item, rememberDistanceUnit())
            if (logged.isNotBlank()) {
                Text(
                    stringResource(R.string.plan_run_logged, logged),
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                )
            }
        } else {
            // Slots + the secondary action share ONE row so the 48dp touch target
            // absorbs the row height rather than adding a separate tall row. The
            // slots alone carry progress (and speak "k of N sets done").
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            ) {
                SetSlotsRow(item)
                if (actionable) {
                    Spacer(Modifier.weight(1f))
                    // Nothing logged yet: ending it means skipping it, so say so.
                    val label = if (item.logged_sets.isEmpty()) {
                        stringResource(R.string.plan_skip_exercise)
                    } else {
                        stringResource(R.string.plan_end_exercise)
                    }
                    // The label (not its touch padding) ends at the content edge.
                    TextAction(
                        label,
                        onEndExercise,
                        modifier = Modifier.offset(x = TextActionInset),
                        description = stringResource(R.string.plan_secondary_action_cd, label, item.exercise),
                    )
                }
            }
            // Warm-ups / light sets stay listed but are marked, so a floor of
            // empty-bar singles can never masquerade as the prescribed work.
            item.logged_sets.filter { !it.qualifies }.forEach { set ->
                Text(
                    "${set.summary} \u00b7 ${stringResource(R.string.plan_set_not_counted)}",
                    color = OnBgMuted,
                    style = Type.Caption,
                )
            }
        }
    }
}

/** The face head of a run item in the display [unit]: the core's volume head
 *  ("6 × 800 m"), whose target and structure move into dedicated, unit-aware face
 *  lines. Null when the item carries no run target or its summary leads with no
 *  volume (then the full summary stays and no face lines are added, so nothing
 *  renders twice). */
internal fun runFaceHead(item: PrescriptionView, unit: DistanceUnit): String? =
    if (item.run_target != null && item.run_head.isNotBlank()) runHeadText(item, unit) else null

/** A run item's volume head in the display [unit], so the head and the pace line
 *  under it never mix km with /mi: a distance head ("8 km") is restated from its
 *  km figure; reps ("6 × 800 m"), minutes and bouts stay as the core wrote them. */
internal fun runHeadText(item: PrescriptionView, unit: DistanceUnit): String {
    val km = item.run_head_km
    return if (km == null || unit == DistanceUnit.Km) {
        item.run_head
    } else {
        "${trimDecimal(metersToDisplay(km * 1000.0, unit))} ${unit.distanceLabel}"
    }
}

/** Why a safety cap shaped [item], in the display [unit]; null when none did. */
internal fun safetyReasonText(item: PrescriptionView, unit: DistanceUnit): String? =
    item.safety_reason.takeIf { it.isNotBlank() }
        ?.let { if (unit == DistanceUnit.Mi) item.safety_reason_mi.ifBlank { it } else it }

/** The feedback message in the display [unit]. */
internal fun feedbackMessage(fb: FeedbackView, unit: DistanceUnit): String =
    if (unit == DistanceUnit.Mi) fb.message_mi.ifBlank { fb.message } else fb.message

/** A plate / load figure with up to two decimals and no trailing zeros
 *  ("20", "2.5", "1.25"). Pure. */
internal fun plateFigure(v: Double): String =
    if (v % 1.0 == 0.0) "${v.toLong()}"
    else String.format(Locale.US, "%.2f", v).trimEnd('0').trimEnd('.')

/** A lift face line without its reps-left clause ("6×5 @ 275 lb · ~1 rep left"
 *  → "6×5 @ 275 lb"; also "1–2 reps left" and "to your last good rep"), for an
 *  item whose newcomer guide already states the effort. Pure. */
internal fun withoutEffortClause(detail: String): String =
    detail.split(" \u00b7 ")
        .filterNot { c -> c.startsWith("~") || c.endsWith(" left") || c == "to your last good rep" }
        .joinToString(" \u00b7 ")

/** A whole-seconds duration as plain copy: "3 min", "45 s", "1:30 min". */
@Composable
private fun durationText(sec: Int): String = when {
    sec % 60 == 0 -> stringResource(R.string.main_minutes_fmt, sec / 60)
    sec < 60 -> stringResource(R.string.plan_dur_sec, sec)
    else -> stringResource(R.string.plan_dur_min_sec, sec / 60, (sec % 60).toString().padStart(2, '0'))
}

/** One compact line for a run's structure: "10 min warm-up · 6 × 800 m with
 *  400 m jog · 10 min cool-down · 4 × 20 s strides", or the run/walk bouts of a
 *  beginner session. A continuous run's head (its distance or time) is the main
 *  block, so the extras around it read "Plus 10 min warm-up · 10 min cool-down".
 *  Blank when the structure holds nothing to show. */
@Composable
private fun runStructureLine(s: RunStructureView): String {
    val parts = mutableListOf<String>()
    if (s.warmup_min > 0) parts += stringResource(R.string.plan_run_warmup, s.warmup_min)
    if (s.run_walk.isNotEmpty()) {
        val first = s.run_walk.first()
        // "Run 1 min, walk 1:30 min, × 8" when every bout repeats (the last bout
        // may drop its walk); otherwise each bout in order.
        val uniform = s.run_walk.size > 1 && first.walk_sec > 0 &&
            s.run_walk.dropLast(1).all { it == first } &&
            s.run_walk.last().run_sec == first.run_sec
        parts += if (uniform) {
            stringResource(
                R.string.plan_run_walk_repeat,
                durationText(first.run_sec),
                durationText(first.walk_sec),
                s.run_walk.size,
            )
        } else {
            s.run_walk.map { b ->
                if (b.walk_sec > 0) {
                    stringResource(R.string.plan_run_walk_bout, durationText(b.run_sec), durationText(b.walk_sec))
                } else {
                    stringResource(R.string.plan_run_walk_last, durationText(b.run_sec))
                }
            }.joinToString(", ")
        }
    }
    if (s.reps > 0 && (s.rep_m > 0 || s.rep_sec > 0)) {
        val reps = if (s.rep_m > 0) {
            stringResource(R.string.plan_run_reps_m, s.reps, s.rep_m)
        } else {
            stringResource(R.string.plan_run_reps_time, s.reps, durationText(s.rep_sec))
        }
        val recovery = when {
            s.recovery_m > 0 -> stringResource(R.string.plan_run_rec_jog_m, s.recovery_m)
            s.recovery_sec > 0 -> stringResource(R.string.plan_run_rec_jog_time, durationText(s.recovery_sec))
            s.recovery_equal_to_rep -> stringResource(R.string.plan_run_rec_equal)
            else -> null
        }
        parts += if (recovery != null) stringResource(R.string.plan_run_reps_with_rec, reps, recovery) else reps
    }
    if (s.cooldown_min > 0) parts += stringResource(R.string.plan_run_cooldown, s.cooldown_min)
    if (s.strides_reps > 0 && s.strides_sec > 0) {
        parts += stringResource(R.string.plan_run_strides, s.strides_reps, s.strides_sec)
    }
    if (parts.isEmpty()) return ""
    val continuous = s.run_walk.isEmpty() && !(s.reps > 0 && (s.rep_m > 0 || s.rep_sec > 0))
    val line = parts.joinToString(" \u00b7 ")
    return if (continuous) stringResource(R.string.plan_run_plus, line) else line
}

/** One quiet face line under a plan item's summary. */
@Composable
private fun FaceLine(text: String, color: Color = OnBgMuted) {
    Text(text, color = color, style = Type.Caption.merge(TabularFigures))
}

/**
 * The do-this numbers on a plan item's face, directly under its summary: for a
 * lift, the plates per side and what was lifted last time (the summary itself
 * states sets × reps @ load and the effort); for a run, the pace in the user's
 * distance unit, the heart-rate range, the effort cue and the session's
 * structure. Every figure is a core wire field; nothing is computed here beyond
 * formatting.
 */
@Composable
private fun PlanItemFace(
    item: PrescriptionView,
    isRun: Boolean,
    runHead: String?,
    weightUnit: WeightUnit,
    distanceUnit: DistanceUnit,
    lastOnFace: String?,
) {
    val lines = buildList<String> {
        if (isRun) {
            val t = item.run_target
            if (t != null && runHead != null) {
                val pace = if (distanceUnit == DistanceUnit.Mi) t.pace_label_mi else t.pace_label_km
                // The band key ("threshold") is engine vocabulary; the Effort line
                // below already says how hard in plain words.
                if (pace.isNotBlank()) add(stringResource(R.string.plan_face_pace_only, pace))
                if (t.hr_label.isNotBlank()) add(stringResource(R.string.plan_face_hr, t.hr_label))
                if (t.effort_detail.isNotBlank()) add(stringResource(R.string.plan_face_effort, t.effort_detail))
                item.run_structure?.let { s -> runStructureLine(s).takeIf { it.isNotBlank() }?.let { add(it) } }
            }
        } else {
            if (item.plates_per_side.isNotEmpty()) {
                add(stringResource(R.string.plan_face_per_side, item.plates_per_side.joinToString(" \u00b7 ") { plateFigure(it) }, weightSuffix(weightUnit)))
            }
            lastOnFace?.let { add(it) }
        }
    }
    if (lines.isEmpty()) return
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        lines.forEach { FaceLine(it) }
    }
}

/** The "what you did last time" line for a lift face: the core's own "Last: …"
 *  anchor when it is one, else the prior top set ("Last: 60 kg × 8"). Null when
 *  the movement was never logged. */
@Composable
private fun lastLine(item: PrescriptionView): String? = when {
    item.anchored_on_last -> item.anchored_on
    item.last_set != null && item.last_set.summary.isNotBlank() ->
        stringResource(R.string.plan_face_last, item.last_set.summary)
    else -> null
}

/** Calisthenics controls on an open item: the "Easier" / "Harder" rung chips
 *  (each sends SetCalisthenicsRung via [onSetRung]), the effort-rating hint when
 *  the rung waits only on a rating, and the first-time invite on a ladder the
 *  athlete has not been placed on yet. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CalisControls(
    calis: CalisItemView,
    unplaced: Boolean,
    onSetRung: ((CalisLadder, Int) -> Unit)?,
) {
    if (calis.needs_effort_rating) FaceLine(stringResource(R.string.plan_calis_rate_effort))
    val chips = if (onSetRung == null) {
        emptyList()
    } else {
        listOfNotNull(
            calis.easier?.takeIf { it.available }?.let { R.string.plan_calis_easier to it },
            calis.harder?.takeIf { it.available }?.let { R.string.plan_calis_harder to it },
        )
    }
    if (unplaced && chips.any { it.first == R.string.plan_calis_harder }) {
        FaceLine(stringResource(R.string.plan_calis_unplaced))
    }
    if (chips.isEmpty()) return
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        chips.forEach { (labelRes, rung) ->
            val label = stringResource(labelRes)
            val cd = if (rung.target.isBlank()) {
                stringResource(R.string.plan_calis_chip_cd_short, label, rung.name)
            } else {
                stringResource(R.string.plan_calis_chip_cd, label, rung.name, rung.target)
            }
            Column(
                modifier = Modifier
                    .heightIn(min = ActionMinHeight)
                    .clip(RoundedCornerShape(Space.Md.dp))
                    .border(1.dp, Accent, RoundedCornerShape(Space.Md.dp))
                    .clickable(role = Role.Button) { onSetRung?.invoke(calis.ladder, rung.rung) }
                    // The clickable merges the label + rung name; the description
                    // reads the whole choice ("Harder: Decline push-up, 5–8 reps").
                    .semantics { contentDescription = cd }
                    .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp),
                verticalArrangement = Arrangement.Center,
            ) {
                Text(label, color = Accent, style = Type.Caption.copy(fontWeight = FontWeight.Bold))
                Text(rung.name, color = OnBgMuted, style = Type.Caption)
            }
        }
    }
}

/** The `plan-item:<exercise>` test tag (`plan-item:run`, `plan-item:check`). */
private fun planItemTag(item: PrescriptionView, isRun: Boolean, isCheck: Boolean): String = when {
    isCheck -> "plan-item:check"
    isRun -> "plan-item:run"
    else -> "plan-item:${item.exercise}"
}

/** What a plan item is called on its face and to a screen reader. */
@Composable
private fun planItemName(item: PrescriptionView, isRun: Boolean, isCheck: Boolean): String = when {
    isCheck -> stringResource(R.string.plan_check_title)
    isRun -> stringResource(R.string.main_run)
    else -> item.exercise
}

/** The note a plan item states on its face rather than behind the session "?":
 *  the return check's stop list and a target-less run's note are safety copy. */
private fun noteOnFace(item: PrescriptionView, isRun: Boolean, isCheck: Boolean): String? =
    item.adjusted_note.takeIf { it.isNotBlank() && (isCheck || (isRun && item.run_target == null)) }

/** The label of the action that logs [item]: "Log set k of N" for a lift, the
 *  caller's run label ("Track run" / "Log run") for a run, none for the return
 *  check (any session logged today completes it). */
@Composable
internal fun planItemActionLabel(item: PrescriptionView, isRun: Boolean, isCheck: Boolean, runActionLabel: String): String? = when {
    isCheck -> null
    isRun -> runActionLabel
    else -> stringResource(R.string.plan_log_set_n_of_n, nextSetNumber(item), plannedSetCount(item))
}

/** The full-width filled "do this next" action of the current plan item. */
@Composable
private fun PlanActionPill(label: String, subject: String, onClick: () -> Unit) {
    PrimaryButton(
        label,
        onClick,
        modifier = Modifier.fillMaxWidth().testTag("plan-action-filled"),
        description = stringResource(R.string.main_action_subject, label, subject),
    )
}

/** The CURRENT plan item (the first open one of an actionable session): its
 *  summary as a bold name over the prescription, the do-this numbers
 *  ([PlanItemFace]), the set slots with "Skip/End exercise", then the one filled
 *  "Log set k of N" (or "Track run") action. It carries no "?" of its own: its
 *  evidence sits in the session's single "?" panel ([PlanItemEvidence]); a
 *  safety-critical item keeps its SAFETY chip on the face. Every other item is a
 *  [PlanItemRow]. Tagged `plan-item:<exercise>` (`plan-item:run`,
 *  `plan-item:check`).
 *
 *  [isCheck] marks the post-illness "Return check" (an item with no exercise in a
 *  lifting session): its stop list sits on the face and any session logged today
 *  completes it, so it has no Log action of its own (the + Log button logs it). */
@Composable
internal fun OpenPlanItemCard(
    item: PrescriptionView,
    isRun: Boolean,
    onLog: () -> Unit,
    onTrack: () -> Unit,
    onEndExercise: () -> Unit,
    // The run action label: "Track run" (live GPS today) on the hero, "Log run"
    // (manual entry) for a past day on the week strip.
    runActionLabel: String,
    // The profile's weight display unit (plates_per_side is already in it).
    weightUnit: WeightUnit = WeightUnit.Kg,
    isCheck: Boolean = false,
    // Calisthenics: the athlete has no placement on this item's ladder yet.
    unplaced: Boolean = false,
    // Calisthenics Easier/Harder: null hides the chips (past days).
    onSetRung: ((CalisLadder, Int) -> Unit)? = null,
    // Plain-language lines for a newcomer (what the prescription means, how to
    // pick a first weight), shown under the prescription. Empty once the athlete
    // has a few sessions behind them.
    guide: List<String> = emptyList(),
) {
    val status = LocalStatusColors.current
    val distanceUnit = rememberDistanceUnit()
    val runHead = if (isRun) runFaceHead(item, distanceUnit) else null
    val summary = runHead ?: item.summary
    // Lifts and calisthenics: a bold exercise line over a regular prescription
    // line, so the name never shares a wrapping line with the numbers. A
    // newcomer's guide spells the reps left out, so the line drops its own.
    val split = if (isRun || isCheck) {
        null
    } else {
        item.face_name.takeIf { it.isNotBlank() }?.let {
            it to if (guide.isEmpty()) item.face_detail else withoutEffortClause(item.face_detail)
        }
    }
    val lastOnFace = if (isRun || isCheck) null else lastLine(item)
    val note = noteOnFace(item, isRun, isCheck)
    val subject = planItemName(item, isRun, isCheck)
    val actionLabel = planItemActionLabel(item, isRun, isCheck, runActionLabel)
    // A run with nothing logged has no completion footer: no "Not logged yet"
    // restating what its Track / Log run action already says.
    val runUnlogged = isRun && !isCheck && runProgressText(LocalContext.current.resources, item, distanceUnit).isBlank()
    // A safety cap states its reason in place of a bare SAFETY badge.
    val safetyReason = safetyReasonText(item, distanceUnit)
    val shape = RoundedCornerShape(Space.Card.dp)
    Box(modifier = Modifier.testTag(planItemTag(item, isRun, isCheck))) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(shape)
                .background(BgTop)
                .then(
                    if (item.safety_critical) Modifier.border(1.dp, status.danger.copy(alpha = 0.35f), shape) else Modifier,
                )
                .padding(Space.Card.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            if (item.safety_critical && safetyReason == null) Chip(stringResource(R.string.main_chip_safety), status.danger)
            // The HRmax glossary term is linked from the session's "?" panel.
            if (split != null) {
                Column {
                    Text(split.first, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                    Text(split.second, color = OnBgBody, style = Type.Body.merge(TabularFigures))
                }
            } else {
                Text(summary, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
            }
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                if (!isCheck) PlanItemFace(item, isRun, runHead, weightUnit, distanceUnit, lastOnFace)
                safetyReason?.let { FaceLine(it, color = OnBgBody) }
                note?.let { FaceLine(stripClaimIds(it)) }
                guide.forEach { FaceLine(it) }
                item.calis?.let { CalisControls(it, unplaced, onSetRung) }
            }
            when {
                isCheck -> FaceLine(stringResource(R.string.plan_check_any_session))
                !runUnlogged -> PlanItemCompletion(item, isRun, actionable = true, onEndExercise = onEndExercise)
            }
            if (actionLabel != null) {
                PlanActionPill(actionLabel, subject, onClick = if (isRun) onTrack else onLog)
            }
        }
    }
}

/**
 * One plan item as a single compact row: the name over its sets × reps @ load
 * (a run's distance or time), no "?", no buttons. With [actionLabel] the whole row
 * is a filled tile and the tap target that logs it (an open item of today's
 * session other than the current one: out-of-order logging stays one tap) and
 * ends in a chevron. Without it the row is a read-only preview (a future session):
 * plain text on the card, no tile, so it never looks tappable. Set slots appear
 * only once a set is logged, so a preview never shows dead empty boxes. A
 * safety-critical item keeps its SAFETY chip, and the return check / a
 * target-less run keep their safety note on the face. Tagged like
 * [OpenPlanItemCard].
 */
@Composable
internal fun PlanItemRow(
    item: PrescriptionView,
    isRun: Boolean,
    isCheck: Boolean = false,
    actionLabel: String? = null,
    onClick: () -> Unit = {},
) {
    val status = LocalStatusColors.current
    val distanceUnit = rememberDistanceUnit()
    val name = planItemName(item, isRun, isCheck)
    val detail = when {
        isCheck -> null
        isRun -> runFaceHead(item, distanceUnit) ?: item.face_lead
        else -> item.face_lead
    }?.takeIf { it.isNotBlank() && it != name }
    val note = noteOnFace(item, isRun, isCheck)?.let(::stripClaimIds)
    val runLogged = if (isRun && !isCheck) runProgressText(LocalContext.current.resources, item, distanceUnit) else ""
    val safetyReason = safetyReasonText(item, distanceUnit)
    val cd = actionLabel?.let { stringResource(R.string.main_action_subject, it, name) }
    val shape = RoundedCornerShape(Space.Card.dp)
    Box(modifier = Modifier.testTag(planItemTag(item, isRun, isCheck))) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .then(
                    if (cd != null) {
                        Modifier
                            .heightIn(min = ActionMinHeight)
                            .clip(shape)
                            .background(BgTop)
                            .clickable(role = Role.Button, onClick = onClick)
                            .semantics { contentDescription = cd }
                            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp)
                    } else {
                        Modifier.padding(vertical = Space.Xs.dp)
                    },
                ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                if (item.safety_critical && safetyReason == null) Chip(stringResource(R.string.main_chip_safety), status.danger)
                Text(name, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                detail?.let { Text(it, color = OnBgMuted, style = Type.Caption.merge(TabularFigures)) }
                safetyReason?.let { Text(it, color = OnBgBody, style = Type.Caption.merge(TabularFigures)) }
                note?.let { Text(it, color = OnBgMuted, style = Type.Caption) }
                if (runLogged.isNotBlank()) {
                    Text(stringResource(R.string.plan_run_logged, runLogged), color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
                }
            }
            if (!isRun && !isCheck && item.sets_done > 0) SetSlotsRow(item)
            if (cd != null) RowChevron()
        }
    }
}

/** The per-item line of an evidence block (load anchor, adjustment note) the
 *  item's face does not already state; null when there is none. */
@Composable
private fun itemEvidenceDetail(item: PrescriptionView, isRun: Boolean, isCheck: Boolean): String? {
    val lastOnFace = if (isRun || isCheck) null else lastLine(item)
    val note = noteOnFace(item, isRun, isCheck)
    return listOfNotNull(
        item.anchored_on.takeIf { it.isNotBlank() && it != lastOnFace },
        item.adjusted_note.takeIf { it.isNotBlank() && note == null },
    ).joinToString(" \u00b7 ").ifBlank { null }
}

/**
 * A session's evidence panel (its one "?"): items resting on the same evidence
 * source (grade, citation, grade note, what would improve it) share ONE block.
 * Each exercise gets one line with what is its own (the reasoning sentences the
 * others in the block don't share, and its load anchor), then the block states
 * once what they share as "Based on", why this grade with the confidence stated
 * once (a range when the items differ), what would improve it, the citation and
 * the glossary links. A Based-on / grade / to-improve block is never repeated
 * per exercise; an item on another source (a run beside lifts, a safety-critical
 * item) gets a block of its own.
 */
@Composable
internal fun SessionItemsEvidence(
    items: List<PrescriptionView>,
    isRun: (PrescriptionView) -> Boolean,
    isCheck: (PrescriptionView) -> Boolean,
    modifier: Modifier = Modifier,
) {
    val groups = remember(items) {
        items.groupBy {
            listOf(it.grade, it.citation, it.why.grade_note, it.why.improves, it.contested, it.safety_critical)
        }.values.toList()
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        groups.forEach { group ->
            val first = group.first()
            val shared = remember(group) { sharedBasisSentences(group.map { it.why.basis }) }
            Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
                group.forEach { item ->
                    val name = planItemName(item, isRun(item), isCheck(item))
                    val own = basisSentences(item.why.basis).filterNot { it in shared }.joinToString(" ")
                    val line = listOfNotNull(
                        own.ifBlank { null },
                        itemEvidenceDetail(item, isRun(item), isCheck(item))?.let(::stripClaimIds),
                    ).joinToString(" \u00b7 ")
                    Text(
                        if (line.isBlank()) name else stringResource(R.string.main_evidence_item_detail, name, line),
                        color = OnBgBody,
                        style = Type.Caption,
                    )
                }
                WhyDetail(
                    why = WhyView(basis = shared.joinToString(" "), grade_note = first.why.grade_note, improves = first.why.improves),
                    grade = first.grade,
                    citation = first.citation,
                    confidence = group.minOf { it.confidence },
                    confidenceHigh = group.maxOf { it.confidence },
                    contested = first.contested,
                    showConfidence = !first.safety_critical,
                    extraDetail = null,
                    glossaryKeys = remember(group) { group.flatMap { glossaryKeysIn(it.summary) }.distinct() },
                )
            }
        }
    }
}

/** A why-basis split into its sentences ("6×5 at 83% … 275 lb.", "Aim to …").
 *  A decimal point ("264.5") never splits. Pure. */
internal fun basisSentences(basis: String): List<String> =
    basis.trim().split(SentenceBreak).map { it.trim() }.filter { it.isNotEmpty() }

private val SentenceBreak = Regex("""(?<=[.!?])\s+""")

/** The basis sentences every one of [bases] states, in the first one's order:
 *  what a shared evidence block says once. For a single item, all of them. Pure. */
internal fun sharedBasisSentences(bases: List<String>): List<String> {
    val split = bases.map(::basisSentences)
    val head = split.firstOrNull() ?: return emptyList()
    return head.filter { s -> split.all { s in it } }
}

/** A settled plan item collapsed to one row (ordered after the open items). A
 *  lift/calisthenics item reads "k/N" with a ✓, "Ended early k/N", "k/N · short
 *  reps" (every set at the load, some reps missed) or "k/N · lighter" (below the
 *  load); a skipped one reads "Skipped" (or "Skipped · pain in <[painArea]>") and one a pain
 *  report stopped after some sets "Stopped · <[painArea]>", both with a neutral
 *  dash and never the done ✓. An exercise the user ended (skipped or early)
 *  offers "Reopen". An item done by its logged sets has nothing to reopen (the
 *  sets still count), and neither does a run (it completes from logged
 *  distance/time) or the return check, so those show no Reopen. */
@Composable
internal fun DonePlanItemRow(
    item: PrescriptionView,
    isRun: Boolean,
    reopenable: Boolean,
    onReopen: () -> Unit,
    isCheck: Boolean = false,
    // The reported body area ("Left knee"), when known.
    painArea: String? = null,
) {
    val status = LocalStatusColors.current
    val tag = planItemTag(item, isRun, isCheck)
    val stopped = !isCheck && item.status == "stopped_pain"
    val skipped = stopped || (!isCheck && (item.status == "skipped" || item.status == "skipped_pain"))
    val progress = when {
        isCheck -> ""
        isRun -> runProgressText(LocalContext.current.resources, item, rememberDistanceUnit())
        else -> "${item.sets_done}/${plannedSetCount(item)}"
    }
    val label = when {
        stopped -> stoppedLabel(painArea)
        item.status == "skipped_pain" && !isCheck -> painArea
            ?.let { stringResource(R.string.plan_skipped_pain_area, it.lowercase(Locale.getDefault())) }
            ?: stringResource(R.string.plan_skipped_pain)
        skipped -> stringResource(R.string.plan_skipped)
        item.ended_early && !isCheck -> stringResource(R.string.plan_ended_early, item.sets_done, plannedSetCount(item))
        item.status == "done_short" && progress.isNotBlank() -> stringResource(R.string.plan_done_short, progress)
        item.status == "done_lighter" && progress.isNotBlank() -> stringResource(R.string.plan_done_lighter, progress)
        else -> progress
    }
    val title = planItemName(item, isRun, isCheck)
    Row(
        modifier = Modifier
            .testTag(tag)
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgTop)
            .padding(horizontal = Space.Card.dp, vertical = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        // A skip is neither done nor failed: a neutral dash, never the green ✓.
        Text(
            if (skipped) "\u2013" else "\u2713",
            color = if (skipped) OnBgMuted else status.done,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
        )
        Text(title, color = OnBgMuted, style = Type.Body, modifier = Modifier.weight(1f))
        if (label.isNotBlank()) Text(label, color = OnBgFaint, style = Type.Caption.merge(TabularFigures))
        if (reopenable && (item.ended_early || (skipped && !stopped)) && !isRun && !isCheck) {
            Spacer(Modifier.size(Space.Xs.dp))
            val reopen = stringResource(R.string.plan_reopen)
            TextAction(
                reopen,
                onReopen,
                description = stringResource(R.string.plan_secondary_action_cd, reopen, item.exercise),
            )
        }
    }
}

/** "Stopped · Left knee" for an exercise a pain report ended after some sets
 *  ("Stopped · pain" when no area is known). */
@Composable
internal fun stoppedLabel(painArea: String?): String =
    painArea?.let { stringResource(R.string.plan_stopped_area, it) } ?: stringResource(R.string.plan_stopped_pain)

/** The gym-floor loop header inside the set editor, painted as a card (BgElevated
 *  + hairline) so it keeps its bounds on the sheet's sunken ground in both themes.
 *  One compact first row reads in a glance: "Set 2 of 6 · 5 reps · ~1 rep left"
 *  on the left (an off-plan exercise: "Set k"; a finished item: "Exercise
 *  complete") and the rest readout on the right. Under it the coach's in-session
 *  cue for this exercise with its one-tap actions ("Use 140 kg", "End
 *  exercise"), then the set slots with the quiet exits ("Log extra set" once the
 *  item is complete, "Done for now" when [showDone]: it closes the loop and
 *  leaves the session in progress), then the last set with Edit. The way forward
 *  after a completed item is the editor's primary action; [nextItem] is offered
 *  here only when that action is busy logging an extra set. The caller guards an
 *  unsaved set on "Done for now". */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SetLoopHeader(
    item: PrescriptionView?,
    // "Set k" for an off-plan exercise; null while no exercise is named yet.
    offPlanSetNumber: Int?,
    // Newest set logged today for this exercise (unix seconds) and its line.
    lastSetSec: Long?,
    lastSetLine: String?,
    // The coach's in-session cue for this exercise (one line), the label of its
    // load action and the actions themselves; null hides each.
    cueText: String?,
    cueLoadLabel: String?,
    onUseCueLoad: (() -> Unit)?,
    onEndExercise: (() -> Unit)?,
    nextItem: PrescriptionView?,
    onDone: () -> Unit,
    onNext: (PrescriptionView) -> Unit,
    onEditLast: (() -> Unit)?,
    onLogExtra: (() -> Unit)?,
    showDone: Boolean = true,
) {
    val done = item?.done == true
    val shape = RoundedCornerShape(Space.Card.dp)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(BgElevated)
            .border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Card), shape)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            val title = when {
                // Never repeats the sheet title ("Log set"): an unnamed
                // off-plan exercise is simply on its first set.
                item == null -> stringResource(R.string.log_set_header_n, offPlanSetNumber ?: 1)
                done -> stringResource(R.string.plan_item_complete)
                else -> stringResource(R.string.log_set_header_n_of_n, nextSetNumber(item), plannedSetCount(item))
            }
            // The rest length is not repeated here: the rest readout counts it.
            val band = if (item != null && !done) setTargetBand(LocalContext.current.resources, item) else ""
            // The set count and the target share the line as two texts, so the
            // count stays its own readable node.
            Row(modifier = Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                Text(title, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
                if (band.isNotBlank()) {
                    Text(
                        " \u00b7 $band",
                        color = OnBgMuted,
                        style = Type.Body,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                }
            }
            RestReadout(lastSetSec, item?.rest_sec ?: 0)
        }
        if (cueText != null) {
            Text(
                cueText,
                color = OnBgBody,
                style = Type.Caption,
                modifier = Modifier
                    .testTag("loop-cue")
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
            val useLoad = if (cueLoadLabel != null) onUseCueLoad else null
            if (useLoad != null || onEndExercise != null) {
                FlowRow(
                    modifier = Modifier.offset(x = -TextActionInset),
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    if (cueLoadLabel != null && useLoad != null) TextAction(cueLoadLabel, useLoad)
                    if (onEndExercise != null) TextAction(stringResource(R.string.log_cue_end_exercise), onEndExercise)
                }
            }
        }
        if ((item != null && !done) || onLogExtra != null || showDone) {
            // Held at the touch-target height from the start, so the first log
            // adding "Done for now" never pushes the warm-up chips down.
            Row(Modifier.heightIn(min = ActionMinHeight), verticalAlignment = Alignment.CenterVertically) {
                if (item != null && !done) SetSlotsRow(item)
                Spacer(Modifier.weight(1f))
                if (onLogExtra != null) TextAction(stringResource(R.string.log_primary_extra_set), onLogExtra)
                if (showDone) TextAction(stringResource(R.string.log_set_done), onDone)
            }
        }
        if (lastSetLine != null && onEditLast != null) {
            val shown = stringResource(R.string.log_last_set, lastSetLine)
            val editCd = stringResource(R.string.log_edit_set_cd, lastSetLine)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .clip(RoundedCornerShape(Space.Sm.dp))
                    .clickable(role = Role.Button, onClick = onEditLast)
                    .semantics { contentDescription = editCd },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
            ) {
                Text(
                    shown,
                    color = OnBgMuted,
                    style = Type.Caption.merge(TabularFigures),
                    modifier = Modifier.weight(1f),
                    maxLines = 1,
                )
                Text(
                    stringResource(R.string.log_edit),
                    color = Accent,
                    style = Type.Caption.copy(fontWeight = FontWeight.Bold),
                )
            }
        }
        if (done && nextItem != null) {
            TextAction(
                stringResource(R.string.plan_next_exercise, nextItem.exercise),
                { onNext(nextItem) },
                modifier = Modifier.offset(x = -TextActionInset),
            )
        }
    }
}

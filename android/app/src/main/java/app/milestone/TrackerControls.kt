package app.milestone

import android.content.res.Resources
import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay

/** Gap between the controls of one control row. */
private val ControlGap = (Space.Md + Space.Sm).dp

/** Diameter of a status-pill dot. */
private val StatusDotSize = 8.dp

/** How often the status pill re-ages the last fix while its state can still change. */
private const val PILL_TICK_MS = 1000L

private val PillShape = RoundedCornerShape(100)

/** The stat sheet's control row: a circle, then the pill(s) sharing the rest. */
@Composable
internal fun ControlRow(content: @Composable RowScope.() -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ControlGap),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/**
 * The control row under the live figures, one layout per recording state:
 *  - captured-but-unsaved run (the save failed or the stop never confirmed): Discard
 *    (confirmed) + Save (retry), NEVER Start, which would truncate the sidecar;
 *  - armed: Cancel + "Start now" (skips the gates: a treadmill, a slow start, a GPS
 *    hiccup); recording otherwise begins by itself at the first qualifying fix;
 *  - locate preview: Back + Start, ALWAYS tappable (an armed start waits for a live
 *    qualifying fix, so a stale lock can never let a tap record nothing);
 *  - paused: Stop (red outline) + Resume as the wide primary, the likely next tap;
 *  - recording: Pause + Stop. Stop is the primary completion action (Accent, not a
 *    destructive red) and only opens the Finish review; Discard lives there, never
 *    beside Stop mid-run, so one stray tap can't end a run.
 * [stopping] (a save in flight) disables every action that could race it.
 */
@Composable
internal fun TrackerControlRow(
    unsavedCapture: Boolean,
    armed: Boolean,
    tracking: Boolean,
    paused: Boolean,
    stopping: Boolean,
    onDiscardCapture: () -> Unit,
    onSaveCapture: () -> Unit,
    onLeave: () -> Unit,
    onStartNow: () -> Unit,
    onStart: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onStop: () -> Unit,
) {
    ControlRow {
        when {
            unsavedCapture -> {
                ControlCircle(
                    R.drawable.ic_ui_close,
                    stringResource(R.string.track_cd_discard_run),
                    danger = true,
                    enabled = !stopping,
                    iconSize = 20.dp,
                    onClick = onDiscardCapture,
                )
                ControlPill(
                    stringResource(R.string.track_btn_save_run),
                    onClick = onSaveCapture,
                    modifier = Modifier.weight(1f),
                    enabled = !stopping,
                )
            }
            armed -> {
                ControlCircle(R.drawable.ic_ui_close, stringResource(R.string.main_cancel), onClick = onLeave)
                ControlPill(
                    stringResource(R.string.track_btn_start_now),
                    onClick = onStartNow,
                    modifier = Modifier.weight(1f),
                    primary = false,
                )
            }
            !tracking -> {
                ControlCircle(R.drawable.ic_ui_close, stringResource(R.string.track_cd_back), onClick = onLeave)
                ControlPill(
                    stringResource(R.string.track_btn_start),
                    onClick = onStart,
                    modifier = Modifier.weight(1f),
                )
            }
            paused -> {
                ControlCircle(
                    R.drawable.ic_track_stop,
                    stringResource(R.string.track_btn_stop),
                    danger = true,
                    enabled = !stopping,
                    iconSize = 18.dp,
                    onClick = onStop,
                )
                ControlPill(
                    stringResource(R.string.track_action_resume),
                    onClick = onResume,
                    modifier = Modifier.weight(1f),
                    iconRes = R.drawable.ic_track_play,
                    state = stringResource(R.string.track_state_paused),
                )
            }
            else -> {
                ControlCircle(
                    R.drawable.ic_track_pause,
                    stringResource(R.string.track_action_pause),
                    state = stringResource(R.string.track_state_recording),
                    onClick = onPause,
                )
                ControlPill(
                    stringResource(R.string.track_btn_stop),
                    onClick = onStop,
                    modifier = Modifier.weight(1f),
                    enabled = !stopping,
                    iconRes = R.drawable.ic_track_stop,
                )
            }
        }
    }
}

/** Circular secondary control on the stat sheet. [danger] tints the outline and
 *  glyph red (discard, stop); [state] is spoken as the control's state. */
@Composable
internal fun ControlCircle(
    iconRes: Int,
    description: String,
    danger: Boolean = false,
    enabled: Boolean = true,
    iconSize: Dp = 22.dp,
    state: String? = null,
    onClick: () -> Unit,
) {
    val status = LocalStatusColors.current
    Box(
        modifier = Modifier
            .size(LargeControlHeight)
            .clip(PillShape)
            .background(BgElevated)
            .border(
                1.dp,
                if (danger) status.danger.copy(alpha = 0.5f) else OnBgBody.copy(alpha = HairlineAlpha.Control),
                PillShape,
            )
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .then(if (state != null) Modifier.semantics { stateDescription = state } else Modifier),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painterResource(iconRes),
            contentDescription = description,
            tint = if (danger) status.danger else OnBgBody,
            modifier = Modifier.size(iconSize),
        )
    }
}

/** The wide pill on the control row: [primary] is the Accent-filled main action
 *  (Start, Stop, Resume while paused, Save run); otherwise the outlined secondary. */
@Composable
internal fun ControlPill(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = true,
    enabled: Boolean = true,
    iconRes: Int? = null,
    state: String? = null,
) {
    val content = if (primary) OnAccent else OnBgBody
    Row(
        modifier = modifier
            .height(LargeControlHeight)
            .clip(PillShape)
            .then(
                if (primary) {
                    Modifier.background(Accent)
                } else {
                    Modifier.background(BgElevated).border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Control), PillShape)
                },
            )
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .then(if (state != null) Modifier.semantics { stateDescription = state } else Modifier),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (iconRes != null) {
            Icon(painterResource(iconRes), contentDescription = null, tint = content, modifier = Modifier.size(18.dp))
        }
        Text(label, color = content, style = Type.Body.copy(fontWeight = FontWeight.ExtraBold))
    }
}

/** System location is off: the loud warn row, with "Open settings" as its fix. */
@Composable
internal fun LocationOffNotice(onOpenSettings: () -> Unit) {
    val status = LocalStatusColors.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(status.warn)
            .padding(Space.Card.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Text(
            stringResource(R.string.track_location_off_caption),
            color = Color.White,
            style = Type.Body,
        )
        Text(
            stringResource(R.string.track_open_settings),
            color = Color.White,
            style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
            modifier = Modifier
                .clip(PillShape)
                .heightIn(min = ActionMinHeight)
                .clickable(role = Role.Button, onClick = onOpenSettings)
                .wrapContentHeight(Alignment.CenterVertically)
                .padding(vertical = Space.Xs.dp),
        )
    }
}

@Composable
private fun StatusDot(color: Color) {
    Box(
        Modifier
            .size(StatusDotSize)
            .clip(PillShape)
            .background(color),
    )
}

/**
 * The map's status pill (one actionable state line behind a state dot), a leaf so
 * the 1 Hz freshness ticker it owns recomposes ONLY the pill, not the whole tracking
 * screen. The ticker ages the last accepted fix so [isFreshLock] decays a stale lock
 * and the signal-loss state can fire. It runs while a lock is still decaying, while
 * armed or locating (so a fresh preview lock lights within its window), or while a
 * recording run ages toward the signal-loss threshold; once none holds, the state is
 * stable until the next fix stamp re-triggers it. The signal-loss BUZZ is the
 * service's (screen-off safe); this pill is the on-screen mirror of the same
 * [signalLossDue]. Exposed to screen readers as ONE polite live region carrying only
 * the state, so GPS drops, pauses and auto-start are announced without leaving the
 * map. Named to avoid the [TrackerPill] enum it renders.
 *
 * [permissionAsked]: before the system prompt was ever shown a missing grant is not a
 * fault yet (the rationale asks for it), so the pill stays neutral.
 */
@Composable
internal fun TrackerStatusPill(
    hasPermission: Boolean,
    locationOn: Boolean,
    tracking: Boolean,
    paused: Boolean,
    armed: Boolean,
    locating: Boolean,
    requirePrecise: Boolean,
    requireMoving: Boolean,
    onRecheckPermission: () -> Unit,
    permissionAsked: Boolean,
    modifier: Modifier = Modifier,
) {
    val status = LocalStatusColors.current
    val lastFix by RunSession.lastFix.collectAsStateWithLifecycle()
    val lastFixElapsedMs by RunSession.lastFixElapsedMs.collectAsStateWithLifecycle()
    var nowElapsed by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(lastFixElapsedMs) { nowElapsed = SystemClock.elapsedRealtime() }
    val freshLock = isFreshLock(lastFix?.accuracyM, lastFixElapsedMs, nowElapsed)
    val signalLost = tracking && signalLossDue(lastFixElapsedMs, nowElapsed)
    val needsTick = freshLock || armed || locating || (tracking && !signalLost)
    LaunchedEffect(needsTick) {
        while (needsTick) {
            // Re-check the location grant each tick: a mid-run revocation must read as
            // PermissionNeeded (which outranks SignalLost in [pillState]), not a lost fix.
            onRecheckPermission()
            nowElapsed = SystemClock.elapsedRealtime()
            delay(PILL_TICK_MS)
        }
    }
    // The pure [pillState] ladder: a missing grant / disabled location outranks all
    // (so the pill can't say "Acquiring GPS…" while it can't get a fix and contradict
    // the sheet); then signal-loss / paused / recording while tracking; gate-aware
    // waiting text while armed; then fresh lock / acquiring. Never a fake position.
    val pill = pillState(hasPermission, locationOn, tracking, paused, armed, signalLost, freshLock, requirePrecise, requireMoving)
    // Only states that block or break recording wear the warn fill; the benign
    // waiting states (acquiring, armed) read as a neutral pill with a muted dot.
    val pillWarn = when (pill) {
        TrackerPill.PermissionNeeded -> permissionAsked
        TrackerPill.LocationOff, TrackerPill.SignalLost -> true
        else -> false
    }
    val dot = when (pill) {
        TrackerPill.Recording, TrackerPill.FreshLock -> Accent
        TrackerPill.Paused -> OnBgFaint
        else -> OnBgMuted
    }
    // The spoken form is state-only: the fresh-lock accuracy figure changes on nearly
    // every fix, which would make a live region chatter, so it reads a stable phrase.
    val pillSpoken = if (pill == TrackerPill.FreshLock) stringResource(R.string.track_cd_pill_fresh_lock) else null
    Row(
        modifier = modifier
            .clip(PillShape)
            .background(if (pillWarn) status.warn else BgElevated)
            .then(if (pillWarn) Modifier else Modifier.border(1.dp, OnBgBody.copy(alpha = HairlineAlpha.Divider), PillShape))
            .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!pillWarn) StatusDot(dot)
        val pillText = when (pill) {
            TrackerPill.PermissionNeeded -> stringResource(R.string.track_pill_permission_needed)
            TrackerPill.LocationOff -> stringResource(R.string.track_pill_location_off)
            TrackerPill.SignalLost -> stringResource(R.string.track_pill_signal_lost)
            TrackerPill.Paused -> stringResource(R.string.track_pill_paused)
            TrackerPill.Recording -> stringResource(R.string.track_pill_recording)
            TrackerPill.ArmedMoving -> stringResource(R.string.track_pill_armed_moving)
            TrackerPill.ArmedPrecise -> stringResource(R.string.track_pill_armed_precise)
            TrackerPill.ArmedWaiting -> stringResource(R.string.track_pill_armed_waiting)
            TrackerPill.FreshLock -> stringResource(
                R.string.track_pill_fresh_lock,
                lastFix?.accuracyM?.let { Math.round(it) } ?: 0L,
            )
            TrackerPill.Acquiring -> stringResource(R.string.track_pill_acquiring)
        }
        // Tabular figures only where the text carries digits (accuracy / states that
        // could tick), so glyph widths stay stable.
        val figures = pill == TrackerPill.FreshLock ||
            pill == TrackerPill.Recording || pill == TrackerPill.Paused
        Text(
            pillText,
            color = if (pillWarn) Color.White else OnBgBody,
            style = if (figures) Type.Caption.merge(TabularFigures) else Type.Caption,
            modifier = Modifier.clearAndSetSemantics {
                contentDescription = pillSpoken ?: pillText
                liveRegion = LiveRegionMode.Polite
            },
        )
    }
}

/**
 * The locate-preview start options: "Follow today's plan" (a switch with the plan's
 * targets under it, when [planItem] exists), then the two independent, stackable,
 * persisted, default-off start gates as checkbox chips under their own overline,
 * with one caption below them that explains only what is on ([startGatesCaptionRes]).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun TrackerStartOptions(
    planItem: PrescriptionView?,
    followPlan: Boolean,
    onFollowPlan: (Boolean) -> Unit,
    unit: DistanceUnit,
    preciseGate: Boolean,
    onTogglePrecise: () -> Unit,
    movingGate: Boolean,
    onToggleMoving: () -> Unit,
) {
    if (planItem != null) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
            LabeledSwitchRow(
                label = stringResource(R.string.track_plan_follow),
                checked = followPlan,
                onCheckedChange = onFollowPlan,
            )
            if (followPlan) RunPlanPreview(item = planItem, unit = unit)
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        TileOverline(stringResource(R.string.track_gates_overline))
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
            verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
        ) {
            ChoiceChip(
                stringResource(R.string.track_gate_precise),
                selected = preciseGate,
                role = Role.Checkbox,
                onClick = onTogglePrecise,
            )
            ChoiceChip(
                stringResource(R.string.track_gate_moving),
                selected = movingGate,
                role = Role.Checkbox,
                onClick = onToggleMoving,
            )
        }
        Text(stringResource(startGatesCaptionRes(preciseGate, movingGate)), color = OnBgMuted, style = Type.Caption)
    }
}

/** The start-gates caption for what is on: one sentence, never both explainers. */
internal fun startGatesCaptionRes(precise: Boolean, moving: Boolean): Int = when {
    precise && moving -> R.string.track_gates_caption_both
    precise -> R.string.track_gates_caption_precise
    moving -> R.string.track_gates_caption_moving
    else -> R.string.track_gates_caption_none
}

/** Before Start: the planned run the tracker will follow, with its targets. */
@Composable
private fun RunPlanPreview(item: PrescriptionView, unit: DistanceUnit) {
    val res = LocalContext.current.resources
    val targets = remember(item.run_target, unit) { runTargetLines(res, item.run_target, unit) }
    val headline = remember(item, unit) { runPlanHeadline(item, unit) }
    PlanCard {
        Text(
            stringResource(R.string.track_plan_title, headline),
            color = OnBgBody,
            style = Type.Body.copy(fontWeight = FontWeight.Bold),
        )
        TargetLines(targets)
    }
}

/** The elevated card the plan preview and the live step guide share. */
@Composable
private fun PlanCard(content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(BgElevated)
            .padding(Space.Card.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        content = content,
    )
}

/** The pre-start plan preview's targets, one per line. */
@Composable
private fun TargetLines(targets: List<String>) {
    targets.forEach { Text(it, color = OnBgMuted, style = Type.Caption.merge(TabularFigures)) }
}

/**
 * While recording a followed plan: the plan's targets folded into one muted line
 * under the live figures ([runTargetSummary]), then, for a guided run, the current
 * step (its name with Skip step on the same row, what is left of it, what comes
 * next). The step title is a polite live region so TalkBack announces each new step;
 * the per-second remaining figure is not, so it doesn't chatter. Renders nothing
 * without steps or targets.
 */
@Composable
internal fun RunGuideCard(item: PrescriptionView, progress: StepProgress?, unit: DistanceUnit, onSkip: () -> Unit) {
    val res = LocalContext.current.resources
    val targets = remember(item.run_target, unit) { runTargetSummary(res, item.run_target, unit) }
    if (progress == null && targets == null) return
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        targets?.let { Text(it, color = OnBgMuted, style = Type.Caption.merge(TabularFigures)) }
        if (progress != null) RunStepCard(res, progress, onSkip)
    }
}

/** The guided run's current step, what is left of it, and the step after it. */
@Composable
private fun RunStepCard(res: Resources, progress: StepProgress, onSkip: () -> Unit) {
    PlanCard {
        val step = progress.step
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val title = if (step != null) runStepTitle(res, step) else stringResource(R.string.track_step_all_done)
            // The step name is the label; what is LEFT of it is the figure the
            // runner watches mid-interval, so it carries the display size.
            Text(
                title,
                color = OnBgBody,
                style = Type.Body.copy(fontWeight = FontWeight.Bold),
                modifier = Modifier
                    .weight(1f)
                    .semantics { liveRegion = LiveRegionMode.Polite },
            )
            if (!progress.finished) {
                // End-aligned: the label (not its touch padding) meets the card edge.
                TextAction(
                    stringResource(R.string.track_step_skip),
                    onClick = onSkip,
                    modifier = Modifier.offset(x = TextActionInset),
                )
            }
        }
        if (step != null) {
            val counting = progress.remainingSec != null || progress.remainingM != null
            Text(
                runStepRemaining(res, progress),
                color = if (counting) OnBgBody else OnBgMuted,
                style = if (counting) Type.Countdown else Type.Caption,
            )
        }
        progress.next?.let { next ->
            Text(
                stringResource(R.string.track_step_next, runStepTitle(res, next)),
                color = OnBgMuted,
                style = Type.Caption,
                modifier = Modifier.padding(top = Space.Xs.dp),
            )
        }
    }
}

/**
 * The stat sheet while precise location is missing: one caption that matches what
 * the user actually allowed ([LocationPrompt]), then ✕ (leave, like before Start)
 * beside the primary action, Continue before the system prompt or Allow location
 * after it. Once refused, "Log run by hand" is the way out without GPS.
 */
@Composable
internal fun LocationPermissionPanel(
    prompt: LocationPrompt,
    onAllow: () -> Unit,
    onClose: () -> Unit,
    onLogManually: (() -> Unit)?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        Text(
            stringResource(
                when (prompt) {
                    LocationPrompt.Rationale -> R.string.track_perm_rationale
                    LocationPrompt.Denied -> R.string.track_perm_denied_caption
                    LocationPrompt.CoarseOnly -> R.string.track_perm_coarse_caption
                    LocationPrompt.Blocked -> R.string.track_perm_blocked_caption
                },
            ),
            color = if (prompt == LocationPrompt.Rationale) OnBgBody else OnBgMuted,
            style = Type.Body,
        )
        if (prompt != LocationPrompt.Rationale && onLogManually != null) {
            // A text action, so no tonal slab sits over the primary below it.
            TextAction(
                stringResource(R.string.track_btn_log_manually),
                onClick = onLogManually,
                chevron = true,
                modifier = Modifier.offset(x = -TextActionInset),
            )
        }
        ControlRow {
            ControlCircle(R.drawable.ic_ui_close, stringResource(R.string.main_close), onClick = onClose)
            ControlPill(
                stringResource(
                    if (prompt == LocationPrompt.Rationale) R.string.track_btn_continue else R.string.track_btn_allow_location,
                ),
                onClick = onAllow,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.testTag
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay

/** How long the "Logged … · Undo" row stays after a loop save. */
private const val UndoWindowMs = 8_000L

/** Snackbar-style "Logged … · Undo" row above the loop's primary action. The
 *  line is announced politely once, so a screen-reader user hears what was saved. */
@Composable
private fun UndoRow(text: String, undoDescription: String, onUndo: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(OnBgBody)
            .padding(start = Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text,
            color = BgTop,
            style = Type.Caption,
            maxLines = 2,
            modifier = Modifier
                .weight(1f)
                .padding(vertical = Space.Sm.dp)
                .semantics { liveRegion = LiveRegionMode.Polite },
        )
        Box(
            modifier = Modifier
                .heightIn(min = ActionMinHeight)
                .clip(RoundedCornerShape(Space.Card.dp))
                .clickable(role = Role.Button, onClick = onUndo)
                .semantics { contentDescription = undoDescription }
                .padding(horizontal = Space.Card.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                stringResource(R.string.log_undo),
                color = BgTop,
                style = Type.Body.copy(fontWeight = FontWeight.ExtraBold),
            )
        }
    }
}

/** Exercise search well (UI sweep §8): the keypad field's look, a `BgSunken` well
 *  in a 1dp [ChoiceCellBorder] hairline (1.5dp `Accent` while focused), with a
 *  `ui-search` 18dp glyph, a Bold value and an `OnBgFaint` "Search exercises"
 *  placeholder. No dropdown; typing filters the quick-pick chips below it. The
 *  keyboard capitalises the first word and its Done key folds it, so it never
 *  stays up over the in-form keypad. */
@Composable
private fun ExerciseSearchField(value: String, onChange: (String) -> Unit, onFocusChange: (Boolean) -> Unit = {}) {
    val focusManager = LocalFocusManager.current
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Space.Card.dp)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(shape)
            .background(BgSunken)
            .border(if (focused) 1.5.dp else 1.dp, if (focused) Accent else ChoiceCellBorder, shape)
            .padding(Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Icon(
            painterResource(R.drawable.ic_ui_search),
            contentDescription = null,
            tint = OnBgFaint,
            modifier = Modifier.size(18.dp),
        )
        BasicTextField(
            value = value,
            onValueChange = onChange,
            singleLine = true,
            textStyle = Type.Body.copy(fontWeight = FontWeight.Bold, color = OnBgBody),
            cursorBrush = SolidColor(Accent),
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.Sentences,
                imeAction = ImeAction.Done,
            ),
            keyboardActions = KeyboardActions(onDone = { focusManager.clearFocus() }),
            modifier = Modifier.weight(1f).testTag("exercise-search").onFocusChanged {
                focused = it.isFocused
                onFocusChange(it.isFocused)
            },
            decorationBox = { inner ->
                if (value.isEmpty()) {
                    Text(stringResource(R.string.log_exercise_search_hint), color = OnBgFaint, style = Type.Body)
                }
                inner()
            },
        )
    }
}

/**
 * Manual lift-set entry. Emits a real [Event.LogSet] with the user's own
 * exercise / weight / reps / RPE; the e1RM + RIR derivation lives in the Rust
 * core, so this form carries no coaching logic. Weight is typed on the in-form
 * [NumericKeypad]; the buffer text is what gets parsed at submit, so the
 * display-committed invariant holds (an invalid buffer blocks the button).
 * Exercise quick-pick chips sit over the free-text field: today's open plan
 * items, then the user's own recent lifts, then the rest of the week's plan,
 * filtered by what is typed.
 *
 * Shell-thinness: no live "→ e1RM …" preview. e1RM is derived only in the
 * core, after submit; a preview would need a core query, never Kotlin math.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun LogSetEditor(
    recentExercises: List<String> = emptyList(),
    // Names of today's + this week's prescribed exercises: the tail of the quick
    // picks (fed by LogSheet.kt), after today's open items and the user's recents.
    plannedExercises: List<String> = emptyList(),
    // Canonical names of today's prescribed TIMED-HOLD rungs. When the picked
    // exercise matches one (case-insensitive), the editor logs a hold (seconds)
    // instead of reps and relabels the weight field "added load".
    holdExercises: Set<String> = emptySet(),
    // Canonical names of prescribed PURE-BODYWEIGHT rungs (added load 0): the
    // weight/added-load field + keypad are hidden for these (a Wall push-up has no
    // load to enter). Weighted rungs and barbell lifts keep the field.
    bodyweightExercises: Set<String> = emptySet(),
    // Non-null → EDIT mode: prefill from this logged set and emit
    // an AmendSet targeting its entry_id instead of a fresh LogSet.
    initial: LiftResultView? = null,
    // NEW-mode coach prefill; ignored when initial != null (EDIT wins).
    prescription: PrescriptionView? = null,
    // NEW-mode exercise to open on when there is no prescription (a calisthenics
    // rung picked from the Log chooser): re-seeded like a quick-pick chip.
    initialExercise: String? = null,
    // Today's actionable session, for the gym-floor loop: the live "Set k of N",
    // the prescribed rest, and the "Next: <exercise>" jump once the current item
    // completes. Null when not logging today's live plan (past-day / blank entry).
    planSession: SessionPlanView? = null,
    // The user's own logged sets (chronological): a chip switch to an off-plan
    // lift re-seeds from its last set, and the loop counts/edits today's sets.
    recentLifts: List<LiftResultView> = emptyList(),
    // Display unit for the weight field. The WIRE value is always kg (the core is
    // SI); lb is a pure display edge — prefill converts kg→lb, Save converts lb→kg.
    weightUnit: WeightUnit = WeightUnit.Kg,
    // Seed the stamp to a plan day's local noon when logging a PAST prescribed
    // session from the week strip; null falls back to now. Chip stays editable.
    initialObservedAt: Long? = null,
    // The coach's adjustments (model.adjustments) and session-review verdicts
    // (model.review_adjustments): the loop shows the current exercise's
    // in-session cue from them, with its one-tap actions.
    adjustments: List<AdjustmentView> = emptyList(),
    reviewAdjustments: List<AdjustmentView> = emptyList(),
    onClose: () -> Unit = {},
    // Report unsaved-edits state so a host sheet's swipe-down can guard it.
    onDirtyChange: (Boolean) -> Unit = {},
    // Whether the host sheet must refuse a swipe or scrim tap: today's planned
    // loop, or any loop once a set is logged in it (close via X or "Done for now" only).
    onSheetLockChange: (Boolean) -> Unit = {},
    // Close the whole sheet: the after-Save close for an EDIT (amend) or a
    // past-day set, and the planned loop's X. A NEW today set loops instead.
    onDone: () -> Unit = {},
    // The loop's "Done for now" / "Finish session": closes the sheet like [onDone]; the
    // host also returns Today to its top.
    onFinish: () -> Unit = onDone,
    onSave: (Event) -> Unit,
) {
    val ctx = LocalContext.current
    val haptic = LocalHapticFeedback.current
    val focusManager = LocalFocusManager.current
    // NEW-entry coach prefill (null in EDIT mode): seeds the fields below, all
    // still user-editable. `initial` always wins so an EDIT is never clobbered.
    val rx = if (initial == null) prescription?.let { prescriptionPrefill(it, weightUnit, AppliedCueLoads.forItem(it)) } else null
    // Field seeds: an edited row prefills from itself, a coach prescription seeds a
    // NEW entry, and a blank open seeds NOTHING (empty exercise/weight, no
    // highlighted reps/RPE) so the editor fabricates no entry (HARD RULE 1).
    val seed = remember(initial, rx, weightUnit) { freshSetSeed(initial, rx, weightUnit) }
    // The entry fields. Reps keep a logged count past the tap range (a 60-rep
    // air-squat set): it is saved as is; the row just won't highlight it. Effort
    // is a per-set self-report: a NEW set starts unrated (the coach's target is
    // only a hint on the scale), an untouched Save logs it unrated (0.0, which the
    // core reads as "not recorded"), and it is never carried into the next set.
    var fields by rememberSaveable(stateSaver = SetEditorState.Saver) { mutableStateOf(SetEditorState.from(seed)) }
    // Case-insensitive canonical names of today's prescribed hold rungs.
    val holdSet = remember(holdExercises) { holdExercises.map { it.trim().lowercase() }.toSet() }
    // Hold mode follows the CURRENT exercise: a prescribed hold rung, OR editing an
    // existing hold set while its exercise is unchanged. Renaming an edited hold set
    // to a barbell lift drops hold mode (so it can't emit a "Bench — 30 s hold" row
    // or pollute the Bench e1RM with a phantom 1-rep set).
    val holdMode = fields.exercise.trim().lowercase() in holdSet ||
        (initial?.hold_sec != null && fields.exercise.trim() == initial.exercise.trim())
    // A pure-bodyweight rung (prescribed added load 0) has no weight to enter, so
    // the field + keypad below are hidden. Follows the CURRENT exercise like
    // holdMode; in EDIT mode only when the logged weight is already 0, so editing a
    // weighted set can never silently drop its load.
    val bodyweightSet = remember(bodyweightExercises) { bodyweightExercises.map { it.trim().lowercase() }.toSet() }
    val bodyweightMode = fields.exercise.trim().lowercase() in bodyweightSet &&
        (initial?.weight_kg ?: 0.0) == 0.0
    // Auto-manage the (added-)load buffer only for a blank, no-load editor as the
    // user types a hold-rung name: reset to "0" (hold) / "" (reps). A reseeded or
    // hand-entered load (weightVerbatim set / weightFresh cleared) is left untouched.
    LaunchedEffect(holdMode) {
        if (initial == null && fields.weightFresh && fields.weightVerbatim == null) {
            fields = fields.copy(weightText = if (holdMode) "0" else "")
        }
    }
    var observedAt by rememberSaveable {
        mutableStateOf(
            initial?.observed_at?.takeIf { it > 0 } ?: initialObservedAt ?: AppClock.nowSec()
        )
    }
    // Whether the user picked the stamp. An untouched loop set is stamped when it is
    // logged (the rest timer and set order read these stamps), not when the editor
    // last re-seeded.
    var observedAtTouched by rememberSaveable { mutableStateOf(false) }
    var dirty by rememberSaveable { mutableStateOf(false) }
    ReportDirty(dirty, onDirtyChange)
    // A legacy row (entry_id == 0) is amended by matching its original stamp,
    // so its date can't be changed without orphaning it; lock the date chip.
    val dateEditable = initial == null || initial.entry_id != 0L
    // The gym-floor loop: every NEW set logged for today (from the plan, the
    // "Today's plan" tile, or a one-off "Log set") keeps the sheet open and
    // re-seeds for the next set. A past-day log (initialObservedAt set) and an EDIT
    // close on Save.
    val loop = initial == null && initialObservedAt == null
    // "Done for now" with an entered-but-unlogged set asks first: log it, discard it, or
    // keep editing. A clean editor (an untouched re-seed) just closes.
    var confirmDone by rememberSaveable { mutableStateOf(false) }
    // Undo for the newest loop save: its entry id and summary line, whether it was
    // a one-tap warm-up, and whether it started the running rest (only then does
    // undoing it cancel the rest).
    var undoEntryId by rememberSaveable { mutableStateOf<Long?>(null) }
    var undoLine by rememberSaveable { mutableStateOf("") }
    var undoIsWarmUp by rememberSaveable { mutableStateOf(false) }
    var undoRestScheduled by rememberSaveable { mutableStateOf(false) }
    // A just-logged set whose warm-up verdict (`qualifies`) is pending from the core.
    var pendingQualifyId by rememberSaveable { mutableStateOf<Long?>(null) }
    // A set of today's opened for editing from the loop, by entry id (a legacy
    // id-0 row by its stamp), re-resolved against the live list so it survives
    // rotation; and whether that nested edit has unsaved changes.
    var editingId by rememberSaveable { mutableStateOf<Long?>(null) }
    var editingAt by rememberSaveable { mutableStateOf(0L) }
    var editDirty by rememberSaveable { mutableStateOf(false) }
    // Sets logged in this loop (work sets and warm-ups).
    var loggedHere by rememberSaveable { mutableStateOf(0) }
    // Clock time of the last one-tap warm-up, for the double-tap guard.
    var lastWarmUpAt by remember { mutableStateOf<Long?>(null) }
    // The new exercise name the user confirmed with "+ Add …".
    var addedName by rememberSaveable { mutableStateOf<String?>(null) }
    // The exercise picker (the planned loop's and an edited set's), collapsed to
    // the name + "Change".
    var pickerOpen by rememberSaveable { mutableStateOf(false) }
    // The weight keypad shows only while the weight is being typed: opened by a
    // tap on the well, folded by its Done key, picking reps or effort, a chip
    // switch, or a save.
    var keypadOpen by rememberSaveable { mutableStateOf(false) }
    // While the exercise search or the weight keypad has focus the pinned effort
    // row steps aside, so the keypad's bottom row and the "+ Add" row are never
    // under it (and a key tap can never land on an effort cell).
    var searchFocused by remember { mutableStateOf(false) }
    val keypadRequester = remember { BringIntoViewRequester() }
    LaunchedEffect(keypadOpen) {
        if (keypadOpen) {
            withFrameNanos { }
            keypadRequester.bringIntoView()
        }
    }

    // The open plan item this exercise belongs to (by name), if any. Never a run
    // item, so a free set on a run day shows no run prescription.
    val liveItem = planSession?.items?.let { planItemFor(it, fields.exercise) }
    // The plan item behind the current exercise: today's live item, else the NEW
    // entry's prescription while its exercise is still the one typed. Feeds the
    // "Last time" line, the plates per side and the calisthenics cues.
    val currentItem = liveItem ?: prescription?.takeIf {
        initial == null && it.exercise.isNotBlank() && it.exercise.trim().equals(fields.exercise.trim(), ignoreCase = true)
    }
    // The coach's effort target for the current exercise: a hint on the scale only,
    // and none for a ballistic power item (max intent, never rated toward a target).
    val targetRpe = if (currentItem?.calis?.ballistic == true) {
        null
    } else {
        liveItem?.target_rpe
            ?: rx?.takeIf { it.exercise.trim().equals(fields.exercise.trim(), ignoreCase = true) }?.targetRpe
    }
    val dayStartSec = localDayStartSec(AppClock.nowSec())
    val todaySets = remember(recentLifts, liveItem, fields.exercise, dayStartSec) {
        todaySetsFor(fields.exercise, liveItem, recentLifts, dayStartSec)
    }
    val lastSet = todaySets.lastOrNull()
    // Any set logged today, of any exercise: the loop's "Done for now" shows from then on.
    val loggedToday = remember(recentLifts, dayStartSec) { recentLifts.any { it.observed_at >= dayStartSec } }
    val todayOpen = remember(planSession) {
        planSession?.items.orEmpty().filter { !it.done }.map { it.exercise }
    }
    val quickPicks = remember(fields.exercise, todayOpen, recentExercises, plannedExercises) {
        quickPickCandidates(fields.exercise, todayOpen, recentExercises, plannedExercises)
    }
    // Every spelling the app already knows, so a name typed in another case is
    // saved under its known form.
    val knownNames = remember(planSession, recentExercises, plannedExercises) {
        planSession?.items.orEmpty().map { it.exercise } + recentExercises + plannedExercises
    }
    // Weight, reps and effort show once the exercise is settled, never for a
    // half-typed search term.
    val chosen = exerciseChosen(
        fields.exercise,
        knownNames,
        added = addedName,
        preset = initialExercise ?: prescription?.exercise,
        editing = initial != null,
    )
    // Today's planned loop (opened from a plan item): X returns to Today.
    val plannedLoop = loop && prescription != null
    val sheetLocked = plannedLoop || (loop && loggedHere > 0)
    LaunchedEffect(sheetLocked) { onSheetLockChange(sheetLocked) }

    // Keep the screen on while the loop is open: a rest is longer than a typical
    // screen timeout, and the countdown should stay readable between sets.
    if (loop) {
        val view = LocalView.current
        DisposableEffect(view) {
            val prev = view.keepScreenOn
            view.keepScreenOn = true
            onDispose { view.keepScreenOn = prev }
        }
        RestAlarmInAppScope()
    }
    // The undo row expires on its own after a short window.
    LaunchedEffect(undoEntryId) {
        if (undoEntryId != null) {
            delay(UndoWindowMs)
            undoEntryId = null
        }
    }

    // Re-seed every editable field for [name] ([reseededTo]). The loop's "Next"
    // only loads the coach seed, so it leaves the editor clean (a plain Finish
    // after it needs no prompt); a chip switch is the user's own edit and marks
    // it dirty.
    fun reseedTo(name: String, markDirty: Boolean = true) {
        fields = fields.reseededTo(name, planSession?.items.orEmpty(), recentLifts, weightUnit, AppliedCueLoads::forItem)
        // A picked exercise closes the picker again (it stays open while the
        // name matches no plan item).
        pickerOpen = false
        keypadOpen = false
        focusManager.clearFocus()
        dirty = markDirty
    }

    // A chooser-picked exercise (no prescription) seeds like a chip pick, from its
    // last logged set when there is one. Once only: a re-composition after the
    // user typed never re-seeds.
    LaunchedEffect(Unit) {
        val name = initialExercise
        if (name != null && initial == null && rx == null && fields.exercise.isBlank()) {
            reseedTo(name, markDirty = false)
        }
    }

    // After a warm-up (a loop save the core marks not-qualifying), the next set is
    // the working set: restore the prescribed load instead of repeating the warm-up
    // load. Only while the weight is still untouched, and only when the plan
    // prescribes a load. A qualifying set keeps the repeat-last-set behaviour.
    LaunchedEffect(pendingQualifyId, liveItem?.logged_sets) {
        val id = pendingQualifyId ?: return@LaunchedEffect
        val item = liveItem
        if (item == null) {
            pendingQualifyId = null
            return@LaunchedEffect
        }
        val logged = item.logged_sets.firstOrNull { it.entry_id == id } ?: return@LaunchedEffect
        pendingQualifyId = null
        if (!logged.qualifies && fields.weightFresh) {
            fields = fields.withWorkingLoad(item, holdMode, weightUnit, AppliedCueLoads::forItem)
        }
    }

    val weightParsed = parseKeypadNumber(fields.weightText)
    // Ranges/chips read in the displayed unit. The lb bound is the EXACT converted
    // kg ceiling (881.85 lb, not a rounded 880) so a wire-valid near-max kg value
    // stays editable in lb mode; the range hint rounds it for display but
    // validation uses the exact bound.
    val weightMax = if (weightUnit == WeightUnit.Lb) kgToLb(SetWeightMaxKg) else SetWeightMaxKg
    val weightValid = weightParsed != null && weightParsed in 0.0..weightMax
    // An empty buffer is "not entered yet", not an error (unlike an out-of-range
    // value): no red, and Save stays gated on a parsed weight.
    val weightEmpty = fields.weightText.isBlank()
    val saveEnabled = fields.exercise.isNotBlank() && (bodyweightMode || weightValid) && (holdMode || fields.reps != null)

    // Log the current set. In the loop the editor then re-seeds for the next set
    // from the values just SAVED (an untouched Save repeats the load actually
    // logged; a fresh entry_id + stamp are minted next time), effort resets to
    // unrated, the rest timer starts, and an undo is offered; otherwise the sheet
    // closes. False when the weight buffer doesn't parse.
    fun commitSet(): Boolean {
        val kg = resolveSaveWeightKg(bodyweightMode, fields.weightFresh, fields.weightVerbatim, fields.weightText, weightUnit)
            ?: return false
        val stamp = if (loop && !observedAtTouched) AppClock.nowSec() else observedAt
        val name = resolveExerciseName(fields.exercise, knownNames)
        val setReps = if (holdMode) 1 else (fields.reps ?: 1)
        val setHold = if (holdMode) fields.holdSec else null
        val effort = fields.rpe
        val ev = setEditEvent(initial, name, kg, setReps, effort ?: 0.0, stamp, holdSec = setHold)
        onSave(ev)
        if (loop) {
            val id = (ev as? Event.LogSet)?.entryId
            fields = fields.afterLoggedSet(name, kg, liveItem, holdMode, weightUnit)
            undoEntryId = id
            undoIsWarmUp = false
            undoLine = "$name ${liftSetLine(ctx.resources, setHold, kg, setReps, effort ?: 0.0, effort != null, weightUnit)}"
            pendingQualifyId = id
            // A set with no prescribed rest leaves a running rest alone.
            val rest = liveItem?.rest_sec ?: 0
            undoRestScheduled = rest > 0
            if (rest > 0) RestAlarm.schedule(ctx, rest, name)
            observedAt = AppClock.nowSec()
            observedAtTouched = false
            keypadOpen = false
            loggedHere += 1
            dirty = false
        } else {
            onDone()
        }
        return true
    }

    // One tap logs a warm-up step at its own load and reps: unrated, no rest
    // timer, and the working load stays in the field for the first work set.
    fun logWarmUp(step: WarmUpStep) {
        val item = liveItem ?: return
        val now = AppClock.nowMillis()
        if (!loopSaveAccepted(lastWarmUpAt, now)) return
        lastWarmUpAt = now
        val ev = Event.LogSet(item.exercise, step.kg, step.reps, 0.0, AppClock.nowSec())
        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
        onSave(ev)
        undoEntryId = ev.entryId
        undoIsWarmUp = true
        undoRestScheduled = false
        undoLine = "${item.exercise} ${liftSetLine(ctx.resources, null, step.kg, step.reps, 0.0, false, weightUnit)}"
        loggedHere += 1
    }

    val nextItem = if (loop && liveItem?.done == true) nextOpenItem(planSession?.items.orEmpty(), fields.exercise) else null
    val primary = loopPrimaryAction(loop, liveItem, nextItem, fields.exercise.isNotBlank(), dirty)
    val primaryLabel = when (primary) {
        LoopPrimary.Save -> null
        LoopPrimary.LogPlannedSet -> liveItem?.let {
            stringResource(R.string.log_primary_set_n_of_n, nextSetNumber(it), plannedSetCount(it))
        }
        LoopPrimary.LogExtraSet -> stringResource(R.string.log_primary_extra_set)
        LoopPrimary.Next -> stringResource(R.string.log_primary_next, nextItem?.exercise.orEmpty())
        LoopPrimary.FinishSession -> stringResource(R.string.log_primary_finish_session)
        LoopPrimary.LogOffPlanSet -> stringResource(R.string.log_primary_set_n, todaySets.size + 1)
        LoopPrimary.LogBlank -> stringResource(R.string.log_set_title_new)
    }
    val movesOn = primary == LoopPrimary.Next || primary == LoopPrimary.FinishSession
    val requestFinish: () -> Unit = { if (dirty) confirmDone = true else onFinish() }

    // The coach's in-session cue for the exercise being lifted (from its first
    // logged set on), with its load change and "End exercise" as one-tap actions.
    val cue = if (loop && liveItem != null && !liveItem.done && liveItem.logged_sets.isNotEmpty()) {
        inSessionCue(liveItem.exercise, adjustments, reviewAdjustments)
    } else {
        null
    }
    // The load action hides once the field already holds that load.
    val cueLoadKg = cue?.suggested_load_kg?.takeIf { s ->
        !(fields.weightFresh && fields.weightVerbatim?.let { Math.abs(it - s) < 1e-6 } == true)
    }
    val cueLoadLabel = cueLoadKg?.let {
        stringResource(R.string.log_cue_use_load, "${loadFigure(it, weightUnit)}\u00A0${weightSuffix(weightUnit)}")
    }
    // Barbell warm-ups before the first work set of a plate-loaded item.
    val workKg = liveItem?.let { it.load_kg ?: it.suggested_load_kg }
    val warmUps = if (
        loop && liveItem != null && !liveItem.done && liveItem.sets_done == 0 &&
        liveItem.plates_per_side.isNotEmpty() && !holdMode && !bodyweightMode && workKg != null
    ) {
        warmUpSteps(workKg, weightUnit, liveItem.exercise)
    } else {
        emptyList()
    }
    // Warm-up loads already logged today (the core marks them not qualifying).
    val warmUpsDoneKg = liveItem?.logged_sets.orEmpty().filter { !it.qualifies }.map { it.weight_kg }

    EditorScaffold(
        title = if (initial == null) stringResource(R.string.log_set_title_new) else stringResource(R.string.log_set_title_edit),
        dirty = dirty,
        saveEnabled = movesOn || saveEnabled,
        onClose = if (plannedLoop) onDone else onClose,
        keepOpenAfterSave = loop,
        actionLabel = primaryLabel,
        onSave = {
            when (primary) {
                LoopPrimary.Next -> nextItem?.let { reseedTo(it.exercise, markDirty = false) }
                LoopPrimary.FinishSession -> onFinish()
                else -> commitSet()
            }
        },
        bottomExtra = {
            val id = undoEntryId
            if (loop && id != null && !searchFocused) {
                UndoRow(
                    stringResource(if (undoIsWarmUp) R.string.log_warmup_logged else R.string.log_undo_logged, undoLine),
                    stringResource(R.string.log_undo_cd, undoLine),
                ) {
                    onSave(Event.DeleteEntry(Event.EntryKind.Set, id))
                    if (undoRestScheduled) RestAlarm.cancel(ctx)
                    undoEntryId = null
                    pendingQualifyId = null
                }
            }
            // Effort sits right above the primary action, in thumb reach and never
            // below the fold. Effort is in reps left (the RIR the core derives the
            // RPE from); the coach's target is stated in the label, never pre-picked
            // or drawn on a cell; tapping the picked cell clears it to unrated. Not
            // offered before an exercise is picked, while the action moves on from a
            // completed exercise, or while the weight keypad or the exercise search
            // is in use.
            if (chosen && !movesOn && !keypadOpen && !searchFocused) {
                val picked = fields.rpe
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f)) {
                        FieldLabel(
                            stringResource(R.string.log_field_rpe),
                            when {
                                // An older half-point effort matches no cell: say it.
                                picked != null && picked !in RepsLeftValues -> repsLeftText(ctx.resources, picked)
                                targetRpe != null -> stringResource(R.string.log_rpe_target_hint, repsLeftText(ctx.resources, targetRpe))
                                picked != null -> null
                                else -> stringResource(R.string.log_optional)
                            },
                        )
                    }
                    GlossaryInfo("rpe")
                }
                EffortScaleRow(picked) { fields = fields.copy(rpe = it); keypadOpen = false; focusManager.clearFocus(); dirty = true }
            }
        },
    ) {
        if (loop) {
            // An off-plan exercise gets the header once a set of it is logged today
            // (a one-off set needs no session chrome). "Done for now" appears once a
            // set is logged today; before that the close button is the way out.
            if (liveItem != null || (fields.exercise.isNotBlank() && todaySets.isNotEmpty())) {
                SetLoopHeader(
                    item = liveItem,
                    offPlanSetNumber = todaySets.size + 1,
                    lastSetSec = lastSet?.observed_at,
                    // While the "Logged … · Undo" row shows, it already names the
                    // set just logged; the header's "Last: … · Edit" line returns
                    // once the undo window closes.
                    lastSetLine = lastSet?.takeIf { undoEntryId == null }?.let {
                        liftSetLine(ctx.resources, it.hold_sec, it.weight_kg, it.reps, it.rpe, it.rpe_recorded, weightUnit)
                    },
                    cueText = cue?.summary?.takeIf { it.isNotBlank() },
                    cueLoadLabel = cueLoadLabel,
                    onUseCueLoad = cueLoadKg?.let { kg ->
                        {
                            fields = fields.copy(weightText = weightFieldText(kg, weightUnit), weightVerbatim = kg, weightFresh = true)
                            keypadOpen = false
                            liveItem?.let { AppliedCueLoads.record(it, kg) }
                        }
                    },
                    onEndExercise = if (cue?.suggests_end_exercise == true && liveItem != null && planSession != null) {
                        { endExerciseWithUndo(ctx.resources, liveItem, planSession.epoch_day, onSave) }
                    } else {
                        null
                    },
                    nextItem = nextItem.takeIf { primary == LoopPrimary.LogExtraSet },
                    onDone = requestFinish,
                    onNext = { reseedTo(it.exercise, markDirty = false) },
                    onEditLast = lastSet?.takeIf { undoEntryId == null }?.let { s ->
                        {
                            editingId = s.entry_id
                            editingAt = s.observed_at
                            editDirty = false
                        }
                    },
                    onLogExtra = if (movesOn && saveEnabled) {
                        {
                            commitSet()
                            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        }
                    } else {
                        null
                    },
                    showDone = loggedToday && primary != LoopPrimary.FinishSession,
                )
            }
            if (confirmDone) {
                AlertDialog(
                    onDismissRequest = { confirmDone = false },
                    shape = RoundedCornerShape(Space.Card.dp),
                    title = { Text(stringResource(R.string.log_finish_unsaved_title)) },
                    text = { Text(stringResource(R.string.log_finish_unsaved_body)) },
                    confirmButton = {
                        if (saveEnabled) {
                            TextButton(onClick = {
                                confirmDone = false
                                if (commitSet()) onFinish()
                            }) { Text(stringResource(R.string.log_finish_log_it)) }
                        }
                    },
                    dismissButton = {
                        Row {
                            TextButton(onClick = { confirmDone = false }) {
                                Text(stringResource(R.string.log_keep_editing))
                            }
                            TextButton(onClick = { confirmDone = false; onFinish() }) {
                                Text(
                                    stringResource(R.string.log_discard_confirm),
                                    color = DangerText,
                                )
                            }
                        }
                    },
                )
            }
        }
        FormCard {
            FieldLabel(stringResource(R.string.log_field_exercise))
            val pickedName = liveItem?.exercise?.takeIf { loop } ?: fields.exercise.takeIf { initial != null && it.isNotBlank() }
            if (pickedName != null && !pickerOpen) {
                // In today's planned loop the plan sets the exercise, and an edited
                // set already has one: its name plus one "Change" action instead of a
                // search field and chip row.
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    Text(
                        pickedName,
                        color = OnBgBody,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                        modifier = Modifier.weight(1f).testTag("loop-exercise"),
                    )
                    TextAction(
                        stringResource(R.string.log_change_exercise),
                        { pickerOpen = true },
                        description = stringResource(R.string.log_change_exercise_cd, pickedName),
                    )
                }
            } else {
                // Typing a new name in NEW mode drops the exact prescription kg, so
                // Save emits the weight as displayed and another exercise's verbatim
                // load never rides along. An EDIT keeps its lossless verbatim:
                // renaming a set must not drift its stored weight.
                ExerciseSearchField(
                    value = fields.exercise,
                    onChange = {
                        fields = fields.copy(exercise = it, weightVerbatim = if (initial == null) null else fields.weightVerbatim)
                        keypadOpen = false
                        dirty = true
                    },
                    onFocusChange = { searchFocused = it },
                )
                FlowRow(
                    modifier = Modifier.fillMaxWidth().selectableGroup(),
                    horizontalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                    verticalArrangement = Arrangement.spacedBy(ChoiceChipGap),
                ) {
                    quickPicks.forEach { name ->
                        ChoiceChip(name, selected = name.equals(fields.exercise.trim(), ignoreCase = true), onClick = { reseedTo(name) })
                    }
                }
                // A name the app doesn't know yet is offered as a new exercise, in
                // the form it will be saved in.
                newExerciseToAdd(fields.exercise, knownNames)?.takeIf { it != addedName }?.let { name ->
                    TextAction(
                        stringResource(R.string.log_add_exercise, name),
                        {
                            fields = fields.copy(exercise = name)
                            addedName = name
                            pickerOpen = false
                            focusManager.clearFocus()
                            dirty = true
                        },
                        modifier = Modifier.offset(x = -TextActionInset),
                    )
                }
            }
            // Weight, reps (and the effort row) wait for an exercise: composed all
            // along at 0dp, so the sheet grows once, when the exercise is picked.
            Column(
                modifier = Modifier.fillMaxWidth().shownOnly(chosen),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                if (warmUps.isNotEmpty()) {
                    FieldLabel(stringResource(R.string.log_warmup_label), stringResource(R.string.log_warmup_hint))
                    WarmUpRow(warmUps, warmUpsDoneKg, weightUnit) { logWarmUp(it) }
                }
                // The weight value sits right under the exercise with its one-tap ±
                // chips; reps and effort (what changes every set) follow, so the effort
                // row is on screen without scrolling in the gym-floor loop.
                if (!bodyweightMode) {
                    // The weight notes ride in the field's own reserved caption slot,
                    // right under the well (no empty slot plus a second block below);
                    // an out-of-range buffer swaps them for the range hint.
                    val notes = weightNoteLines(
                        enterHint = weightEmpty,
                        // A first session with no load to start from: how to pick one.
                        firstWeightTimes = currentItem?.takeIf {
                            initial == null && it.calis == null && it.reps_low > 0 && it.last_set == null &&
                                it.logged_sets.isEmpty() && it.load_kg == null && it.suggested_load_kg == null
                        }?.let { item -> item.target_rpe?.let(::prescribedRir)?.let { item.reps_low + it } },
                        // "Last session" only while today's "Last: …" line and the undo
                        // row (both naming today's newest set) are absent.
                        lastTime = currentItem?.last_set?.summary?.takeIf {
                            initial == null && it.isNotBlank() && lastSet == null && undoEntryId == null
                        },
                        // The core's plate breakdown is for the load it prescribed or
                        // suggested; shown only while the field still holds that load.
                        plates = currentItem?.let { item ->
                            val basis = item.load_kg ?: item.suggested_load_kg
                            val v = fields.weightVerbatim
                            item.plates_per_side.takeIf {
                                it.isNotEmpty() && basis != null && fields.weightFresh && v != null &&
                                    Math.abs(v - basis) < 1e-6
                            }
                        },
                        weightUnit = weightUnit,
                    )
                    // The extra bottom step separates the notes from the next field's
                    // label; with no notes the empty caption slot is gap enough.
                    Box(Modifier.padding(bottom = if (notes.isEmpty()) 0.dp else Space.Md.dp)) {
                        KeypadValueField(
                            if (holdMode || currentItem?.calis != null) {
                                stringResource(R.string.log_field_added_load)
                            } else {
                                stringResource(R.string.log_field_weight)
                            },
                            weightSuffix(weightUnit), fields.weightText,
                            active = keypadOpen,
                            invalid = !weightEmpty && !weightValid,
                            min = 0.0, max = weightMax, format = "%.1f",
                            adjustments = plateSteps(weightUnit),
                            onActivate = { focusManager.clearFocus(); keypadOpen = true },
                            // Only the ± chips report a whole new value (keys go through the
                            // keypad below); a plate change needs no keypad, so it folds.
                            onText = { fields = fields.copy(weightText = trimWeightText(it), weightFresh = false); keypadOpen = false; dirty = true },
                            hint = notes.joinToString("\n").ifEmpty { null },
                        )
                    }
                    // The keypad shows only while the weight is being typed (tap the
                    // well), right under it; the ± chips cover a plate change.
                    if (keypadOpen) {
                        Box(Modifier.bringIntoViewRequester(keypadRequester)) {
                            NumericKeypad(
                                onKey = { key ->
                                    fields = fields.copy(weightText = editNumericBuffer(fields.weightText, key, fields.weightFresh), weightFresh = false)
                                    dirty = true
                                },
                                onBackspace = {
                                    fields = fields.copy(weightText = fields.weightText.dropLast(1), weightFresh = false)
                                    dirty = true
                                },
                                onDone = { keypadOpen = false },
                            )
                        }
                    }
                }
                // Hold (calisthenics static rung): seconds instead of reps. Otherwise a
                // reps tap-scale up to [SetRepsScaleMax]. Picking reps or effort hands
                // focus away from the weight, so the keypad folds.
                if (holdMode) {
                    val holdLabel = stringResource(R.string.log_field_hold)
                    FieldLabel(holdLabel)
                    ScrollableScaleRow(HoldScale, fields.holdSec, { "$it" }, label = holdLabel) {
                        fields = fields.copy(holdSec = it); keypadOpen = false; focusManager.clearFocus(); dirty = true
                    }
                } else {
                    // The picked cell is the readout; only a logged count past the tap
                    // scale (which no cell can show) is spelled out. With no count yet
                    // the row opens centred on the common 8–10 band, nothing picked.
                    val repsLabel = stringResource(R.string.log_field_reps)
                    FieldLabel(repsLabel, fields.reps?.takeIf { it > SetRepsScaleMax }?.let { "$it" })
                    Box(Modifier.testTag("reps-scale")) {
                        ScrollableScaleRow(RepsScale, fields.reps, { "$it" }, label = repsLabel, initialCenter = UnsetRepsCentre) {
                            fields = fields.copy(reps = it); keypadOpen = false; focusManager.clearFocus(); dirty = true
                        }
                    }
                }
            }
            ObservedAtRow(observedAt, withTime = true, enabled = dateEditable) {
                observedAt = it
                observedAtTouched = true
                dirty = true
            }
        }
    }

    // Edit a set logged today without leaving the loop (the loop's own state stays
    // put underneath). Saves an AmendSet through the same event path. Neither an
    // outside tap nor Back drops an edited set silently: Back is the editor's own
    // close (with its discard confirm), and an outside tap only closes it clean.
    val editing = editingId?.let { id ->
        recentLifts.firstOrNull { if (id != 0L) it.entry_id == id else it.observed_at == editingAt }
    }
    editing?.let { set ->
        val closeEdit = { editingId = null; editDirty = false }
        Dialog(
            onDismissRequest = { if (!editDirty) closeEdit() },
            properties = DialogProperties(usePlatformDefaultWidth = false, dismissOnClickOutside = false),
        ) {
            Surface(
                color = BgSunken,
                shape = RoundedCornerShape(Space.Card.dp),
                modifier = Modifier.fillMaxWidth().padding(Space.Md.dp),
            ) {
                Column(Modifier.padding(horizontal = Space.Sheet.dp).padding(bottom = Space.Lg.dp)) {
                    CompositionLocalProvider(LocalEditorPinnedActions provides true) {
                        LogSetEditor(
                            recentExercises = recentExercises,
                            holdExercises = holdExercises,
                            bodyweightExercises = bodyweightExercises,
                            initial = set,
                            weightUnit = weightUnit,
                            onClose = closeEdit,
                            onDirtyChange = { editDirty = it },
                            onDone = closeEdit,
                        ) { ev -> onSave(ev) }
                    }
                }
            }
        }
    }
}

/** The effort scale, in reps left: 4+ / 3 / 2 / 1 / 0, stored as the RIR-based
 *  RPE the core reads (10 − reps left; 4 or more = 6). */
private val RepsLeftValues = listOf(6.0, 7.0, 8.0, 9.0, 10.0)

/** Keeps a field group composed (its state and scroll position survive) while it
 *  takes no room, draws nothing and is hidden from TalkBack until [shown]. */
private fun Modifier.shownOnly(shown: Boolean): Modifier =
    if (shown) {
        this
    } else {
        clearAndSetSemantics {}.layout { measurable, constraints ->
            val placeable = measurable.measure(constraints)
            layout(placeable.width, 0) {}
        }
    }

/** A cell's reps-left figure: "4+", "3" … "0". */
private fun repsLeftFigure(rpe: Double): String {
    val rir = (10.0 - rpe).coerceAtLeast(0.0)
    return if (rir >= 4.0) "4+" else "${rir.toInt()}"
}

/** The effort scale as one row of five equal cells, reps left ("4+" … "0"),
 *  filled when picked; the coach's target is stated in the field label, never
 *  drawn on a cell. Tapping the picked cell clears it ([onPick] null). A stored
 *  half-point effort (from an older entry) matches no cell and is shown in the
 *  label instead. TalkBack reads "2 reps left". Tagged `effort-scale`. */
@Composable
private fun EffortScaleRow(current: Double?, onPick: (Double?) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Min).testTag("effort-scale").selectableGroup(),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        RepsLeftValues.forEach { v ->
            val selected = current == v
            val cd = when (v) {
                6.0 -> stringResource(R.string.log_effort_cd_4plus)
                9.0 -> stringResource(R.string.log_effort_cd_1)
                else -> stringResource(R.string.log_effort_cd, repsLeftFigure(v))
            }
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .heightIn(min = ActionMinHeight)
                    .choiceCell(selected)
                    .selectable(selected = selected, role = Role.RadioButton) { onPick(if (selected) null else v) }
                    .semantics { contentDescription = cd }
                    .padding(vertical = Space.Md.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    repsLeftFigure(v),
                    color = choiceCellContent(selected),
                    style = Type.Body.merge(TabularFigures).copy(fontWeight = FontWeight.Bold),
                    maxLines = 1,
                )
            }
        }
    }
}

/** The captions under the set editor's weight value: the "enter a weight" nudge
 *  while it is empty (on a first session with no load to start from, how to pick
 *  one: a weight you could lift about [firstWeightTimes] times), the athlete's
 *  last top set of this movement from a prior session ("Last time: 60 kg × 8 @
 *  RPE 7"), and the plates to load on each side of the bar for the prescribed
 *  load. At most two at once (the nudge only shows for an empty field, the plates
 *  only for the prescribed load), which is what the field's two-line caption slot
 *  holds. */
@Composable
private fun weightNoteLines(
    enterHint: Boolean,
    firstWeightTimes: Int?,
    lastTime: String?,
    plates: List<Double>?,
    weightUnit: WeightUnit,
): List<String> =
    listOfNotNull(
        when {
            !enterHint -> null
            firstWeightTimes != null -> stringResource(R.string.log_hint_first_weight, firstWeightTimes)
            else -> stringResource(R.string.log_hint_enter_weight)
        },
        lastTime?.let { stringResource(R.string.log_last_time, it) },
        plates?.let { p ->
            stringResource(R.string.log_plates_per_side, p.joinToString(" · ") { plateFigure(it) }, weightSuffix(weightUnit))
        },
    )

/** The warm-up row on set 1 of a barbell lift: one cell per step ("Bar", then
 *  the figures) with its reps under it, each a single tap that logs that
 *  warm-up. A step whose load is already logged as a warm-up today ([doneKg])
 *  is filled with a ✓; it stays tappable for a repeat. */
@Composable
private fun WarmUpRow(steps: List<WarmUpStep>, doneKg: List<Double>, weightUnit: WeightUnit, onLog: (WarmUpStep) -> Unit) {
    val barLabel = stringResource(R.string.log_warmup_bar)
    val doneState = stringResource(R.string.log_warmup_done_state)
    Row(
        modifier = Modifier.fillMaxWidth().height(IntrinsicSize.Min).testTag("warmup-row"),
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        steps.forEach { step ->
            val done = doneKg.any { Math.abs(it - step.kg) < 1e-3 }
            val load = "${loadFigure(step.kg, weightUnit)}\u00A0${weightSuffix(weightUnit)}"
            val cd = stringResource(R.string.log_warmup_cd, "$load × ${step.reps}")
            val figure = if (step.bar) barLabel else loadFigure(step.kg, weightUnit)
            Column(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .heightIn(min = ActionMinHeight)
                    .choiceCell(done)
                    .clickable(role = Role.Button) { onLog(step) }
                    .semantics {
                        contentDescription = cd
                        if (done) stateDescription = doneState
                    }
                    .padding(vertical = Space.Sm.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(
                    if (done) "\u2713\u00A0$figure" else figure,
                    color = choiceCellContent(done),
                    style = Type.Body.merge(TabularFigures),
                    maxLines = 1,
                )
                Text(
                    "\u00D7${step.reps}",
                    color = choiceCellContent(done),
                    style = Type.Caption.merge(TabularFigures),
                    maxLines = 1,
                )
            }
        }
    }
}

/** New-entry reps tap-scale ceiling. The core accepts far higher counts and the
 *  EDIT path preserves an out-of-range seed, but the tap scale needs a bound; 200
 *  covers high-rep calisthenics (air squats, push-ups) without a 10k-cell row. */
private const val SetRepsScaleMax = 200

/** The tap scales, built once: the reps scale has 200 cells. */
private val RepsScale = (1..SetRepsScaleMax).toList()
private val HoldScale = HoldSecRange.toList()

/** Where an unset reps scale opens: the common 8–10 band in view. */
private const val UnsetRepsCentre = 9

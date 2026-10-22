package app.milestone

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.dp

/** Which number the shared keypad is typing into. */
private enum class KeyField { Bodyweight, RestingHr, WeeklyRun, LongestRun, RaceTime }

/** The number field after this one on its screen, where the keypad's Next moves
 *  (null on a screen's last field). */
private val KeyField.next: KeyField?
    get() = when (this) {
        KeyField.Bodyweight -> KeyField.RestingHr
        KeyField.WeeklyRun -> KeyField.LongestRun
        else -> null
    }

/** The keypad buffer [field] edits. */
private fun SetupAnswers.buffer(field: KeyField): String = when (field) {
    KeyField.Bodyweight -> bwText
    KeyField.RestingHr -> rhrText
    KeyField.WeeklyRun -> weeklyText
    KeyField.LongestRun -> longestText
    KeyField.RaceTime -> raceDigits
}

/** [text] typed into [field]; a person or distance number is marked touched, so
 *  Finish submits what it shows instead of the seed. */
private fun SetupAnswers.withBuffer(field: KeyField, text: String): SetupAnswers = when (field) {
    KeyField.Bodyweight -> copy(bwText = text, bodyweightTouched = true)
    KeyField.RestingHr -> copy(rhrText = text, restingHrTouched = true)
    KeyField.WeeklyRun -> copy(weeklyText = text, weeklyTouched = true)
    KeyField.LongestRun -> copy(longestText = text, longestTouched = true)
    KeyField.RaceTime -> copy(raceDigits = text)
}

/** An answers edit, applied to the latest answers. */
private typealias UpdateAnswers = ((SetupAnswers) -> SetupAnswers) -> Unit

/** The one pinned keypad as a screen's number fields see it: [active] is the
 *  field it is typing into while shown (null while put away), [type] writes a
 *  field's buffer. */
private class SetupKeypad(
    val active: KeyField?,
    private val requesters: Map<KeyField, BringIntoViewRequester>,
    val activate: (KeyField) -> Unit,
    val type: (KeyField, String) -> Unit,
) {
    /** Tags a field's slot so the screen can scroll it clear of the keypad. */
    fun slot(field: KeyField): Modifier = Modifier.bringIntoViewRequester(requesters.getValue(field))
}

/**
 * Guided setup (modality-first, with a health screen).
 * Plain-language screens: what you train, your goal (a Both profile answers its
 * lifting goal, running goal and which leads on separate screens; calisthenics
 * adds a "Where are you now?" placement, a race goal adds race details), the
 * days you can train, level (lifting and running separately for Both), your
 * running right now, about you, a health screen, and a final "Your plan" review,
 * each with a one-line consequence, ending in a single [Event.SetProfile] (the
 * same wire the full editor uses) plus one rung event per answered calisthenics
 * placement. Seedable: when [initial] is non-null (the Profile "Re-run guided
 * setup" row) every answer is pre-filled from the current profile so the user can
 * review and adjust. [switchTo] (a Profile Focus switch) opens with that modality
 * picked, straight at the first question it adds, and Next walks only the
 * questions the switch needs before returning to the review.
 */
@Composable
internal fun GuidedSetup(
    initial: ProfileDraft? = null,
    switchTo: Modality? = null,
    // The model's calisthenics ladders: a re-run seeds the placements from the rungs
    // they hold now.
    calisLadders: List<CalisLadderView> = EventDispatcher.model.value?.calisthenics_ladders.orEmpty(),
    onComplete: (ProfileDraft, List<Event.SetCalisthenicsRung>) -> Unit,
    onSkip: () -> Unit,
) {
    val ctx = LocalContext.current
    // The distance unit can be picked on the first number step (one Units row with
    // the weight unit); the pick is an answer, saved as the app setting on Finish.
    val systemDistanceUnit = rememberDistanceUnit()
    val today = remember { todayEpochDay() }
    val seedModality = initial?.let(::modalityOf)
    // A Focus switch that turns a side back on seeds that side's answers kept when
    // it was switched off.
    val switchTarget = switchTo?.takeIf { initial != null && it != seedModality }
    val runStash = remember {
        ModalityStash.run(ctx).takeIf { switchTarget != null && switchTarget != Modality.Lift && seedModality == Modality.Lift }
    }
    val liftStash = remember {
        ModalityStash.lift(ctx).takeIf { switchTarget != null && switchTarget != Modality.Run && seedModality == Modality.Run }
    }
    // A Both profile's running level is the shell's record of the answer (the core
    // keeps only the `advanced` it drives).
    val recordedRunLevel = remember(initial) { initial?.let { RunLevelAnswer.read(ctx, it.advanced) } }
    // A re-run's ladders that already hold a rung: an unanswered placement keeps it
    // (nothing is sent for it), so it never reads "start easy".
    val currentRungs = remember(initial) { if (initial != null) currentPlacementRungs(calisLadders) else emptyMap() }
    val seed = remember(initial, recordedRunLevel, systemDistanceUnit) {
        seedAnswers(initial, today, recordedRunLevel, systemDistanceUnit, runStash, currentRungs)
    }
    val seedWeeklyKm = remember(initial) { seedWeeklyRunKm(initial, runStash) }
    // A switch opens at the first question its modality adds, in "back to review"
    // mode against the profile's current path.
    val switchReturn = if (switchTarget != null) seed.path else null

    var answers by rememberSaveable {
        mutableStateOf(if (switchTarget != null && initial != null) switchedAnswers(seed, switchTarget, initial, runStash, liftStash) else seed)
    }
    val update: UpdateAnswers = { edit -> answers = edit(answers) }
    var step by rememberSaveable { mutableIntStateOf(if (switchReturn != null) switchStartStep(answers, switchReturn) else 0) }
    // Set while an edit started from the review (or a Focus switch) is walking
    // only the screens it changed; Next then returns to the review.
    var reviewReturn by rememberSaveable { mutableStateOf(switchReturn) }
    // The review row an edit started from (null for a Focus switch, which never
    // showed the review): it names the way back "Back to review" and is the row the
    // review scrolls to on return.
    var editedFrom by rememberSaveable { mutableStateOf<SetupStep?>(null) }
    var revealRow by rememberSaveable { mutableStateOf<SetupStep?>(null) }
    var activeField by rememberSaveable { mutableStateOf(KeyField.Bodyweight) }
    // Calculator-style entry: the first key after (re)activating a field replaces it.
    var fresh by rememberSaveable { mutableStateOf(true) }
    // The pinned keypad shows only while a number field has focus; tapping a
    // non-number answer or blank space puts it away so nothing sits half-hidden.
    var keypadOpen by rememberSaveable { mutableStateOf(true) }
    // Confirm-before-discard flag for a step-0 back-out with unsaved answers.
    var confirmLeave by rememberSaveable { mutableStateOf(false) }

    fun valuesOf(a: SetupAnswers) = SetupValues(a, seed, initial, seedWeeklyKm, a.distanceChoice ?: systemDistanceUnit, today)
    val a = answers
    val distanceUnit = a.distanceChoice ?: systemDistanceUnit
    val v = valuesOf(a)

    // Nothing is dispatched until Finish, so backing out of step 0 (or Skip) with
    // answers diverging from the seed would silently discard real input; that
    // path is gated behind a confirm. A pristine wizard (fresh install, or an
    // untouched re-run) exits without a prompt.
    val dirty = a != seed

    // System back mirrors the on-screen Back button (step > 0 steps back); at step 0
    // it leaves setup, confirming first only when answers would be lost. GuidedSetup
    // renders as a full-screen replacement, not a dialog, so without this every
    // system-back at any step would exit the app and drop every answer.
    BackHandler {
        when {
            step > 0 -> step--
            dirty -> confirmLeave = true
            else -> onSkip()
        }
    }

    val steps = a.path.steps
    val stepIndex = step.coerceIn(0, steps.lastIndex)
    val current = steps[stepIndex]
    // A review row jumps to its step; Next then walks only what that edit changes.
    val goTo: (SetupStep) -> Unit = { target ->
        reviewReturn = answers.path
        editedFrom = target
        step = steps.indexOf(target).coerceAtLeast(0)
    }
    val canAdvance = v.ready(current)
    // While returning to the review: the next screen the edit still needs, or
    // null when the primary button goes straight back to the review.
    val pendingStep = reviewReturn?.let { base -> nextPendingStep(steps, stepIndex, base, a.modality, v::ready) }
    val backToReview = reviewReturn != null && current != SetupStep.Review && pendingStep == null
    val blockedHint = if (canAdvance) null else stepBlockedHint(current, v)

    // Keypad plumbing: one keypad edits whichever field is active.
    fun setBuffer(f: KeyField, text: String) {
        answers = answers.withBuffer(f, text)
        fresh = false
    }
    fun activate(f: KeyField) {
        activeField = f
        fresh = true
        keypadOpen = true
    }
    fun pressKey(key: Char) {
        if (activeField == KeyField.RaceTime) {
            appendRaceDigit(answers.raceDigits, key, fresh)?.let { setBuffer(KeyField.RaceTime, it) }
        } else {
            setBuffer(activeField, editNumericBuffer(answers.buffer(activeField), key, fresh))
        }
    }

    val scroll = rememberScrollState()
    val topFadePx = with(LocalDensity.current) { Space.Screen.dp.toPx() }
    val topFade = Brush.verticalGradient(listOf(BgTop, BgTop.copy(alpha = 0f)), endY = topFadePx)
    // One requester per number field, so the field being typed (or the next one
    // on the keypad's Next) can be scrolled clear of the pinned keypad.
    val fieldRequesters = remember { KeyField.entries.associateWith { BringIntoViewRequester() } }
    // Each screen opens at its top (a review "Edit" jump must not land mid-page)
    // with the keypad on that screen's first number. Saveable, so a rotation
    // (same step) keeps the user's scroll position and field focus.
    var initedStep by rememberSaveable { mutableStateOf<SetupStep?>(null) }
    LaunchedEffect(current) {
        if (current == initedStep) return@LaunchedEffect
        initedStep = current
        scroll.scrollTo(0)
        when (current) {
            // The screen opens on the birthday picker, so no number has focus yet.
            SetupStep.About -> keypadOpen = false
            SetupStep.RunNow -> activate(KeyField.WeeklyRun)
            SetupStep.Race -> activate(KeyField.RaceTime)
            else -> {}
        }
    }
    val keypadStep = current == SetupStep.About || current == SetupStep.RunNow ||
        (current == SetupStep.Race && a.recentDistance != null)
    val showKeypad = keypadStep && keypadOpen
    // The keypad never covers the field being typed: once it (re)opens or the
    // focus moves, and the shrunken viewport has laid out, scroll that field in.
    LaunchedEffect(showKeypad, activeField, current) {
        if (showKeypad) {
            withFrameNanos { }
            fieldRequesters.getValue(activeField).bringIntoView()
        }
    }
    val keypad = SetupKeypad(
        active = activeField.takeIf { showKeypad },
        requesters = fieldRequesters,
        activate = ::activate,
        type = ::setBuffer,
    )
    // The keypad band's key moves on to the screen's next number field (keypad
    // still open, that field scrolled clear of it), so a field under the open
    // keypad is never out of reach; on the last field it is Done and puts the
    // keypad away.
    val nextField = activeField.next
    // Units are a preference, not an answer: one quiet toggle that sets weight and
    // distance together (as Profile › Units can split them later), asked on the
    // first number screen of the path, before any distance is typed.
    val unitsOnStep = if (SetupStep.RunNow in steps) SetupStep.RunNow else SetupStep.About
    val selectUnits: (WeightUnit, DistanceUnit) -> Unit = { weight, distance ->
        keypadOpen = false
        answers = answers.withUnits(weight, distance, shownDistance = answers.distanceChoice ?: systemDistanceUnit)
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(BgTop),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = Space.Screen.dp)
                .padding(top = Space.Lg.dp, bottom = Space.Lg.dp),
        ) {
            // Header: step progress + Skip. The step question below is the
            // screen's one title. The review has only Back and Finish to choose
            // from, so Skip is gone there; the row keeps its height so the page
            // never jumps.
            Row(
                modifier = Modifier.fillMaxWidth().heightIn(min = ActionMinHeight),
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StepProgress(
                    stepNumber = stepIndex + 1,
                    stepCount = steps.size,
                    fraction = setupProgress(steps, stepIndex, a.modality),
                    modifier = Modifier.weight(1f),
                )
                if (current != SetupStep.Review) {
                    // Skip routes through the same confirm-before-discard guard as a
                    // step-0 system-back.
                    TextAction(stringResource(R.string.setup_skip), onClick = { if (dirty) confirmLeave = true else onSkip() })
                }
            }
            Spacer(Modifier.size(Space.Lg.dp))

            Column(
                modifier = Modifier
                    .weight(1f)
                    // A tap on blank space (not on a field or answer, which consume
                    // their own taps) puts the keypad away.
                    .pointerInput(keypadStep) {
                        if (keypadStep) detectTapGestures { keypadOpen = false }
                    }
                    // Content scrolled up under the top edge fades out instead of
                    // showing a clipped sliver of its last line.
                    .drawWithContent {
                        drawContent()
                        if (scroll.canScrollBackward) {
                            drawRect(
                                topFade,
                                size = Size(size.width, topFadePx),
                            )
                        }
                    }
                    .verticalScroll(scroll),
                verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                when (current) {
                    SetupStep.Train -> {
                        StepHeader(stringResource(R.string.setup_step_train_q))
                        ChoiceGroup {
                            Modality.entries.forEach { m ->
                                SetupChoiceCard(stringResource(m.titleRes), stringResource(m.consequenceRes), selected = a.modality == m) {
                                    update { it.copy(modality = m) }
                                }
                            }
                        }
                    }
                    SetupStep.LiftGoal -> LiftGoalStep(a, today, update)
                    SetupStep.Placement -> PlacementStep(a, currentRungs.keys, update)
                    SetupStep.RunGoal -> {
                        StepHeader(stringResource(R.string.setup_step_run_q))
                        ChoiceGroup {
                            GoalDistance.entries.forEach { d ->
                                SetupChoiceCard(stringResource(d.labelRes), goalBlurb(d, distanceUnit), selected = a.goalDistance == d) {
                                    update { it.copy(goalDistance = d) }
                                }
                            }
                        }
                    }
                    SetupStep.Race -> RaceStep(a, v, today, keypad, update)
                    SetupStep.Lead -> {
                        StepHeader(stringResource(R.string.setup_step_lead_q), stringResource(R.string.setup_step_lead_sub))
                        ChoiceGroup {
                            BothPriority.entries.forEach { p ->
                                SetupChoiceCard(stringResource(p.titleRes), stringResource(p.consequenceRes), selected = a.bothPriority == p) {
                                    update { it.copy(bothPriority = p) }
                                }
                            }
                        }
                    }
                    SetupStep.Days -> DaysStep(a, v, update)
                    SetupStep.Level -> LevelStep(a, update)
                    SetupStep.RunNow -> {
                        StepHeader(stringResource(R.string.setup_step_runnow_q), stringResource(R.string.setup_step_runnow_sub))
                        if (unitsOnStep == SetupStep.RunNow) UnitsRow(a.weightUnit, distanceUnit, selectUnits)
                        RunNowFields(a, v, distanceUnit, keypad)
                    }
                    SetupStep.About -> AboutStep(
                        a,
                        v,
                        keypad,
                        today,
                        unitsRow = { if (unitsOnStep == SetupStep.About) UnitsRow(a.weightUnit, distanceUnit, selectUnits) },
                        onBirthday = { day ->
                            update { it.withBirthday(day) }
                            keypadOpen = false
                        },
                        onSkipBirthday = {
                            update { it.withoutBirthday() }
                            keypadOpen = false
                        },
                        onSex = { sex ->
                            update { it.copy(sex = sex) }
                            keypadOpen = false
                        },
                    )
                    SetupStep.Health -> HealthStep(a, v, update)
                    SetupStep.Review -> {
                        StepHeader(stringResource(R.string.setup_step_review_q), stringResource(R.string.setup_step_review_sub))
                        val draft = v.draft()
                        val m = a.modality
                        if (draft != null && m != null) {
                            ReviewCard(reviewEntries(a, m, draft, distanceUnit, currentRungs.keys, today), reveal = revealRow, onRevealed = { revealRow = null }) { goTo(it) }
                        }
                    }
                }
                // Breathing room above the pinned keypad / button bar, so the last
                // answer never ends flush against the viewport edge.
                Spacer(Modifier.size(Space.Screen.dp))
            }

            if (showKeypad) {
                KeypadBand(
                    nextField = nextField,
                    onAdvance = { if (nextField != null) activate(nextField) else keypadOpen = false },
                    onKey = { pressKey(it) },
                    onBackspace = { setBuffer(activeField, answers.buffer(activeField).dropLast(1)) },
                    // Whole numbers (age, heart rate, a finish time) get no
                    // decimal key that would do nothing.
                    allowDecimal = activeField == KeyField.Bodyweight ||
                        activeField == KeyField.WeeklyRun || activeField == KeyField.LongestRun,
                )
            } else {
                // Steps without the keypad band get the same full-bleed seam while
                // more content sits below the fold, so the cut edge reads as
                // "scroll for more" (always composed: the bar never shifts).
                HorizontalDivider(
                    modifier = Modifier.fullBleed(Space.Screen.dp),
                    thickness = 1.dp,
                    color = if (scroll.canScrollForward) HairlineStrong else Color.Transparent,
                )
            }

            Spacer(Modifier.size(Space.Md.dp))
            if (blockedHint != null) {
                Text(
                    blockedHint,
                    color = OnBgMuted,
                    style = Type.Caption,
                    modifier = Modifier.fillMaxWidth().padding(bottom = Space.Sm.dp),
                )
            }
            // Back is a quiet text action, so no tonal slab sits beside the primary.
            // Its slot is always composed (invisible on the first step), so Next keeps
            // the same place and width from the first step on.
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val backShown = stepIndex > 0
                Box(if (backShown) Modifier else Modifier.alpha(0f).clearAndSetSemantics {}) {
                    TextAction(
                        stringResource(R.string.setup_btn_back),
                        onClick = { step = stepIndex - 1 },
                        enabled = backShown,
                        modifier = Modifier.offset(x = -TextActionInset),
                    )
                }
                PrimaryButton(
                    stringResource(
                        when {
                            current == SetupStep.Review -> R.string.setup_btn_finish
                            // "Back to" only where the user came from the review; a
                            // Focus switch reaches it for the first time.
                            backToReview && editedFrom != null -> R.string.setup_btn_back_to_review
                            backToReview -> R.string.setup_btn_review_answers
                            else -> R.string.setup_btn_next
                        },
                    ),
                    onClick = {
                        when {
                            current != SetupStep.Review && reviewReturn != null -> {
                                // An edit from the review: on to the next screen it
                                // invalidated, else straight back to the review, at
                                // the row the edit started from.
                                val next = pendingStep
                                if (next != null) {
                                    step = next
                                } else {
                                    step = steps.lastIndex
                                    reviewReturn = null
                                    revealRow = editedFrom
                                    editedFrom = null
                                }
                            }
                            current != SetupStep.Review -> step = stepIndex + 1
                            else -> {
                                // Every required answer is guaranteed set by the gates above;
                                // draft() returns null only if one is somehow missing.
                                val done = answers
                                valuesOf(done).draft()?.let { draft ->
                                    val rungs = if (draft.resistanceModality == ResistanceModality.Calisthenics) {
                                        placementEvents(done, seed, AppClock.nowMillis() / 1000L)
                                    } else {
                                        emptyList()
                                    }
                                    // The units picked in the setup become the app setting.
                                    done.distanceChoice?.let {
                                        AppSettings.setDistanceUnitOverride(ctx, if (it == DistanceUnit.Mi) DistanceUnitOverride.Mi else DistanceUnitOverride.Km)
                                    }
                                    if (done.modality == Modality.Both) done.runLevel?.let { RunLevelAnswer.save(ctx, it) }
                                    onComplete(draft, rungs)
                                }
                            }
                        }
                    },
                    modifier = Modifier.weight(1f),
                    enabled = canAdvance,
                )
            }
        }

        if (confirmLeave) {
            LeaveSetupDialog(
                onDismiss = { confirmLeave = false },
                onLeave = {
                    confirmLeave = false
                    onSkip()
                },
            )
        }
    }
}

/** What a gated step says is missing while Next is disabled. */
@Composable
private fun stepBlockedHint(step: SetupStep, v: SetupValues): String? = when (step) {
    SetupStep.Train -> stringResource(R.string.setup_need_train)
    SetupStep.LiftGoal, SetupStep.RunGoal -> stringResource(R.string.setup_need_goal)
    SetupStep.Race -> stringResource(R.string.setup_need_race_time)
    SetupStep.Lead -> stringResource(R.string.setup_need_lead)
    SetupStep.Days -> {
        val days = v.totalDays
        if (days == null) stringResource(R.string.setup_days_pick_hint) else pluralStringResource(R.plurals.setup_days_weekdays_hint, days, days)
    }
    SetupStep.Level -> stringResource(R.string.setup_need_level)
    SetupStep.RunNow -> stringResource(R.string.setup_need_fix_value)
    SetupStep.About -> if (!v.ageAnswered) stringResource(R.string.setup_about_pick_hint) else stringResource(R.string.setup_need_fix_value)
    SetupStep.Placement, SetupStep.Health, SetupStep.Review -> null
}

/** The lifting goal, the test/meet date it may carry, and how the user lifts. */
@Composable
private fun LiftGoalStep(a: SetupAnswers, today: Long, update: UpdateAnswers) {
    StepHeader(stringResource(R.string.setup_step_lift_q))
    ChoiceGroup {
        LiftFocus.entries.forEach { g ->
            SetupChoiceCard(stringResource(g.titleRes), stringResource(g.consequenceRes), selected = a.liftFocus == g) {
                update { it.copy(liftFocus = g) }
            }
        }
    }
    // A test day or meet shapes strength/power work (taper and peak).
    if (a.liftFocus != null && a.liftFocus != LiftFocus.Muscle) {
        Spacer(Modifier.size(Space.Sm.dp))
        SubHeading(stringResource(R.string.setup_meet_q))
        Text(stringResource(R.string.setup_meet_sub), color = OnBgMuted, style = Type.Caption)
        DateAnswerRow(a.meetDate, today, future = true, noneLabel = stringResource(R.string.setup_date_none)) { day ->
            update { it.copy(meetDate = day) }
        }
    }
    Spacer(Modifier.size(Space.Sm.dp))
    SubHeading(stringResource(R.string.setup_modality_strength_q))
    ChoiceGroup {
        ResistanceModality.entries.forEach { rm ->
            SetupChoiceCard(stringResource(rm.setupTitleRes), stringResource(rm.setupConsequenceRes), selected = a.resistanceModality == rm) {
                update { it.copy(resistanceModality = rm) }
            }
        }
    }
}

/** "Where are you now?": the equipment on hand, then one placement per ladder.
 *  On a [kept] ladder (a re-run's, already holding a rung) no answer keeps that rung. */
@Composable
private fun PlacementStep(a: SetupAnswers, kept: Set<CalisLadder>, update: UpdateAnswers) {
    StepHeader(stringResource(R.string.setup_step_place_q), stringResource(R.string.setup_step_place_sub))
    // Equipment first: it decides which pull-up answers are possible.
    SubHeading(stringResource(R.string.setup_equip_q))
    EquipmentChips(a.equipment) { e ->
        // Without a bar or rings no pull-up rung can be trained.
        update { it.copy(equipment = e, pullPlace = if (e.canHang) it.pullPlace else null) }
    }
    Spacer(Modifier.size(Space.Sm.dp))
    PLACEMENT_LADDERS.forEach { (ladder, questionRes) ->
        SubHeading(stringResource(questionRes))
        val answer = a.placement(ladder)
        // Pull-up rungs need something to hang from (the core's rung_available);
        // unanswered equipment means "everything".
        val rungsOff = ladder == CalisLadder.VerticalPull && a.equipment?.canHang == false
        // null = "Not sure, start easy" (the first rung), or on a kept ladder "Keep
        // my current level". One full-width row per rung, easy → hard top to
        // bottom, so the ladder's order survives every screen width.
        val options = listOf<CalisPlacement?>(null) + CalisPlacement.entries.filter { it.ladder == ladder }
        val noAnswerRes = if (ladder in kept) R.string.setup_place_keep else R.string.setup_place_not_sure
        Column(
            modifier = Modifier.fillMaxWidth().selectableGroup(),
            verticalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            options.forEach { opt ->
                AnswerChip(
                    stringResource(opt?.labelRes ?: noAnswerRes),
                    selected = opt == answer,
                    modifier = Modifier.fillMaxWidth(),
                    contentAlignment = Alignment.CenterStart,
                    enabled = opt == null || !rungsOff,
                ) {
                    update { it.withPlacement(ladder, opt) }
                }
            }
            if (rungsOff) {
                Text(stringResource(R.string.setup_place_pull_needs_bar), color = OnBgMuted, style = Type.Caption)
            }
        }
    }
}

/** Race details: the race date and a recent result (its finish time on the keypad). */
@Composable
private fun RaceStep(a: SetupAnswers, v: SetupValues, today: Long, keypad: SetupKeypad, update: UpdateAnswers) {
    StepHeader(stringResource(R.string.setup_step_race_q), stringResource(R.string.setup_step_race_sub))
    SubHeading(stringResource(R.string.setup_race_date_q))
    DateAnswerRow(a.raceDate, today, future = true, noneLabel = stringResource(R.string.setup_date_none)) { day ->
        update { it.copy(raceDate = day) }
    }
    Spacer(Modifier.size(Space.Sm.dp))
    SubHeading(stringResource(R.string.setup_recent_race_q))
    Text(stringResource(R.string.setup_recent_race_sub), color = OnBgMuted, style = Type.Caption)
    ChipChoiceRow(listOf<RaceDistance?>(null) + RaceDistance.entries, a.recentDistance, { d -> stringResource(d?.labelRes ?: R.string.setup_dont_know) }) { d ->
        update { it.copy(recentDistance = d, raceDigits = if (d == null) "" else it.raceDigits) }
        keypad.activate(KeyField.RaceTime)
    }
    if (a.recentDistance != null) {
        Box(keypad.slot(KeyField.RaceTime)) {
            RaceTimeField(a.raceDigits, active = keypad.active != null, invalid = v.raceTimeInvalid) { keypad.activate(KeyField.RaceTime) }
        }
    }
}

/** How many days, the Both lift/run split, and which weekdays suit. */
@Composable
private fun DaysStep(a: SetupAnswers, v: SetupValues, update: UpdateAnswers) {
    val q = when (a.modality) {
        Modality.Lift -> R.string.setup_step_days_lift_q
        Modality.Run -> R.string.setup_step_days_run_q
        else -> R.string.setup_step_days_q
    }
    StepHeader(stringResource(q))
    // `days` is null until the user taps: nothing is pre-highlighted.
    ScrollableScaleRow(v.dayRange.toList(), a.days, { "$it" }, label = stringResource(R.string.setup_days_scale_label)) { d ->
        update { it.copy(days = d) }
    }
    val totalDays = v.totalDays
    val liftOptions = v.liftOptions
    val liftSplit = v.liftSplit
    if (a.modality == Modality.Both && totalDays != null && liftOptions != null && liftSplit != null) {
        val splitQ = stringResource(R.string.setup_days_lift_split_q)
        SubHeading(splitQ)
        // Same scale cells as the days answer above, so one step has one
        // selection shape.
        ScrollableScaleRow(liftOptions.toList(), liftSplit, { "$it" }, label = splitQ) { d ->
            update { it.copy(liftSplit = d) }
        }
        // Honest split: what the engine will actually schedule.
        Text(hybridSplitText(totalDays, liftSplit, a.bothPriority), color = OnBgMuted, style = Type.Caption)
    }
    Spacer(Modifier.size(Space.Sm.dp))
    SubHeading(stringResource(R.string.setup_weekdays_q))
    WeekdayChips(a.weekdays) { mask -> update { it.copy(weekdays = mask) } }
}

/** Roughly how experienced: one level, or lifting and running apart for Both. */
@Composable
private fun LevelStep(a: SetupAnswers, update: UpdateAnswers) {
    StepHeader(stringResource(R.string.setup_step_level_q))
    if (a.modality == Modality.Both) SubHeading(stringResource(R.string.setup_level_lifting))
    // Each card describes the person. A run-only profile's one level is its
    // running level, so it reads the running lines.
    ChoiceGroup {
        SetupLevel.entries.forEach { l ->
            val line = if (a.modality == Modality.Run) l.runConsequenceRes else l.consequenceRes
            SetupChoiceCard(stringResource(l.titleRes), stringResource(line), selected = a.level == l) {
                update { it.copy(level = l) }
            }
        }
    }
    if (a.modality == Modality.Both) {
        Spacer(Modifier.size(Space.Sm.dp))
        SubHeading(stringResource(R.string.setup_level_running))
        ChoiceGroup {
            SetupLevel.entries.forEach { l ->
                SetupChoiceCard(stringResource(l.titleRes), stringResource(l.runConsequenceRes), selected = a.runLevel == l) {
                    update { it.copy(runLevel = l) }
                }
            }
        }
    }
}

/** One keypad number field: [field]'s buffer, outlined while the keypad types into it. */
@Composable
private fun SetupKeypad.NumberField(
    field: KeyField,
    label: String,
    unit: String,
    text: String,
    invalid: Boolean,
    range: ClosedFloatingPointRange<Double>,
    format: String,
    hint: String? = null,
) {
    KeypadValueField(
        label, unit, text,
        active = active == field,
        invalid = invalid,
        min = range.start, max = range.endInclusive, format = format,
        // The pinned keypad is the one input; no ±1 steppers.
        adjustments = emptyList(),
        onActivate = { activate(field) },
        onText = { type(field, it) },
        hint = hint,
    )
}

/** Running right now: weekly distance (with its "I don't run yet" answer) and the
 *  longest recent run, in the user's distance unit. */
@Composable
private fun RunNowFields(a: SetupAnswers, v: SetupValues, distanceUnit: DistanceUnit, keypad: SetupKeypad) {
    val unit = distanceUnit.distanceLabel
    // The chip sits right under its field (the field's caption slot is the only
    // gap), so it reads as that field's answer.
    Column(keypad.slot(KeyField.WeeklyRun)) {
        keypad.NumberField(
            KeyField.WeeklyRun,
            stringResource(R.string.setup_runnow_weekly_label), stringResource(R.string.setup_unit_per_week, unit), a.weeklyText,
            invalid = v.weeklyInvalid, range = v.weeklyBounds, format = "%.0f",
        )
        // A real answer (0), unlike leaving the field blank.
        AnswerChip(stringResource(R.string.setup_runnow_none), selected = v.weeklyRunKm == 0.0) {
            keypad.type(KeyField.WeeklyRun, "0")
            keypad.activate(KeyField.LongestRun)
        }
    }
    Box(keypad.slot(KeyField.LongestRun)) {
        keypad.NumberField(
            KeyField.LongestRun,
            stringResource(R.string.setup_runnow_longest_label), unit, a.longestText,
            invalid = v.longestInvalid, range = v.longestBounds, format = "%.1f",
        )
    }
}

/** About you: birthday (a date or "Prefer not to say"), sex, the units row when
 *  this is the path's first number screen, bodyweight and resting heart rate. */
@Composable
private fun AboutStep(
    a: SetupAnswers,
    v: SetupValues,
    keypad: SetupKeypad,
    today: Long,
    unitsRow: @Composable () -> Unit,
    onBirthday: (Long) -> Unit,
    onSkipBirthday: () -> Unit,
    onSex: (SexAnswer) -> Unit,
) {
    StepHeader(stringResource(R.string.setup_step_about_q))
    // The birthday needs an answer, a date or "Prefer not to say" (Next's caption
    // says so); the core derives the age from it every day. The youth hold keys
    // off that age: say so the moment an under-18 birthday is picked, not only on
    // the Health step. Without an age, heart-rate targets fall back to effort (a
    // measured max heart rate still anchors them, so the note then stays away). A
    // re-run of a profile with only a stated age keeps it, with a nudge to add the
    // birthday.
    BirthdayAnswer(
        a.birthDay,
        today,
        hint = when {
            isMinor(v.ageYears) -> stringResource(R.string.setup_health_defer_youth)
            a.birthSkipped -> if (v.measuredHrMax == null) stringResource(R.string.setup_birthday_skipped_note) else null
            a.birthDay == null && v.ageYears != null -> stringResource(R.string.setup_birthday_legacy, trimDecimal(v.ageYears))
            else -> null
        },
        skipped = a.birthSkipped,
        onSkip = onSkipBirthday,
        onChange = onBirthday,
    )
    // Keypad fields below (the keypad is pinned under this scroll area while one
    // has focus); an empty optional field submits nothing. Picking a sex puts the
    // keypad away.
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        FieldLabel(stringResource(R.string.setup_sex_label))
        // Unset until tapped (nothing highlighted); "Prefer not to say" is a real
        // answer that stores no sex.
        ChipChoiceRow(SexAnswer.entries.toList(), a.sex, { stringResource(sexLabelRes(it.female)) }, onSex)
        Text(stringResource(R.string.setup_sex_caption), color = OnBgMuted, style = Type.Caption)
    }
    unitsRow()
    Box(keypad.slot(KeyField.Bodyweight)) {
        keypad.NumberField(
            KeyField.Bodyweight,
            stringResource(R.string.setup_row_bodyweight), weightSuffix(a.weightUnit), a.bwText,
            invalid = v.bodyweightInvalid, range = v.bodyweightRange, format = "%.1f",
        )
    }
    Box(keypad.slot(KeyField.RestingHr)) {
        keypad.NumberField(
            KeyField.RestingHr,
            stringResource(R.string.setup_row_resting_hr), stringResource(R.string.setup_unit_bpm), a.rhrText,
            invalid = v.restingHrInvalid, range = RESTING_HR_RANGE, format = "%.0f",
        )
    }
}

/** One quiet toggle that sets weight and distance units together; a mixed pair
 *  shows neither selected. */
@Composable
private fun UnitsRow(weightUnit: WeightUnit, distanceUnit: DistanceUnit, onSelect: (WeightUnit, DistanceUnit) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
        FieldLabel(stringResource(R.string.setup_about_units_label))
        val metric = weightUnit == WeightUnit.Kg && distanceUnit == DistanceUnit.Km
        val imperial = weightUnit == WeightUnit.Lb && distanceUnit == DistanceUnit.Mi
        TwoSegmentRow(
            left = "${weightSuffix(WeightUnit.Kg)} · ${DistanceUnit.Km.distanceLabel}",
            right = "${weightSuffix(WeightUnit.Lb)} · ${DistanceUnit.Mi.distanceLabel}",
            selectedIndex = if (metric) 0 else if (imperial) 1 else -1,
        ) { i ->
            if (i == 0) onSelect(WeightUnit.Kg, DistanceUnit.Km) else onSelect(WeightUnit.Lb, DistanceUnit.Mi)
        }
    }
}

/** The health screen: the PAR-Q+ and the current-state rows in one card of
 *  hairline-divided switch rows (Profile's health card), and the hold they raise. */
@Composable
private fun HealthStep(a: SetupAnswers, v: SetupValues, update: UpdateAnswers) {
    StepHeader(
        stringResource(R.string.setup_step_health_q),
        stringResource(R.string.setup_step_health_sub),
    )
    // The headline line already says "Turn on any that apply", so the questions
    // follow with no heading or intro of their own.
    PlainCard {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
            ParqQuestions(a.health, divider = { HealthRowHairline() }, showIntro = false, leadingDivider = false) { h ->
                update { it.copy(health = h) }
            }
            HealthRowHairline()
            CurrentHealthRows(a.health, divider = { HealthRowHairline() }) { h -> update { it.copy(health = h) } }
        }
    }

    // The youth gate must appear in this preview or a 14-17 y/o would be told
    // nothing while the core silently holds their plan (HARD RULE 3: the safety
    // consequence has to be surfaced). When age is the only reason, say so: no
    // toggle here is on.
    val healthHold = healthDefers(null, a.health)
    if (healthHold || isMinor(v.ageYears)) {
        Text(
            stringResource(if (healthHold) R.string.setup_health_defer_notice else R.string.setup_health_defer_youth),
            color = OnBgMuted,
            style = Type.Caption,
        )
    }
}

/** The row divider inside the health card. */
@Composable
private fun HealthRowHairline() {
    HorizontalDivider(thickness = 1.dp, color = OnBgBody.copy(alpha = HairlineAlpha.Row))
}

/** The review rows for [draft] (what Finish sends): each answer once, unanswered
 *  optional fields left out. [today] dates the birthday's age. */
@Composable
private fun reviewEntries(a: SetupAnswers, m: Modality, draft: ProfileDraft, distanceUnit: DistanceUnit, kept: Set<CalisLadder>, today: Long): List<ReviewEntry> {
    val unit = distanceUnit.distanceLabel
    return buildList {
        add(ReviewEntry(stringResource(R.string.setup_review_train), stringResource(m.titleRes), SetupStep.Train))
        if (m != Modality.Run) {
            val goal = a.liftFocus?.let { stringResource(it.titleRes) }
            val how = stringResource(a.resistanceModality.setupTitleRes)
            val meet = draft.meetEpochDay?.let { stringResource(R.string.setup_review_meet, epochDayLabel(it)) }
            add(
                ReviewEntry(
                    stringResource(R.string.setup_review_lift_goal),
                    listOfNotNull(listOfNotNull(goal, how).joinToString(" · "), meet).joinToString("\n"),
                    SetupStep.LiftGoal,
                ),
            )
        }
        if (a.calisthenics) {
            // An answered ladder names its rung; an unanswered one keeps the rung it
            // holds (a re-run), else starts easy.
            val placed = PLACEMENT_LADDERS.mapNotNull { (ladder, questionRes) ->
                val answer = a.placement(ladder)
                when {
                    answer != null -> stringResource(answer.labelRes)
                    ladder in kept -> stringResource(R.string.setup_review_place_keep, stringResource(questionRes))
                    else -> null
                }
            }.ifEmpty { listOf(stringResource(R.string.setup_review_place_easy)) }
            val equip = draft.calisEquipment?.let { stringResource(R.string.setup_review_equipment, equipmentText(it)) }
            add(
                ReviewEntry(
                    stringResource(R.string.setup_review_place),
                    listOfNotNull(placed.joinToString(" · "), equip).joinToString("\n"),
                    SetupStep.Placement,
                ),
            )
        }
        if (m != Modality.Lift) {
            // The race date and recent result ride under the goal they belong to,
            // only when given.
            val raceLines = if (draft.goalDistance.isRace) {
                listOfNotNull(
                    draft.goalRaceEpochDay?.let { stringResource(R.string.setup_review_race_date, epochDayLabel(it)) },
                    draft.recentRace?.let { stringResource(R.string.setup_review_recent_race, recentRaceText(it, distanceUnit)) },
                )
            } else {
                emptyList()
            }
            add(
                ReviewEntry(
                    stringResource(R.string.setup_review_run_goal),
                    (listOf(stringResource(draft.goalDistance.labelRes)) + raceLines).joinToString("\n"),
                    SetupStep.RunGoal,
                ),
            )
        }
        if (m == Modality.Both) {
            val lead = when (draft.concurrentGoal.lead()) {
                BothPriority.LiftLean -> R.string.setup_lead_lifting
                BothPriority.RunLean -> R.string.setup_lead_running
            }
            add(ReviewEntry(stringResource(R.string.setup_review_lead), stringResource(lead), SetupStep.Lead))
        }
        // Days: "5 days · 2 lift · 3 run" (the Profile row's wording), the
        // weekdays under it.
        val d = a.days ?: 0
        val count = pluralStringResource(R.plurals.setup_days_count, d, d)
        val daysHead = if (m == Modality.Both) {
            val lift = draft.liftDaysPerWeek ?: HYBRID_DEFAULT_LIFT_DAYS
            val run = draft.runningDaysPerWeek
            val rest = (d - lift - run).coerceAtLeast(0)
            count + " · " + if (rest > 0) {
                stringResource(R.string.setup_days_split_rest_sub, lift, run, rest)
            } else {
                stringResource(R.string.setup_days_split_sub, lift, run)
            }
        } else {
            count
        }
        val daysLines = listOf(daysHead, weekdaysLine(draft.trainingWeekdays, d))
        add(ReviewEntry(stringResource(R.string.setup_review_days), daysLines.joinToString("\n"), SetupStep.Days))
        // Experience on one line: the shared level when both sides match, else
        // "Lifting: X · Running: Y".
        val lvl = a.level
        if (lvl != null) {
            val rl = a.runLevel
            val levelText = if (m == Modality.Both && rl != null && rl != lvl) {
                stringResource(R.string.setup_review_level_lifting, stringResource(lvl.titleRes)) + " · " +
                    stringResource(R.string.setup_review_level_running, stringResource(rl.titleRes))
            } else {
                stringResource(lvl.titleRes)
            }
            add(ReviewEntry(stringResource(R.string.setup_review_level), levelText, SetupStep.Level))
        }
        // Starting volume: sets and weekly distance on one line, the stated
        // longest run under it when given.
        val runKm = weeklyVolumeScaleValue(draft.runningKmPerWeek, distanceUnit)
        val runs = draft.runningDaysPerWeek > 0
        val liftVolume = draft.weeklySets.takeIf { it > 0 }?.let { stringResource(R.string.setup_review_lift_volume, it) }
        val volume = when {
            !runs -> liftVolume.orEmpty()
            !draft.runningKmPerWeekSet ->
                listOfNotNull(liftVolume, stringResource(R.string.setup_review_run_volume_unset)).joinToString(" · ")
            draft.runningKmPerWeek == 0.0 ->
                listOfNotNull(liftVolume, stringResource(R.string.setup_runnow_none)).joinToString(" · ")
            draft.weeklySets > 0 -> stringResource(R.string.setup_review_volume_both, draft.weeklySets, runKm, unit)
            else -> stringResource(R.string.setup_review_run_volume, runKm, unit)
        }
        val longest = draft.statedLongestRecentKm?.let {
            stringResource(R.string.setup_review_runnow_longest, distanceBufferText(it, distanceUnit), unit)
        }
        add(
            ReviewEntry(
                stringResource(R.string.setup_review_volume),
                listOfNotNull(volume, longest).joinToString("\n"),
                if (runs) SetupStep.RunNow else SetupStep.Level,
            ),
        )
        val aboutFacts = listOfNotNull(
            draft.birthEpochDay?.let { birthdayLabel(it, today) }
                ?: draft.ageYears?.let { stringResource(R.string.setup_review_age, trimDecimal(it)) }
                ?: stringResource(R.string.setup_review_age_not_given),
            draft.bodyweightKg?.let { stringResource(R.string.setup_bodyweight_value, bodyweightBufferText(it, a.weightUnit), weightSuffix(a.weightUnit)) },
            a.sex?.let { stringResource(sexLabelRes(it.female)) },
            draft.restingHrBpm?.let { stringResource(R.string.setup_bpm_value, trimDecimal(it)) },
        )
        // The youth hold comes from age, so it shows here, not under Health (whose
        // answers may all be "No").
        val aboutLines = listOfNotNull(
            aboutFacts.joinToString(" · "),
            if (isMinor(draft.ageYears)) stringResource(R.string.setup_review_about_hold) else null,
        )
        add(ReviewEntry(stringResource(R.string.setup_review_about), aboutLines.joinToString("\n"), SetupStep.About))
        // Health: how many answers apply, and the hold whenever one defers (the
        // safety call stays on the face).
        val healthLines = listOfNotNull(
            healthSummary(draft.health),
            if (healthDefers(null, draft.health)) stringResource(R.string.setup_review_health_hold) else null,
        )
        add(ReviewEntry(stringResource(R.string.setup_review_health), healthLines.joinToString("\n"), SetupStep.Health))
    }
}

/** The pinned keypad band: one keypad for the screen's numbers, below the fields so
 *  it never scrolls away while typing (no IME popup), on its own full-bleed raised
 *  band with a hairline seam, so the scroll area visibly ends above it instead of
 *  slicing content mid-glyph. Its Next moves to [nextField]; Done (last field)
 *  closes the keypad (there is no IME to dismiss). */
@Composable
private fun KeypadBand(
    nextField: KeyField?,
    onAdvance: () -> Unit,
    onKey: (Char) -> Unit,
    onBackspace: () -> Unit,
    allowDecimal: Boolean,
) {
    Column(
        modifier = Modifier
            .fullBleed(Space.Screen.dp)
            .background(BgElevated),
    ) {
        HorizontalDivider(thickness = 1.dp, color = HairlineStrong)
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = Space.Screen.dp),
            horizontalArrangement = Arrangement.End,
        ) {
            val nextFieldName = when (nextField) {
                KeyField.Bodyweight -> stringResource(R.string.setup_row_bodyweight)
                KeyField.RestingHr -> stringResource(R.string.setup_row_resting_hr)
                KeyField.LongestRun -> stringResource(R.string.setup_runnow_longest_label)
                else -> null
            }
            TextAction(
                stringResource(if (nextField != null) R.string.setup_keypad_next else R.string.setup_keypad_done),
                onClick = onAdvance,
                description = nextFieldName?.let { stringResource(R.string.setup_keypad_next_cd, it) },
            )
        }
        Box(Modifier.padding(start = Space.Screen.dp, end = Space.Screen.dp, bottom = Space.Md.dp)) {
            NumericKeypad(onKey = onKey, onBackspace = onBackspace, allowDecimal = allowDecimal)
        }
    }
}

/** "Leave setup?": confirms dropping answers that were never sent. */
@Composable
private fun LeaveSetupDialog(onDismiss: () -> Unit, onLeave: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(stringResource(R.string.setup_leave_title)) },
        text = { Text(stringResource(R.string.setup_leave_body)) },
        confirmButton = {
            TextButton(onClick = onLeave) { Text(stringResource(R.string.setup_leave_confirm)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.setup_leave_dismiss)) }
        },
    )
}

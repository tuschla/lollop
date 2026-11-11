package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import app.milestone.resources.Res
import app.milestone.resources.activity_overline
import app.milestone.resources.ic_content_run
import app.milestone.resources.ic_content_set_dumbbell
import app.milestone.resources.ic_safety_shield
import app.milestone.resources.ic_safety_warning_triangle
import app.milestone.resources.log_rest_left_cd
import app.milestone.resources.log_undo
import app.milestone.resources.log_value_min
import app.milestone.resources.main_baseline_paused
import app.milestone.resources.main_blocked_still_works
import app.milestone.resources.main_building_baseline
import app.milestone.resources.main_checkin_for_today
import app.milestone.resources.main_chip_on_hold
import app.milestone.resources.main_chip_safety
import app.milestone.resources.main_coaching_notes
import app.milestone.resources.main_done_run_of
import app.milestone.resources.main_e1rm_caption
import app.milestone.resources.main_e1rm_caption_short
import app.milestone.resources.main_e1rm_change_since
import app.milestone.resources.main_e1rm_change_subject
import app.milestone.resources.main_e1rm_change_why
import app.milestone.resources.main_e1rm_title
import app.milestone.resources.main_e1rm_trend_cd
import app.milestone.resources.main_evidence_grade
import app.milestone.resources.main_hold_review_health
import app.milestone.resources.main_moved_to
import app.milestone.resources.main_next_session_overline
import app.milestone.resources.main_overline_progress
import app.milestone.resources.main_overline_readiness
import app.milestone.resources.main_overline_this_week
import app.milestone.resources.main_pain_prefix
import app.milestone.resources.main_pill_effort
import app.milestone.resources.main_pill_hrv
import app.milestone.resources.main_pill_resting_hr
import app.milestone.resources.main_pill_strength
import app.milestone.resources.main_pill_wellness
import app.milestone.resources.main_log_run_title
import app.milestone.resources.main_plan_item_track
import app.milestone.resources.main_plan_prompt_body
import app.milestone.resources.main_plan_prompt_cta
import app.milestone.resources.main_plan_prompt_title
import app.milestone.resources.main_plan_withheld_empty_body
import app.milestone.resources.main_plan_withheld_empty_cta
import app.milestone.resources.main_plan_withheld_empty_title
import app.milestone.resources.main_plan_withheld_health_body
import app.milestone.resources.main_plan_withheld_health_title
import app.milestone.resources.main_readiness_earlier
import app.milestone.resources.main_remove_plan
import app.milestone.resources.main_remove_plan_message
import app.milestone.resources.main_remove_plan_title
import app.milestone.resources.main_remove_report
import app.milestone.resources.main_rest_day
import app.milestone.resources.main_rest_day_next_overline
import app.milestone.resources.main_rest_pill
import app.milestone.resources.main_restart_hrv_baseline
import app.milestone.resources.main_safety_critical_suffix
import app.milestone.resources.main_session_done_met_cd
import app.milestone.resources.main_session_done_overline
import app.milestone.resources.main_session_done_resume
import app.milestone.resources.main_session_done_review
import app.milestone.resources.main_session_done_title
import app.milestone.resources.main_session_on_hold
import app.milestone.resources.main_setup_prompt_body
import app.milestone.resources.main_setup_prompt_cta
import app.milestone.resources.main_setup_prompt_title
import app.milestone.resources.main_sparse_week
import app.milestone.resources.main_status_adjusted_today
import app.milestone.resources.main_status_coach_note
import app.milestone.resources.main_status_mark_safety
import app.milestone.resources.main_status_on_hold
import app.milestone.resources.main_status_pain
import app.milestone.resources.main_status_pain_area
import app.milestone.resources.main_status_unwell
import app.milestone.resources.main_today_empty
import app.milestone.resources.main_todays_session
import app.milestone.resources.plan_guide_first_version
import app.milestone.resources.plan_guide_first_weight
import app.milestone.resources.plan_guide_spelled
import app.milestone.resources.ui_label_value
import kotlin.math.abs
import kotlinx.datetime.LocalDate
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.pluralStringResource
import org.jetbrains.compose.resources.stringResource

/** Where Today's actions lead outside the screen. */
internal class TodayActions(
    val onEvent: (Event) -> Unit,
    val onStartSetup: () -> Unit,
    // Re-runs guided setup seeded from the profile (a profile with nothing to plan).
    val onRerunSetup: () -> Unit,
    // Opens Profile at a row (the way to fix a plan that can't be built).
    val onOpenProfile: (ProfileRowId) -> Unit,
    // Opens the Log sheet on one editor (today's check-in, the session review).
    val onOpenSheet: (LogMode) -> Unit,
    val onLogPlanItem: (PrescriptionView) -> Unit,
    val onLogPastItem: (PrescriptionView, Long) -> Unit,
    val onLogPastRun: (Long) -> Unit,
    // Null where the platform has no tracker: a planned run's action then opens
    // the run editor.
    val onTrackRun: (() -> Unit)?,
)

/**
 * Today: the status cards, the readiness strip, the hero (the next session, else
 * the plan or setup prompt), this week, then the e1RM trend. While training is
 * blocked the pinned banner carries the call; Today then shows only a hold card
 * the banner does not state, what still works and the read-only week. The call
 * is the core's `today_headline`; the shell never ranks it.
 */
@Composable
internal fun TodayDestination(
    model: ViewModel,
    listState: androidx.compose.foundation.lazy.LazyListState,
    actions: TodayActions,
) {
    val onEvent = actions.onEvent
    // Removing the generated plan is confirm-gated.
    var confirmRemovePlan by rememberSaveable { mutableStateOf(false) }
    // The day open under the week strip, hoisted so a move opens its target day.
    var weekSelectedDay by rememberSaveable { mutableStateOf(-1L) }
    val res = currentResources()
    val moveSession = { from: Long, to: Long ->
        onEvent(Event.MoveSession(from, to))
        weekSelectedDay = to
        UiMessages.post(
            UiMessage(
                res.getString(Res.string.main_moved_to, planDateLabel(to)),
                actionLabel = res.getString(Res.string.log_undo),
                onAction = { onEvent(Event.MoveSession(to, from)) },
            ),
        )
    }

    val hasCoachSignal = model.feedback != null ||
        model.adjustments.isNotEmpty() ||
        model.review_adjustments.isNotEmpty() ||
        model.input_count > 0 ||
        // A morning check-in is coaching signal too; even pre-baseline
        // (the "collecting baseline" honesty state), and once derived signals land.
        model.latest_checkin != null ||
        model.readiness_summary.isNotEmpty() ||
        model.baseline_status.isNotEmpty()
    val hasAnything = hasCoachSignal || model.lifts.isNotEmpty() || model.runs.isNotEmpty()

    val trend = remember(model.lifts, model.next_session, model.week_plan) { e1rmTrend(model) }

    // Modality gating: the e1RM trend is a lifting signal. A not-yet-set-up
    // user (profile == null) still sees it.
    val showLifting = model.profile == null || model.showLifting()
    val weightUnit = model.weightUnit()
    val hold = remember(model.today_headline, model.adjustments) { holdEvidence(model) }
    // The area an active pain report names: an exercise it stopped reads
    // "Stopped · <area>" in the hero and the week strip alike.
    val painArea = painAreaLabel(model.active_pain?.area)

    // The session column (hero, or the hold while training is blocked) and the
    // week column (this week, progress). One list on a phone; side by side on a
    // wide window (visual-design-system §5 "Adaptive layout").
    val blocked = model.train_blocked
    val youthPause = blocked && holdAction(model.hold_reason, model.safety_tier) == HoldAction.CheckAge
    val hasWeek = model.week_plan.isNotEmpty() || model.program != null
    val nextSession = model.next_session
    val showWeek = if (blocked) hasWeek else nextSession != null && hasWeek
    val showProgress = !blocked && trend != null && showLifting
    val sessionItems: androidx.compose.foundation.lazy.LazyListScope.() -> Unit = {
        if (blocked) {
            // Under 18 the pause is no pinned DO-NOT-TRAIN banner but this calm
            // card, the list's first item, so it scrolls with the page.
            if (youthPause) {
                item { YouthHoldCard(model, actions.onOpenProfile) }
            }
            // A hold the banner (or the youth card) states in full gets nothing
            // here. Any other (a medical referral, a review deferral) keeps the
            // core's own hold summary as one card: its instruction is more
            // specific than the banner's.
            val pregnancyWarning = model.profile?.health?.pregnancy_warning_sign == true
            if (hold != null && !bannerStatesHold(model.hold_reason, model.safety_tier, pregnancyWarning)) {
                item {
                    // Safety is a rule, not a probability: no confidence meter.
                    EvidenceCard(
                        hold.summary, hold.evidence,
                        confidenceInWhy = false, why = hold.why,
                        proseSummary = hold.prose,
                        glossary = hold.glossary, spokenSubject = hold.subject,
                        status = StatusTone.Adjusted,
                        statusOverline = stringResource(Res.string.main_status_on_hold),
                    )
                }
            }
            // What still works while training is paused (the youth card says it itself).
            if (!youthPause) {
                item {
                    Text(
                        stringResource(Res.string.main_blocked_still_works),
                        color = OnBgMuted,
                        style = Type.Caption,
                    )
                }
            }
            // Under 18 (WHO 5–17, youth-pa-001): the weekly activity card, where the
            // hero would be. Information and tracking only; the hold is unchanged.
            model.youth_activity?.let { activity ->
                item { SectionOverline(stringResource(Res.string.activity_overline)) }
                item {
                    YouthActivityCard(activity, onLog = { actions.onOpenSheet(LogMode.Activity) }, onEvent = onEvent)
                }
            }
        } else {
            // The readiness strip mounts only with a pill or a baseline caption to show:
            // passive per-signal states, no ring, no 0–100 number (INVARIANT 1).
            val hasReadinessPills = model.stripReadinessRows().isNotEmpty() || model.baseline_status.isNotEmpty()
            item {
                TodayHeroCard(
                    model = model,
                    hasSignal = hasAnything,
                    actions = actions,
                    onMove = moveSession,
                    painArea = painArea,
                    readinessStrip = if (hasReadinessPills) {
                        { ReadinessStrip(model, onEvent, onCheckin = { actions.onOpenSheet(LogMode.Checkin) }) }
                    } else {
                        null
                    },
                )
            }
        }
    }
    val weekItems: androidx.compose.foundation.lazy.LazyListScope.() -> Unit = {
        // This week: the count, the strip and the plan footer as one card, only
        // when a plan exists. While training is blocked the week stays in view,
        // read-only (held days read "paused").
        if (showWeek) {
            item { SectionOverline(stringResource(Res.string.main_overline_this_week)) }
            item {
                ThisWeekCard(
                    consistency = model.consistency,
                    week = model.week_plan,
                    today = model.today(),
                    weightUnit = weightUnit,
                    program = model.program,
                    // Nothing logged yet: the week card's last line.
                    footnote = if (!blocked && !hasAnything) stringResource(Res.string.main_today_empty) else null,
                    painArea = painArea,
                    onRemovePlan = { confirmRemovePlan = true },
                    actions = if (blocked) {
                        null
                    } else {
                        WeekActions(
                            onLogItem = actions.onLogPastItem,
                            onLogRun = actions.onLogPastRun,
                            onEndExercise = { item, day -> endExerciseWithUndo(res, item, day, onEvent) },
                            onReopen = { item, day -> reopenExercise(item, day, onEvent) },
                            onMove = moveSession,
                        )
                    },
                    heroDay = if (blocked) -1L else nextSession?.epoch_day ?: -1L,
                    selectedDay = weekSelectedDay,
                    onSelectDay = { weekSelectedDay = it },
                )
            }
        }
    }
    // Nothing yet and no week card to carry the hint.
    val emptyHint: androidx.compose.foundation.lazy.LazyListScope.() -> Unit = {
        if (!blocked && !hasAnything && !showWeek) {
            item {
                Text(
                    stringResource(Res.string.main_today_empty),
                    color = OnBgFaint,
                    style = Type.Caption,
                )
            }
        }
    }
    // Progress: the e1RM trend of the plan's main lift, the last section.
    val progressItems: androidx.compose.foundation.lazy.LazyListScope.() -> Unit = {
        if (showProgress && trend != null) {
            item { SectionOverline(stringResource(Res.string.main_overline_progress)) }
            item { E1rmTrendCard(trend, weightUnit) }
        }
    }

    val layout = LocalWindowLayout.current
    // The Log FAB floats over a phone's list; with the side rail Log is the rail's.
    val bottomClearance = if (layout.navRail) Space.Lg.dp else TodayFabClearance
    val listSpacing = Arrangement.spacedBy(Space.Screen.dp)
    if (layout.twoPane && (showWeek || showProgress)) {
        TwoPaneRow(
            first = {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.weight(1f).fillMaxSize().testTag("today-session-pane"),
                    contentPadding = PaddingValues(start = Space.Screen.dp, end = Space.Md.dp, top = Space.Screen.dp, bottom = bottomClearance),
                    verticalArrangement = listSpacing,
                ) {
                    sessionItems()
                    emptyHint()
                }
            },
            second = {
                LazyColumn(
                    modifier = Modifier.weight(1f).fillMaxSize().testTag("today-week-pane"),
                    contentPadding = PaddingValues(start = Space.Md.dp, end = Space.Screen.dp, top = Space.Screen.dp, bottom = bottomClearance),
                    verticalArrangement = listSpacing,
                ) {
                    weekItems()
                    progressItems()
                }
            },
        )
    } else {
        ReadableWidth { gutter ->
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = gutter, end = gutter, top = Space.Screen.dp, bottom = bottomClearance),
                verticalArrangement = listSpacing,
            ) {
                sessionItems()
                weekItems()
                emptyHint()
                progressItems()
            }
        }
    }

    ClearConfirmDialog(
        visible = confirmRemovePlan,
        title = stringResource(Res.string.main_remove_plan_title),
        message = stringResource(Res.string.main_remove_plan_message),
        confirmLabel = stringResource(Res.string.main_remove_plan),
        onDismiss = { confirmRemovePlan = false },
        onClear = { onEvent(Event.ClearPlan) },
    )
}

/**
 * Which cards Today's stack shows, decided from the model alone ([todayCards];
 * [TodayHeroCard] renders them). [sessionAdjustmentAbove]: a card above the
 * session already states its adjustment, so the session card never repeats it;
 * [foldCount] notes fold into one row once there are two or more.
 */
internal class TodayCards(
    val showHeadline: Boolean,
    val headlineAdjustment: AdjustmentView?,
    // The headline hosts the hold's safety rows / the session-wide coaching rows.
    val headlineHosts: Boolean,
    val headlineHostsGeneral: Boolean,
    // The headline is a status card (an adjustment or a safety call).
    val headlineIsStatus: Boolean,
    val showFeedback: Boolean,
    val doneOwnsFeedback: Boolean,
    val sessionDone: SessionDoneView?,
    val todayItems: List<PrescriptionView>,
    val hostAdjustment: AdjustmentView?,
    val safetyRows: List<AdjustmentView>,
    val groupFaceText: String,
    val softResolutions: List<HoldResolutionView>,
    val foldedResolution: HoldResolutionView?,
    val painReported: Boolean,
    val generalHost: AdjustmentView?,
    val generalRows: List<AdjustmentView>,
    val winAdvice: AdjustmentView?,
    val coachingCards: List<AdjustmentView>,
    val foldHeadline: Boolean,
    val foldFeedback: Boolean,
    val foldCount: Int,
    val sessionAdjustment: AdjustmentView?,
    val sessionAdjustmentCard: AdjustmentView?,
    val sessionAdjustmentAbove: Boolean,
    val metaOnFace: List<AdjustmentView>,
    val sessionMeta: List<AdjustmentView>,
) {
    val hasSafetyHost: Boolean get() = headlineHosts || hostAdjustment != null
    val folded: Boolean get() = foldCount >= 2
}

/**
 * Today's card selection (training not blocked). Coaching adjustments read as one
 * card per exercise ([coachingPerExercise]); once today's session is done the
 * adjustments that shaped it are spent and only safety calls and next-time notes
 * stay. One safety card hosts each hold, its other safety rows behind its "?". A
 * new best and its lift's next-time advice (the row the core marks
 * `win_advice`) share one card.
 * [hasSignal] false (a brand-new user) leaves only the plan-state card. Pure.
 */
internal fun todayCards(model: ViewModel, hasSignal: Boolean): TodayCards {
    val headline = model.today_headline
    val fb = model.feedback
    val showFeedback = fb != null && headline.kind != HeadlineKind.Feedback
    val today = model.today()
    val sessionDone = model.session_done?.takeIf { it.items.isNotEmpty() }
    // The done card owns a non-safety takeaway; a safety one keeps its own card.
    val doneOwnsFeedback = sessionDone != null && showFeedback && !fb.safety_critical
    val todayItems = model.week_plan.firstOrNull { it.epoch_day == today }?.items.orEmpty()
    // An in-session cue is spent as soon as its own exercise is settled.
    val settledToday = todayItems
        .filter { it.exercise.isNotBlank() && (it.done || it.status.isSettled) }
        .map { it.exercise }
        .let(::nameKeys)
        .toSet()
    val spent = { a: AdjustmentView ->
        !a.safety_critical && (
            sessionDone != null ||
                ((a.suggested_load_kg != null || a.suggests_end_exercise) &&
                    a.exercise?.let(::nameKey) in settledToday)
            )
    }
    // Several rules emit the same summary: the core marks the rows the headline or
    // the next session's adjustment already states.
    val nextSession = model.next_session
    // A return check states its advice on its own item: no card for it above.
    val sessionOwned = nextSession?.adjustment_on_item == true
    val ownedBySession = { a: AdjustmentView -> sessionOwned && a.repeats_session }
    val headlineAdjustment = model.adjustments.firstOrNull { it.repeats_headline }
    val showHeadline = headline.kind != HeadlineKind.Prescription &&
        headlineAdjustment?.let(spent) != true &&
        !(headline.kind == HeadlineKind.Adjustment && sessionOwned && headline.repeats_session)
    val listed = model.adjustments.byAdjustmentPriority().filterNot { it.repeats_headline }.filterNot(spent).filterNot(ownedBySession) +
        model.review_adjustments.byAdjustmentPriority().filterNot { it.repeats_headline }
    // A resolvable hold that does not block training has no banner: its
    // "I've recovered" action lives on Today.
    val softResolutions = model.hold_resolutions.filter { it.action_label.isNotBlank() }

    val headlineHosts = hasSignal && showHeadline && headline.grade != null && headline.safety_critical
    val safetyListed = if (hasSignal) listed.filter { it.safety_critical } else emptyList()
    val hostAdjustment = if (headlineHosts) null else safetyListed.firstOrNull()
    val hasSafetyHost = headlineHosts || hostAdjustment != null
    val safetyRows = if (hasSafetyHost) safetyListed.filterNot { it === hostAdjustment } else emptyList()
    val foldedResolution = if (hasSafetyHost) softResolutions.firstOrNull() else null
    val coaching = coachingPerExercise(
        headlineExercise = headlineAdjustment?.exercise?.takeIf { showHeadline },
        candidates = listed.filterNot { it.safety_critical },
    )
    // The session's own non-safety adjustment: its prose is a status card above
    // the session unless another adjustment card already speaks.
    val sessionAdjustment = nextSession?.adjustment?.takeIf {
        nextSession.status == SessionStatus.Adjusted && !it.safety_critical && !sessionOwned
    }
    val adjustedAbove = hasSignal && (
        hasSafetyHost || coaching.isNotEmpty() ||
            (showHeadline && headline.kind == HeadlineKind.Adjustment && headline.grade != null)
        )
    // A card above already states the session's adjustment.
    val repeatedAbove = sessionOwned || hasSignal && (
        (showHeadline && headline.repeats_session) || (showFeedback && fb.repeats_session) ||
            safetyListed.any { it.repeats_session } || coaching.any { it.repeats_session }
        )
    val sessionAdjustmentCard = sessionAdjustment?.takeIf { !adjustedAbove && !repeatedAbove }
    // Engine-meta disclosures sit in the session card's "?". While a hold or a
    // safety adjustment is active they are dropped ("loads stay at plan" would
    // contradict it); without a session card only a safety-critical one stays.
    val holdActive = hasSafetyHost || model.hold_resolutions.isNotEmpty()
    val meta = if (hasSignal && !holdActive) listOfNotNull(model.provisional, model.autoreg_source) else emptyList()
    // Session-wide coaching (no exercise named) reads as one card: an adjustment
    // headline speaking session-wide hosts it, else the first such row.
    val headlineHostsGeneral = hasSignal && showHeadline && headline.kind == HeadlineKind.Adjustment &&
        headline.grade != null && !headlineHosts && headlineAdjustment?.exercise.isNullOrBlank()
    val coachGeneral = coaching.filter { it.exercise.isNullOrBlank() }
    val generalHost = if (headlineHostsGeneral) null else coachGeneral.firstOrNull()
    val generalRows = coachGeneral.filterNot { it === generalHost }
    val winText = when {
        fb == null || fb.safety_critical || fb.grade == null || fb.category != FeedbackCategory.PositiveMastery -> null
        showFeedback && !doneOwnsFeedback -> fb.message
        showHeadline && headline.kind == HeadlineKind.Feedback -> headline.summary
        else -> null
    }
    val winAdvice = winText?.let { coaching.firstOrNull { it.win_advice } }
    val coachingCards = coaching.filterNot { a -> generalRows.any { it === a } || a === winAdvice }
    // While today's session is under way its non-safety notes fold into one row,
    // so the session stays near the top.
    val inProgress = hasSignal && sessionDone == null &&
        todayItems.any { it.done || it.ended_early || it.logged_sets.isNotEmpty() }
    val foldHeadline = inProgress && showHeadline && headline.grade != null &&
        !headline.safety_critical && (headline.kind == HeadlineKind.Adjustment || headline.kind == HeadlineKind.Feedback)
    val foldFeedback = inProgress && showFeedback && !doneOwnsFeedback && !fb.safety_critical
    return TodayCards(
        showHeadline = showHeadline,
        headlineAdjustment = headlineAdjustment,
        headlineHosts = headlineHosts,
        headlineHostsGeneral = headlineHostsGeneral,
        headlineIsStatus = showHeadline && headline.grade != null && (headline.kind == HeadlineKind.Adjustment || headlineHosts),
        showFeedback = showFeedback,
        doneOwnsFeedback = doneOwnsFeedback,
        sessionDone = sessionDone,
        todayItems = todayItems,
        hostAdjustment = hostAdjustment,
        safetyRows = safetyRows,
        groupFaceText = foldedResolution?.paused_text.orEmpty().ifBlank { safetyRows.firstOrNull()?.summary.orEmpty() },
        softResolutions = softResolutions,
        foldedResolution = foldedResolution,
        painReported = model.readiness_summary.any { it.signal == ReadinessSignal.Pain },
        generalHost = generalHost,
        generalRows = generalRows,
        winAdvice = winAdvice,
        coachingCards = coachingCards,
        foldHeadline = foldHeadline,
        foldFeedback = foldFeedback,
        foldCount = (if (foldHeadline) 1 else 0) + (if (foldFeedback) 1 else 0) +
            (if (inProgress) coachingCards.size else 0),
        sessionAdjustment = sessionAdjustment,
        sessionAdjustmentCard = sessionAdjustmentCard,
        sessionAdjustmentAbove = repeatedAbove || sessionAdjustmentCard != null,
        metaOnFace = if (nextSession == null) meta.filter { it.safety_critical } else emptyList(),
        sessionMeta = if (nextSession != null) meta else emptyList(),
    )
}

/**
 * Today's card stack (ui-sweep §5), as [todayCards] picks it: the safety status
 * cards, the other status cards, the remaining coaching (a graded win, the
 * ungraded all-clear as a plain card), the [readinessStrip], the session-done
 * moment, then the plan-state card. An active pain report always offers "Remove
 * report". Only runs while training is not blocked.
 */
@Composable
private fun TodayHeroCard(
    model: ViewModel,
    hasSignal: Boolean,
    actions: TodayActions,
    // Moves a session (posts "Moved to … · Undo").
    onMove: (from: Long, to: Long) -> Unit,
    // The area the active pain report names ("Stopped · <area>").
    painArea: String?,
    // The readiness strip, placed between the status cards and the hero.
    readinessStrip: (@Composable () -> Unit)?,
) {
    val onEvent = actions.onEvent
    val headline = model.today_headline
    val fb = model.feedback
    val today = model.today()
    val nextSession = model.next_session
    val distanceUnit = rememberDistanceUnit()
    val res = currentResources()
    val c = remember(model, hasSignal) { todayCards(model, hasSignal) }
    var resolveSignal by rememberSaveable { mutableStateOf<ReadinessSignal?>(null) }
    var removePain by rememberSaveable { mutableStateOf(false) }
    var notesOpen by rememberSaveable(today) { mutableStateOf(false) }
    val painOnHost = c.painReported && c.hasSafetyHost
    // The hold group: call + one supporting line + its action on the face, the
    // other safety rows behind the host's "?".
    val groupFooter: (@Composable () -> Unit)? = if (c.groupFaceText.isNotBlank()) {
        { Text(c.groupFaceText, color = OnBgMuted, style = Type.Caption) }
    } else {
        null
    }
    val groupPanel: (@Composable () -> Unit)? = if (c.safetyRows.isNotEmpty()) {
        { HoldGroupPanel(c.safetyRows, c.groupFaceText) }
    } else {
        null
    }
    val groupActionLabel = c.foldedResolution?.action_label
    val groupOnAction: (() -> Unit)? = c.foldedResolution?.let { r -> { resolveSignal = r.signal } }
    // Status-card overlines say what the card is about, so colour is never the only signal.
    val illnessActive = model.hold_resolutions.any { it.signal == ReadinessSignal.Illness } ||
        model.readiness_summary.any { it.signal == ReadinessSignal.Illness }
    val painOverline = painArea?.let { stringResource(Res.string.main_status_pain_area, it) }
        ?: stringResource(Res.string.main_status_pain)
    val unwellOverline = stringResource(Res.string.main_status_unwell)
    val adjustedOverline = stringResource(Res.string.main_status_adjusted_today)
    val coachNoteOverline = stringResource(Res.string.main_status_coach_note)
    val safetyOverline = when {
        c.foldedResolution?.signal == ReadinessSignal.Pain || (c.foldedResolution == null && c.painReported) -> painOverline
        c.foldedResolution?.signal == ReadinessSignal.Illness || illnessActive -> unwellOverline
        else -> adjustedOverline
    }
    val coachOverline = { a: AdjustmentView? ->
        when {
            !a?.exercise.isNullOrBlank() -> coachNoteOverline
            illnessActive -> unwellOverline
            else -> adjustedOverline
        }
    }
    val removeReportLabel = stringResource(Res.string.main_remove_report)
    val generalFooter: (@Composable () -> Unit)? = if (c.generalRows.isNotEmpty()) {
        { c.generalRows.forEach { Text(it.summary, color = OnBgBody, style = Type.Body) } }
    } else {
        null
    }
    val generalPanel: (@Composable () -> Unit)? = if (c.generalRows.isNotEmpty()) {
        { Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) { c.generalRows.forEach { PanelAdjustment(it) } } }
    } else {
        null
    }
    val winFooter: (@Composable () -> Unit)? = c.winAdvice?.let { a -> { Text(a.summary, color = OnBgBody, style = Type.Body) } }
    val winPanel: (@Composable () -> Unit)? = c.winAdvice?.let { a -> { PanelAdjustment(a) } }

    // Today's call. An adjustment (or a safety call) is an `Adjusted` status card;
    // a graded feedback headline keeps the full EvidenceCard; the ungraded
    // all-clear is a plain card. A "prescription" headline restates the session
    // card, so it gets none.
    val headlineCard: @Composable () -> Unit = {
        if (c.showHeadline) {
            if (headline.grade != null) {
                val isAdjustment = headline.kind == HeadlineKind.Adjustment
                val hostsWin = c.winAdvice != null && headline.kind == HeadlineKind.Feedback
                EvidenceCard(
                    headline.summary, headline.evidence,
                    glossary = headline.glossary, spokenSubject = headline.subject,
                    status = if (isAdjustment || c.headlineHosts) StatusTone.Adjusted else null,
                    statusOverline = if (c.headlineHosts) safetyOverline else coachOverline(c.headlineAdjustment),
                    why = headline.why,
                    footer = when {
                        c.headlineHosts -> groupFooter
                        c.headlineHostsGeneral -> generalFooter
                        hostsWin -> winFooter
                        else -> null
                    },
                    panelExtra = when {
                        c.headlineHosts -> groupPanel
                        c.headlineHostsGeneral -> generalPanel
                        hostsWin -> winPanel
                        else -> null
                    },
                    actionLabel = if (c.headlineHosts) groupActionLabel else null,
                    onAction = if (c.headlineHosts) groupOnAction else null,
                    secondaryActionLabel = if (c.headlineHosts && painOnHost) removeReportLabel else null,
                    onSecondaryAction = { removePain = true },
                    actionPrimary = false,
                    proseSummary = isAdjustment || headline.prose,
                )
            } else {
                PlainCard {
                    Text(
                        headline.summary,
                        color = OnBgBody,
                        style = Type.BodyBold,
                    )
                }
            }
        }
    }
    val feedbackCard: @Composable () -> Unit = {
        if (c.showFeedback && !c.doneOwnsFeedback && fb != null) {
            val message = fb.message
            if (fb.grade == null) {
                // An ungraded note asserts no claim: no evidence chrome, like the all-clear.
                PlainCard { Text(message, color = OnBgBody, style = Type.Body) }
            } else {
                EvidenceCard(
                    message, fb.evidence,
                    fb.category_label.ifBlank { null },
                    why = fb.why,
                    glossary = fb.glossary, spokenSubject = fb.subject,
                    proseSummary = fb.prose,
                    footer = winFooter,
                    panelExtra = winPanel,
                )
            }
        }
    }
    val coachingCard: @Composable (AdjustmentView) -> Unit = { a ->
        val hostsGeneral = a === c.generalHost
        EvidenceCard(
            a.summary, a.evidence,
            why = a.why,
            glossary = a.glossary, spokenSubject = a.subject,
            status = StatusTone.Adjusted,
            statusOverline = coachOverline(a),
            footer = if (hostsGeneral) generalFooter else null,
            panelExtra = if (hostsGeneral) generalPanel else null,
            proseSummary = true,
        )
    }

    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        if (c.headlineHosts) headlineCard()
        c.hostAdjustment?.let { a ->
            EvidenceCard(
                a.summary, a.evidence,
                why = a.why,
                glossary = a.glossary, spokenSubject = a.subject,
                proseSummary = a.prose,
                status = StatusTone.Adjusted,
                statusOverline = safetyOverline,
                footer = groupFooter,
                panelExtra = groupPanel,
                actionLabel = groupActionLabel,
                onAction = groupOnAction,
                secondaryActionLabel = if (painOnHost) removeReportLabel else null,
                onSecondaryAction = { removePain = true },
                actionPrimary = false,
            )
        }
        c.softResolutions.filterNot { it === c.foldedResolution }.forEach { r ->
            HoldResolutionCard(
                r,
                overline = if (r.signal == ReadinessSignal.Pain) painOverline else unwellOverline,
                onResolve = { resolveSignal = r.signal },
                onRemoveReport = if (c.painReported && !c.hasSafetyHost && r.signal == ReadinessSignal.Pain) ({ removePain = true }) else null,
            )
        }
        // A pain report with no card of its own above still gets its undo.
        if (c.painReported && !c.hasSafetyHost && c.softResolutions.none { it.signal == ReadinessSignal.Pain }) {
            StatusCard(
                tone = StatusTone.Adjusted,
                overline = painOverline,
                footer = { StatusCardFooter(primaryLabel = null, onPrimary = {}, secondaryLabel = removeReportLabel, onSecondary = { removePain = true }) },
            ) {
                model.active_pain?.context?.let {
                    Text(stringResource(Res.string.main_pain_prefix, it), color = OnBgBody, style = Type.Body)
                }
            }
        }
        if (hasSignal) {
            if (c.folded) {
                CoachingNotesRow(c.foldCount, notesOpen) { notesOpen = !notesOpen }
                if (notesOpen) {
                    if (c.foldHeadline) headlineCard()
                    if (c.foldFeedback) feedbackCard()
                    c.coachingCards.forEach { coachingCard(it) }
                }
            } else {
                if (c.headlineIsStatus && !c.headlineHosts) headlineCard()
                c.coachingCards.forEach { coachingCard(it) }
            }
        }
        // The session's own adjustment when no card above speaks for it.
        c.sessionAdjustmentCard?.let { coachingCard(it) }
        if (hasSignal) {
            if (!c.headlineIsStatus && !(c.folded && c.foldHeadline)) headlineCard()
            c.metaOnFace.forEach { m ->
                EvidenceCard(
                    m.summary, m.evidence,
                    why = m.why,
                    glossary = m.glossary, spokenSubject = m.subject,
                    proseSummary = m.prose,
                )
            }
            if (!(c.folded && c.foldFeedback)) feedbackCard()
        }
        readinessStrip?.let { strip -> Box(Modifier.padding(vertical = Space.Sm.dp)) { strip() } }
        c.sessionDone?.let { done ->
            // Finishing ends every open exercise; resuming reopens them all and
            // returns to the first one's set loop.
            val ended = c.todayItems.filter { it.ended_early && it.exercise.isNotBlank() }
            SessionDoneCard(
                done,
                feedback = if (c.doneOwnsFeedback) fb else null,
                painArea = painArea,
                distanceUnit = distanceUnit,
                onResume = ended.firstOrNull()?.let { first ->
                    {
                        ended.forEach { reopenExercise(it, today, onEvent) }
                        actions.onLogPlanItem(first)
                    }
                },
                onReview = { actions.onOpenSheet(LogMode.Review) },
            )
        }
        // The plan-state card, always: the next session, else the way to a plan.
        when {
            nextSession != null -> NextSessionCard(
                nextSession,
                painArea = painArea,
                today = today,
                restToday = model.today_surface.rest_today,
                adjustmentAbove = c.sessionAdjustmentAbove,
                adjusted = c.sessionAdjustment != null,
                coachStatus = c.sessionMeta,
                weightUnit = model.weightUnit(),
                ladders = model.calisthenics_ladders,
                showGuide = model.today_surface.show_guide,
                onLogItem = actions.onLogPlanItem,
                onTrackRun = actions.onTrackRun ?: { actions.onOpenSheet(LogMode.Run) },
                runActionLabel = if (actions.onTrackRun != null) Res.string.main_plan_item_track else Res.string.main_log_run_title,
                onEndExercise = { endExerciseWithUndo(res, it, today, onEvent) },
                onReopen = { reopenExercise(it, today, onEvent) },
                onSetRung = { ladder, rung -> onEvent(Event.SetCalisthenicsRung(ladder, rung, AppClock.nowSec())) },
                onMove = onMove,
            )
            model.profile != null -> when (model.plan_withheld) {
                // GeneratePlan can build a plan: one tap makes it appear.
                null -> PromptCard(
                    stringResource(Res.string.main_plan_prompt_title),
                    stringResource(Res.string.main_plan_prompt_body),
                    stringResource(Res.string.main_plan_prompt_cta),
                ) { onEvent(Event.GeneratePlan(today)) }
                // It can't: say why and where to fix it, never a dead button.
                PlanWithheld.HealthHold -> PromptCard(
                    stringResource(Res.string.main_plan_withheld_health_title),
                    stringResource(Res.string.main_plan_withheld_health_body),
                    stringResource(Res.string.main_hold_review_health),
                ) { actions.onOpenProfile(holdProfileRow(model.hold_reason, model.profile.health)) }
                // Nothing to plan: guided setup re-runs from the profile's answers.
                PlanWithheld.NothingToPlan -> PromptCard(
                    stringResource(Res.string.main_plan_withheld_empty_title),
                    stringResource(Res.string.main_plan_withheld_empty_body),
                    stringResource(Res.string.main_plan_withheld_empty_cta),
                    actions.onRerunSetup,
                )
            }
            // No profile yet: the plan is built from it, so this is the one setup entry point.
            else -> PromptCard(
                stringResource(Res.string.main_setup_prompt_title),
                stringResource(Res.string.main_setup_prompt_body),
                stringResource(Res.string.main_setup_prompt_cta),
                actions.onStartSetup,
            )
        }
    }
    HoldResolveDialog(
        resolution = resolveSignal?.let { sig -> model.hold_resolutions.firstOrNull { it.signal == sig } },
        model = model,
        onDismiss = { resolveSignal = null },
        onEvent = onEvent,
    )
    RemoveReportDialog(
        signal = if (removePain) ReadinessSignal.Pain else null,
        onDismiss = { removePain = false },
        onEvent = onEvent,
        model = model,
    )
}

/** "2 coaching notes ›": the session-in-progress fold of Today's coaching cards;
 *  a tap opens them under it. */
@Composable
private fun CoachingNotesRow(count: Int, open: Boolean, onToggle: () -> Unit) {
    PlainCard {
        ChevronRow(
            onClick = onToggle,
            modifier = Modifier.clip(RoundedCornerShape(Space.Md.dp)).testTag("coaching-notes"),
            expanded = open,
            spacing = Space.Md.dp,
        ) {
            Text(pluralStringResource(Res.plurals.main_coaching_notes, count, count), color = OnBgBody, style = Type.Body)
        }
    }
}

/**
 * The coaching (non-safety) adjustments Today shows: ONE per exercise. For an
 * exercise the headline already speaks about, none; otherwise the in-session cue
 * (it reacts to today's sets, so it is the newest word) wins over a plan-level
 * or next-time note, else the first in priority order. Adjustments with no
 * exercise all stay. Order is preserved. Pure.
 */
internal fun coachingPerExercise(headlineExercise: String?, candidates: List<AdjustmentView>): List<AdjustmentView> {
    val key = { a: AdjustmentView -> a.exercise?.takeIf { it.isNotBlank() }?.let(::nameKey) }
    val inSession = { a: AdjustmentView -> a.suggested_load_kg != null || a.suggests_end_exercise }
    val headlineKey = headlineExercise?.takeIf { it.isNotBlank() }?.let(::nameKey)
    val chosen = candidates.groupBy(key).mapValues { (_, group) -> group.firstOrNull(inSession) ?: group.first() }
    return candidates.filter { a ->
        val k = key(a)
        k == null || (k != headlineKey && chosen[k] === a)
    }
}

/** Today's list bottom inset: the Log FAB (56dp) plus the scaffold's 16dp FAB
 *  margin plus a 16dp gap, so the last row scrolls fully clear of the floating
 *  button. Rows themselves keep the full card width (no FAB lane). */
private val TodayFabClearance = 56.dp + 16.dp + 16.dp

// Signals that need hardware the app can't read yet: never shown as pills
// until an integration exists.
private val hardwareOnlySignals = setOf(
    ReadinessSignal.BarVelocity, ReadinessSignal.VelocityLoss, ReadinessSignal.AerobicDecoupling, ReadinessSignal.HrvCv,
)

/**
 * Readiness strip (01-today §A.1): one qualitative pill per measured signal and
 * one caption per channel still building its baseline. Passive status only
 * (logging is the Log button's), no composite score (INVARIANT 1); hardware-only
 * signals and a signal a hold card already states are left out. A suspended HRV
 * baseline cannot recover on its own, so its line carries the restart action.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ReadinessStrip(model: ViewModel, onEvent: (Event) -> Unit, onCheckin: () -> Unit) {
    val measuredRows = model.stripReadinessRows()
    PlainCard {
        TileOverline(stringResource(Res.string.main_overline_readiness), color = Accent)
        if (measuredRows.isNotEmpty()) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
            ) {
                // The core's judged state only, never the raw reading (a z-score or
                // ln rMSSD is jargon without a key).
                measuredRows.forEach { row ->
                    SignalPill(
                        label = readinessPillLabel(row.signal),
                        // A reading from an earlier check-in never passes as today's; a
                        // stale row's core state ("out of date") already says so.
                        state = if (row.from_today || row.stale) row.state else stringResource(Res.string.main_readiness_earlier, row.state),
                        safetyCritical = row.safety_critical,
                    )
                }
            }
        }
        // No check-in yet today: the pills above are older readings, so invite
        // today's instead of letting them look current.
        if (!model.checked_in_today) {
            TextAction(
                stringResource(Res.string.main_checkin_for_today),
                onClick = onCheckin,
                chevron = true,
                modifier = Modifier.offset(x = -TextActionInset),
            )
        }
        model.baseline_status.forEach { b ->
            val label = readinessPillLabel(b.signal)
            val line = when (b.kind) {
                BaselineKind.SparseWeek -> stringResource(Res.string.main_sparse_week, label, b.have, b.need)
                BaselineKind.Suspended -> stringResource(Res.string.main_baseline_paused, label)
                else -> stringResource(Res.string.main_building_baseline, label, b.have, b.need)
            }
            Text(line, color = OnBgMuted, style = Type.Caption)
            if (b.kind == BaselineKind.Suspended && b.signal == ReadinessSignal.HrvLnRmssd) {
                TextAction(
                    stringResource(Res.string.main_restart_hrv_baseline),
                    onClick = { onEvent(Event.RestartHrvBaseline()) },
                    modifier = Modifier.offset(x = -TextActionInset),
                )
            }
        }
    }
}

/** The readiness rows the strip shows as pills: measured signals with an app
 *  integration, minus any signal an active hold card on the hero already states.
 *  A scoped pain stop (the session still runs) keeps its pill, like a tendon
 *  report: its hero card speaks for the stopped exercises, not the readiness. */
private fun ViewModel.stripReadinessRows() =
    readiness_summary.filter { row ->
        row.signal !in hardwareOnlySignals &&
            (hold_resolutions.none { it.signal == row.signal } || row.signal == ReadinessSignal.Pain)
    }

/** One readiness pill. A safety-critical signal is marked by more than colour: a
 *  warning glyph on the warn fill (the amber tier of the adjustment it drives);
 *  the pill speaks as one node. */
@Composable
private fun SignalPill(label: String, state: String, safetyCritical: Boolean) {
    val desc = "$label: $state" + if (safetyCritical) stringResource(Res.string.main_safety_critical_suffix) else ""
    val onPill = if (safetyCritical) OnStatusFill else OnBgMuted
    Row(
        modifier = Modifier
            .clip(PillShape)
            .background(if (safetyCritical) LocalStatusColors.current.warn else BgTop)
            .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp)
            .clearAndSetSemantics { contentDescription = desc },
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (safetyCritical) {
            Icon(
                painterResource(Res.drawable.ic_safety_warning_triangle),
                contentDescription = null,
                tint = onPill,
                modifier = Modifier.size(12.dp),
            )
        }
        // One text, so a long state wraps under the label inside the pill instead
        // of opening a second column beside it.
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = if (safetyCritical) onPill else OnBgFaint)) { append(label) }
                append(" ")
                withStyle(SpanStyle(color = onPill)) { append(state) }
            },
            style = Type.Caption.merge(TabularFigures),
        )
    }
}

/** The Progress card's data: one lift's best e1RM per session, oldest first. */
internal class E1rmTrend(
    val exercise: String,
    val series: List<Double>,
    val firstDay: LocalDate,
)

/** Logged days a lift needs before its e1RM series is a trend. */
private const val E1RM_TREND_MIN_DAYS = 3

/** The e1RM trend for the plan's main lift (the first lift of the next session,
 *  then of the week plan), else the most recently logged weighted lift: the best
 *  core-derived e1RM of each logged day, oldest → newest. Null with fewer than
 *  [E1RM_TREND_MIN_DAYS] dated sessions. A bodyweight/hold set has no
 *  e1RM; an undated set belongs to no session. */
internal fun e1rmTrend(model: ViewModel): E1rmTrend? {
    val weighted = model.lifts.filter { it.hold_sec == null && it.e1rm_kg > 0.0 && it.observed_at > 0L }
    if (weighted.isEmpty()) return null
    val zone = TimeZone.currentSystemDefault()
    val spellings = weighted.map { it.exercise }.distinct()
    val keyOf = spellings.zip(nameKeys(spellings)).toMap()
    fun build(exercise: String): E1rmTrend? {
        val key = nameKey(exercise)
        val sets = weighted.filter { keyOf[it.exercise] == key }
        val byDay = sets.groupBy { localEpochDay(it.observed_at, zone) }.entries.sortedBy { it.key }
        if (byDay.size < E1RM_TREND_MIN_DAYS) return null
        return E1rmTrend(
            exercise = exercise,
            series = byDay.map { (_, day) -> day.maxOf { it.e1rm_kg } },
            firstDay = LocalDate.fromEpochDays(byDay.first().key),
        )
    }
    val planLifts = (listOfNotNull(model.next_session) + model.week_plan)
        .flatMap { it.items }
        .map { it.exercise }
        .filter { it.isNotBlank() }
        .distinct()
    return planLifts.firstNotNullOfOrNull(::build) ?: build(weighted.last().exercise)
}

/**
 * e1RM trend card (01-today §A.3): "<exercise> · e1RM" with its glossary "?", the
 * latest best as the big figure, a change pill over the same window the
 * sparkline draws ("+12.5 kg since Aug 12", so the two never disagree), the
 * sparkline and a caption naming what one point is. The pill states a
 * difference, never a verdict.
 */
@Composable
private fun E1rmTrendCard(trend: E1rmTrend, weightUnit: WeightUnit) {
    val latest = trend.series.last()
    val unit = weightSuffix(weightUnit)
    val since = remember(trend.firstDay) { localizedDayFormatter("MMMd").format(trend.firstDay) }
    val chartDesc = stringResource(
        Res.string.main_e1rm_trend_cd,
        trend.exercise,
        trend.series.size,
        loadFigure(trend.series.first(), weightUnit),
        loadFigure(latest, weightUnit),
        unit,
    )
    // Rounded once, in the display unit, from the two plotted points.
    val shownDelta = (if (weightUnit == WeightUnit.Lb) kgToLb(latest) - kgToLb(trend.series.first()) else latest - trend.series.first())
        .let { (it * 10.0).roundOrZero() / 10.0 }
    var badgeWhy by rememberSaveable(trend.exercise) { mutableStateOf(false) }
    PlainCard {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Text(
                stringResource(Res.string.main_e1rm_title, trend.exercise),
                color = OnBgBody,
                style = Type.BodyBold,
                modifier = Modifier.weight(1f),
            )
            GlossaryInfo("e1rm")
        }
        // A change that rounds to zero says nothing: no pill at all.
        if (shownDelta != 0.0) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                Text(
                    stringResource(
                        Res.string.main_e1rm_change_since,
                        (if (shownDelta > 0) "+" else "−") + trimDecimal(abs(shownDelta)),
                        unit,
                        since,
                    ),
                    color = OnBgBody,
                    style = Type.Chip.merge(TabularFigures),
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(OnBgMuted.copy(alpha = 0.14f))
                        .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp),
                )
                DisclosureButton(badgeWhy, subject = stringResource(Res.string.main_e1rm_change_subject)) { badgeWhy = !badgeWhy }
            }
            if (badgeWhy) {
                Text(
                    pluralStringResource(Res.plurals.main_e1rm_change_why, trend.series.size, trend.series.size, since),
                    color = OnBgMuted,
                    style = Type.Caption,
                )
            }
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clearAndSetSemantics { contentDescription = chartDesc },
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.Bottom,
        ) {
            Row(verticalAlignment = Alignment.Bottom) {
                Text(
                    weightFigure(latest, weightUnit),
                    color = OnBgBody,
                    style = Type.Display.copy(fontWeight = FontWeight.ExtraBold).merge(TabularFigures),
                )
                Text(" $unit", color = OnBgFaint, style = Type.Caption)
            }
            Sparkline(
                trend.series.map { it.toFloat() },
                modifier = Modifier
                    .width(140.dp)
                    .height(40.dp),
            )
        }
        // The pill above already names the start day; without it the caption does.
        Text(
            if (shownDelta != 0.0) stringResource(Res.string.main_e1rm_caption_short) else stringResource(Res.string.main_e1rm_caption, since),
            color = OnBgMuted,
            style = Type.Caption,
        )
    }
}

/**
 * The Today hero session card: the overline, the title, ONE "?" over all its
 * evidence, then the items: in today's actionable session the first open item
 * is an [OpenPlanItemCard], the others [PlanItemRow]s; settled items are ✓ rows.
 */
@Composable
private fun NextSessionCard(
    ns: SessionPlanView,
    today: Long,
    restToday: Boolean,
    // A card above already states the session's adjustment.
    adjustmentAbove: Boolean,
    // A non-safety adjustment reshaped the session: the title wears the
    // "Adjusted" chip (its prose lives in the status card above).
    adjusted: Boolean,
    coachStatus: List<AdjustmentView>,
    weightUnit: WeightUnit,
    ladders: List<CalisLadderView>,
    // A newcomer's first lifting days: the session spells the prescription out.
    showGuide: Boolean,
    onLogItem: (PrescriptionView) -> Unit,
    onTrackRun: () -> Unit,
    runActionLabel: StringResource,
    onEndExercise: (PrescriptionView) -> Unit,
    onReopen: (PrescriptionView) -> Unit,
    onSetRung: (LadderId, Int) -> Unit,
    onMove: (from: Long, to: Long) -> Unit,
    // The area an active pain report names (a settled row it stopped reads
    // "Stopped · <area>").
    painArea: String?,
) {
    val status = LocalStatusColors.current
    val isToday = ns.epoch_day == today
    val discipline = ns.session_type.discipline
    // Only today's live/adjusted session is actionable; a future NEXT SESSION stays informational.
    val actionable = isToday && ns.actionable
    val isCheck = { item: PrescriptionView -> isReturnCheckItem(item, ns.session_type) }
    val isRun = { item: PrescriptionView -> isRunItem(item, ns.session_type) }
    // The session's adjustment, unless it already renders as its own card above.
    val adjustment = ns.adjustment?.takeIf { ns.status == SessionStatus.Adjusted && !adjustmentAbove }
    val hasEvidence = ns.items.isNotEmpty() || coachStatus.isNotEmpty() || adjustment != null
    var expanded by rememberSaveable(ns.epoch_day, ns.title) { mutableStateOf(false) }
    PlainCard {
        // Self-describing accent overline naming the card for a newcomer (the one
        // section-label grammar). A plan rest day today folds into it ("REST DAY ·
        // NEXT SESSION") so the card still leads with its overline.
        TileOverline(
            when {
                isToday -> stringResource(Res.string.main_todays_session)
                restToday -> stringResource(Res.string.main_rest_day_next_overline)
                else -> stringResource(Res.string.main_next_session_overline)
            },
            color = Accent,
        )
        // The title keeps its words whole: while its longest word fits beside
        // "Move" and the "?", they trail the title; otherwise (a large font on a
        // narrow phone) they drop to their own line under it, at the end.
        val titleLead: @Composable () -> Unit = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Discipline icon tile: Run / Lift get a symbol; Rest gets none.
                val iconRes = when (discipline) {
                    SessionDiscipline.Run -> Res.drawable.ic_content_run
                    SessionDiscipline.Lift -> Res.drawable.ic_content_set_dumbbell
                    SessionDiscipline.Rest -> null
                }
                if (iconRes != null) {
                    IconTile(
                        painterResource(iconRes),
                        Accent, AccentTint, size = 36.dp,
                    )
                    Spacer(Modifier.size(Space.Md.dp))
                }
                Row(
                    Modifier.weight(1f),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    Column(Modifier.weight(1f, fill = false)) {
                        SheetTitle(ns.title)
                        // Today is already marked in the week strip below; only a
                        // future-dated session keeps its date label.
                        if (!isToday) {
                            Text(planDateLabel(ns.epoch_day), color = OnBgMuted, style = Type.Caption)
                        }
                    }
                    if (adjusted) AdjustedChip()
                }
            }
        }
        val titleActions: @Composable () -> Unit = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Rescheduling, used rarely: a quiet trailing "Move" beside the "?",
                // never a row of its own between the title and the first exercise.
                SessionMoveAction(ns, today, onMove)
                if (hasEvidence) {
                    // The disc, not its touch target, ends at the card's content edge.
                    Box(Modifier.offset(x = DisclosureEdgeInset)) {
                        DisclosureButton(expanded, subject = ns.title) { expanded = !expanded }
                    }
                }
            }
        }
        BesideOrStacked(titleLead, titleActions, Modifier.fillMaxWidth()) { lead, actionsWidth, width ->
            lead.minIntrinsicWidth(Constraints.Infinity) + actionsWidth <= width
        }
        // A rest running from the set loop stays visible after the sheet closes.
        if (actionable) {
            LocalRestRunning.current.collectAsState().value?.let { RestPill(it) }
        }
        // Where a moved session came from / why a move was refused.
        SessionMoveNotes(ns)
        if (ns.status == SessionStatus.Blocked) Chip(stringResource(Res.string.main_chip_on_hold), status.danger)
        // A safety-critical adjustment that reshaped the session keeps its SAFETY
        // card on the face (hard rule). Any other one is stated once, in its
        // status card above: the hero wears the "Adjusted" chip and its evidence
        // joins the session panel.
        adjustment?.takeIf { it.safety_critical }?.let { adj ->
            EvidenceCard(
                adj.summary, adj.evidence,
                confidenceInWhy = false, why = adj.why,
                glossary = adj.glossary, spokenSubject = adj.subject,
                proseSummary = adj.prose,
                // One surface step down so the nested card reads as nested.
                container = BgTop,
            )
        }
        if (expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                coachStatus.forEach { PanelAdjustment(it) }
                adjustment?.takeIf { !it.safety_critical }?.let { PanelAdjustment(it, showSummary = false) }
                SessionItemsEvidence(ns.items, isRun, isCheck)
            }
        }
        if (ns.items.isEmpty()) {
            Text(
                if (ns.status == SessionStatus.Blocked) {
                    stringResource(Res.string.main_session_on_hold)
                } else {
                    stringResource(Res.string.main_rest_day)
                },
                color = OnBgMuted,
                style = Type.Body,
            )
        } else {
            // next_session keeps done items: open items render first, settled ones
            // collapse to a ✓ row below.
            // A ladder the athlete was never placed on (calisthenics): the current
            // item invites a Harder tap instead of weeks below their level.
            val unplaced = ladders.filter { it.current_rung == null }.map { it.ladder }.toSet()
            SessionItemList(
                items = ns.items,
                isRun = isRun,
                isCheck = isCheck,
                actionable = actionable,
                reopenable = actionable,
                runActionLabel = stringResource(runActionLabel),
                weightUnit = weightUnit,
                painArea = painArea,
                onLog = onLogItem,
                onRun = onTrackRun,
                onEndExercise = onEndExercise,
                onReopen = onReopen,
                unplaced = { item -> item.calis?.let { it.ladder in unplaced && it.harder?.available == true } == true },
                onSetRung = onSetRung,
                guide = { item -> firstSessionGuide(item, isRun(item), isCheck(item), showGuide) },
            )
        }
    }
}

/** "Rest 1:42" while a rest from the set loop is still running; nothing once it
 *  is over. Ticks every second only while shown. */
@Composable
private fun RestPill(run: RestRun) {
    val now = rememberTicker(1_000L)
    val leftSec = ((run.endAtMillis - now) / 1000L).toInt()
    if (leftSec <= 0) return
    val clock = fmtClock(leftSec)
    val desc = stringResource(Res.string.log_rest_left_cd, clock)
    Text(
        stringResource(Res.string.main_rest_pill, clock),
        color = OnBgBody,
        style = Type.Chip.merge(TabularFigures),
        modifier = Modifier
            .clip(PillShape)
            .background(AccentTint)
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp)
            .clearAndSetSemantics { contentDescription = desc },
    )
}

/** The prescribed reps-in-reserve as a whole number (RIR = 10 − RPE), or null
 *  for an open-ended ("4+") or half-point target the guide can't state plainly. Pure. */
internal fun prescribedRir(rpe: Double): Int? {
    val rir = 10.0 - rpe
    return if (rir >= 0.0 && rir < 4.0 && rir % 1.0 == 0.0) rir.toInt() else null
}

/** For an item with no load to start from (never logged, no prescribed or
 *  suggested load): the reps a first weight should be liftable for, reps_low
 *  plus the prescribed reps in reserve. Null otherwise. Pure. */
internal fun PrescriptionView.firstWeightReps(): Int? {
    if (reps_low <= 0 || last_set != null || logged_sets.isNotEmpty() || load_kg != null || suggested_load_kg != null) return null
    return target_rpe?.let(::prescribedRir)?.let { reps_low + it }
}

/**
 * Plain-language lines for a newcomer's current lift, from the item's own numbers:
 * the prescription spelled out ("5 sets of 5 reps, stopping about 3 reps short of
 * failure") while the core says to guide ([TodaySurfaceView.show_guide]), and, when the item has no
 * history and no load to start from, how to pick a first weight: one you could
 * lift about reps + reps-in-reserve times (the definition of reps in reserve).
 * A bodyweight exercise picks its version (the easier / harder rung) instead of
 * a weight. The reps-left target is said once, in the spelled-out line.
 */
@Composable
private fun firstSessionGuide(item: PrescriptionView, isRun: Boolean, isCheck: Boolean, showGuide: Boolean): List<String> {
    if (isRun || isCheck || item.timed_hold || !showGuide) return emptyList()
    if (item.sets <= 0 || item.reps_low <= 0) return emptyList()
    val rir = item.target_rpe?.let(::prescribedRir) ?: return emptyList()
    val reps = if (item.reps_high > item.reps_low) "${item.reps_low}\u2013${item.reps_high}" else "${item.reps_low}"
    val bodyweight = item.calis != null && (item.added_kg ?: 0.0) == 0.0
    return buildList {
        add(pluralStringResource(Res.plurals.plan_guide_spelled, rir, item.sets, reps, rir))
        item.firstWeightReps()?.let {
            add(stringResource(if (bodyweight) Res.string.plan_guide_first_version else Res.string.plan_guide_first_weight, it))
        }
    }
}

/** One adjustment inside a card's "?" panel: its summary, then its evidence,
 *  under ONE badge. A safety-critical one wears the SAFETY chip alone (and no
 *  confidence): its grade is stated in words in "Why this grade", never as a
 *  second badge on the same statement. */
@Composable
private fun PanelAdjustment(a: AdjustmentView, showSummary: Boolean = true) {
    val status = LocalStatusColors.current
    val gradeInWords = gradeLabel(a.grade)?.let { stringResource(Res.string.main_evidence_grade, it) }.orEmpty()
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        if (a.safety_critical) Chip(stringResource(Res.string.main_chip_safety), status.danger)
        if (showSummary) {
            Text(a.summary, color = OnBgBody, style = Type.Caption.copy(fontWeight = FontWeight.Bold))
        }
        WhyDetail(
            why = if (a.safety_critical) a.why.copy(grade_note = a.why.grade_note.ifBlank { gradeInWords }) else a.why,
            evidence = if (a.safety_critical) a.evidence.copy(grade = null) else a.evidence,
            showConfidence = !a.safety_critical,
        )
    }
}

/** The hold's follow-up safety rows inside its host card's "?" panel: each row's
 *  summary (the face line's own row needs no repeated lead) and evidence. */
@Composable
private fun HoldGroupPanel(rows: List<AdjustmentView>, faceText: String) {
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        rows.forEach { a -> PanelAdjustment(a, showSummary = a.summary != faceText) }
    }
}

/** A resolvable hold that does not block training (an eased illness, a pain stop
 *  on one exercise): an `Adjusted` [StatusCard] with the core's plain "paused
 *  until…" line, its "I've recovered" / "I'm pain-free now" action (confirmed by
 *  [HoldResolveDialog]) and "Remove report" ([onRemoveReport]). */
@Composable
private fun HoldResolutionCard(
    r: HoldResolutionView,
    overline: String,
    onResolve: () -> Unit,
    onRemoveReport: (() -> Unit)?,
) {
    StatusCard(
        tone = StatusTone.Adjusted,
        overline = overline,
        glyph = painterResource(Res.drawable.ic_safety_shield),
        markDescription = stringResource(Res.string.main_status_mark_safety),
        footer = {
            StatusCardFooter(
                primaryLabel = r.action_label,
                onPrimary = onResolve,
                secondaryLabel = onRemoveReport?.let { stringResource(Res.string.main_remove_report) },
                onSecondary = { onRemoveReport?.invoke() },
            )
        },
    ) {
        if (r.paused_text.isNotBlank()) {
            Text(r.paused_text, color = OnBgBody, style = Type.Body)
        }
    }
}

/**
 * The session-done moment: the session's title, one line per item with the
 * plan rows' own marks and words ([SettledMark], [settledLabel]; a run in the
 * user's unit), the coaching takeaway when the core has one, then "Review
 * session ›" and, when finishing ended exercises, "Resume session" ([onResume]).
 */
@Composable
private fun SessionDoneCard(
    done: SessionDoneView,
    feedback: FeedbackView?,
    // The area an active pain report names: a row it stopped reads "Stopped · <area>".
    painArea: String?,
    distanceUnit: DistanceUnit,
    onResume: (() -> Unit)?,
    onReview: () -> Unit,
) {
    Box(Modifier.testTag("session-done")) {
        PlainCard {
            TileOverline(stringResource(Res.string.main_session_done_overline), color = Accent)
            SheetTitle(done.title.ifBlank { stringResource(Res.string.main_session_done_title) })
            done.items.forEach { row ->
                val skipped = row.status.isSkipped
                val summary = sessionDoneRunLine(row, distanceUnit) ?: row.summary
                val line = settledLabel(row.status, summary, painArea)
                val desc = if (row.met && !skipped) {
                    stringResource(Res.string.main_session_done_met_cd, row.exercise, summary)
                } else {
                    stringResource(Res.string.ui_label_value, row.exercise, line)
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clearAndSetSemantics { contentDescription = desc },
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    SettledMark(skipped)
                    Column(modifier = Modifier.weight(1f)) {
                        Text(row.exercise, color = OnBgBody, style = Type.Body)
                        if (line.isNotBlank()) {
                            Text(line, color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
                        }
                    }
                }
            }
            feedback?.let { fb ->
                val message = fb.message
                if (fb.grade == null) {
                    // An ungraded note asserts no claim: plain text, no evidence chrome.
                    Text(message, color = OnBgBody, style = Type.Body)
                } else {
                    // The takeaway is a nested tile one surface step down, like every
                    // other nested surface in a card (no outline).
                    EvidenceCard(
                        message, fb.evidence,
                        fb.category_label.ifBlank { null },
                        why = fb.why,
                        glossary = fb.glossary, spokenSubject = fb.subject,
                        proseSummary = fb.prose,
                        container = BgTop,
                    )
                }
            }
            Column(Modifier.offset(x = -TextActionInset)) {
                TextAction(stringResource(Res.string.main_session_done_review), onClick = onReview, chevron = true)
                onResume?.let { TextAction(stringResource(Res.string.main_session_done_resume), onClick = it) }
            }
        }
    }
}

/** A Session Done run row in the user's unit, from the core's numbers: "5.9 mi ·
 *  60 min", "5 of 5.9 mi · 60 min" when the distance (at display precision) is
 *  under the planned one, "34 of 45 min" for a run planned by time. Null for a
 *  lift row (no run figures); the caller then shows the core's summary. */
@Composable
private fun sessionDoneRunLine(row: SessionDoneItemView, unit: DistanceUnit): String? {
    if (row.run_km == null && row.run_min == null) return null
    val tenth = { km: Double -> (metersToDisplay(km * 1000.0, unit) * 10.0).roundOrZero() / 10.0 }
    val dist = row.run_km?.let(tenth)
    val plannedDist = row.run_planned_km?.let(tenth)
    val min = row.run_min?.let { it.roundOrZero().toInt() }
    val plannedMin = row.run_planned_min?.let { it.roundOrZero().toInt() }
    val distText = dist?.let { d ->
        val shown = "${trimDecimal(d)}\u00A0${unit.distanceLabel}"
        if (plannedDist != null && d < plannedDist) {
            stringResource(Res.string.main_done_run_of, trimDecimal(d), "${trimDecimal(plannedDist)}\u00A0${unit.distanceLabel}")
        } else {
            shown
        }
    }
    val minText = min?.let { m ->
        if (plannedMin != null && plannedDist == null && m < plannedMin) {
            stringResource(Res.string.main_done_run_of, "$m", stringResource(Res.string.log_value_min, plannedMin))
        } else {
            stringResource(Res.string.log_value_min, m)
        }
    }
    return listOfNotNull(distText, minText).joinToString(" \u00b7 ").ifBlank { null }
}

/** A plan-state card with one way forward: a title, a body and a full-width action. */
@Composable
private fun PromptCard(title: String, body: String, cta: String, onClick: () -> Unit) {
    PlainCard {
        SheetTitle(title)
        Text(body, color = OnBgMuted, style = Type.Body)
        PrimaryButton(cta, onClick, Modifier.fillMaxWidth())
    }
}

/** Readiness-strip pill name: the short form where one exists ("HRV", not
 *  "HRV (ln rMSSD)"), else the signal's full label. */
@Composable
private fun readinessPillLabel(signal: ReadinessSignal): String = when (signal) {
    ReadinessSignal.HrvLnRmssd -> stringResource(Res.string.main_pill_hrv)
    ReadinessSignal.WellnessZ -> stringResource(Res.string.main_pill_wellness)
    ReadinessSignal.EstimatedOneRm -> stringResource(Res.string.main_pill_strength)
    ReadinessSignal.Rpe -> stringResource(Res.string.main_pill_effort)
    ReadinessSignal.RestingHr -> stringResource(Res.string.main_pill_resting_hr)
    else -> stringResource(signal.labelRes)
}

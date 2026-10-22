package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import java.util.Locale

/**
 * Today (01-today + 02-coach, merged: the Coach tab is folded into Today so one
 * screen owns the day). Order: readiness strip →
 * today's call → adjustments & feedback (directly below the call, they can
 * carry safety-relevant guidance and must never sit below the fold) → plan
 * (next-session hero, this week) → e1RM trend. The calculators live on Profile.
 * States: B. empty "Get started" (then the plan prompt); C. safety override
 * (the global banner dominates; latest guidance + the read-only week).
 *
 * "Today's call" renders the CORE-owned `today_headline` (safety hold >
 * adjustment > feedback > all-clear), prioritization is coaching logic and
 * lives in the core; the shell never computes it.
 */
@Composable
internal fun TodayDestination(
    model: ViewModel,
    listState: androidx.compose.foundation.lazy.LazyListState,
    onEvent: (Event) -> Unit,
    onStartSetup: () -> Unit,
    onLogPlanItem: (PrescriptionView) -> Unit = {},
    onLogPastItem: (PrescriptionView, Long) -> Unit = { _, _ -> },
    onLogPastRun: (Long) -> Unit = {},
    onTrackRun: () -> Unit = {},
    // Opens the Log sheet on one editor (today's check-in, the session review).
    onOpenSheet: (LogMode) -> Unit = {},
    // The pain report as last entered (its area, and the Undo after removing it).
    lastPain: PainDetail? = null,
    // Opens Profile at a row (the way to fix a plan that can't be built).
    onOpenProfile: (ProfileRowId) -> Unit = {},
    // Re-runs guided setup seeded from the profile (a profile with nothing to plan).
    onRerunSetup: () -> Unit = {},
) {
    val headline = model.today_headline
    // Last logged activity: the view lists are chronological oldest-first -
    // lastOrNull() is the most recent of each. Used for the hasActivity signal;
    // Today shows no recent-activity list (History owns that).
    val lastLift = model.lifts.lastOrNull()
    val lastRun = model.runs.lastOrNull()
    // Removing the generated plan is confirm-gated.
    var confirmRemovePlan by rememberSaveable { mutableStateOf(false) }
    // The day open under the week strip, hoisted so a move opens its target day.
    var weekSelectedDay by rememberSaveable { mutableStateOf(-1L) }
    val res = LocalContext.current.resources
    val moveSession = { from: Long, to: Long ->
        onEvent(Event.MoveSession(from, to))
        weekSelectedDay = to
        UiMessages.post(
            UiMessage(
                res.getString(R.string.main_moved_to, planDateLabel(to)),
                actionLabel = res.getString(R.string.log_undo),
                onAction = { EventDispatcher.send(Event.MoveSession(to, from)) },
            ),
        )
    }

    val hasCoachSignal = model.feedback != null ||
        model.adjustments.isNotEmpty() ||
        model.review_adjustments.isNotEmpty() ||
        model.input_count > 0 ||
        // A morning check-in is coaching signal too; even pre-baseline
        // (the "collecting baseline" honesty state), and once derived signals land.
        model.checkin_today != null ||
        model.readiness_summary.isNotEmpty() ||
        model.baseline_status.isNotEmpty()
    val hasActivity = lastLift != null || lastRun != null
    val hasAnything = hasCoachSignal || hasActivity

    val trend = remember(model.lifts, model.next_session, model.week_plan) { e1rmTrend(model) }

    // Modality gating: the e1RM trend is a lifting signal. A not-yet-set-up
    // user (profile == null) still sees it.
    val showLifting = model.profile == null || model.showLifting()
    // Weight display unit for kg figures on Today (the e1RM trend card).
    val weightUnit = model.weightUnit()

    // Memoized so the Today list doesn't re-run the priority sort every
    // recomposition (mirrors the SafetyBanner lookup).
    val safetyCard = remember(model.adjustments) { dominantSafetyAdjustment(model) }

    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(
            start = Space.Screen.dp, end = Space.Screen.dp,
            top = Space.Screen.dp, bottom = TodayFabClearance,
        ),
        verticalArrangement = Arrangement.spacedBy(Space.Screen.dp),
    ) {
        if (model.train_blocked) {
            val youthPause = holdAction(model.hold_reason, model.safety_tier) == HoldAction.CheckAge
            // Under 18 the pause is no pinned DO-NOT-TRAIN banner but this calm
            // card, the list's first item, so it scrolls with the page.
            if (youthPause) {
                item { YouthHoldCard(model, onOpenProfile) }
            }
            // C. Safety override: the banner (global, pinned above) dominates and
            // carries the call ("DO NOT TRAIN"), the reason, the way back and the
            // hold's evidence behind its "?". A hold the banner (or the youth
            // card) states in full ([bannerStatesHold]) gets nothing on Today that
            // repeats it. Any other hold (a medical referral, a review deferral)
            // keeps the core's own hold summary as one SAFETY card, since its
            // instruction is more specific than the banner. Prefer the core's OWN hold resolution:
            // build_headline's "safety_hold" rung already picks the dominant source
            // (onboarding gate > readiness stop > review deferral) WITH its
            // evidence, including a gates-only medical-referral hold, which
            // produces NO adjustment row at all; the highest-priority safety
            // adjustment stays as the fallback.
            val hold = headline?.takeIf { it.kind == "safety_hold" && it.summary.isNotBlank() }
            val pregnancyWarning = model.profile?.health?.pregnancy_warning_sign == true
            if (!bannerStatesHold(model.hold_reason, model.safety_tier, pregnancyWarning) && (hold != null || safetyCard != null)) {
                item {
                    // Safety hold card: no confidence; safety is a rule, not a
                    // probability. Grade + citation stay behind the "?".
                    if (hold != null) {
                        EvidenceCard(
                            hold.summary, hold.grade, hold.citation,
                            hold.confidence, hold.safety_critical, hold.contested,
                            confidenceInWhy = false, why = hold.why,
                            status = StatusTone.Adjusted,
                            statusOverline = stringResource(R.string.main_status_on_hold),
                        )
                    } else if (safetyCard != null) {
                        EvidenceCard(
                            safetyCard.summary, safetyCard.grade, safetyCard.citation,
                            safetyCard.confidence, safetyCard.safety_critical, safetyCard.contested,
                            confidenceInWhy = false, why = safetyCard.why,
                            status = StatusTone.Adjusted,
                            statusOverline = stringResource(R.string.main_status_on_hold),
                        )
                    }
                }
            }
            // What still works while training is paused, so the blocked screen
            // isn't a dead end. Logging stays on the Log button (no tiles here). The
            // youth card already says what still works.
            if (!youthPause) {
                item {
                    Text(
                        stringResource(R.string.main_blocked_still_works),
                        color = OnBgMuted,
                        style = Type.Caption,
                    )
                }
            }
            // Under 18 (WHO 5–17, youth-pa-001): the weekly activity card, where the
            // hero would be. Information and tracking only; the hold is unchanged.
            model.youth_activity?.let { activity ->
                item { SectionOverline(stringResource(R.string.activity_overline)) }
                item {
                    YouthActivityCard(activity, onLog = { onOpenSheet(LogMode.Activity) }, onEvent = onEvent)
                }
            }
            // The week stays in view, read-only (held days read "on hold"), so the
            // paused screen still shows where the plan stands. Nothing here logs
            // or moves a session while the hold is active.
            if (model.week_plan.isNotEmpty() || model.program != null) {
                item { SectionOverline(stringResource(R.string.main_overline_this_week)) }
                item {
                    ThisWeekCard(
                        consistency = model.consistency
                            ?.takeIf { model.week_plan.isNotEmpty() && it.this_week_planned > 0 },
                        week = model.week_plan,
                        today = model.today(),
                        weightUnit = weightUnit,
                        program = model.program,
                        footnote = null,
                        onRemovePlan = { confirmRemovePlan = true },
                        onLogItem = { _, _ -> },
                        onLogRun = {},
                        onEndExercise = { _, _ -> },
                        onReopen = { _, _ -> },
                        onMove = { _, _ -> },
                        heroDay = -1L,
                        selectedDay = weekSelectedDay,
                        onSelectDay = { weekSelectedDay = it },
                        readOnly = true,
                    )
                }
            }
            // Today shows no recent-activity copy here (History owns that).
            return@LazyColumn
        }

        // 1. Status cards, then the readiness strip, then the hero (all inside
        // TodayHeroCard). The strip shows passive per-signal qualitative pills
        // only; every signal is logged through the Log button / check-in, never
        // from the strip. Deliberately NO ring and NO 0–100 number (INVARIANT 1).
        // Render NOTHING when there is no readiness data at all: gate on actual
        // pills (measured signals or building baselines), not `hasAnything`, so an
        // empty strip never mounts (no caption either).
        val hasReadinessPills = model.stripReadinessRows().isNotEmpty() || model.baseline_status.isNotEmpty()

        // 2. Today hero: one card merging Today's call + feedback +
        // adjustments + the next-session/plan-state card. The call/adjustments
        // portion only renders when there's signal; the plan-state card always
        // renders (a set-up user sees their next session; a profile-less user
        // sees the setup prompt). Evidence chrome is unchanged.
        item {
            TodayHeroCard(
                model = model,
                hasSignal = hasAnything,
                onEvent = onEvent,
                onStartSetup = onStartSetup,
                onLogItem = onLogPlanItem,
                onTrackRun = onTrackRun,
                onOpenSheet = onOpenSheet,
                onMove = moveSession,
                lastPain = lastPain,
                onOpenProfile = onOpenProfile,
                onRerunSetup = onRerunSetup,
                readinessStrip = if (hasReadinessPills) {
                    { ReadinessStrip(model, onEvent, onCheckin = { onOpenSheet(LogMode.Checkin) }) }
                } else {
                    null
                },
            )
        }

        // 3. This week: the planned-vs-done count, the week strip and the plan
        // footer as ONE card under one overline, only when a plan exists (the
        // hero already renders the next-session card itself). The plan summary is
        // a compact plain-language footer with a confirm-gated "Remove plan".
        val nextSession = model.next_session
        val showWeek = nextSession != null && (model.week_plan.isNotEmpty() || model.program != null)
        if (showWeek) {
            item { SectionOverline(stringResource(R.string.main_overline_this_week)) }
            item {
                ThisWeekCard(
                    // Planned vs done: one neutral count line, never
                    // a streak, a loss, or an unlabeled chart. A week with nothing
                    // planned has nothing to count.
                    consistency = model.consistency
                        ?.takeIf { model.week_plan.isNotEmpty() && it.this_week_planned > 0 },
                    week = model.week_plan,
                    today = model.today(),
                    weightUnit = weightUnit,
                    program = model.program,
                    // Minimal empty state: nothing logged yet. Anchored as the
                    // week card's last line, where the empty week shows.
                    footnote = if (!hasAnything) stringResource(R.string.main_today_empty) else null,
                    onRemovePlan = { confirmRemovePlan = true },
                    onLogItem = onLogPastItem,
                    onLogRun = onLogPastRun,
                    onEndExercise = { item, day -> endExerciseWithUndo(res, item, day, onEvent) },
                    onReopen = { item, day -> if (item.exercise.isNotBlank()) onEvent(Event.SetExerciseEnded(item.exercise, day, ended = false)) },
                    onMove = moveSession,
                    heroDay = nextSession.epoch_day,
                    selectedDay = weekSelectedDay,
                    onSelectDay = { weekSelectedDay = it },
                )
            }
        }

        // Minimal empty state with no plan to host it: no tiles; everything logs
        // via the Log button. Shown only when there's genuinely nothing yet.
        if (!hasAnything && !showWeek) {
            item {
                Text(
                    stringResource(R.string.main_today_empty),
                    color = OnBgFaint,
                    style = Type.Caption,
                )
            }
        }

        // Progress: the e1RM trend of the plan's main lift (else the most recently
        // logged weighted lift), gated on showLifting. Rendered only with at least
        // three sessions of real e1RMs: fewer is not a trend (a bodyweight/hold set
        // has no e1RM). The last Today section.
        if (trend != null && showLifting) {
            item { SectionOverline(stringResource(R.string.main_overline_progress)) }
            item { E1rmTrendCard(trend, weightUnit) }
        }
    }

    // Remove-plan confirm: honest copy, reuses the ClearConfirmDialog pattern.
    // Confirm dispatches Event.ClearPlan.
    ClearConfirmDialog(
        visible = confirmRemovePlan,
        title = stringResource(R.string.main_remove_plan_title),
        message = stringResource(R.string.main_remove_plan_message),
        confirmLabel = stringResource(R.string.main_remove_plan),
        onDismiss = { confirmRemovePlan = false },
        onClear = { onEvent(Event.ClearPlan) },
    )
}

/**
 * Today's card stack (ui-sweep §5): the status cards (safety ones first, then
 * the adjusted ones, each an `Adjusted` [StatusCard] via [EvidenceCard]
 * `status`), the remaining coaching (a graded win, the ungraded all-clear as a
 * PlainCard), the [readinessStrip], then the session-done moment and the
 * plan-state card: the next session, else the plan prompt (profile set) or the
 * setup prompt (no profile). Evidence chrome is unchanged (summary + SAFETY on
 * the face, everything else behind the "?").
 *
 * Coaching adjustments read as ONE card per exercise, the newest cue winning
 * ([coachingPerExercise]); once today's session is done the adjustments that
 * shaped it are spent and only safety calls and next-time notes stay. A new best
 * and its lift's next-time advice share one card ([winAdviceFor]); while the
 * session is under way, two or more non-safety coaching notes fold into one "N
 * coaching notes" row ([CoachingNotesRow]) so the session stays near the top. An
 * active pain report that does not block training still offers "Remove report"
 * (a mistaken report must always be undoable), next to the core's "I'm pain-free
 * now" when it has one. An adjustment is stated once: the session card wears the
 * "Adjusted" chip instead of repeating the prose.
 *
 * [hasSignal] gates the call/feedback/adjustments block so a brand-new user
 * sees only the setup prompt; the plan-state
 * card always renders. The train_blocked safety branch is handled by the caller and
 * never reaches here.
 */
@Composable
private fun TodayHeroCard(
    model: ViewModel,
    hasSignal: Boolean,
    onEvent: (Event) -> Unit,
    onStartSetup: () -> Unit,
    onLogItem: (PrescriptionView) -> Unit = {},
    onTrackRun: () -> Unit = {},
    onOpenSheet: (LogMode) -> Unit = {},
    // Moves a session (posts "Moved to … · Undo").
    onMove: (from: Long, to: Long) -> Unit = { _, _ -> },
    // The pain report as last entered here (its body area), for "Report removed · Undo".
    lastPain: PainDetail? = null,
    onOpenProfile: (ProfileRowId) -> Unit = {},
    onRerunSetup: () -> Unit = {},
    // The readiness strip, placed between the status cards and the hero.
    readinessStrip: (@Composable () -> Unit)? = null,
) {
    val headline = model.today_headline
    val fb = model.feedback
    val showFeedback = fb != null && headline?.kind != "feedback"
    val today = model.today()
    // Today's session is complete: the done card owns the non-safety coaching
    // takeaway (a PR, a derived session note) so it renders ONCE, inside the
    // moment it belongs to. A safety-critical message keeps its own card.
    val sessionDone = model.session_done?.takeIf { it.items.isNotEmpty() }
    val doneOwnsFeedback = sessionDone != null && showFeedback && !fb.safety_critical
    // Today's adjustments (first-set checks, in-session cues) reshape today's
    // session; once it is done they are spent, and an in-session cue ("…for the
    // remaining sets") is spent as soon as its own exercise is settled. Safety
    // calls always stay.
    val settledToday = remember(model.week_plan, today) {
        model.week_plan.firstOrNull { it.epoch_day == today }?.items.orEmpty()
            .filter { it.exercise.isNotBlank() && (it.done || it.status in SettledItemStatuses) }
            .map { it.exercise.trim().lowercase(Locale.ROOT) }
            .toSet()
    }
    val spent = { a: AdjustmentView ->
        !a.safety_critical && (
            sessionDone != null ||
                ((a.suggested_load_kg != null || a.suggests_end_exercise) &&
                    a.exercise?.trim()?.lowercase(Locale.ROOT) in settledToday)
            )
    }
    // Dedup key is (summary, citation): several rules emit the same summary
    // string, so matching on text alone would vanish the second rule's evidence.
    val isHeadline = { a: AdjustmentView ->
        headline?.kind == "adjustment" &&
            a.summary == headline.summary && a.citation == headline.citation
    }
    // A post-illness return check states its advice on the session's own item,
    // so the adjustment behind it gets no card above the session.
    val sessionOwned = returnCheckAdjustmentKey(model.next_session)
    val ownedBySession = { a: AdjustmentView -> (a.summary to a.citation) == sessionOwned }
    val headlineAdjustment = model.adjustments.firstOrNull(isHeadline)
    val showHeadline = headline != null && headline.kind != "prescription" &&
        headlineAdjustment?.let(spent) != true &&
        !(headline.kind == "adjustment" && (headline.summary to headline.citation) == sessionOwned)
    // Priority sort + headline dedupe is pure work over the wire lists, memoize
    // it so it doesn't re-run on every unrelated recomposition (only when the
    // adjustment lists or the headline actually change).
    val listedAdjustments = remember(model.adjustments, headline, sessionDone, settledToday, sessionOwned) {
        model.adjustments.byAdjustmentPriority().filterNot(isHeadline).filterNot(spent).filterNot(ownedBySession)
    }
    val listedReviewAdjustments = remember(model.review_adjustments, headline) {
        model.review_adjustments.byAdjustmentPriority().filterNot(isHeadline)
    }
    val nextSession = model.next_session
    // A training-plan rest day today: stated plainly on the hero instead of
    // only showing the next (future) session.
    val restToday = model.week_plan.any { it.epoch_day == today && it.status == "rest" }
    val weightUnit = model.weightUnit()
    // A resolvable hold that does NOT block training (an eased illness, an
    // item-scoped pain stop) has no banner: its "I've recovered" action lives here.
    val softResolutions = if (model.train_blocked) emptyList() else model.hold_resolutions.filter { it.action_label.isNotBlank() }
    var resolveSignal by rememberSaveable { mutableStateOf<String?>(null) }
    // A pain report that does not block training (it eases or stops one
    // exercise) is still undoable from Today, with the banner's own wording.
    val painReported = !model.train_blocked && model.readiness_summary.any { it.signal == "Pain" }
    var removePain by rememberSaveable { mutableStateOf(false) }
    // The area an active pain report names: an exercise it stopped after some
    // sets (status stopped_pain) reads "Stopped · <area>", never a done ✓.
    val painArea = painAreaLabel(lastPain?.location, model.readiness_summary.firstOrNull { it.signal == "Pain" }?.detail)

    // One safety card per hold: the first safety-critical card (the headline when
    // it is one, else the top safety adjustment) hosts the group. It keeps the
    // SAFETY chip and its single "?" and reads call + one supporting line + the
    // hold's "I've recovered" action; the hold's other safety adjustments (e.g.
    // "Head-cold rule: easier and shorter today" under "Head cold: about 50% fewer
    // sets…") sit inside that same "?" panel, never stacked on the face.
    val headlineHosts = hasSignal && showHeadline && headline.grade.isNotEmpty() && headline.safety_critical
    val safetyListed = if (hasSignal) (listedAdjustments + listedReviewAdjustments).filter { it.safety_critical } else emptyList()
    val hostAdjustment = if (headlineHosts) null else safetyListed.firstOrNull()
    val hasSafetyHost = headlineHosts || hostAdjustment != null
    val safetyRows: List<AdjustmentView> = if (!hasSafetyHost) {
        emptyList()
    } else {
        safetyListed.filterNot { it === hostAdjustment }
    }
    // The coaching (non-safety) adjustment cards: one per exercise.
    val coaching = remember(listedAdjustments, listedReviewAdjustments, headlineAdjustment, showHeadline) {
        coachingPerExercise(
            headlineExercise = headlineAdjustment?.exercise?.takeIf { showHeadline },
            candidates = (listedAdjustments + listedReviewAdjustments).filterNot { it.safety_critical },
        )
    }
    // The session's own (non-safety) adjustment. The hero never restates its prose
    // (ui-sweep §5 dedupe): it wears the "Adjusted" chip, and the prose lives in a
    // status card above, its own card unless another adjustment card already speaks.
    val sessionAdjustment = nextSession?.adjustment?.takeIf {
        nextSession.status == "adjusted" && !it.safety_critical && (it.summary to it.citation) != sessionOwned
    }
    val adjustedAbove = hasSignal && (
        headlineHosts || hostAdjustment != null || coaching.isNotEmpty() ||
            (showHeadline && headline.kind == "adjustment" && headline.grade.isNotEmpty())
        )
    // Every (summary, citation) already rendered as its own card above the
    // session, so the session card never repeats one of them (one render per
    // adjustment).
    val baseKeys: Set<Pair<String, String>> = remember(hasSignal, headline, showHeadline, fb, safetyListed, coaching, sessionOwned) {
        if (!hasSignal) {
            setOfNotNull(sessionOwned)
        } else {
            buildSet {
                if (showHeadline) add(headline.summary to headline.citation)
                if (showFeedback) add(fb.message to fb.citation)
                safetyListed.forEach { add(it.summary to it.citation) }
                coaching.forEach { add(it.summary to it.citation) }
                sessionOwned?.let(::add)
            }
        }
    }
    val sessionAdjustmentCard = sessionAdjustment?.takeIf { !adjustedAbove && (it.summary to it.citation) !in baseKeys }
    val renderedKeys = sessionAdjustmentCard?.let { baseKeys + (it.summary to it.citation) } ?: baseKeys
    // Engine-meta disclosures (how provisional the call is, which signal source
    // drives autoregulation) are not actionable: they live in the session card's
    // "?" panel, never on the face. While a hold or safety adjustment is active
    // they are dropped entirely: "loads stay at plan" contradicts the hold that
    // just changed the loads. With no session card to host them, only a
    // safety-critical one keeps a card of its own.
    val holdActive = hasSafetyHost || model.hold_resolutions.isNotEmpty()
    val meta = if (hasSignal && !holdActive) listOfNotNull(model.provisional, model.autoreg_source) else emptyList()
    val metaOnFace = if (nextSession == null) meta.filter { it.safety_critical } else emptyList()
    val sessionMeta = if (nextSession != null) meta else emptyList()
    val foldedResolution = if (hasSafetyHost) softResolutions.firstOrNull() else null
    val groupFaceText = foldedResolution?.paused_text.orEmpty().ifBlank { safetyRows.firstOrNull()?.summary.orEmpty() }
    val painOnHost = painReported && hasSafetyHost
    val groupFooter: (@Composable () -> Unit)? = if (groupFaceText.isNotBlank()) {
        { HoldGroupLine(safetyRows, foldedResolution?.paused_text.orEmpty()) }
    } else {
        null
    }
    val groupPanel: (@Composable () -> Unit)? = if (safetyRows.isNotEmpty()) {
        { HoldGroupPanel(safetyRows, groupFaceText) }
    } else {
        null
    }
    val groupActionLabel = foldedResolution?.action_label
    val groupOnAction: (() -> Unit)? = foldedResolution?.let { r -> { resolveSignal = r.signal } }
    // Status-card overlines (ui-sweep §5): what the card is about, in words, so
    // colour is never the only signal.
    val illnessActive = model.hold_resolutions.any { it.signal == "Illness" } ||
        model.readiness_summary.any { it.signal == "Illness" }
    val painOverline = painArea?.let { stringResource(R.string.main_status_pain_area, it) }
        ?: stringResource(R.string.main_status_pain)
    val unwellOverline = stringResource(R.string.main_status_unwell)
    val adjustedOverline = stringResource(R.string.main_status_adjusted_today)
    val coachNoteOverline = stringResource(R.string.main_status_coach_note)
    val safetyOverline = when {
        foldedResolution?.signal == "Pain" || (foldedResolution == null && painReported) -> painOverline
        foldedResolution?.signal == "Illness" || illnessActive -> unwellOverline
        else -> adjustedOverline
    }
    val coachOverline = { a: AdjustmentView? ->
        when {
            !a?.exercise.isNullOrBlank() -> coachNoteOverline
            illnessActive -> unwellOverline
            else -> adjustedOverline
        }
    }
    val removeReportLabel = stringResource(R.string.main_remove_report)
    // Session-wide coaching (no exercise named, e.g. a check-in's and an HRV
    // dip's "stop a rep earlier") reads as ONE card: an adjustment headline speaking
    // session-wide hosts them, else the first of them does; the others are lines
    // under its summary with their evidence behind its one "?".
    val headlineHostsGeneral = hasSignal && showHeadline && headline.kind == "adjustment" &&
        headline.grade.isNotEmpty() && !headlineHosts && headlineAdjustment?.exercise.isNullOrBlank()
    val coachGeneral = coaching.filter { it.exercise.isNullOrBlank() }
    val generalHost = if (headlineHostsGeneral) null else coachGeneral.firstOrNull()
    val generalRows = coachGeneral.filterNot { it === generalHost }
    val generalFooter: (@Composable () -> Unit)? = if (generalRows.isNotEmpty()) {
        { generalRows.forEach { Text(it.summary, color = OnBgBody, style = Type.Body) } }
    } else {
        null
    }
    val generalPanel: (@Composable () -> Unit)? = if (generalRows.isNotEmpty()) {
        { Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) { generalRows.forEach { PanelAdjustment(it) } } }
    } else {
        null
    }
    val distanceUnit = rememberDistanceUnit()
    val res = LocalContext.current.resources
    // A new best and the same lift's next-time advice read as ONE card: the win,
    // the advice under it, the advice's evidence inside the win's "?".
    val winText = when {
        fb == null || fb.safety_critical || fb.grade.isBlank() || fb.category != WinCategory -> null
        showFeedback && !doneOwnsFeedback -> feedbackMessage(fb, distanceUnit)
        showHeadline && headline.kind == "feedback" -> headline.summary
        else -> null
    }
    val winAdvice = winText?.let { winAdviceFor(it, coaching) }
    val winFooter: (@Composable () -> Unit)? = winAdvice?.let { a -> { Text(a.summary, color = OnBgBody, style = Type.Body) } }
    val winPanel: (@Composable () -> Unit)? = winAdvice?.let { a -> { PanelAdjustment(a) } }
    val coachingCards = coaching.filterNot { a -> generalRows.any { it === a } || a === winAdvice }
    // While today's session is under way, its coaching notes (the call when it is
    // a cue or a win, the feedback, the per-exercise cues) fold into ONE row, so
    // the session itself stays near the top. Safety cards never fold.
    val todayItems = model.week_plan.firstOrNull { it.epoch_day == today }?.items.orEmpty()
    val inProgress = hasSignal && sessionDone == null &&
        todayItems.any { it.done || it.ended_early || it.logged_sets.isNotEmpty() }
    val foldHeadline = inProgress && showHeadline && headline.grade.isNotEmpty() &&
        !headline.safety_critical && (headline.kind == "adjustment" || headline.kind == "feedback")
    val foldFeedback = inProgress && showFeedback && !doneOwnsFeedback && !fb.safety_critical
    val foldCount = (if (foldHeadline) 1 else 0) + (if (foldFeedback) 1 else 0) +
        (if (inProgress) coachingCards.size else 0)
    val folded = foldCount >= 2
    var notesOpen by rememberSaveable(today) { mutableStateOf(false) }

    // Today's call, the core-owned headline. A downgrade-class ADJUSTMENT (or a
    // safety call) is an `Adjusted` [StatusCard] (headline IS the call, no
    // confidence meter on the face); a graded feedback headline keeps the full
    // EvidenceCard; the ungraded all-clear is a plain card with no chrome.
    //
    // A "prescription" headline ("Next: Recovery run - 30 min · 76% HRmax")
    // restates the session NextSessionCard renders below, so it gets no
    // card of its own: NextSessionCard IS the hero face (its title serves
    // as the headline).
    val headlineCard: @Composable () -> Unit = {
        if (showHeadline) {
            if (headline.grade.isNotEmpty()) {
                val isAdjustment = headline.kind == "adjustment"
                val hostsWin = winAdvice != null && headline.kind == "feedback"
                EvidenceCard(
                    headline.summary, headline.grade, headline.citation,
                    headline.confidence, headline.safety_critical, headline.contested,
                    // All chrome (grade + confidence) lives behind the "?";
                    // confidenceInWhy keeps the figure in-panel for the
                    // adjustment / graded feedback.
                    confidenceInWhy = true,
                    status = if (isAdjustment || headlineHosts) StatusTone.Adjusted else null,
                    statusOverline = if (headlineHosts) safetyOverline else coachOverline(headlineAdjustment),
                    why = headline.why,
                    footer = when {
                        headlineHosts -> groupFooter
                        headlineHostsGeneral -> generalFooter
                        hostsWin -> winFooter
                        else -> null
                    },
                    panelExtra = when {
                        headlineHosts -> groupPanel
                        headlineHostsGeneral -> generalPanel
                        hostsWin -> winPanel
                        else -> null
                    },
                    actionLabel = if (headlineHosts) groupActionLabel else null,
                    onAction = if (headlineHosts) groupOnAction else null,
                    secondaryActionLabel = if (headlineHosts && painOnHost) removeReportLabel else null,
                    onSecondaryAction = { removePain = true },
                    actionPrimary = false,
                    proseSummary = isAdjustment,
                )
            } else {
                PlainCard {
                    Text(
                        headline.summary,
                        color = OnBgBody,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                    )
                }
            }
        }
    }
    val feedbackCard: @Composable () -> Unit = {
        if (showFeedback && !doneOwnsFeedback) {
            val message = feedbackMessage(fb, distanceUnit)
            if (fb.grade.isBlank()) {
                // An ungraded note asserts no claim: no evidence chrome, like the all-clear.
                PlainCard { Text(message, color = OnBgBody, style = Type.Body) }
            } else {
                EvidenceCard(
                    message, fb.grade, fb.citation, fb.confidence, fb.safety_critical, fb.contested,
                    fb.category_label.ifBlank { null },
                    why = fb.why,
                    footer = winFooter,
                    panelExtra = winPanel,
                )
            }
        }
    }
    val coachingCard: @Composable (AdjustmentView) -> Unit = { a ->
        val hostsGeneral = a === generalHost
        EvidenceCard(
            a.summary, a.grade, a.citation, a.confidence, a.safety_critical, a.contested,
            confidenceInWhy = true, why = a.why,
            status = StatusTone.Adjusted,
            statusOverline = coachOverline(a),
            footer = if (hostsGeneral) generalFooter else null,
            panelExtra = if (hostsGeneral) generalPanel else null,
            proseSummary = true,
        )
    }
    val headlineIsStatus = showHeadline && headline.grade.isNotEmpty() && (headline.kind == "adjustment" || headlineHosts)

    // Order (ui-sweep §5): safety status cards, then the other status cards, then
    // the remaining coaching (a win, the all-clear), the readiness strip, and the
    // hero. Cards sit Space.Md apart.
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        if (hasSignal && headlineHosts) headlineCard()
        // The safety host carries the hold's other safety rows behind its "?".
        if (hasSignal) {
            hostAdjustment?.let { a ->
                EvidenceCard(
                    a.summary, a.grade, a.citation, a.confidence, a.safety_critical, a.contested,
                    confidenceInWhy = true, why = a.why,
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
        }
        softResolutions.filterNot { it === foldedResolution }.forEach { r ->
            HoldResolutionCard(
                r,
                overline = if (r.signal == "Pain") painOverline else unwellOverline,
                onResolve = { resolveSignal = r.signal },
                onRemoveReport = if (painReported && !hasSafetyHost && r.signal == "Pain") ({ removePain = true }) else null,
            )
        }
        // A pain report with no card of its own above still gets its undo.
        if (painReported && !hasSafetyHost && softResolutions.none { it.signal == "Pain" }) {
            StatusCard(
                tone = StatusTone.Adjusted,
                overline = painOverline,
                footer = { StatusCardFooter(primaryLabel = null, onPrimary = {}, secondaryLabel = removeReportLabel, onSecondary = { removePain = true }) },
            ) {
                painSubline(model, null)?.let {
                    Text(stringResource(R.string.main_pain_prefix, it), color = OnBgBody, style = Type.Body)
                }
            }
        }
        if (hasSignal) {
            if (folded) {
                CoachingNotesRow(foldCount, notesOpen) { notesOpen = !notesOpen }
                if (notesOpen) {
                    if (foldHeadline) headlineCard()
                    if (foldFeedback) feedbackCard()
                    coachingCards.forEach { coachingCard(it) }
                }
            } else {
                if (headlineIsStatus && !headlineHosts) headlineCard()
                coachingCards.forEach { coachingCard(it) }
            }
        }
        // The session's own adjustment when no card above speaks for it: its prose
        // lives here, the hero only wears the "Adjusted" chip.
        sessionAdjustmentCard?.let { coachingCard(it) }
        if (hasSignal) {
            if (!headlineIsStatus && !(folded && foldHeadline)) headlineCard()
            // Safety-critical engine meta with no hold to contradict (meta is
            // empty while one is active).
            metaOnFace.forEach { m ->
                EvidenceCard(
                    m.summary, m.grade, m.citation, m.confidence, m.safety_critical, m.contested,
                    confidenceInWhy = true, why = m.why,
                )
            }
            if (!(folded && foldFeedback)) feedbackCard()
        }
        readinessStrip?.let { strip -> Box(Modifier.padding(vertical = Space.Sm.dp)) { strip() } }
        // The session-done moment: what was done, per item, plus the
        // coaching takeaway when there is one, and the way back into the session.
        sessionDone?.let { done ->
            // Finishing ends every open exercise; resuming reopens them all and
            // returns to the first one's set loop.
            val ended = todayItems.filter { it.ended_early && it.exercise.isNotBlank() }
            SessionDoneCard(
                done,
                feedback = if (doneOwnsFeedback) fb else null,
                items = todayItems,
                painArea = painArea,
                onResume = ended.firstOrNull()?.let { first ->
                    {
                        ended.forEach { onEvent(Event.SetExerciseEnded(it.exercise, today, ended = false)) }
                        onLogItem(first)
                    }
                },
                onReview = { onOpenSheet(LogMode.Review) },
            )
        }
        // Plan-state card, always. Lead with the concrete next session; else offer
        // to generate a plan (profile set) or route into guided setup (no profile -
        // the one setup entry point on this screen).
        when {
            nextSession != null -> NextSessionCard(
                nextSession,
                painArea = painArea,
                today = today,
                restToday = restToday,
                renderedKeys = renderedKeys,
                adjusted = sessionAdjustment != null,
                coachStatus = sessionMeta,
                weightUnit = weightUnit,
                ladders = model.calisthenics_ladders,
                liftDaysLogged = remember(model.lifts) { loggedLiftDays(model.lifts) },
                onLogItem = onLogItem,
                onTrackRun = onTrackRun,
                onEndExercise = { endExerciseWithUndo(res, it, today, onEvent) },
                onReopen = { if (it.exercise.isNotBlank()) onEvent(Event.SetExerciseEnded(it.exercise, today, ended = false)) },
                onSetRung = { ladder, rung -> onEvent(Event.SetCalisthenicsRung(ladder, rung, AppClock.nowSec())) },
                onMove = onMove,
            )
            model.profile != null -> when (val withheld = model.plan_withheld) {
                // GeneratePlan can build a plan: one tap makes it appear.
                null -> PlanPromptCard { onEvent(Event.GeneratePlan(today)) }
                // It can't: never a dead button, say why and where to fix it.
                else -> PlanWithheldCard(withheld, holdProfileRow(model.hold_reason, model.profile?.health), onOpenProfile, onRerunSetup)
            }
            else -> SetupPromptCard(onStartSetup)
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
        lastPain = lastPain,
    )
}

/** "2 coaching notes ›": the session-in-progress fold of Today's coaching cards;
 *  a tap opens them under it. */
@Composable
private fun CoachingNotesRow(count: Int, open: Boolean, onToggle: () -> Unit) {
    PlainCard {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = ActionMinHeight)
                .clip(RoundedCornerShape(Space.Md.dp))
                .clickable(role = Role.Button, onClick = onToggle)
                .testTag("coaching-notes"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            Text(
                pluralStringResource(R.plurals.main_coaching_notes, count, count),
                color = OnBgBody,
                style = Type.Body,
                modifier = Modifier.weight(1f),
            )
            RowChevron(open)
        }
    }
}

/** The core's feedback category for a new best ("Recent win"). */
private const val WinCategory = "PositiveMastery"

/** The next-time advice that belongs with a win: the first coaching row for a
 *  named lift that the win's text names ("New best on Back squat…"), and
 *  that is not an in-session cue (no load change for the remaining sets, no
 *  "end the exercise"). Null when none matches. Pure. */
internal fun winAdviceFor(winText: String, coaching: List<AdjustmentView>): AdjustmentView? =
    coaching.firstOrNull { a ->
        val exercise = a.exercise?.takeIf { it.isNotBlank() }
        exercise != null && a.suggested_load_kg == null && !a.suggests_end_exercise &&
            winText.contains(exercise, ignoreCase = true)
    }

/**
 * The coaching (non-safety) adjustments Today shows: ONE per exercise. For an
 * exercise the headline already speaks about, none; otherwise the in-session cue
 * (it reacts to today's sets, so it is the newest word) wins over a plan-level
 * or next-time note, else the first in priority order. Adjustments with no
 * exercise all stay. Order is preserved. Pure.
 */
internal fun coachingPerExercise(headlineExercise: String?, candidates: List<AdjustmentView>): List<AdjustmentView> {
    val key = { a: AdjustmentView -> a.exercise?.takeIf { it.isNotBlank() }?.lowercase(Locale.ROOT) }
    val inSession = { a: AdjustmentView -> a.suggested_load_kg != null || a.suggests_end_exercise }
    val headlineKey = headlineExercise?.takeIf { it.isNotBlank() }?.lowercase(Locale.ROOT)
    val chosen = candidates.groupBy(key).mapValues { (_, group) -> group.firstOrNull(inSession) ?: group.first() }
    return candidates.filter { a ->
        val k = key(a)
        k == null || (k != headlineKey && chosen[k] === a)
    }
}

/** Distinct local days with at least one logged lift set. */
private fun loggedLiftDays(lifts: List<LiftResultView>): Int {
    val zone = java.time.ZoneId.systemDefault()
    return lifts.asSequence()
        .filter { it.observed_at > 0L }
        .map { java.time.Instant.ofEpochSecond(it.observed_at).atZone(zone).toLocalDate() }
        .distinct()
        .count()
}

/** Item statuses after which an exercise has nothing left to do today. */
private val SettledItemStatuses = setOf("done", "done_short", "done_lighter", "skipped", "skipped_pain", "stopped_pain")

/** Today's list bottom inset: the Log FAB (56dp) plus the scaffold's 16dp FAB
 *  margin plus a 16dp gap, so the last row scrolls fully clear of the floating
 *  button. Rows themselves keep the full card width (no FAB lane). */
private val TodayFabClearance = 56.dp + 16.dp + 16.dp

// Signals that need hardware the app can't read yet: never shown as pills
// until an integration exists.
private val hardwareOnlySignals = setOf(
    "BarVelocity", "VelocityLoss", "AerobicDecoupling", "HrvCv",
)

/**
 * Readiness strip (01-today §A.1):
 * overline + one qualitative pill per MEASURED signal (from `readiness_summary`),
 * plus one quiet caption per channel still collecting its baseline (no pill: a
 * progress note is not a reading). PASSIVE status only; every signal is logged
 * through the Log button / check-in, not from the strip. No headline sentence here (it
 * lives on the hero card; dedupe), no "-" placeholders, hardware-only signals
 * stay hidden until an integration exists, and a signal an active hold card on
 * the hero already states (e.g. the head cold behind "I've recovered") is not
 * repeated as a pill. No composite score (INVARIANT 1). The one exception to
 * "passive" is a SUSPENDED HRV baseline (readings jumped, e.g. a new device): it
 * cannot recover on its own, so its one line carries the restart action.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ReadinessStrip(model: ViewModel, onEvent: (Event) -> Unit = {}, onCheckin: () -> Unit = {}) {
    val measuredRows = model.stripReadinessRows()
    // Channels still collecting a baseline: an honest progress caption instead
    // of a fabricated number.
    val collecting = model.baseline_status
    // No empty-state here: with zero readiness data the strip renders NOTHING, no
    // caption, no "Readiness -" line. The Today mount site skips this composable
    // entirely when [hasReadinessPills] is false (see the mount gate), so this only
    // ever runs with at least one pill or baseline caption to show.
    PlainCard {
        TileOverline(stringResource(R.string.main_overline_readiness), color = Accent)
        if (measuredRows.isNotEmpty()) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
                verticalArrangement = Arrangement.spacedBy(Space.Sm.dp + Space.Xs.dp),
            ) {
                // Only signals that actually have data, states verbatim from the core.
                // The face carries the KB-judged state only, never the raw unit-less
                // reading (a z-score / ln rMSSD / bpm delta is jargon without a key).
                measuredRows.forEach { row ->
                    SignalPill(
                        label = readinessPillLabel(row.signal),
                        // A reading from an earlier check-in never passes as today's; a
                        // stale row's core state ("out of date") already says so.
                        state = if (row.from_today || row.stale) row.state else stringResource(R.string.main_readiness_earlier, row.state),
                        color = OnBgMuted,
                        safetyCritical = row.safety_critical,
                        holdActive = model.train_blocked,
                    )
                }
            }
        }
        // No check-in yet today: the pills above are older readings, so invite
        // today's instead of letting them look current.
        if (!model.checked_in_today) {
            TextAction(
                stringResource(R.string.main_checkin_for_today),
                onClick = onCheckin,
                chevron = true,
                modifier = Modifier.offset(x = -TextActionInset),
            )
        }
        // Mid-collection channels: still building a baseline, a sparse week, or a
        // paused baseline, each as one plain caption line under the friendly
        // signal name ("HRV", never "HRV (rMSSD)").
        collecting.forEach { b ->
            val label = readinessPillLabel(b.signal).ifBlank { b.label }
            val line = when (b.kind) {
                "SparseWeek" -> stringResource(R.string.main_sparse_week, label, b.have, b.need)
                "Suspended" -> stringResource(R.string.main_baseline_paused, label)
                else -> stringResource(R.string.main_building_baseline, label, b.have, b.need)
            }
            if (b.kind == "Suspended" && b.signal == "HrvLnRmssd") {
                // A suspended HRV baseline: its one line plus the restart action.
                // Why it paused rides in the session card's "?" (the coach-status
                // line), so it is not restated here.
                Text(line, color = OnBgMuted, style = Type.Caption)
                TextAction(
                    stringResource(R.string.main_restart_hrv_baseline),
                    onClick = { onEvent(Event.RestartHrvBaseline()) },
                    modifier = Modifier.offset(x = -TextActionInset),
                )
            } else {
                Text(line, color = OnBgMuted, style = Type.Caption)
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
            (hold_resolutions.none { it.signal == row.signal } || (row.signal == "Pain" && !train_blocked))
    }

@Composable
private fun SignalPill(
    label: String,
    state: String,
    color: Color,
    safetyCritical: Boolean = false,
    // A training hold is active: only then does a safety pill wear danger red.
    holdActive: Boolean = false,
) {
    // A safety-critical signal is marked BY MORE THAN COLOUR (WCAG 1.4.1): a leading
    // warning glyph reads the alarm even where the tint can't (colour-blind / mono).
    // The whole pill carries one spoken description naming the signal, its state, and
    // "safety critical" when applicable, instead of two disconnected label/value reads.
    val desc = "$label: $state" + if (safetyCritical) stringResource(R.string.main_safety_critical_suffix) else ""
    // Proportional severity: red is reserved for a training hold (the same red as
    // DO NOT TRAIN). With a hold active the pill renders on the DANGER SURFACE
    // (danger ground + DangerOn text, ≈4.9:1); a safety signal that only eases
    // training (an above-the-neck cold) uses the warn fill with white text
    // (≈4.9:1, Theme.kt), the amber tier of the adjustment it drives.
    // Non-critical pills keep the muted BgTop chip.
    val status = LocalStatusColors.current
    val fill = when {
        !safetyCritical -> BgTop
        holdActive -> status.danger
        else -> status.warn
    }
    val onFill = if (holdActive) DangerOn else Color.White
    val onPill = if (safetyCritical) onFill else color
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .background(fill)
            .padding(horizontal = Space.Md.dp + Space.Xs.dp, vertical = Space.Sm.dp)
            .clearAndSetSemantics { contentDescription = desc },
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (safetyCritical) {
            Icon(
                painterResource(R.drawable.ic_safety_warning_triangle),
                contentDescription = null,
                tint = onPill,
                modifier = Modifier.size(12.dp),
            )
        }
        // One text, so a long state wraps under the label inside the pill instead
        // of opening a second column beside it.
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(color = if (safetyCritical) onFill else OnBgFaint)) { append(label) }
                append(" ")
                withStyle(SpanStyle(color = onPill)) { append(state) }
            },
            style = Type.Caption.merge(TabularFigures),
        )
    }
}

/** The Progress card's data: one lift's best e1RM per session, oldest first. */
private class E1rmTrend(
    val exercise: String,
    val series: List<Double>,
    val firstDay: java.time.LocalDate,
)

/** The e1RM trend for the plan's main lift (the first lift of the next session,
 *  then of the week plan), else the most recently logged weighted lift: the best
 *  core-derived e1RM of each logged day, oldest → newest. Null with fewer than
 *  three sessions (fewer is not a trend). A bodyweight/hold set has no e1RM. */
private fun e1rmTrend(model: ViewModel): E1rmTrend? {
    val weighted = model.lifts.filter { it.hold_sec == null && it.e1rm_kg > 0.0 }
    if (weighted.isEmpty()) return null
    val zone = java.time.ZoneId.systemDefault()
    fun build(exercise: String): E1rmTrend? {
        val sets = weighted.filter { it.exercise.equals(exercise, ignoreCase = true) }
        val byDay = sets.groupBy { java.time.Instant.ofEpochSecond(it.observed_at).atZone(zone).toLocalDate() }.toSortedMap()
        if (byDay.size < 3) return null
        return E1rmTrend(
            exercise = exercise,
            series = byDay.values.map { day -> day.maxOf { it.e1rm_kg } },
            firstDay = byDay.firstKey(),
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
 * e1RM trend card (01-today §A.3), self-explanatory at a glance: a sentence-case
 * title "<exercise> · e1RM" with its glossary "?", the latest best as the big
 * figure, a labelled change pill measured over the SAME window the sparkline draws
 * ("+12.5 kg since Aug 12": first point → latest point, so the pill can never
 * contradict the line) with its own "?" line, the sparkline, and a caption naming
 * what one point is and the period ("Best set per session since Aug 12"). Every
 * number is core-derived; the pill states a difference, never a verdict.
 */
@Composable
private fun E1rmTrendCard(trend: E1rmTrend, weightUnit: WeightUnit = WeightUnit.Kg) {
    val latest = trend.series.last()
    val unit = weightSuffix(weightUnit)
    val since = remember(trend.firstDay) { trend.firstDay.format(localizedDayFormatter("MMMd")) }
    val chartDesc = stringResource(
        R.string.main_e1rm_trend_cd,
        trend.exercise,
        trend.series.size,
        loadFigure(trend.series.first(), weightUnit),
        loadFigure(latest, weightUnit),
        unit,
    )
    // Rounded once, in the display unit, from the two plotted points.
    val shownDelta = (if (weightUnit == WeightUnit.Lb) kgToLb(latest) - kgToLb(trend.series.first()) else latest - trend.series.first())
        .let { Math.round(it * 10.0) / 10.0 }
    var badgeWhy by rememberSaveable(trend.exercise) { mutableStateOf(false) }
    PlainCard {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
        ) {
            Text(
                stringResource(R.string.main_e1rm_title, trend.exercise),
                color = OnBgBody,
                style = Type.Body.copy(fontWeight = FontWeight.Bold),
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
                        R.string.main_e1rm_change_since,
                        (if (shownDelta > 0) "+" else "−") + trimDecimal(Math.abs(shownDelta)),
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
                DisclosureButton(badgeWhy, subject = stringResource(R.string.main_e1rm_change_subject)) { badgeWhy = !badgeWhy }
            }
            if (badgeWhy) {
                Text(
                    pluralStringResource(R.plurals.main_e1rm_change_why, trend.series.size, trend.series.size, since),
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
                color = Accent,
                modifier = Modifier
                    .width(140.dp)
                    .height(40.dp),
                endDot = true,
            )
        }
        Text(stringResource(R.string.main_e1rm_caption, since), color = OnBgMuted, style = Type.Caption)
    }
}

/** A kg weight rendered in the chosen unit, trimmed to one decimal like the metric
 *  faces (a whole value drops the ".0"). Conversion via Units.kt (kg → lb). */
internal fun weightFigure(kg: Double, unit: WeightUnit): String =
    trimDecimal(if (unit == WeightUnit.Lb) kgToLb(kg) else kg)

/**
 * The Today hero session card, self-describing: a "TODAY'S SESSION" / "NEXT
 * SESSION" overline, a discipline icon beside the title, and ONE "?" on the title
 * row whose panel holds every piece of evidence the card rests on (the engine
 * status lines, the session's adjustment, each item's grade/citation/why), so no
 * item carries a disclosure of its own.
 *
 * Items: in today's actionable session the CURRENT item (the first open one) is a
 * full [OpenPlanItemCard] with its slots, "Skip exercise" and the one filled
 * "Log set k of N" action, which keeps the per-set gym-floor loop one tap away;
 * every other open item is one compact tappable [PlanItemRow] (a tap logs it). Any
 * other session (a future NEXT SESSION, a blocked day) is a read-only preview:
 * compact rows, no controls, no empty slots, since it cannot be logged yet.
 * Settled items collapse to a ✓ row below. A hold empties the items (the plan
 * never renders load numbers through a hold).
 *
 * [restToday] states a plan rest day as one quiet line above a future-dated
 * session. [renderedKeys] are (summary, citation) pairs already rendered as cards
 * above the hero; the session's own adjustment is stated on the face only when it
 * is not one of them (once, as one line; a safety-critical one keeps its SAFETY
 * card). [coachStatus] is the engine-meta disclosure (provisional /
 * autoregulation source): panel only, never on the face. A session the athlete
 * moved says where from; one that can move offers "Move session" ([onMove]).
 */
@Composable
private fun NextSessionCard(
    ns: SessionPlanView,
    today: Long,
    restToday: Boolean = false,
    renderedKeys: Set<Pair<String, String>> = emptySet(),
    // A non-safety adjustment reshaped the session: the title wears the
    // "Adjusted" chip (its prose lives in the status card above).
    adjusted: Boolean = false,
    coachStatus: List<AdjustmentView> = emptyList(),
    weightUnit: WeightUnit = WeightUnit.Kg,
    ladders: List<CalisLadderView> = emptyList(),
    // Distinct days with logged lifts: the first few sessions spell the
    // prescription out for a newcomer.
    liftDaysLogged: Int = Int.MAX_VALUE,
    onLogItem: (PrescriptionView) -> Unit = {},
    onTrackRun: () -> Unit = {},
    onEndExercise: (PrescriptionView) -> Unit = {},
    onReopen: (PrescriptionView) -> Unit = {},
    onSetRung: (CalisLadder, Int) -> Unit = { _, _ -> },
    onMove: (from: Long, to: Long) -> Unit = { _, _ -> },
    // The area an active pain report names (a settled row it stopped reads
    // "Stopped · <area>").
    painArea: String? = null,
) {
    val status = LocalStatusColors.current
    val isToday = ns.epoch_day == today
    val discipline = sessionDiscipline(ns.session_type)
    // The exact gate the Log sheet's "Today's plan" fast-path uses: only today's
    // live/adjusted session is actionable; a future NEXT SESSION stays informational.
    val actionable = isToday && (ns.status == "next" || ns.status == "adjusted")
    val isCheck = { item: PrescriptionView -> isReturnCheckItem(item, ns.session_type) }
    val isRun = { item: PrescriptionView -> !isCheck(item) && (discipline == "Run" || item.exercise.isEmpty()) }
    // The session's adjustment, unless it already renders as its own card above.
    val adjustment = ns.adjustment?.takeIf { ns.status == "adjusted" && (it.summary to it.citation) !in renderedKeys }
    val hasEvidence = ns.items.isNotEmpty() || coachStatus.isNotEmpty() || adjustment != null
    var expanded by rememberSaveable(ns.epoch_day, ns.title) { mutableStateOf(false) }
    PlainCard {
        // Self-describing accent overline naming the card for a newcomer (the one
        // section-label grammar). A plan rest day today folds into it ("REST DAY ·
        // NEXT SESSION") so the card still leads with its overline.
        TileOverline(
            when {
                isToday -> stringResource(R.string.main_todays_session)
                restToday -> stringResource(R.string.main_rest_day_next_overline)
                else -> stringResource(R.string.main_next_session_overline)
            },
            color = Accent,
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            // Discipline icon tile: Run / Lift get a symbol; Rest gets none.
            val iconRes = when (discipline) {
                "Run" -> R.drawable.ic_content_run
                "Lift" -> R.drawable.ic_content_set_dumbbell
                else -> null
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
                    Text(ns.title, color = OnBgBody, style = Type.Title)
                    // Today is already marked in the week strip below; only a
                    // future-dated session keeps its date label.
                    if (!isToday) {
                        Text(planDateLabel(ns.epoch_day), color = OnBgMuted, style = Type.Caption)
                    }
                }
                if (adjusted) AdjustedChip()
            }
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
        // A rest running from the set loop stays visible after the sheet closes.
        if (actionable) {
            RestAlarm.running.collectAsState().value?.let { RestPill(it) }
        }
        // Where a moved session came from / why a move was refused.
        SessionMoveNotes(ns)
        if (ns.status == "blocked") Chip(stringResource(R.string.main_chip_on_hold), status.danger)
        // A safety-critical adjustment that reshaped the session keeps its SAFETY
        // card on the face (hard rule). Any other one is stated once, in its
        // status card above: the hero wears the "Adjusted" chip and its evidence
        // joins the session panel.
        adjustment?.takeIf { it.safety_critical }?.let { adj ->
            EvidenceCard(
                adj.summary, adj.grade, adj.citation, adj.confidence,
                adj.safety_critical, adj.contested,
                confidenceInWhy = false, why = adj.why,
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
                if (ns.status == "blocked") {
                    stringResource(R.string.main_session_on_hold)
                } else {
                    stringResource(R.string.main_rest_day)
                },
                color = OnBgMuted,
                style = Type.Body,
            )
        } else {
            // next_session keeps done items: open items render first, settled ones
            // collapse to a ✓ row below.
            val (openItems, doneItems) = ns.items.partition { !it.done }
            val current = if (actionable) openItems.firstOrNull() else null
            // A ladder the athlete was never placed on (calisthenics): the current
            // item invites a Harder tap instead of weeks below their level.
            val unplaced = ladders.filter { it.current_rung == null }.map { it.ladder }.toSet()
            val trackLabel = stringResource(R.string.main_plan_item_track)
            openItems.forEach { item ->
                if (item === current) {
                    OpenPlanItemCard(
                        item = item,
                        isRun = isRun(item),
                        onLog = { onLogItem(item) },
                        onTrack = onTrackRun,
                        onEndExercise = { onEndExercise(item) },
                        runActionLabel = trackLabel,
                        weightUnit = weightUnit,
                        isCheck = isCheck(item),
                        unplaced = item.calis?.let { it.ladder in unplaced && it.harder?.available == true } == true,
                        onSetRung = onSetRung,
                        guide = firstSessionGuide(item, isRun(item), isCheck(item), liftDaysLogged),
                    )
                } else {
                    PlanItemRow(
                        item = item,
                        isRun = isRun(item),
                        isCheck = isCheck(item),
                        actionLabel = if (actionable) planItemActionLabel(item, isRun(item), isCheck(item), trackLabel) else null,
                        onClick = { if (isRun(item)) onTrackRun() else onLogItem(item) },
                    )
                }
            }
            doneItems.forEach { item ->
                DonePlanItemRow(
                    item = item,
                    isRun = isRun(item),
                    reopenable = actionable,
                    onReopen = { onReopen(item) },
                    isCheck = isCheck(item),
                    painArea = painArea,
                )
            }
        }
    }
}

/** "Rest 1:42" while a rest from the set loop is still running; nothing once it
 *  is over. Ticks every second only while shown. */
@Composable
private fun RestPill(run: RestAlarm.RestRun) {
    val now = rememberTicker(1_000L)
    val leftSec = ((run.endAtMillis - now) / 1000L).toInt()
    if (leftSec <= 0) return
    val clock = fmtClock(leftSec)
    val desc = stringResource(R.string.main_rest_pill_cd, clock)
    Text(
        stringResource(R.string.main_rest_pill, clock),
        color = OnBgBody,
        style = Type.Chip.merge(TabularFigures),
        modifier = Modifier
            .clip(RoundedCornerShape(100))
            .background(AccentTint)
            .padding(horizontal = Space.Md.dp, vertical = Space.Sm.dp)
            .clearAndSetSemantics { contentDescription = desc },
    )
}

/** Lifting days after which the hero stops spelling the prescription out. */
private const val GuidedLiftDays = 3

/** The prescribed reps-in-reserve as a whole number (RIR = 10 − RPE), or null
 *  for an open-ended ("4+") or half-point target the guide can't state plainly. Pure. */
internal fun prescribedRir(rpe: Double): Int? {
    val rir = 10.0 - rpe
    return if (rir >= 0.0 && rir < 4.0 && rir % 1.0 == 0.0) rir.toInt() else null
}

/**
 * Plain-language lines for a newcomer's current lift, from the item's own numbers:
 * the prescription spelled out ("5 sets of 5 reps, stopping about 3 reps short of
 * failure") for the first [GuidedLiftDays] lifting days, and, when the item has no
 * history and no load to start from, how to pick a first weight: one you could
 * lift about reps + reps-in-reserve times (the definition of reps in reserve).
 * A bodyweight exercise picks its version (the easier / harder rung) instead of
 * a weight. The reps-left target is said once, in the spelled-out line.
 */
@Composable
private fun firstSessionGuide(item: PrescriptionView, isRun: Boolean, isCheck: Boolean, liftDaysLogged: Int): List<String> {
    if (isRun || isCheck || item.timed_hold || liftDaysLogged >= GuidedLiftDays) return emptyList()
    if (item.sets <= 0 || item.reps_low <= 0) return emptyList()
    val rir = item.target_rpe?.let(::prescribedRir) ?: return emptyList()
    val reps = if (item.reps_high > item.reps_low) "${item.reps_low}\u2013${item.reps_high}" else "${item.reps_low}"
    val noStart = item.last_set == null && item.logged_sets.isEmpty() &&
        item.load_kg == null && item.suggested_load_kg == null
    val bodyweight = item.calis != null && (item.added_kg ?: 0.0) == 0.0
    return buildList {
        add(pluralStringResource(R.plurals.plan_guide_spelled, rir, item.sets, reps, rir))
        if (noStart) {
            add(
                stringResource(
                    if (bodyweight) R.string.plan_guide_first_version else R.string.plan_guide_first_weight,
                    item.reps_low + rir,
                ),
            )
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
    val gradeInWords = gradeLabel(a.grade)?.let { stringResource(R.string.main_evidence_grade, it) }.orEmpty()
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        if (a.safety_critical) Chip(stringResource(R.string.main_chip_safety), status.danger)
        if (showSummary) {
            Text(a.summary, color = OnBgBody, style = Type.Caption.copy(fontWeight = FontWeight.Bold))
        }
        WhyDetail(
            why = if (a.safety_critical) a.why.copy(grade_note = a.why.grade_note.ifBlank { gradeInWords }) else a.why,
            grade = if (a.safety_critical) "" else a.grade,
            citation = a.citation,
            confidence = a.confidence,
            contested = a.contested,
            showConfidence = !a.safety_critical,
            extraDetail = null,
        )
    }
}

/**
 * The one supporting line under a hold's safety call: the core's plain
 * "paused until…" line when the card also carries the hold's resolve action
 * (it explains that action), else the hold's first follow-up adjustment. It has
 * no "?" of its own: the follow-up rows' evidence sits in the host card's single
 * "?" panel ([HoldGroupPanel]), so the card reads call + one line + action.
 */
@Composable
private fun HoldGroupLine(rows: List<AdjustmentView>, pausedText: String) {
    val faceText = pausedText.ifBlank { rows.firstOrNull()?.summary.orEmpty() }
    if (faceText.isNotBlank()) Text(faceText, color = OnBgMuted, style = Type.Caption)
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
    onRemoveReport: (() -> Unit)? = null,
) {
    StatusCard(
        tone = StatusTone.Adjusted,
        overline = overline,
        glyph = painterResource(R.drawable.ic_safety_shield),
        markDescription = stringResource(R.string.main_status_mark_safety),
        footer = {
            StatusCardFooter(
                primaryLabel = r.action_label,
                onPrimary = onResolve,
                secondaryLabel = onRemoveReport?.let { stringResource(R.string.main_remove_report) },
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
 * The session-done moment: once today's planned session is
 * complete, its title and one line per item in the hero's own verdict (its
 * status) with the hero's own marks: a green ✓ for work done, with the status
 * spelled out ("· short reps", "· lighter") when it fell short of the plan, and a
 * neutral dash with "Skipped" / "Stopped · <area>" for an exercise skipped or
 * ended by a pain report, with the coaching takeaway (a new best, a derived
 * session note) when the core has one. A run reads its distance and time in
 * the user's unit ("5 of 5.9 mi · 60 min" when under the plan). Then the way
 * on: "How did it go? ›" (the session review) and, when finishing ended
 * exercises, "Resume session" ([onResume]). Descriptive, so no evidence chrome
 * of its own; the takeaway keeps its "?". [items] are today's plan items, the
 * status source for an older core that sends none on the row.
 */
@Composable
private fun SessionDoneCard(
    done: SessionDoneView,
    feedback: FeedbackView?,
    items: List<PrescriptionView> = emptyList(),
    // The area an active pain report names: a row it stopped reads "Stopped · <area>".
    painArea: String? = null,
    onResume: (() -> Unit)? = null,
    onReview: () -> Unit = {},
) {
    val status = LocalStatusColors.current
    val distanceUnit = rememberDistanceUnit()
    Box(Modifier.testTag("session-done")) {
        PlainCard {
            TileOverline(stringResource(R.string.main_session_done_overline), color = Accent)
            Text(
                done.title.ifBlank { stringResource(R.string.main_session_done_title) },
                color = OnBgBody,
                style = Type.Title,
            )
            done.items.forEach { row ->
                val rowStatus = row.status.ifBlank {
                    items.firstOrNull { it.exercise.equals(row.exercise, ignoreCase = true) }?.status.orEmpty()
                }
                val skippedLabel = when (rowStatus) {
                    "stopped_pain" -> stoppedLabel(painArea)
                    "skipped_pain" -> stringResource(R.string.plan_skipped_pain)
                    "skipped" -> stringResource(R.string.plan_skipped)
                    else -> null
                }
                val summary = sessionDoneRunLine(row, distanceUnit) ?: row.summary
                val line = skippedLabel ?: when {
                    summary.isBlank() -> summary
                    rowStatus == "done_short" -> stringResource(R.string.plan_done_short, summary)
                    rowStatus == "done_lighter" -> stringResource(R.string.plan_done_lighter, summary)
                    else -> summary
                }
                val desc = if (row.met && skippedLabel == null) {
                    stringResource(R.string.main_session_done_met_cd, row.exercise, summary)
                } else {
                    stringResource(R.string.main_session_done_status_cd, row.exercise, line)
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clearAndSetSemantics { contentDescription = desc },
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                ) {
                    // The hero's marks: ✓ = work done (its line says when it fell
                    // short), a dash a deliberate skip or a pain stop; never a red
                    // failure mark.
                    Text(
                        if (skippedLabel != null) "\u2013" else "\u2713",
                        color = if (skippedLabel != null) OnBgMuted else status.done,
                        style = Type.Body.copy(fontWeight = FontWeight.Bold),
                    )
                    Column(modifier = Modifier.weight(1f)) {
                        Text(row.exercise, color = OnBgBody, style = Type.Body)
                        if (line.isNotBlank()) {
                            Text(line, color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
                        }
                    }
                }
            }
            feedback?.let { fb ->
                val message = feedbackMessage(fb, rememberDistanceUnit())
                if (fb.grade.isBlank()) {
                    // An ungraded note asserts no claim: plain text, no evidence chrome.
                    Text(message, color = OnBgBody, style = Type.Body)
                } else {
                    // The takeaway is a nested tile one surface step down, like every
                    // other nested surface in a card (no outline).
                    EvidenceCard(
                        message, fb.grade, fb.citation, fb.confidence, fb.safety_critical, fb.contested,
                        fb.category_label.ifBlank { null },
                        why = fb.why,
                        container = BgTop,
                    )
                }
            }
            Column(Modifier.offset(x = -TextActionInset)) {
                TextAction(stringResource(R.string.main_session_done_review), onClick = onReview, chevron = true)
                onResume?.let { TextAction(stringResource(R.string.main_session_done_resume), onClick = it) }
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
    val tenth = { km: Double -> Math.round(metersToDisplay(km * 1000.0, unit) * 10.0) / 10.0 }
    val dist = row.run_km?.let(tenth)
    val plannedDist = row.run_planned_km?.let(tenth)
    val min = row.run_min?.let { Math.round(it).toInt() }
    val plannedMin = row.run_planned_min?.let { Math.round(it).toInt() }
    val distText = dist?.let { d ->
        val shown = "${trimDecimal(d)}\u00A0${unit.distanceLabel}"
        if (plannedDist != null && d < plannedDist) {
            stringResource(R.string.main_done_run_of, trimDecimal(d), "${trimDecimal(plannedDist)}\u00A0${unit.distanceLabel}")
        } else {
            shown
        }
    }
    val minText = min?.let { m ->
        if (plannedMin != null && plannedDist == null && m < plannedMin) {
            stringResource(R.string.main_done_run_of, "$m", stringResource(R.string.log_value_min, plannedMin))
        } else {
            stringResource(R.string.log_value_min, m)
        }
    }
    return listOfNotNull(distText, minText).joinToString(" \u00b7 ").ifBlank { null }
}

/** The "Plan my training" call-to-action shown when a profile exists, no plan
 *  has been generated yet and the core can build one (`plan_withheld` null). */
@Composable
private fun PlanPromptCard(onGenerate: () -> Unit) {
    PlainCard {
        Text(stringResource(R.string.main_plan_prompt_title), color = OnBgBody, style = Type.Title)
        Text(
            stringResource(R.string.main_plan_prompt_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        PrimaryButton(
            stringResource(R.string.main_plan_prompt_cta),
            onGenerate,
            Modifier.fillMaxWidth(),
        )
    }
}

/** In place of "Plan my training" when the core can't build a plan
 *  ([withheld], `plan_withheld`): why, and the way to fix it, so the plan card
 *  never offers a tap that does nothing: a health hold opens its own answer
 *  ([healthRow]) in Profile; a profile with nothing to plan re-runs guided setup
 *  from its answers ([onRerunSetup]), whose Finish plans the training. */
@Composable
private fun PlanWithheldCard(
    withheld: String,
    healthRow: ProfileRowId,
    onOpenProfile: (ProfileRowId) -> Unit,
    onRerunSetup: () -> Unit,
) {
    val health = withheld == "health_hold"
    PlainCard {
        Text(
            stringResource(if (health) R.string.main_plan_withheld_health_title else R.string.main_plan_withheld_empty_title),
            color = OnBgBody,
            style = Type.Title,
        )
        Text(
            stringResource(if (health) R.string.main_plan_withheld_health_body else R.string.main_plan_withheld_empty_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        PrimaryButton(
            stringResource(if (health) R.string.main_hold_review_health else R.string.main_plan_withheld_empty_cta),
            { if (health) onOpenProfile(healthRow) else onRerunSetup() },
            Modifier.fillMaxWidth(),
        )
    }
}

/** Shown on Today when no profile exists yet: the plan is built from the profile,
 *  so route the user into guided setup rather than leaving the plan section blank. */
@Composable
private fun SetupPromptCard(onStartSetup: () -> Unit) {
    PlainCard {
        Text(stringResource(R.string.main_setup_prompt_title), color = OnBgBody, style = Type.Title)
        Text(
            stringResource(R.string.main_setup_prompt_body),
            color = OnBgMuted,
            style = Type.Body,
        )
        PrimaryButton(
            stringResource(R.string.main_setup_prompt_cta),
            onStartSetup,
            Modifier.fillMaxWidth(),
        )
    }
}

/**
 * Display label for a wire readiness-signal name. Falls back to the raw wire
 * name for a signal the shell hasn't caught up to.
 */
@Composable
private fun readinessSignalLabel(wire: String): String =
    ReadinessSignal.entries.firstOrNull { it.name == wire }?.let { stringResource(it.labelRes) } ?: wire

/**
 * Readiness-strip pill name: the copy deck's friendly name where one exists
 * (deck says "HRV", not the wire's "HRV (ln rMSSD)"); every other signal keeps
 * its full label. Display-only, the wire value never changes.
 */
@Composable
private fun readinessPillLabel(wire: String): String = when (wire) {
    "HrvLnRmssd" -> "HRV"
    "WellnessZ" -> "Wellness"
    "EstimatedOneRm" -> "Strength"
    "Rpe" -> "Effort"
    "RestingHr" -> "Resting HR"
    else -> readinessSignalLabel(wire)
}

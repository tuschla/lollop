package app.milestone

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.Placeable
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.milestone.resources.Res
import app.milestone.resources.coach_export_action
import app.milestone.resources.ic_safety_warning_triangle
import app.milestone.resources.log_undo
import app.milestone.resources.main_add_details
import app.milestone.resources.main_clear_readiness_message
import app.milestone.resources.main_clear_readiness_title
import app.milestone.resources.main_delete
import app.milestone.resources.main_do_not_train
import app.milestone.resources.main_do_not_train_subject
import app.milestone.resources.main_evidence_prefix
import app.milestone.resources.main_hold_check_age
import app.milestone.resources.main_hold_paused_pregnancy
import app.milestone.resources.main_hold_paused_youth
import app.milestone.resources.main_hold_reason_checkins
import app.milestone.resources.main_hold_reason_doctor
import app.milestone.resources.main_hold_reason_generic
import app.milestone.resources.main_hold_reason_hrv
import app.milestone.resources.main_hold_reason_ill_profile
import app.milestone.resources.main_hold_reason_illness
import app.milestone.resources.main_hold_reason_medical_signal
import app.milestone.resources.main_hold_reason_pain
import app.milestone.resources.main_hold_reason_performance
import app.milestone.resources.main_hold_reason_pregnancy
import app.milestone.resources.main_hold_reason_pregnancy_warning
import app.milestone.resources.main_hold_reason_single_day
import app.milestone.resources.main_hold_reason_youth
import app.milestone.resources.main_hold_review_health
import app.milestone.resources.main_hold_review_health_hint
import app.milestone.resources.main_hold_youth_subject
import app.milestone.resources.main_hold_youth_title
import app.milestone.resources.main_logged_by_mistake
import app.milestone.resources.main_medical_hold_body
import app.milestone.resources.main_pain_prefix
import app.milestone.resources.main_programming_paused
import app.milestone.resources.main_remove_illness_title
import app.milestone.resources.main_remove_pain_title
import app.milestone.resources.main_remove_report
import app.milestone.resources.main_remove_report_message
import app.milestone.resources.main_remove_report_message_hold
import app.milestone.resources.main_report_removed
import app.milestone.resources.main_reported_hold_body
import app.milestone.resources.main_resolve_hold_confirm
import org.jetbrains.compose.resources.StringResource
import org.jetbrains.compose.resources.painterResource
import org.jetbrains.compose.resources.stringResource

/**
 * The banner's one way back for the active hold, from the core's `hold_reason`,
 * else the legacy safety tier: a mistaken pain or illness report is removed
 * (only that report), a self-reported illness on the profile ends at the core's
 * one illness action ("I'm better", which also resolves any illness report), an
 * age hold points at the age row, a health-answer hold at the health answers,
 * anything else keeps the guarded clear of readiness inputs.
 */
internal enum class HoldAction { RemovePain, RemoveIllness, EndIllness, CheckAge, ReviewHealth, ClearReadiness }

/** Profile health answers that hold training until a doctor's OK. */
private val DoctorHoldReasons = setOf(HoldReason.Parq, HoldReason.Pregnancy, HoldReason.Reds, HoldReason.InjuryRehab, HoldReason.Symptomatic)

internal fun holdAction(holdReason: HoldReason?, tier: SafetyTier?): HoldAction = when (holdReason) {
    HoldReason.Pain -> HoldAction.RemovePain
    HoldReason.Illness -> HoldAction.RemoveIllness
    HoldReason.AcuteIllnessProfile -> HoldAction.EndIllness
    HoldReason.Youth -> HoldAction.CheckAge
    in DoctorHoldReasons -> HoldAction.ReviewHealth
    HoldReason.MedicalSignal -> HoldAction.ClearReadiness
    else -> when (tier) {
        SafetyTier.Pain -> HoldAction.RemovePain
        SafetyTier.Illness -> HoldAction.RemoveIllness
        SafetyTier.MedicalReferral -> HoldAction.ReviewHealth
        else -> HoldAction.ClearReadiness
    }
}

/**
 * The Profile row a hold banner's "Check age" / "Review health answers" lands on:
 * the answer that raised the hold, never a list summary that reads "None apply"
 * while the cause sits elsewhere. [health] (the profile's answers) picks between
 * rows of one reason; a reason without its own row opens the health questions.
 */
internal fun holdProfileRow(holdReason: HoldReason?, health: HealthScreen?): ProfileRowId = when (holdReason) {
    HoldReason.Youth -> ProfileRowId.Age
    HoldReason.Pregnancy -> if (health?.pregnancy_warning_sign == true) ProfileRowId.PregnancyWarning else ProfileRowId.Pregnant
    HoldReason.Symptomatic -> ProfileRowId.ParqSymptom
    HoldReason.InjuryRehab -> ProfileRowId.ParqInjury
    HoldReason.Parq -> ProfileRowId.ParqClearance
    // A RED-S hold may come from a weekly check-in answer, not the profile row.
    HoldReason.Reds -> if (health?.reds_signal == true) ProfileRowId.Reds else ProfileRowId.HealthQuestions
    HoldReason.AcuteIllnessProfile -> ProfileRowId.Ill
    else -> ProfileRowId.HealthQuestions
}

/** The hold is stated in full where it renders (its reason and its one way back),
 *  so Today adds no SAFETY card repeating it: a pain or illness report, the
 *  profile's "I'm ill right now" answer, a pregnancy warning sign (the core's
 *  first gate, so its card is always the warning the banner names), or the
 *  under-18 pause ([YouthHoldCard] carries its reason and evidence). A
 *  plain pregnancy's card carries the reference the banner leaves out, and the
 *  other health answers' cards name the answer behind the banner's generic line. */
internal fun bannerStatesHold(holdReason: HoldReason?, tier: SafetyTier?, pregnancyWarning: Boolean): Boolean =
    tier == SafetyTier.Pain || tier == SafetyTier.Illness ||
        holdAction(holdReason, tier).let { it == HoldAction.EndIllness || it == HoldAction.CheckAge } ||
        (holdReason == HoldReason.Pregnancy && pregnancyWarning)

/** The banner's plain reason line ("You reported pain today."), never engine
 *  vocabulary like "red flag" or "tier". A pregnancy reads like its own
 *  Profile row; [pregnancyWarning] (a warning sign is on) says to stop and
 *  seek care. */
internal fun holdReasonLineRes(holdReason: HoldReason?, tier: SafetyTier?, pregnancyWarning: Boolean): StringResource = when (holdReason) {
    HoldReason.Pain -> Res.string.main_hold_reason_pain
    HoldReason.Illness -> Res.string.main_hold_reason_illness
    HoldReason.AcuteIllnessProfile -> Res.string.main_hold_reason_ill_profile
    HoldReason.Youth -> Res.string.main_hold_reason_youth
    HoldReason.Pregnancy -> if (pregnancyWarning) Res.string.main_hold_reason_pregnancy_warning else Res.string.main_hold_reason_pregnancy
    HoldReason.Parq, HoldReason.Reds, HoldReason.InjuryRehab, HoldReason.Symptomatic -> Res.string.main_hold_reason_doctor
    HoldReason.MedicalSignal -> Res.string.main_hold_reason_medical_signal
    null -> when (tier) {
        SafetyTier.MedicalReferral -> Res.string.main_hold_reason_doctor
        SafetyTier.Pain -> Res.string.main_hold_reason_pain
        SafetyTier.Illness -> Res.string.main_hold_reason_illness
        SafetyTier.ObjectivePerformance -> Res.string.main_hold_reason_performance
        SafetyTier.SubjectiveMultiDay -> Res.string.main_hold_reason_checkins
        SafetyTier.HrvTrend -> Res.string.main_hold_reason_hrv
        SafetyTier.SingleDayMarker -> Res.string.main_hold_reason_single_day
        null -> Res.string.main_hold_reason_generic
    }
}

/**
 * The DO-NOT-TRAIN banner with the confirm dialogs its actions need, the one
 * host for every screen that pins it (Today and the run tracker). The under-18
 * pause renders nothing here (it is Today's [YouthHoldCard]). Without
 * [onOpenProfile] the banner states the way back as text.
 */
@Composable
internal fun SafetyBannerHost(
    model: ViewModel,
    onEvent: (Event) -> Unit,
    modifier: Modifier = Modifier,
    onAddDetails: () -> Unit,
    onOpenProfile: ((ProfileRowId) -> Unit)? = null,
) {
    var confirmReadiness by rememberSaveable { mutableStateOf(false) }
    var removeReport by rememberSaveable { mutableStateOf<ReadinessSignal?>(null) }
    var resolveHoldSignal by rememberSaveable { mutableStateOf<ReadinessSignal?>(null) }
    val action = holdAction(model.hold_reason, model.safety_tier)
    val onAction: (() -> Unit)? = when (action) {
        HoldAction.RemovePain -> { { removeReport = ReadinessSignal.Pain } }
        HoldAction.RemoveIllness -> { { removeReport = ReadinessSignal.Illness } }
        HoldAction.ClearReadiness -> { { confirmReadiness = true } }
        HoldAction.ReviewHealth ->
            onOpenProfile?.let { open -> { open(holdProfileRow(model.hold_reason, model.profile?.health)) } }
        // The core's illness action (a solid button) is the way back; the
        // under-18 pause has no pinned banner at all.
        HoldAction.EndIllness, HoldAction.CheckAge -> null
    }
    SafetyBanner(
        model = model,
        modifier = modifier,
        holdDetail = model.active_pain?.context,
        action = action,
        onAction = onAction,
        onAddDetails = onAddDetails,
        onResolveHold = { resolveHoldSignal = it.signal },
    )
    if (confirmReadiness) ClearConfirmDialog(
        title = stringResource(Res.string.main_clear_readiness_title),
        message = stringResource(Res.string.main_clear_readiness_message),
        onDismiss = { confirmReadiness = false },
        onClear = { onEvent(Event.ClearReadiness) },
    )
    RemoveReportDialog(signal = removeReport, onDismiss = { removeReport = null }, onEvent = onEvent, model = model)
    HoldResolveDialog(
        resolution = resolveHoldSignal?.let { sig -> model.hold_resolutions.firstOrNull { it.signal == sig } },
        model = model,
        onDismiss = { resolveHoldSignal = null },
        onEvent = onEvent,
    )
}

/**
 * Confirm for removing ONE mistaken pain or illness report ([signal]; null
 * renders nothing). The removal is UNDATED (observed_at 0): the newest report of
 * that signal is deleted outright, unlike "I'm pain-free now" / "I've recovered"
 * (ResolveHold), which keeps it in history for the days it shaped.
 */
@Composable
internal fun RemoveReportDialog(
    signal: ReadinessSignal?,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
    // Words the dialog (does the report hold training) and holds the report
    // "Report removed · Undo" restores ([ViewModel.active_pain]).
    model: ViewModel,
) {
    if (signal == null) return
    val illness = signal == ReadinessSignal.Illness
    val removed = stringResource(Res.string.main_report_removed)
    val undo = stringResource(Res.string.log_undo)
    val holds = model.train_blocked
    ClearConfirmDialog(
        title = stringResource(if (illness) Res.string.main_remove_illness_title else Res.string.main_remove_pain_title),
        message = stringResource(if (holds) Res.string.main_remove_report_message_hold else Res.string.main_remove_report_message),
        confirmLabel = stringResource(Res.string.main_delete),
        onDismiss = onDismiss,
        onClear = {
            val row = model.readiness_summary.firstOrNull { it.signal == signal }
            val restore: Event? = when {
                illness && row != null -> Event.SubmitReadiness(signal, row.value, AppClock.nowSec())
                !illness -> model.active_pain?.let { Event.SubmitReadiness(signal, 1.0, AppClock.nowSec(), pain = it.report) }
                else -> null
            }
            onEvent(Event.RemoveReadiness(signal, observedAt = 0L))
            UiMessages.post(
                if (restore != null) {
                    UiMessage(removed, actionLabel = undo, onAction = { onEvent(restore) })
                } else {
                    UiMessage(removed)
                },
            )
        },
    )
}

/**
 * DO-NOT-TRAIN banner (01-today §C, INVARIANT 3): pinned above content, never
 * dismissable, only while the core blocks training (`train_blocked`). It always
 * states the hold whole (headline, reason, actions); only its extra text scrolls
 * inside it. The "?" shows grade + citation, never a confidence meter.
 * [holdDetail] is the characterized pain sub-line; the reason line and the one
 * way back follow [action].
 */
@Composable
private fun SafetyBanner(
    model: ViewModel,
    modifier: Modifier,
    holdDetail: String?,
    action: HoldAction,
    onAction: (() -> Unit)?,
    onAddDetails: () -> Unit,
    onResolveHold: (HoldResolutionView) -> Unit,
) {
    // Proportional safety: the pinned red banner is reserved for a genuine
    // training block. A non-blocking safety_tier (e.g. HrvTrend) is handled by
    // the amber Today card, not here, and the under-18 pause by YouthHoldCard.
    if (!model.train_blocked || action == HoldAction.CheckAge) return
    val tier = model.safety_tier
    val status = LocalStatusColors.current
    val bg = status.danger
    // A slow, gentle pulse ring so the hold reads as the most urgent state.
    val pulse = run {
        val transition = rememberInfiniteTransition(label = "safety")
        transition.animateFloat(
            initialValue = 0f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(PulseHalfPeriodMs), RepeatMode.Reverse),
            label = "pulse",
        )
    }
    // Read the animated value in the DRAW phase, not composition: the pulse ring
    // re-renders each frame without recomposing the whole banner.
    val cornerPx = with(LocalDensity.current) { Space.Card.dp.toPx() }
    val strokePx = with(LocalDensity.current) { 2.dp.toPx() }
    val pulseModifier = Modifier.drawBehind {
        drawRoundRect(
            color = OnStatusFill.copy(alpha = PulseAlphaMin + pulse.value * PulseAlphaSpan),
            cornerRadius = CornerRadius(cornerPx, cornerPx),
            style = Stroke(width = strokePx),
        )
    }
    var whyOpen by rememberSaveable { mutableStateOf(false) }
    // A profile health answer (or a reported warning sign) is not a readiness
    // report, so the "?" fallback copy must not call it one.
    val profileHold = action == HoldAction.ReviewHealth ||
        action == HoldAction.EndIllness || model.hold_reason == HoldReason.MedicalSignal
    val pregnancyWarning = model.profile?.health?.pregnancy_warning_sign == true
    val subLine = holdDetail?.takeIf { action == HoldAction.RemovePain }
        ?.let { stringResource(Res.string.main_pain_prefix, it) }
        ?: stringResource(holdReasonLineRes(model.hold_reason, tier, pregnancyWarning))
    // The core's plain "why it's paused and how it clears" line replaces
    // the generic caption when a resolvable hold is active (an illness
    // always has one); otherwise the caption names this hold's own way back
    // (only a PAR-Q-style answer is "cleared", a pregnancy pauses plans
    // while its answer is on).
    val resolutions = model.hold_resolutions
    val pausedText = resolutions.firstOrNull { it.paused_text.isNotBlank() }?.paused_text
    // The banner stays pinned and is never overlapped, but at a large font on a
    // short screen it may not take the whole screen: what it adds beyond stating
    // the hold (the "paused" line, an open "?" panel) stops at a share of the
    // screen height and scrolls inside the banner, so the destination under it
    // (and its Log button) keeps room. The headline, the reason and the actions
    // are never part of that share: they always show whole (INVARIANT 3), the
    // banner growing past the share when they alone need more.
    val density = LocalDensity.current
    val capPx = with(density) { (screenHeightDp() * BannerMaxScreenFraction).dp.roundToPx() }
    val padPx = with(density) { Space.Card.dp.roundToPx() }
    val gapPx = with(density) { Space.Md.dp.roundToPx() }
    Card(
        colors = CardDefaults.cardColors(containerColor = bg),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = modifier.fillMaxWidth().then(pulseModifier).testTag("safety-banner"),
    ) {
        val headline: @Composable () -> Unit = {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
                ) {
                    Icon(
                        painterResource(Res.drawable.ic_safety_warning_triangle),
                        contentDescription = null,
                        tint = OnStatusFill,
                        modifier = Modifier.size(26.dp),
                    )
                    Text(
                        stringResource(Res.string.main_do_not_train),
                        color = OnStatusFill,
                        style = BannerTitleStyle,
                    )
                }
                Text(subLine, color = DangerOn, style = Type.Body)
            }
        }
        val paused: @Composable () -> Unit = {
            Text(
                pausedText ?: stringResource(
                    if (model.hold_reason == HoldReason.Pregnancy && !pregnancyWarning) {
                        Res.string.main_hold_paused_pregnancy
                    } else {
                        Res.string.main_programming_paused
                    },
                ),
                color = DangerOn,
                style = Type.Caption,
                modifier = Modifier.verticalScroll(rememberScrollState()),
            )
        }
        val reportHold = action == HoldAction.RemovePain || action == HoldAction.RemoveIllness ||
            action == HoldAction.ClearReadiness
        val undoLabel = when (action) {
            HoldAction.RemovePain, HoldAction.RemoveIllness -> stringResource(Res.string.main_remove_report)
            HoldAction.ClearReadiness -> stringResource(Res.string.main_logged_by_mistake)
            HoldAction.ReviewHealth -> stringResource(Res.string.main_hold_review_health)
            HoldAction.EndIllness, HoldAction.CheckAge -> null
        }
        val opensProfile = action == HoldAction.ReviewHealth
        val hasTextActions = reportHold || opensProfile || (undoLabel != null && onAction != null)
        // The way back to training: the core's resolve actions (history kept;
        // one illness action ends every illness hold). Solid white, the
        // banner's primary actions.
        val solidActions = resolutions.filter { it.action_label.isNotBlank() }
            .map { r -> r.action_label to { onResolveHold(r) } }
        val disclosureSubject = stringResource(Res.string.main_do_not_train_subject)
        val disclosure: @Composable () -> Unit = {
            DisclosureButton(whyOpen, tint = OnStatusFill, onTint = bg, subject = disclosureSubject) { whyOpen = !whyOpen }
        }
        val actions: @Composable () -> Unit = {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                solidActions.forEachIndexed { i, (label, onClick) ->
                    if (!hasTextActions && i == solidActions.lastIndex) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
                        ) {
                            BannerSolidAction(label, bg, onClick, Modifier.weight(1f))
                            disclosure()
                        }
                    } else {
                        BannerSolidAction(label, bg, onClick)
                    }
                }
                if (hasTextActions || solidActions.isEmpty()) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.Top,
                    ) {
                        // Each text action on its own line (never joined by a stray "·").
                        Column(Modifier.weight(1f).offset(x = -TextActionInset)) {
                            if (reportHold) {
                                TextAction(stringResource(Res.string.main_add_details), onClick = onAddDetails, tint = OnStatusFill)
                            }
                            if (undoLabel != null && onAction != null) {
                                TextAction(undoLabel, onClick = onAction, tint = OnStatusFill, chevron = opensProfile)
                            } else if (opensProfile) {
                                // No Profile flow on this screen (the tracker): the way back as text.
                                Text(
                                    stringResource(Res.string.main_hold_review_health_hint),
                                    color = DangerOn,
                                    style = Type.Caption,
                                    modifier = Modifier.padding(start = TextActionInset, top = Space.Md.dp),
                                )
                            }
                        }
                        disclosure()
                    }
                }
            }
        }
        val why: @Composable () -> Unit = {
            if (whyOpen) {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    HoldEvidencePanel(model, profileHold, strong = OnStatusFill, muted = DangerOn)
                }
            }
        }
        Layout(listOf(headline, paused, actions, why), Modifier.padding(Space.Card.dp)) { slots, c ->
            val loose = c.copy(minWidth = 0, minHeight = 0, maxHeight = Constraints.Infinity)
            val head = slots[0].first().measure(loose)
            val acts = slots[2].first().measure(loose)
            // What the cap leaves after the parts that always show whole; the paused
            // line takes it first (half of it while the "?" panel is open), the panel
            // the rest. A part with no room left is not shown at all.
            var room = capPx - 2 * padPx - head.height - gapPx - acts.height
            val whyM = slots[3].firstOrNull()
            fun flex(m: Measurable?, share: Int): Placeable? =
                if (m == null || share <= gapPx) null else m.measure(loose.copy(maxHeight = share - gapPx))
            val p = flex(slots[1].firstOrNull(), if (whyM != null) room / 2 else room)
            room -= p?.let { it.height + gapPx } ?: 0
            val w = flex(whyM, room)
            val parts = listOfNotNull(head, p, acts, w)
            val width = if (c.hasBoundedWidth) c.maxWidth else parts.maxOf { it.width }
            val height = parts.sumOf { it.height } + gapPx * (parts.size - 1)
            layout(width, height.coerceIn(c.minHeight, c.maxHeight)) {
                var y = 0
                for (part in parts) {
                    part.placeRelative(0, y)
                    y += part.height + gapPx
                }
            }
        }
    }
}

/**
 * The evidence behind the hold ([holdEvidence]) under a hold card's open "?":
 * grade + citation, no confidence meter. Without any, a plain line says what
 * kind of hold it is ([profileHold]: a profile answer, else a reported signal).
 */
@Composable
private fun HoldEvidencePanel(model: ViewModel, profileHold: Boolean, strong: Color, muted: Color) {
    val hold = remember(model.today_headline, model.adjustments) { holdEvidence(model) }
    if (hold != null) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(hold.summary, color = strong, style = Type.Caption)
            gradeLabel(hold.evidence.grade)?.let { gradeText ->
                Text(
                    stringResource(Res.string.main_evidence_prefix, gradeText),
                    color = muted,
                    style = Type.Caption,
                )
            }
            if (hold.evidence.citation.isNotBlank()) {
                Text(
                    citationLabel(hold.evidence.citation),
                    color = muted,
                    style = Type.Caption,
                )
            }
        }
    } else {
        Text(
            if (profileHold) {
                stringResource(Res.string.main_medical_hold_body)
            } else {
                stringResource(Res.string.main_reported_hold_body)
            },
            color = muted,
            style = Type.Caption,
        )
    }
}

/**
 * The under-18 plan pause (hold_reason "youth"), Today's first item rather than
 * the pinned DO-NOT-TRAIN banner: nothing here asks the user to stop moving
 * (the weekly activity card below asks for 60 active minutes a day), so it is a
 * calm `Info` [StatusCard] (accent ring, info mark, no pulse), and it scrolls
 * with the page instead of taking the viewport at a large font scale. It states
 * the reason, what still works, "Check age" (Profile's Age row via
 * [onOpenProfile]), the coach summary share, and the hold's evidence behind its "?".
 */
@Composable
internal fun YouthHoldCard(model: ViewModel, onOpenProfile: (ProfileRowId) -> Unit) {
    val ctx = currentPlatformContext()
    var whyOpen by rememberSaveable { mutableStateOf(false) }
    StatusCard(
        tone = StatusTone.Info,
        overline = stringResource(Res.string.main_hold_youth_title),
        trailing = {
            DisclosureButton(whyOpen, subject = stringResource(Res.string.main_hold_youth_subject)) { whyOpen = !whyOpen }
        },
    ) {
        Text(stringResource(Res.string.main_hold_reason_youth), color = OnBgBody, style = Type.Body)
        Text(stringResource(Res.string.main_hold_paused_youth), color = OnBgMuted, style = Type.Caption)
        if (whyOpen) HoldEvidencePanel(model, profileHold = true, strong = OnBgBody, muted = OnBgMuted)
        Column(Modifier.offset(x = -TextActionInset)) {
            TextAction(
                stringResource(Res.string.main_hold_check_age),
                onClick = { onOpenProfile(holdProfileRow(model.hold_reason, model.profile?.health)) },
                chevron = true,
            )
            TextAction(
                stringResource(Res.string.coach_export_action),
                onClick = { shareCoachSummary(ctx, model) },
                chevron = true,
            )
        }
    }
}

private const val PulseHalfPeriodMs = 1200
private const val PulseAlphaMin = 0.15f
private const val PulseAlphaSpan = 0.4f

/** Share of the screen height past which the pinned DO-NOT-TRAIN banner adds no
 *  more than its hold statement (its extra text scrolls inside it instead). */
private const val BannerMaxScreenFraction = 0.5f

internal val BannerTitleStyle = androidx.compose.ui.text.TextStyle(
    fontSize = 23.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.Black, textDirection = AppTextDirection,
)

@Composable
private fun BannerSolidAction(label: String, onFill: Color, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(ActionShape)
            .background(OnStatusFill)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = onFill, style = Type.BodyBold)
    }
}

/**
 * Confirm for the core's resolve actions ("I'm pain-free now" / "I've recovered"
 * / "I'm better"), then [holdResolutionEvents]. Resolving keeps the reports in
 * history, so the confirm is Accent, not danger red. Renders nothing while
 * [resolution] is null.
 */
@Composable
internal fun HoldResolveDialog(
    resolution: HoldResolutionView?,
    model: ViewModel,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
) {
    if (resolution == null) return
    ClearConfirmDialog(
        title = resolution.action_label,
        message = resolution.confirm_text,
        confirmLabel = stringResource(Res.string.main_resolve_hold_confirm),
        confirmColor = Accent,
        onDismiss = onDismiss,
        onClear = { holdResolutionEvents(resolution, model.profile, model.today(), AppClock.nowSec()).forEach(onEvent) },
    )
}

internal class HoldEvidence(
    val summary: String,
    val evidence: EvidenceTagView,
    val why: WhyView,
    val glossary: List<GlossarySpan>,
    val subject: String,
    val prose: Boolean,
)

/** [HoldEvidence] of the headline's safety-hold rung (it also covers a gates-only
 *  medical referral, which emits no adjustment row), else of the top
 *  safety-critical adjustment. */
internal fun holdEvidence(model: ViewModel): HoldEvidence? =
    model.today_headline.takeIf { it.kind == HeadlineKind.SafetyHold && it.summary.isNotBlank() }
        ?.let { HoldEvidence(it.summary, it.evidence, it.why, it.glossary, it.subject, it.prose) }
        ?: dominantSafetyAdjustment(model)?.let { HoldEvidence(it.summary, it.evidence, it.why, it.glossary, it.subject, it.prose) }

/**
 * What confirming [resolution] sends: [Event.ResolveHold] dated [nowSec] (every
 * report of the signal retracted, history kept), then, when the core says the
 * same tap ends the profile's "I'm ill right now" answer, that answer switched
 * off from [today]. One confirm ends every illness hold the user reported.
 */
internal fun holdResolutionEvents(
    resolution: HoldResolutionView,
    profile: Profile?,
    today: Long,
    nowSec: Long,
): List<Event> = buildList {
    add(Event.ResolveHold(resolution.signal, nowSec))
    if (resolution.ends_profile_illness && profile != null) {
        val draft = Event.SetProfile.from(profile)
        add(draft.copy(health = draft.health.copy(acute_illness = false)).effectiveFrom(today))
    }
}


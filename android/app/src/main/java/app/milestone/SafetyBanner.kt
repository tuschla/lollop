package app.milestone

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/**
 * The banner's one way back for the active hold, from the core's `hold_reason`,
 * else the legacy safety tier: a mistaken pain or illness report is removed
 * (only that report), a self-reported illness on the profile ends at the core's
 * one illness action ("I'm better", which also resolves any illness report), an
 * age hold points at the age row, a health-answer hold at the health answers,
 * anything else keeps the guarded clear of readiness inputs.
 */
internal enum class HoldAction { RemovePain, RemoveIllness, EndIllness, CheckAge, ReviewHealth, ClearReadiness }

/** Profile health answers that hold training until a professional's OK. */
private val ProfessionalHoldReasons = setOf("parq", "pregnancy", "reds", "injury_rehab", "symptomatic")

/** [HoldAction] for the core's `hold_reason` (snake_case) or, without one, its tier. */
internal fun holdAction(holdReason: String?, tier: String?): HoldAction = when (holdReason) {
    "pain" -> HoldAction.RemovePain
    "illness" -> HoldAction.RemoveIllness
    "acute_illness_profile" -> HoldAction.EndIllness
    "youth" -> HoldAction.CheckAge
    in ProfessionalHoldReasons -> HoldAction.ReviewHealth
    "medical_signal" -> HoldAction.ClearReadiness
    else -> when (tier) {
        "Pain" -> HoldAction.RemovePain
        "Illness" -> HoldAction.RemoveIllness
        "MedicalReferral" -> HoldAction.ReviewHealth
        else -> HoldAction.ClearReadiness
    }
}

/**
 * The Profile row a hold banner's "Check age" / "Review health answers" lands on:
 * the answer that raised the hold, never a list summary that reads "None apply"
 * while the cause sits elsewhere. [health] (the profile's answers) picks between
 * rows of one reason; a reason without its own row opens the health questions.
 */
internal fun holdProfileRow(holdReason: String?, health: HealthScreen?): ProfileRowId = when (holdReason) {
    "youth" -> ProfileRowId.Age
    "pregnancy" -> if (health?.pregnancy_warning_sign == true) ProfileRowId.PregnancyWarning else ProfileRowId.Pregnant
    "symptomatic" -> ProfileRowId.ParqSymptom
    "injury_rehab" -> ProfileRowId.ParqInjury
    "parq" -> ProfileRowId.ParqClearance
    // A RED-S hold may come from a weekly check-in answer, not the profile row.
    "reds" -> if (health?.reds_signal == true) ProfileRowId.Reds else ProfileRowId.HealthQuestions
    "acute_illness_profile" -> ProfileRowId.Ill
    else -> ProfileRowId.HealthQuestions
}

/** The hold is stated in full where it renders (its reason and its one way back),
 *  so Today adds no SAFETY card repeating it: a pain or illness report, the
 *  profile's "I'm ill right now" answer, a pregnancy warning sign (the core's
 *  first gate, so its card is always the warning the banner names), or the
 *  under-18 pause ([YouthHoldCard] carries its reason and evidence). A
 *  plain pregnancy's card carries the reference the banner leaves out, and the
 *  other health answers' cards name the answer behind the banner's generic line. */
internal fun bannerStatesHold(holdReason: String?, tier: String?, pregnancyWarning: Boolean): Boolean =
    tier == "Pain" || tier == "Illness" ||
        holdAction(holdReason, tier).let { it == HoldAction.EndIllness || it == HoldAction.CheckAge } ||
        (holdReason == "pregnancy" && pregnancyWarning)

/** The banner's plain reason line ("You reported pain today."), never engine
 *  vocabulary like "red flag" or "tier"; an unknown reason gets the generic line.
 *  A pregnancy reads like its own Profile row; [pregnancyWarning] (a warning
 *  sign is on) says to stop and seek care. */
private fun holdReasonLineRes(holdReason: String?, tier: String?, pregnancyWarning: Boolean): Int = when (holdReason) {
    "pain" -> R.string.main_hold_reason_pain
    "illness" -> R.string.main_hold_reason_illness
    "acute_illness_profile" -> R.string.main_hold_reason_ill_profile
    "youth" -> R.string.main_hold_reason_youth
    "pregnancy" -> if (pregnancyWarning) R.string.main_hold_reason_pregnancy_warning else R.string.main_hold_reason_pregnancy
    in ProfessionalHoldReasons -> R.string.main_hold_reason_professional
    "medical_signal" -> R.string.main_hold_reason_medical_signal
    else -> when (tier) {
        "MedicalReferral" -> R.string.main_hold_reason_professional
        "Pain" -> R.string.main_hold_reason_pain
        "Illness" -> R.string.main_hold_reason_illness
        "ObjectivePerformance" -> R.string.main_hold_reason_performance
        "SubjectiveMultiDay" -> R.string.main_hold_reason_checkins
        "HrvTrend" -> R.string.main_hold_reason_hrv
        "SingleDayMarker" -> R.string.main_hold_reason_single_day
        else -> R.string.main_hold_reason_generic
    }
}

/**
 * The DO-NOT-TRAIN banner with everything its actions need: the confirm dialogs
 * for removing a mistaken report / clearing readiness inputs and the confirm for
 * the core's resolve actions ("I'm pain-free now" / "I've recovered" / "I'm
 * better"). One host for every screen that pins the banner (Today's scaffold and
 * the run tracker), so the safety paths never drift apart. The under-18 pause is
 * not pinned: it is Today's calm [YouthHoldCard], so the host renders nothing for it.
 * [onAddDetails] opens the readiness editor and [onOpenProfile] opens Profile at
 * a row; where a screen has no such flow (null) the banner states the way back
 * as text instead.
 */
@Composable
internal fun SafetyBannerHost(
    model: ViewModel,
    onEvent: (Event) -> Unit,
    modifier: Modifier = Modifier,
    lastPain: PainDetail? = null,
    onAddDetails: (() -> Unit)? = null,
    onOpenProfile: ((ProfileRowId) -> Unit)? = null,
) {
    var confirmReadiness by rememberSaveable { mutableStateOf(false) }
    var removeReport by rememberSaveable { mutableStateOf<ReadinessSignal?>(null) }
    var resolveHoldSignal by rememberSaveable { mutableStateOf<String?>(null) }
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
        holdDetail = painSubline(model, lastPain),
        action = action,
        onAction = onAction,
        onAddDetails = onAddDetails,
        onResolveHold = { resolveHoldSignal = it.signal },
    )
    ClearReadinessDialog(
        visible = confirmReadiness,
        onDismiss = { confirmReadiness = false },
        onEvent = onEvent,
    )
    RemoveReportDialog(signal = removeReport, onDismiss = { removeReport = null }, onEvent = onEvent, model = model, lastPain = lastPain)
    HoldResolveDialog(
        resolution = resolveHoldSignal?.let { sig -> model.hold_resolutions.firstOrNull { it.signal == sig } },
        model = model,
        onDismiss = { resolveHoldSignal = null },
        onEvent = onEvent,
    )
}

/** Confirm for "Logged by mistake? Clear readiness inputs…": every readiness input
 *  and the adjustments (and hold) they produced go. */
@Composable
private fun ClearReadinessDialog(visible: Boolean, onDismiss: () -> Unit, onEvent: (Event) -> Unit) {
    ClearConfirmDialog(
        visible = visible,
        title = stringResource(R.string.main_clear_readiness_title),
        message = stringResource(R.string.main_clear_readiness_message),
        confirmLabel = stringResource(R.string.main_clear),
        onDismiss = onDismiss,
        onClear = { onEvent(Event.ClearReadiness) },
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
    // The live model (whether the report holds training, the illness level) and
    // the pain report as entered here: they word the dialog and rebuild the
    // report for "Report removed · Undo". Without them there is no Undo.
    model: ViewModel? = null,
    lastPain: PainDetail? = null,
) {
    if (signal == null) return
    val illness = signal == ReadinessSignal.Illness
    val removed = stringResource(R.string.main_report_removed)
    val undo = stringResource(R.string.log_undo)
    val holds = model?.train_blocked ?: true
    ClearConfirmDialog(
        visible = true,
        title = stringResource(if (illness) R.string.main_remove_illness_title else R.string.main_remove_pain_title),
        message = stringResource(if (holds) R.string.main_remove_report_message_hold else R.string.main_remove_report_message),
        confirmLabel = stringResource(R.string.main_delete),
        onDismiss = onDismiss,
        onClear = {
            val row = model?.readiness_summary?.firstOrNull { it.signal == signal.name }
            val restore: Event? = when {
                illness && row != null -> Event.SubmitReadiness(signal, row.value, AppClock.nowSec())
                !illness && lastPain != null -> Event.SubmitReadiness(signal, 1.0, AppClock.nowSec(), pain = lastPain)
                else -> null
            }
            onEvent(Event.RemoveReadiness(signal, observedAt = 0L))
            UiMessages.post(
                if (restore != null) {
                    UiMessage(removed, actionLabel = undo, onAction = { EventDispatcher.send(restore) })
                } else {
                    UiMessage(removed)
                },
            )
        },
    )
}

/**
 * DO-NOT-TRAIN banner (01-today §C, INVARIANT 3). Pinned above content on every
 * screen; never scrollable, never dismissable. Renders ONLY when the core
 * actually blocks training (`train_blocked`), a downgrade-class adjustment
 * (HRV/wellness dip) is NOT a hold and renders as an amber inline card on Today,
 * never here; nor is the under-18 pause ([YouthHoldCard]), which asks for no
 * stop and sits above the weekly activity it encourages. Safety is a rule, not a
 * probability: the "?" disclosure
 * shows grade + citation but NO confidence meter. `holdDetail` is the
 * body-part/character sub-line for a characterized Pain report. The reason line
 * and the one way back follow [action]; the core's resolve actions ("I'm
 * pain-free now" / "I've recovered") and "I'm better" are the solid white
 * buttons, then "Add details" and the undo / resolve path each on its own line,
 * with the "?" at the end of that block (or beside the last solid button when
 * there are no text actions). Captions on the danger fill use
 * [DangerOn] (AA at caption size; translucent white is not).
 */
@Composable
private fun SafetyBanner(
    model: ViewModel,
    modifier: Modifier,
    holdDetail: String?,
    action: HoldAction,
    onAction: (() -> Unit)?,
    onAddDetails: (() -> Unit)?,
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
            color = Color.White.copy(alpha = PulseAlphaMin + pulse.value * PulseAlphaSpan),
            cornerRadius = CornerRadius(cornerPx, cornerPx),
            style = Stroke(width = strokePx),
        )
    }
    var whyOpen by rememberSaveable { mutableStateOf(false) }
    // A profile health answer (or a reported warning sign) is not a readiness
    // report, so the "?" fallback copy must not call it one.
    val profileHold = action == HoldAction.ReviewHealth ||
        action == HoldAction.EndIllness || model.hold_reason == "medical_signal"
    Card(
        colors = CardDefaults.cardColors(containerColor = bg),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
        shape = RoundedCornerShape(Space.Card.dp),
        modifier = modifier.fillMaxWidth().then(pulseModifier),
    ) {
        Column(Modifier.padding(Space.Card.dp), verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
            ) {
                Icon(
                    painterResource(R.drawable.ic_safety_warning_triangle),
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(26.dp),
                )
                Text(
                    stringResource(R.string.main_do_not_train),
                    color = Color.White,
                    style = Type.BannerTitle,
                )
            }
            // Sub-line: the characterized pain detail when we have it ("right
            // knee · sharp · 6/10"), else the reason the hold applies.
            val pregnancyWarning = model.profile?.health?.pregnancy_warning_sign == true
            val subLine = holdDetail?.takeIf { action == HoldAction.RemovePain }
                ?.let { stringResource(R.string.main_pain_prefix, it) }
                ?: stringResource(holdReasonLineRes(model.hold_reason, tier, pregnancyWarning))
            // The core's plain "why it's paused and how it clears" line replaces
            // the generic caption when a resolvable hold is active (an illness
            // always has one); otherwise the caption names this hold's own way back
            // (only a PAR-Q-style answer is "cleared", a pregnancy pauses plans
            // while its answer is on).
            val resolutions = model.hold_resolutions
            val pausedText = resolutions.firstOrNull { it.paused_text.isNotBlank() }?.paused_text
            Text(subLine, color = DangerOn, style = Type.Body)
            Text(
                pausedText ?: stringResource(
                    if (model.hold_reason == "pregnancy" && !pregnancyWarning) {
                        R.string.main_hold_paused_pregnancy
                    } else {
                        R.string.main_programming_paused
                    },
                ),
                color = DangerOn,
                style = Type.Caption,
            )
            val reportHold = action == HoldAction.RemovePain || action == HoldAction.RemoveIllness ||
                action == HoldAction.ClearReadiness
            val showAddDetails = onAddDetails != null && reportHold
            val undoLabel = when (action) {
                HoldAction.RemovePain, HoldAction.RemoveIllness -> stringResource(R.string.main_remove_report_action)
                HoldAction.ClearReadiness -> stringResource(R.string.main_logged_by_mistake)
                HoldAction.ReviewHealth -> stringResource(R.string.main_hold_review_health)
                HoldAction.EndIllness, HoldAction.CheckAge -> null
            }
            val opensProfile = action == HoldAction.ReviewHealth
            val hasTextActions = showAddDetails || opensProfile || (undoLabel != null && onAction != null)
            // The way back to training: the core's resolve actions (history kept;
            // one illness action ends every illness hold). Solid white, the
            // banner's primary actions.
            val solidActions = resolutions.filter { it.action_label.isNotBlank() }
                .map { r -> r.action_label to { onResolveHold(r) } }
            val disclosureSubject = stringResource(R.string.main_do_not_train_subject)
            val disclosure: @Composable () -> Unit = {
                DisclosureButton(whyOpen, tint = Color.White, onTint = bg, subject = disclosureSubject) { whyOpen = !whyOpen }
            }
            solidActions.forEachIndexed { i, (label, onClick) ->
                if (!hasTextActions && i == solidActions.lastIndex) {
                    // No text actions below: the "?" shares the last action's row
                    // instead of spending a row of its own.
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
                        if (showAddDetails) {
                            TextAction(stringResource(R.string.main_add_details), onClick = onAddDetails, tint = Color.White)
                        }
                        if (undoLabel != null && onAction != null) {
                            TextAction(undoLabel, onClick = onAction, tint = Color.White, chevron = opensProfile)
                        } else if (opensProfile) {
                            // No Profile flow on this screen (the tracker): the way back as text.
                            Text(
                                stringResource(R.string.main_hold_review_health_hint),
                                color = DangerOn,
                                style = Type.Caption,
                                modifier = Modifier.padding(start = TextActionInset, top = Space.Md.dp),
                            )
                        }
                    }
                    disclosure()
                }
            }
            if (whyOpen) HoldEvidencePanel(model, profileHold, strong = Color.White, muted = DangerOn)
        }
    }
}

/**
 * The evidence behind the hold, verbatim from the core (never invented here),
 * under a hold card's open "?". Prefers the headline's "safety_hold" rung:
 * build_headline resolves the DOMINANT hold source including gates-only medical
 * referrals, which emit NO adjustment row; falls back to the safety-critical
 * adjustment. Safety is a rule, not a probability: grade + citation only, NO
 * confidence meter. Without either, a plain line says what kind of hold it is
 * ([profileHold]: a profile answer, else a reported signal). [strong] tints the
 * summary, [muted] the rest.
 */
@Composable
private fun HoldEvidencePanel(model: ViewModel, profileHold: Boolean, strong: Color, muted: Color) {
    val hold = model.today_headline?.takeIf {
        it.kind == "safety_hold" && it.summary.isNotBlank()
    }
    val adj = remember(model.adjustments) { dominantSafetyAdjustment(model) }
    val holdSummary = hold?.summary ?: adj?.summary
    val holdGrade = hold?.grade ?: adj?.grade
    val holdCitation = hold?.citation ?: adj?.citation
    if (holdSummary != null) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(holdSummary, color = strong, style = Type.Caption)
            gradeLabel(holdGrade ?: "")?.let { gradeText ->
                Text(
                    stringResource(R.string.main_evidence_prefix, gradeText),
                    color = muted,
                    style = Type.Caption,
                )
            }
            if (!holdCitation.isNullOrBlank()) {
                Text(
                    citationLabel(holdCitation),
                    color = muted,
                    style = Type.Caption,
                )
            }
        }
    } else {
        Text(
            if (profileHold) {
                stringResource(R.string.main_medical_hold_body)
            } else {
                stringResource(R.string.main_reported_hold_body)
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
    val ctx = LocalContext.current
    var whyOpen by rememberSaveable { mutableStateOf(false) }
    StatusCard(
        tone = StatusTone.Info,
        overline = stringResource(R.string.main_hold_youth_title),
        trailing = {
            DisclosureButton(whyOpen, subject = stringResource(R.string.main_hold_youth_subject)) { whyOpen = !whyOpen }
        },
    ) {
        Text(stringResource(R.string.main_hold_reason_youth), color = OnBgBody, style = Type.Body)
        Text(stringResource(R.string.main_hold_paused_youth), color = OnBgMuted, style = Type.Caption)
        if (whyOpen) HoldEvidencePanel(model, profileHold = true, strong = OnBgBody, muted = OnBgMuted)
        Column(Modifier.offset(x = -TextActionInset)) {
            TextAction(
                stringResource(R.string.main_hold_check_age),
                onClick = { onOpenProfile(holdProfileRow(model.hold_reason, model.profile?.health)) },
                chevron = true,
            )
            TextAction(
                stringResource(R.string.coach_export_action),
                onClick = { shareCoachSummary(ctx, model) },
                chevron = true,
            )
        }
    }
}

/** The banner's pulse ring: one fade from [PulseAlphaMin] to
 *  [PulseAlphaMin] + [PulseAlphaSpan] white takes [PulseHalfPeriodMs], then reverses. */
private const val PulseHalfPeriodMs = 1200
private const val PulseAlphaMin = 0.15f
private const val PulseAlphaSpan = 0.4f

/** A solid white full-width action on the danger banner (its primary actions). */
@Composable
private fun BannerSolidAction(label: String, onFill: Color, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(ActionShape)
            .background(Color.White)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Md.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = onFill, style = Type.Body.copy(fontWeight = FontWeight.Bold))
    }
}

/**
 * Confirm for the core's resolve actions ("I'm pain-free now" / "I've recovered"
 * / "I'm better"): its own action label + confirm copy, then
 * [holdResolutionEvents]. Resolving is not destructive (the reports stay in
 * history), so the confirm is the Accent action, not the danger-red clear
 * dialog. Renders nothing while [resolution] is null.
 */
@Composable
internal fun HoldResolveDialog(
    resolution: HoldResolutionView?,
    model: ViewModel,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
) {
    if (resolution == null) return
    AlertDialog(
        onDismissRequest = onDismiss,
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(resolution.action_label) },
        text = { Text(resolution.confirm_text) },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                holdResolutionEvents(resolution, model.profile, model.today(), AppClock.nowSec()).forEach(onEvent)
            }) { Text(stringResource(R.string.main_resolve_hold_confirm), color = Accent) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.main_cancel)) }
        },
    )
}

/**
 * What confirming [resolution] sends: [Event.ResolveHold] dated [nowSec] (every
 * report of the signal retracted, history kept), then, when the core says the
 * same tap ends the profile's "I'm ill right now" answer, that answer switched
 * off from [today]. One confirm ends every illness hold the user reported.
 */
internal fun holdResolutionEvents(
    resolution: HoldResolutionView,
    profile: ProfileView?,
    today: Long,
    nowSec: Long,
): List<Event> {
    val signal = runCatching { ReadinessSignal.valueOf(resolution.signal) }.getOrNull() ?: return emptyList()
    return buildList {
        add(Event.ResolveHold(signal, nowSec))
        if (resolution.ends_profile_illness && profile != null) {
            val draft = ProfileDraft.from(profile)
            add(draft.copy(health = draft.health.copy(acute_illness = false)).toEvent(today))
        }
    }
}

/**
 * Sub-line for a pain hold's banner: prefer the core's own `detail` on the Pain
 * readiness row (populated when the core surfaces the characterized report),
 * else fall back to the shell echo of what the user just entered in triage.
 * Returns null when there's no characterized pain (bare/legacy report): the
 * banner then shows the generic tier line.
 */
// l10n: needs context plumbing (pure display helper, no Context; builds a joined phrase)
internal fun painSubline(model: ViewModel, echo: PainDetail?): String? {
    val coreDetail = model.readiness_summary.firstOrNull { it.signal == "Pain" }?.detail
    if (!coreDetail.isNullOrBlank()) return coreDetail
    if (echo == null) return null
    val parts = buildList {
        echo.location?.let { add(it) }
        add(painKindLabel(echo.kind))
        echo.severity?.let { add("$it/10") }
        if (echo.trend == PainTrend.Rising) add("worsening")
    }
    return parts.joinToString(" · ")
}

/** Plain-language label for a [PainKind]. Display-only. */
// l10n: needs context plumbing (pure display helper, no Context; multi-branch copy)
private fun painKindLabel(kind: PainKind): String = when (kind) {
    PainKind.SharpJoint -> "sharp/joint"
    PainKind.TendonLoadRelated -> "tendon-like"
    PainKind.Doms -> "muscle soreness"
    PainKind.Other -> "not sure"
}

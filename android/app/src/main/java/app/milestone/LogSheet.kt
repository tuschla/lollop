package app.milestone

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

/**
 * Body of the Log [ModalBottomSheet] (05-log): the chooser list first, then the
 * picked editor. Chooser order: the "Today's plan" fast-path, Track run (GPS)
 * on top so it is never below the fold, the everyday entries, the danger-styled
 * Report pain right after them (still on screen at sheet open), the weekly
 * check-in, then one "More" row folding the rarer entries. Each editor shows a
 * close · title row with its primary action pinned at the bottom; closing an
 * editor returns to the chooser (with a discard confirm when dirty).
 */
@Composable
internal fun LogSheetContent(
    mode: LogMode,
    model: ViewModel,
    initialReadinessSignal: ReadinessSignal,
    onMode: (LogMode) -> Unit,
    onEvent: (Event) -> Unit,
    onTrackRun: () -> Unit,
    onDismiss: () -> Unit,
    // The shown editor reports whether it has unsaved edits so the host's
    // swipe-down dismissal can guard it. Default no-op keeps other callers simple.
    onDirtyChange: (Boolean) -> Unit = {},
    // Coach prescription to prefill the Set editor from (NEW mode only); null
    // opens blank. `onPlanPrefill` records which exercise the "Today's plan"
    // fast-path picked so the host can resolve it against the live model.
    planPrefill: PrescriptionView? = null,
    onPlanPrefill: (String) -> Unit = {},
    // Seed the Set/Run editor's stamp to a PAST plan day's local noon; null =
    // today's flow. The editors' date chips stay user-editable.
    planObservedAt: Long? = null,
    // The Set editor's loop locks the sheet against swipe / scrim dismissal.
    onSheetLockChange: (Boolean) -> Unit = {},
    // The loop's "Done for now" / "Finish session": closes the sheet like
    // [onDismiss]; the host also returns Today to its top.
    onFinishSession: () -> Unit = onDismiss,
) {
    val ctx = LocalContext.current
    // Weight-unit choice for the set editor's keypad + load faces.
    val weightUnit = model.weightUnit()
    // Run import: pick one or MANY GPX/TCX/FIT files (history backfill is one round
    // trip, not one per run). The shared [RunImport] pipeline parses shell-side and
    // hands the core the SAME LogRunTrack event a live-tracked save uses; it runs
    // process-scoped and toasts a batch summary, so the sheet closes right away.
    val importGpx = rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.GetMultipleContents(),
    ) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        onDismiss()
        RunImport.importRuns(ctx, uris)
    }
    // Every editor pins its primary action to a bottom bar (thumb reach) and
    // scrolls its own body; the chooser scrolls as a whole.
    val pinnedEditor = mode != LogMode.Chooser
    val distanceUnit = rememberDistanceUnit()
    val scroll = rememberScrollState()
    // A calisthenics rung picked in the chooser: the Set editor opens on it.
    // Cleared by the plain "Log set" row so a blank entry stays blank.
    var ladderPick by rememberSaveable { mutableStateOf<String?>(null) }
    // The chooser's "More" group (rarer entries); closed at every sheet open.
    var moreOpen by rememberSaveable { mutableStateOf(false) }
    // The chooser's calisthenics movements: every main ladder, plus a skill or
    // power ladder once the coach has placed the athlete on it. Each shows its
    // current rung; core order keeps one movement pattern together.
    val ladderRows = remember(model.calisthenics_ladders) {
        model.calisthenics_ladders
            .filter { (!it.skill && !it.power) || it.current_rung != null }
            .groupBy { it.pattern }
            .flatMap { (pattern, group) ->
                group.mapNotNull { l ->
                    l.rungs.getOrNull(l.current_rung ?: 0)?.takeIf { it.available }?.let { pattern to it }
                }
            }
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            // The exercise search field is the one IME-summoning field in the
            // flow; grow the bottom inset with the keyboard so it can't cover
            // the form (the keypad editors never open the IME).
            .imePadding()
            // Every form, the pinned action and the chooser's scroll viewport end
            // above the system nav bar (applied outside the scroll, so the last
            // chooser tiles never scroll under it at a large font scale).
            .navigationBarsPadding()
            // The tallest form exceeds a ModalBottomSheet's max height; without a
            // scroll the fields below the fold would be unreachable.
            .then(if (pinnedEditor) Modifier else Modifier.verticalScroll(scroll))
            .padding(horizontal = Space.Sheet.dp)
            .padding(bottom = Space.Lg.dp),
        verticalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
    ) {
      CompositionLocalProvider(LocalEditorPinnedActions provides pinnedEditor) {
        when (mode) {
            LogMode.Chooser -> {
                // Modality-gate the lift/run entries so a run-only profile isn't
                // offered "Log set" (and vice-versa). A not-yet-set-up user
                // (profile == null) still sees everything. Pain / check-ins /
                // review / raw signal are modality-agnostic and always shown.
                val liftVisible = model.profile == null || model.showLifting()
                val runVisible = model.profile == null || model.showRunning()
                // "Today's plan" fast-path: the highlighted way to start today's
                // prescribed session (the hero carries no start action). Only a
                // trainable session dated today, with items, reaches here; once
                // it's logged the core advances next_session off today so the
                // row disappears.
                val ns = model.next_session
                if (ns != null && ns.epoch_day == model.today() && ns.items.isNotEmpty() &&
                    (ns.status == "next" || ns.status == "adjusted")
                ) {
                    val nsDiscipline = sessionDiscipline(ns.session_type)
                    val item = firstOpenItem(ns.items) ?: ns.items.first()
                    LogOptionRow(
                        stringResource(R.string.main_todays_plan, ns.title),
                        // A run's distance reads in the profile's unit like every other surface.
                        glueFigures(if (nsDiscipline == "Run" && item.run_head.isNotBlank()) runHeadText(item, distanceUnit) else planRowSubtitle(item)),
                        if (nsDiscipline == "Run") R.drawable.ic_content_run else R.drawable.ic_content_set_dumbbell,
                        LogRowTone.Plan,
                    ) {
                        if (nsDiscipline == "Run") {
                            onTrackRun(); onDismiss()
                        } else {
                            onPlanPrefill(item.exercise)
                            onMode(LogMode.Set)
                        }
                    }
                } else {
                    // After Finish the core moves next_session off today; today's own
                    // lift session, once something of it is logged, can be resumed
                    // (its open item, else its last one for an extra set).
                    val today = model.week_plan.firstOrNull {
                        it.epoch_day == model.today() && it.items.isNotEmpty() &&
                            sessionDiscipline(it.session_type) == "Lift" &&
                            it.items.any { i -> i.sets_done > 0 || i.logged_sets.isNotEmpty() }
                    }
                    if (today != null && liftVisible && !model.train_blocked) {
                        val item = firstOpenItem(today.items) ?: today.items.last()
                        LogOptionRow(
                            stringResource(R.string.log_chooser_resume, today.title),
                            glueFigures(planRowSubtitle(item)),
                            R.drawable.ic_content_set_dumbbell,
                            LogRowTone.Plan,
                        ) {
                            onPlanPrefill(item.exercise)
                            onMode(LogMode.Set)
                        }
                    }
                }
                if (runVisible) {
                    // One of the most-used actions: first plain row, never below
                    // the fold. Location-pin icon tells it apart from Log run.
                    LogOptionRow(
                        stringResource(R.string.main_track_run_title), stringResource(R.string.main_track_run_sub),
                        R.drawable.ic_content_track_run,
                    ) {
                        onTrackRun()
                        onDismiss()
                    }
                }
                if (liftVisible) {
                    LogOptionRow(
                        stringResource(R.string.main_log_set_title), stringResource(R.string.main_log_set_sub),
                        R.drawable.ic_content_set_dumbbell,
                    ) {
                        ladderPick = null
                        onMode(LogMode.Set)
                    }
                }
                if (runVisible) {
                    LogOptionRow(
                        stringResource(R.string.main_log_run_title), stringResource(R.string.main_log_run_sub),
                        R.drawable.ic_content_run,
                    ) { onMode(LogMode.Run) }
                }
                // Under 18: other active time for the weekly activity card.
                if (model.youth_activity != null) {
                    LogOptionRow(
                        stringResource(R.string.activity_editor_title), stringResource(R.string.activity_chooser_sub),
                        R.drawable.ic_content_run,
                    ) { onMode(LogMode.Activity) }
                }
                // Morning check-in: the engine's main readiness input, in a fixed
                // slot; once today's is in, its subline says so, so nobody
                // double-submits or skips it by guesswork.
                val checkinDay = model.checkin_today?.observed_at?.let { localEpochDay(it, java.time.ZoneId.systemDefault()) }
                LogOptionRow(
                    stringResource(R.string.main_morning_checkin_title),
                    stringResource(
                        if (checkinDay == model.today()) R.string.main_morning_checkin_done_sub
                        else R.string.main_morning_checkin_sub,
                    ),
                    R.drawable.ic_content_readiness_heart,
                ) { onMode(LogMode.Checkin) }
                // Report pain: danger-outlined and right after the everyday rows,
                // so the safety path is on screen at sheet open without pushing
                // Track run down. Opens the triage sheet; nothing is submitted
                // until "Report pain" inside it, so an accidental tap is harmless.
                LogOptionRow(
                    stringResource(R.string.main_report_pain), stringResource(R.string.main_report_pain_sub),
                    R.drawable.ic_safety_warning_triangle,
                    LogRowTone.Danger,
                ) { onMode(LogMode.Pain) }
                LogOptionRow(
                    stringResource(R.string.main_weekly_checkin_title), stringResource(R.string.main_weekly_checkin_sub),
                    R.drawable.ic_content_checkin_calendar,
                ) { onMode(LogMode.WeeklyCheckin) }
                // The rarer entries fold behind one quiet "More" row so the
                // everyday list stays short: the session review, run import, the
                // calisthenics movement list (today's plan row already starts the
                // prescribed one) and the raw lab-signal editor.
                ExpandRow(stringResource(R.string.log_chooser_more), moreOpen) { moreOpen = !moreOpen }
                if (moreOpen) {
                    ChooserGroup {
                        ChooserTextRow(
                            stringResource(R.string.main_session_review_title),
                            stringResource(R.string.main_session_review_sub),
                        ) { onMode(LogMode.Review) }
                        if (runVisible) {
                            // Run import: a run recorded on another device/app.
                            // Mime is "*/*": providers commonly report .gpx/.tcx/.fit as
                            // octet-stream; the content sniffer rejects anything else with a
                            // clear message.
                            ChooserTextRow(
                                stringResource(R.string.main_import_run_title),
                                stringResource(R.string.main_import_run_sub),
                            ) { importGpx.launch("*/*") }
                        }
                        // Lab-data path: the raw-signal editor (enter a z-score /
                        // bpm delta directly). Secondary to the human check-in.
                        ChooserTextRow(
                            stringResource(R.string.main_advanced_signal_title),
                            stringResource(R.string.main_advanced_signal_sub),
                        ) { onMode(LogMode.Readiness) }
                    }
                    // Ladders the kit limits say why, under the movements they replace.
                    val equipmentNotes = model.calisthenics_ladders.map { it.equipment_note }.filter { it.isNotBlank() }.distinct()
                    if (liftVisible && (ladderRows.isNotEmpty() || equipmentNotes.isNotEmpty())) {
                        SectionOverline(stringResource(R.string.log_chooser_calis_header))
                        if (ladderRows.isNotEmpty()) {
                            ChooserGroup {
                                ladderRows.forEach { (pattern, rung) ->
                                    CalisRungRow(pattern, rung) {
                                        ladderPick = rung.name
                                        onMode(LogMode.Set)
                                    }
                                }
                            }
                        }
                        equipmentNotes.forEach { Text(it, color = OnBgMuted, style = Type.Caption) }
                    }
                }
            }
            LogMode.Set -> LogSetEditor(
                // Quick-picks: the user's own most recent exercises, newest
                // first (History list is oldest-first on the wire). Memoized on
                // model.lifts so the full-history scan isn't rerun per recomposition.
                recentExercises = remember(model.lifts) {
                    model.lifts.asReversed().map { it.exercise }.distinct()
                },
                // Modality-aware quick picks: today's + this week's prescribed
                // exercises; no hard-coded barbell defaults.
                plannedExercises = remember(model.next_session, model.week_plan) {
                    (model.next_session?.items.orEmpty() + model.week_plan.flatMap { it.items })
                        .map { it.exercise }
                        .filter { it.isNotEmpty() }
                        .distinct()
                },
                // Canonical names of today's + this week's prescribed timed-hold
                // rungs, so the editor logs those as holds (seconds) not reps.
                holdExercises = remember(model.next_session, model.week_plan, model.calisthenics_ladders) {
                    (
                        (model.next_session?.items.orEmpty() + model.week_plan.flatMap { it.items })
                            .filter { it.timed_hold }
                            .map { it.exercise } +
                            // A hold rung picked from the chooser.
                            model.calisthenics_ladders.flatMap { it.rungs }
                                .filter { it.timed_hold }
                                .map { it.name }
                        ).toSet()
                },
                // Prescribed pure-bodyweight rungs (added load 0, and the rung takes
                // no belt or vest): the editor hides the weight field for these — a
                // Wall push-up has no load to enter. A load-capable rung keeps its
                // added-load field even at 0.
                bodyweightExercises = remember(model.next_session, model.week_plan) {
                    (model.next_session?.items.orEmpty() + model.week_plan.flatMap { it.items })
                        .filter { it.added_kg == 0.0 && it.calis?.load_capable != true }
                        .map { it.exercise }
                        .toSet()
                },
                weightUnit = weightUnit,
                initialObservedAt = planObservedAt,
                prescription = planPrefill,
                initialExercise = ladderPick.takeIf { planPrefill == null },
                // The gym-floor loop needs today's session to compute "Set k of N"
                // and pick the next open item; only today's own flow (no back-dated
                // stamp) loops. Today's week row keeps its items once they are done,
                // unlike the hero, which moves on when the whole day completes.
                planSession = model.week_plan.firstOrNull {
                    planObservedAt == null && it.epoch_day == model.today() && it.items.isNotEmpty()
                },
                recentLifts = model.lifts,
                adjustments = model.adjustments,
                reviewAdjustments = model.review_adjustments,
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
                onSheetLockChange = onSheetLockChange,
                // Dispatch-only: a NEW today set keeps the sheet open and loops; an
                // EDIT (amend) and the planned loop's X close via onDone, the loop's
                // Finish via onFinish.
                onDone = onDismiss,
                onFinish = onFinishSession,
            ) { set -> onEvent(set) }
            LogMode.Run -> LogRunEditor(
                initialObservedAt = planObservedAt,
                prescription = planPrefill,
                // The run type of the planned run this entry logs (today's, or the
                // past plan day's), by the Finish review's rule.
                plannedType = remember(model.week_plan, planObservedAt) {
                    val day = planObservedAt?.let { localEpochDay(it, java.time.ZoneId.systemDefault()) } ?: model.today()
                    model.week_plan.firstOrNull {
                        it.epoch_day == day && sessionDiscipline(it.session_type) == "Run" &&
                            it.status != "done" && it.status != "done_lighter" && it.status != "blocked"
                    }?.let { s ->
                        val item = s.items.firstOrNull { it.run_target != null } ?: s.items.firstOrNull()
                        workoutTypeForPlan(s.session_type, item?.run_target?.band)
                    }
                },
                profile = model.profile,
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { run -> onEvent(run); onDismiss() }
            // The primary human morning check-in. The core derives
            // the z-scores; the user never enters one.
            LogMode.Checkin -> MorningCheckinSheet(
                echo = model.checkin_today,
                illnessPrompt = model.illness_check_prompt,
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { checkin, illness ->
                checkin?.let(onEvent)
                // AUTOREG-4: the illness answer is its own readiness report.
                illness?.let(onEvent)
                onDismiss()
            }
            LogMode.Readiness -> ReadinessEditor(
                // Red-flag fence position comes from the core's signal_groups.
                signalGroups = model.signal_groups.associate { it.signal to it.group },
                initialSignal = initialReadinessSignal,
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { r -> onEvent(r); onDismiss() }
            // The session review (off day + hand-logged run details) and the weekly
            // check-in are separate events with separate lifecycles.
            LogMode.Review -> SessionReviewSheet(
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { review -> onEvent(review); onDismiss() }
            LogMode.WeeklyCheckin -> WeeklyCheckinSheet(
                female = model.profile?.female,
                checkinPresent = model.weekly_checkin_present,
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
                onClear = { onEvent(Event.ClearWeeklyCheckin); onDismiss() },
            ) { checkin -> onEvent(checkin); onDismiss() }
            // Pain triage → full PainDetail, then the hold.
            LogMode.Pain -> {
                val saved = stringResource(R.string.main_pain_report_saved)
                PainTriageSheet(
                    onClose = { onMode(LogMode.Chooser) },
                    onDirtyChange = onDirtyChange,
                ) { r ->
                    onEvent(r)
                    onDismiss()
                    UiMessages.post(UiMessage(saved))
                }
            }
            LogMode.Activity -> LogActivityEditor(
                onClose = { onMode(LogMode.Chooser) },
                onDirtyChange = onDirtyChange,
            ) { activity -> onEvent(activity); onDismiss() }
        }
      }
    }
}

/** A log-chooser row's tone: the plain list surface, the Accent "Today's plan"
 *  fast-path fill, or the "Report pain" safety path (plain surface wearing
 *  [StatusTone.Danger]: its ring, tile tint and text). */
private enum class LogRowTone { Plain, Plan, Danger }

/**
 * One log-chooser row (05-log §1): 38dp icon tile, a title over a one-line
 * subtitle, trailing chevron. Full-width ≥48dp tap target; every tone shares
 * the same layout so all rows have one height. Icons are Accent on plain rows,
 * the row's text colour on the Accent fill, and the [StatusTone.Danger] colours
 * on the danger row, so it matches a Danger [StatusCard] in both modes.
 */
@Composable
private fun LogOptionRow(
    title: String,
    subtitle: String,
    iconRes: Int,
    tone: LogRowTone = LogRowTone.Plain,
    onClick: () -> Unit,
) {
    val danger = StatusTone.Danger.colors()
    val ground = if (tone == LogRowTone.Plan) Accent else BgElevated
    val titleColor = when (tone) {
        LogRowTone.Plain -> OnBgBody
        LogRowTone.Plan -> OnAccent
        LogRowTone.Danger -> danger.text
    }
    val subColor = when (tone) {
        LogRowTone.Plain -> OnBgFaint
        LogRowTone.Plan -> OnAccent
        LogRowTone.Danger -> danger.text
    }
    val outline = when (tone) {
        LogRowTone.Plain -> OnBgBody.copy(alpha = HairlineAlpha.Control)
        LogRowTone.Plan -> null
        LogRowTone.Danger -> danger.ring
    }
    val shape = RoundedCornerShape(Space.Card.dp)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(shape)
            .background(ground)
            .then(if (outline != null) Modifier.border(1.dp, outline, shape) else Modifier)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(Space.Card.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        IconTile(
            painterResource(iconRes),
            when (tone) {
                LogRowTone.Plain -> Accent
                LogRowTone.Plan -> titleColor
                LogRowTone.Danger -> danger.glyph
            },
            when (tone) {
                LogRowTone.Plain -> AccentTint
                LogRowTone.Plan -> titleColor.copy(alpha = 0.16f)
                LogRowTone.Danger -> danger.tileFill
            },
            size = 38.dp,
        )
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(title, color = titleColor, style = Type.Body.copy(fontWeight = FontWeight.Bold))
            Text(subtitle, color = subColor, style = Type.Caption)
        }
        if (tone == LogRowTone.Plain) {
            RowChevron()
        } else {
            Icon(
                painterResource(R.drawable.ic_ui_chevron_right),
                contentDescription = null,
                tint = titleColor,
                modifier = Modifier.size(16.dp),
            )
        }
    }
}

/** The plan fast-path's subline: the first open item's name and its headline
 *  prescription ("Back Squat: 4×3 @ 117.5 kg"), without the " · " qualifiers
 *  the Today card already shows, so it stays one short line. Pure. */
private fun planRowSubtitle(item: PrescriptionView): String =
    if (item.face_name.isNotBlank()) "${item.face_name}: ${item.face_lead}" else item.face_lead

/** A figure and its unit (and "@" and its load) held on one line with
 *  non-breaking spaces, so a large font scale never wraps "275 / lb". Pure. */
internal fun glueFigures(text: String): String =
    text.replace(FigureUnit, "$1\u00A0$2").replace(AtFigure, "@\u00A0$1")

private val FigureUnit = Regex("""(\d) (kg|lb|km|mi|min|s)\b""")
private val AtFigure = Regex("""@ (\d)""")

/** One calisthenics movement in the Log chooser's "More" group: the current
 *  rung's name over its movement pattern and target ("Vertical pull · 5–8 reps").
 *  A ≥48dp whole-row tap opens the set editor on that rung. */
@Composable
private fun CalisRungRow(pattern: String, rung: CalisRungView, onClick: () -> Unit) {
    ChooserTextRow(rung.name, listOf(pattern, rung.target).filter { it.isNotBlank() }.joinToString(" · "), onClick)
}

/** A compact chooser entry inside a [ChooserGroup]: a bold title over a
 *  one-line subtitle and a trailing chevron, no icon tile. Full-width ≥48dp tap. */
@Composable
private fun ChooserTextRow(title: String, subtitle: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = Space.Card.dp, vertical = Space.Sm.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Card.dp),
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            Text(title, color = OnBgBody, style = Type.Body.copy(fontWeight = FontWeight.Bold))
            Text(subtitle, color = OnBgFaint, style = Type.Caption)
        }
        RowChevron()
    }
}

/** One rounded list surface holding several [ChooserTextRow]s; clipped before
 *  the rows' clickables so their ripples stay inside the rounded shape. */
@Composable
private fun ChooserGroup(content: @Composable () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Space.Card.dp))
            .background(BgElevated),
    ) { content() }
}


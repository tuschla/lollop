package app.milestone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import java.util.Locale

/** A plan day's short date in the device locale ("Thu, Oct 15" / "Do., 15. Okt."). */
internal fun planDateLabel(epochDay: Long): String =
    java.time.LocalDate.ofEpochDay(epochDay).format(localizedDayFormatter("EEEMMMd"))

/** Two-letter weekday for the week strip (Mo Tu We Th Fr Sa Su), unambiguous
 *  where a single narrow initial collides (M/T/T, S/S). */
private fun weekdayShort(epochDay: Long): String = when (
    java.time.LocalDate.ofEpochDay(epochDay).dayOfWeek
) {
    java.time.DayOfWeek.MONDAY -> "Mo"
    java.time.DayOfWeek.TUESDAY -> "Tu"
    java.time.DayOfWeek.WEDNESDAY -> "We"
    java.time.DayOfWeek.THURSDAY -> "Th"
    java.time.DayOfWeek.FRIDAY -> "Fr"
    java.time.DayOfWeek.SATURDAY -> "Sa"
    java.time.DayOfWeek.SUNDAY -> "Su"
}

/** English weekday name (e.g. "Monday") for the week-strip accessibility
 *  description. Pinned to Locale.ENGLISH, NOT the device locale: the rest of the
 *  spoken phrase (sessionDiscipline, weekDayStatusPhrase) and the visible strip
 *  ([weekdayShort]) are hardcoded English (l10n-TODO), so a localized weekday here
 *  made TalkBack disagree with the screen. Localize all four together if l10n lands. */
private fun weekdayFull(epochDay: Long): String =
    java.time.LocalDate.ofEpochDay(epochDay).dayOfWeek
        .getDisplayName(java.time.format.TextStyle.FULL, Locale.ENGLISH)

/** Plain-language week-strip day status for the accessibility description. */
// l10n: needs context plumbing (pure display helper, no Context; multi-branch copy)
private fun weekDayStatusPhrase(status: String): String = when (status) {
    "next" -> "next session"
    "adjusted" -> "adjusted"
    "blocked" -> "on hold"
    "done" -> "done"
    "done_lighter" -> "done, lighter than planned"
    "before_plan" -> "before your plan started"
    "missed" -> "not logged"
    "partial" -> "partly done"
    "rest" -> "rest day"
    else -> "planned"
}

/** The post-illness "Return check" ([PrescriptionView.return_check]) shown as
 *  its own card in a lifting session: neither a run to track nor sets to count;
 *  any logged session today completes it. In a run session it stays a run. */
internal fun isReturnCheckItem(item: PrescriptionView, sessionType: String): Boolean =
    item.return_check && sessionDiscipline(sessionType) == "Lift"

/** The (summary, citation) of [ns]'s adjustment when the session is the return
 *  check: its item already states the dose and the stop list, so the adjustment
 *  card behind it is not shown again. Null for any other session. */
internal fun returnCheckAdjustmentKey(ns: SessionPlanView?): Pair<String, String>? =
    ns?.adjustment?.takeIf { ns.items.any { it.return_check } }?.let { it.summary to it.citation }

/**
 * Rescheduling on a session card: where a moved session came from and
 * why a requested move was refused (the core's plain sentence). Emits nothing
 * for a session with neither.
 */
@Composable
internal fun SessionMoveNotes(s: SessionPlanView) {
    s.moved_from_epoch_day?.let { from ->
        Text(stringResource(R.string.main_moved_from, weekdayFull(from)), color = OnBgMuted, style = Type.Caption)
    }
    s.move_note?.takeIf { it.isNotBlank() }?.let {
        Text(it, color = OnBgMuted, style = Type.Caption)
    }
}

/**
 * "Move" on a session's title row: a quiet muted text action (a rarely used
 * reschedule never outweighs the session), spoken "Move session: <title>". It
 * offers the core's `move_targets` (days that keep the spacing between hard
 * sessions, today included when valid) as a day picker of whole-row options and
 * sends [Event.MoveSession]. A session the core says cannot move but explains why
 * (`move_blocked_reason`) keeps "Move" and the dialog says why, so the action
 * never silently vanishes. Emits nothing when there is neither.
 */
@Composable
internal fun SessionMoveAction(s: SessionPlanView, today: Long, onMove: (Long, Long) -> Unit, modifier: Modifier = Modifier) {
    val blockedReason = s.move_blocked_reason?.takeIf { it.isNotBlank() }
    if (s.move_targets.isEmpty() && blockedReason == null) return
    var picking by rememberSaveable(s.epoch_day) { mutableStateOf(false) }
    TextAction(
        stringResource(R.string.main_move_short),
        onClick = { picking = true },
        description = stringResource(R.string.plan_secondary_action_cd, stringResource(R.string.main_move_session), s.title),
        modifier = modifier,
        tint = OnBgMuted,
    )
    if (!picking) return
    if (s.move_targets.isEmpty()) {
        AlertDialog(
            onDismissRequest = { picking = false },
            shape = RoundedCornerShape(Space.Card.dp),
            title = { Text(stringResource(R.string.main_move_blocked_title, s.title)) },
            text = { Text(blockedReason.orEmpty(), style = Type.Body) },
            confirmButton = {
                TextButton(onClick = { picking = false }) { Text(stringResource(R.string.log_ok)) }
            },
        )
        return
    }
    val move = { day: Long ->
        picking = false
        onMove(s.epoch_day, day)
    }
    val todayLabel = stringResource(R.string.main_move_today)
    AlertDialog(
        onDismissRequest = { picking = false },
        shape = RoundedCornerShape(Space.Card.dp),
        title = { Text(stringResource(R.string.main_move_session_title, s.title)) },
        text = {
            Column {
                Text(stringResource(R.string.main_move_session_body), style = Type.Body)
                Spacer(Modifier.height(Space.Sm.dp))
                // Each target day is a whole-row option with a chevron (the
                // option-list row), never a filled box that reads as an input.
                s.move_targets.forEach { day ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = ActionMinHeight)
                            .clip(RoundedCornerShape(Space.Md.dp))
                            .testTag("move-target:$day")
                            .clickable(role = Role.Button) { move(day) },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            stringResource(R.string.main_move_to_day, if (day == today) todayLabel else planDateLabel(day)),
                            color = OnBgBody,
                            style = Type.Body,
                            modifier = Modifier.weight(1f),
                        )
                        RowChevron()
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { picking = false }) { Text(stringResource(R.string.main_cancel)) }
        },
    )
}

/**
 * The one coaching note the This-week face shows: the most actionable of the
 * core's extra consistency notes. `notes[0]` is the core's done/planned summary
 * (the face states the count itself, so it is never on the face); after it the
 * core emits, in order (`build_consistency`), the repeated-plan-week note and its
 * re-entry hint (only while [ConsistencyView.repeating_week]), then the
 * missed-session note (exactly when a week row reads "missed"). The missed-session
 * move/carry-on suggestion wins, else the re-entry hint ("add your time off");
 * the "repeating this plan week" explanation never reaches the face.
 */
private fun consistencyFaceNote(c: ConsistencyView, week: List<SessionPlanView>): GuidanceView? {
    val extra = c.notes.drop(1)
    return when {
        extra.isEmpty() -> null
        week.any { it.status == "missed" } -> extra.last()
        c.repeating_week -> extra.getOrNull(1)
        else -> null
    }
}

/**
 * "This week" as ONE minimal card: one plain count line ("2 of 4 sessions
 * done"), at most ONE coaching note (the most actionable, [consistencyFaceNote]),
 * the week strip, and the plan footer (the plan summary as a compact
 * plain-language caption, never the raw "name · phase - week X of Y" internals).
 * No streak, no loss framing, no red, no unlabeled chart. ONE "?"
 * serves the whole block, on the first row: behind it sit the share of planned
 * sessions done over the recent plan weeks, every other note (e.g. why the plan
 * repeats a week) with its evidence (grade + citation + why), the plan's
 * evidence, and the quiet "Remove plan" action (confirm-gated, never an instant
 * ClearPlan). While the panel is open the face note moves into it, so no line
 * shows twice. A Hypertrophy plan adds a collapsed "Weekly sets per muscle" table
 * (core rows).
 */
@Composable
internal fun ThisWeekCard(
    consistency: ConsistencyView?,
    week: List<SessionPlanView>,
    today: Long,
    weightUnit: WeightUnit,
    program: ProgramSummaryView?,
    // One quiet closing line (the "nothing logged yet" hint), or null.
    footnote: String?,
    onRemovePlan: () -> Unit,
    onLogItem: (PrescriptionView, Long) -> Unit,
    onLogRun: (Long) -> Unit,
    onEndExercise: (PrescriptionView, Long) -> Unit,
    onReopen: (PrescriptionView, Long) -> Unit,
    onMove: (from: Long, to: Long) -> Unit,
    heroDay: Long,
    // The day open under the strip (-1 = none), hoisted so a move can open its target.
    selectedDay: Long,
    onSelectDay: (Long) -> Unit,
    readOnly: Boolean = false,
) {
    // Keyed on the plan's identity so an open "?" panel resets when the plan
    // changes (mirrors EvidenceCard's saveable keying).
    var expanded by rememberSaveable(program?.name, program?.citation) { mutableStateOf(false) }
    val notes = consistency?.notes.orEmpty()
    val faceNote = consistency?.let { consistencyFaceNote(it, week) }
    val hiddenNotes = notes.drop(1).filterNot { it === faceNote }
    val hasDisclosure = notes.isNotEmpty() || program != null
    val panel: @Composable () -> Unit = {
        if (expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.Sm.dp)) {
                // The count line's own evidence, led by the recent-weeks share.
                consistency?.share_pct?.let { pct ->
                    Text(stringResource(R.string.main_consistency_share, pct), color = OnBgBody, style = Type.Caption)
                }
                notes.firstOrNull()?.let { n ->
                    WhyDetail(
                        why = n.why,
                        grade = n.grade,
                        citation = n.citation,
                        confidence = n.confidence,
                        contested = n.contested,
                        showConfidence = true,
                        extraDetail = null,
                    )
                }
                // The face note (moved in here while open), then every note that
                // never reaches the face, each led by its own summary.
                (listOfNotNull(faceNote) + hiddenNotes).forEach { n ->
                    Text(n.summary, color = OnBgBody, style = Type.Caption.copy(fontWeight = FontWeight.Bold))
                    WhyDetail(
                        why = n.why,
                        grade = n.grade,
                        citation = n.citation,
                        confidence = n.confidence,
                        contested = n.contested,
                        showConfidence = true,
                        extraDetail = null,
                    )
                }
                program?.let { prog ->
                    WhyDetail(
                        why = prog.why,
                        grade = prog.grade,
                        citation = prog.citation,
                        confidence = prog.confidence,
                        contested = prog.contested,
                        showConfidence = true,
                        extraDetail = null,
                    )
                    // Remove plan, a destructive action, kept OFF the main face and
                    // revealed only inside the "?" disclosure.
                    Text(
                        stringResource(R.string.main_remove_plan),
                        color = OnBgMuted,
                        style = Type.Caption.copy(fontWeight = FontWeight.Bold),
                        modifier = Modifier
                            .minimumInteractiveComponentSize()
                            .clip(RoundedCornerShape(Space.Sm.dp))
                            .clickable(role = Role.Button) { onRemovePlan() }
                            .padding(vertical = Space.Sm.dp, horizontal = Space.Xs.dp),
                    )
                }
            }
        }
    }
    Box(Modifier.testTag("this-week")) {
        PlainCard {
            if (consistency != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        pluralStringResource(
                            R.plurals.main_week_done_count,
                            consistency.this_week_planned,
                            consistency.this_week_done,
                            consistency.this_week_planned,
                        ),
                        color = OnBgBody,
                        style = Type.Body,
                        modifier = Modifier.weight(1f),
                    )
                    if (hasDisclosure) {
                        DisclosureButton(expanded, subject = stringResource(R.string.main_consistency_subject)) { expanded = !expanded }
                    }
                }
                if (!expanded) {
                    faceNote?.let { Text(it.summary, color = OnBgMuted, style = Type.Caption) }
                }
                panel()
            }
            if (week.isNotEmpty()) {
                WeekStrip(
                    week,
                    today = today,
                    weightUnit = weightUnit,
                    onLogItem = onLogItem,
                    onLogRun = onLogRun,
                    onEndExercise = onEndExercise,
                    onReopen = onReopen,
                    onMove = onMove,
                    heroDay = heroDay,
                    selectedDay = selectedDay,
                    onSelectDay = onSelectDay,
                    readOnly = readOnly,
                )
            }
            // The one "nothing logged yet" hint closes the week itself, only when
            // no "0 of N sessions done" line above already says it.
            if (consistency == null) footnote?.let { Text(it, color = OnBgFaint, style = Type.Caption) }
            program?.let { prog ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        planPlainLabel(prog),
                        color = OnBgMuted,
                        style = Type.Caption,
                        modifier = Modifier.weight(1f),
                    )
                    // No count row above to carry it: the block's one "?" sits here.
                    if (consistency == null) DisclosureButton(expanded, subject = planPlainLabel(prog)) { expanded = !expanded }
                }
                if (consistency == null) panel()
                if (prog.muscle_volume.isNotEmpty()) MuscleVolumeSection(prog)
            }
        }
    }
}

/** Hypertrophy plan: the PLANNED weekly sets per muscle (what the plan delivers,
 *  not what was logged), collapsed to ONE summary line ("Every muscle on target"
 *  / "On target except rear delts"); a tap opens the two-column table (muscle
 *  left, "planned / target" right in whole sets; a muscle with no direct target
 *  reads "4 (no target)"). A planned shortfall is not lateness, so every tone is
 *  neutral: a short row is bold, and the core's note about the shortfall sits
 *  right under the first short row. Each row speaks the core's own full label.
 *  Nested in the This-week card, so its title is a card-level label. */
@Composable
private fun MuscleVolumeSection(prog: ProgramSummaryView) {
    var expanded by rememberSaveable(prog.name) { mutableStateOf(false) }
    // Whole sets on the face: fractional counting stays the core's arithmetic.
    val sets = { m: MuscleVolumeView -> Math.round(m.delivered_sets) }
    // target_sets 0 = no direct target: never "short", shown as the bare count.
    val isShort = { m: MuscleVolumeView -> m.target_sets > 0 && sets(m) < m.target_sets }
    val short = prog.muscle_volume.filter(isShort)
    val summary = if (short.isEmpty()) {
        stringResource(R.string.main_muscle_volume_all_on_target)
    } else {
        stringResource(
            R.string.main_muscle_volume_short,
            short.joinToString(", ") { it.muscle.replaceFirstChar { c -> c.lowercase(Locale.US) } },
        )
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = ActionMinHeight)
            .clip(RoundedCornerShape(Space.Md.dp))
            .clickable(role = Role.Button) { expanded = !expanded },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
            TileOverline(stringResource(R.string.main_muscle_volume_title))
            Text(summary, color = OnBgMuted, style = Type.Caption)
        }
        RowChevron(expanded)
    }
    if (!expanded) return
    val firstShort = prog.muscle_volume.indexOfFirst(isShort)
    val note = prog.muscle_volume_note.takeIf { it.isNotBlank() }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        prog.muscle_volume.forEachIndexed { i, m ->
            val isRowShort = isShort(m)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clearAndSetSemantics { contentDescription = m.label },
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(m.muscle, color = OnBgBody, style = Type.Caption, modifier = Modifier.weight(1f))
                Text(
                    if (m.target_sets > 0) {
                        "${sets(m)} / ${m.target_sets}"
                    } else {
                        stringResource(R.string.main_muscle_volume_no_target, "${sets(m)}")
                    },
                    color = when {
                        isRowShort -> OnBgBody
                        m.target_sets > 0 -> OnBgMuted
                        else -> OnBgFaint
                    },
                    style = Type.Caption.merge(TabularFigures)
                        .copy(fontWeight = if (isRowShort) FontWeight.Bold else FontWeight.Normal),
                )
            }
            if (i == firstShort && note != null) {
                Text(note, color = OnBgMuted, style = Type.Caption)
            }
        }
        if (firstShort < 0 && note != null) {
            Text(note, color = OnBgMuted, style = Type.Caption)
        }
    }
}

/**
 * A plain-language one-liner for the plan footer: "Strength · building up ·
 * week 5 of 6". Takes the descriptive tail of the core's `name` (an older core
 * raw-concats "Hybrid - strength + running"), sentence-cased, and turns the enum
 * phase word into human copy. Never touches wire values, display only.
 */
// l10n: needs context plumbing (pure display helper, no Context; multi-branch copy)
private fun planPlainLabel(prog: ProgramSummaryView): String {
    val focus = prog.name.substringAfter(" - ", prog.name).replaceFirstChar { it.titlecase(Locale.US) }
    val phaseWord = when (prog.phase) {
        "Base" -> "building a base"
        "Build" -> "building up"
        "Peak" -> "hardest stretch"
        "Taper" -> "easing off"
        "Deload" -> "lighter week"
        else -> prog.phase.lowercase(Locale.US)
    }
    return "$focus · $phaseWord · week ${prog.week} of ${prog.weeks_total}"
}

/** Height of a week-strip day's glyph slot (the ✓ or the dot). */
private val DayGlyphSlot = 16.dp

/** A week-strip day's status dot. */
private val DayDotSize = 8.dp

/** The week strip: 7 equal tappable day columns across the full card width,
 *  each showing its weekday, the day-type token (Lift/Run/Rest, from the core's
 *  session_type) and ONE status glyph: ✓ done, a filled dot for today's (or the
 *  next) session, a hollow ring for a planned or missed day, a half-filled one for
 *  a partly done day. TODAY is ringed + accent-bold. Tapping a day reveals its
 *  sessions inline below, where a swapped or lighter day says so in words. Every
 *  field is core-emitted; the "today" mark is the shell's own clock (already sent
 *  to the core via SetToday), not a fabricated value. */
@Composable
private fun WeekStrip(
    week: List<SessionPlanView>,
    today: Long,
    weightUnit: WeightUnit = WeightUnit.Kg,
    onLogItem: (PrescriptionView, Long) -> Unit = { _, _ -> },
    onLogRun: (Long) -> Unit = {},
    onEndExercise: (PrescriptionView, Long) -> Unit = { _, _ -> },
    onReopen: (PrescriptionView, Long) -> Unit = { _, _ -> },
    onMove: (from: Long, to: Long) -> Unit = { _, _ -> },
    // The day the hero card shows: its move control lives there, not twice.
    heroDay: Long = -1L,
    // The open day (-1 = none) and its setter: hoisted to Today.
    selectedDay: Long = -1L,
    onSelectDay: (Long) -> Unit = {},
    // A training hold is active: the strip shows the week but logs, moves and
    // reopens nothing.
    readOnly: Boolean = false,
) {
    val status = LocalStatusColors.current
    Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            modifier = Modifier.fillMaxWidth(),
        ) {
            week.forEachIndexed { i, s ->
                // Per-day status → dot appearance. Filled = a settled call
                // (next/done/adjusted/blocked); a hollow ring = still open
                // (planned) or open-and-past (missed). GREEN (the done mark)
                // marks a completed day, NOT hrZone1's teal, which means "easy
                // aerobic zone" everywhere else. Rest days render no dot at all
                // (handled below). "missed" wears a neutral hollow ring and
                // "partial" (some work logged, under the lighter credit) a
                // half-filled one, both neutral
                // (OnBgMuted), NOT danger red: a skipped planned day is not a
                // safety event, and red is reserved for real safety states.
                // Only "blocked" (a safety hold) keeps danger.
                val (dotColor, dotFilled) = when (s.status) {
                    "next" -> Accent to true
                    "adjusted" -> status.warn to true
                    "blocked" -> status.danger to true
                    "done", "done_lighter" -> status.done to true
                    "missed", "partial" -> OnBgMuted to false
                    "rest" -> OnBgFaint to false
                    else -> OnBgFaint to false // planned
                }
                val isToday = s.epoch_day == today
                val sel = selectedDay == s.epoch_day
                // A day before the plan started is neither planned nor missed: a
                // neutral dash, no dot, nothing to count.
                val beforePlan = s.status == "before_plan"
                val isDone = s.status == "done" || s.status == "done_lighter"
                // Done by a same-discipline session on another day ("Done Tue").
                val creditedFrom = s.credited_from_epoch_day?.takeIf { isDone && it != s.epoch_day }
                // One spoken description per day column: the weekday, its discipline,
                // and its status (the dot's fill/colour is a purely visual cue).
                val todayPrefix = stringResource(R.string.main_a11y_today_prefix)
                val dayDesc = buildString {
                    if (isToday) append(todayPrefix)
                    append(weekdayFull(s.epoch_day))
                    append(", ")
                    if (!beforePlan) {
                        append(sessionDiscipline(s.session_type))
                        append(", ")
                    }
                    append(weekDayStatusPhrase(s.status))
                    creditedFrom?.let { append(", on ").append(weekdayFull(it)) }
                }
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
                    modifier = Modifier
                        .weight(1f)
                        // ≥48dp tap target for each day column (accessibility).
                        .heightIn(min = ActionMinHeight)
                        // Clip before clickable so the ripple stays rounded.
                        .clip(RoundedCornerShape(Space.Md.dp))
                        .background(
                            if (sel) Accent.copy(alpha = 0.12f) else Color.Transparent,
                            RoundedCornerShape(Space.Md.dp),
                        )
                        // TODAY is marked distinctly with an accent ring.
                        .then(
                            if (isToday) {
                                Modifier.border(1.dp, Accent, RoundedCornerShape(Space.Md.dp))
                            } else {
                                Modifier
                            },
                        )
                        // One tab per day: selection is spoken, not only coloured.
                        .selectable(selected = sel, role = Role.Tab) { onSelectDay(if (sel) -1L else s.epoch_day) }
                        .padding(vertical = Space.Sm.dp, horizontal = Space.Xs.dp)
                        // Merge the weekday/discipline glyphs into one named, tappable
                        // node (keeps the onClick; replaces the terse child texts).
                        .semantics(mergeDescendants = true) { contentDescription = dayDesc },
                ) {
                    Text(
                        weekdayShort(s.epoch_day),
                        color = if (isToday || sel) Accent else OnBgBody,
                        style = if (isToday) Type.Chip else Type.Chip.copy(fontWeight = FontWeight.Normal),
                        maxLines = 1,
                    )
                    // Day-type/label token, from the core's session_type.
                    Text(
                        if (beforePlan) "\u2014" else sessionDiscipline(s.session_type),
                        color = OnBgMuted,
                        style = Type.Chip,
                        maxLines = 1,
                    )
                    // One fixed-height glyph slot, so every column keeps one height.
                    Box(Modifier.height(DayGlyphSlot), contentAlignment = Alignment.Center) {
                        when {
                            s.status == "rest" || beforePlan -> Unit
                            // Non-colour done cue (a11y): a ✓ glyph, not hue alone, so
                            // a colour-blind user can answer "did I finish?" at a glance.
                            isDone -> Text(
                                "\u2713",
                                color = status.done,
                                style = Type.Chip.copy(fontWeight = FontWeight.Bold),
                                textAlign = TextAlign.Center,
                                maxLines = 1,
                            )
                            else -> Box(
                                Modifier
                                    .size(DayDotSize)
                                    .clip(RoundedCornerShape(DayDotSize / 2))
                                    .then(
                                        if (dotFilled) {
                                            Modifier.background(dotColor)
                                        } else {
                                            Modifier.border(1.5.dp, dotColor, RoundedCornerShape(DayDotSize / 2))
                                        },
                                    )
                                    .then(if (s.status == "partial") Modifier.testTag("week-partial-mark") else Modifier),
                            ) {
                                if (s.status == "partial") {
                                    Box(Modifier.fillMaxHeight().width(DayDotSize / 2).background(dotColor))
                                }
                            }
                        }
                    }
                }
            }
        }
        val expanded = week.firstOrNull { it.epoch_day == selectedDay }
        if (expanded != null) {
            val s = expanded
            // Inside the This-week card: a hairline sets the day apart instead of
            // a card-in-card, so the day's item cards keep their one-step-down tint.
            Column(verticalArrangement = Arrangement.spacedBy(Space.Md.dp)) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(1.dp)
                        .background(Hairline),
                )
                // The day's items carry no disclosure of their own: their evidence
                // sits behind this one "?" on the day's header.
                var evidenceOpen by rememberSaveable(s.epoch_day) { mutableStateOf(false) }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(s.title, color = OnBgBody, style = Type.Body)
                        Text(planDateLabel(s.epoch_day), color = OnBgMuted, style = Type.Caption)
                        // What the strip's single glyph leaves out: the day a swap
                        // was done on, a lighter or partly done day.
                        val creditedFrom = s.credited_from_epoch_day
                            ?.takeIf { (s.status == "done" || s.status == "done_lighter") && it != s.epoch_day }
                        val statusLine = when {
                            creditedFrom != null && s.status == "done_lighter" ->
                                stringResource(R.string.main_week_done_on_lighter, planDateLabel(creditedFrom))
                            creditedFrom != null -> stringResource(R.string.main_week_done_on, planDateLabel(creditedFrom))
                            s.status == "done_lighter" -> stringResource(R.string.main_week_day_lighter)
                            s.status == "partial" -> stringResource(R.string.main_week_day_partial)
                            else -> null
                        }
                        statusLine?.let { Text(it, color = OnBgMuted, style = Type.Caption) }
                    }
                    // "Move" for a day the core says can still move (a missed day
                    // can be made up): a quiet trailing action, never its own row.
                    // The hero's own day keeps that control on the hero only.
                    if (!readOnly && s.epoch_day != heroDay) SessionMoveAction(s, today, onMove)
                    if (s.items.isNotEmpty()) {
                        DisclosureButton(evidenceOpen, subject = s.title) { evidenceOpen = !evidenceOpen }
                    }
                }
                // Moved here / why a move was refused.
                SessionMoveNotes(s)
                val isCheck = { item: PrescriptionView -> isReturnCheckItem(item, s.session_type) }
                val isRun = { item: PrescriptionView -> !isCheck(item) && item.exercise.isEmpty() }
                if (evidenceOpen) SessionItemsEvidence(s.items, isRun, isCheck)
                if (s.items.isEmpty()) {
                    Text(
                        when (s.status) {
                            "rest" -> stringResource(R.string.main_rest_day)
                            "before_plan" -> stringResource(R.string.main_week_before_plan)
                            else -> EmptyValuePlaceholder
                        },
                        color = OnBgMuted,
                        style = Type.Body,
                    )
                } else {
                    // A past MISSED day is actionable: its first still-unlogged item is
                    // the full card (Log/Track dated to that day, per-set slots, "End
                    // exercise"), the others are compact rows a tap logs. Today and
                    // future days are read-only previews (the hero owns today): compact
                    // rows, no empty slots. Settled items collapse to a ✓ row; an
                    // exercise ended early stays reopenable on today's and past days,
                    // including once the whole day reads done and leaves the hero.
                    val pastActionable = !readOnly && s.epoch_day < today && s.status == "missed"
                    val reopenable = !readOnly && s.epoch_day <= today
                    val (openItems, doneItems) = s.items.partition { !it.done }
                    val current = if (pastActionable) openItems.firstOrNull() else null
                    val logRunLabel = stringResource(R.string.main_log_run_title)
                    openItems.forEach { item ->
                        if (item === current) {
                            OpenPlanItemCard(
                                item = item,
                                isRun = isRun(item),
                                onLog = { onLogItem(item, s.epoch_day) },
                                onTrack = { onLogRun(s.epoch_day) },
                                onEndExercise = { onEndExercise(item, s.epoch_day) },
                                runActionLabel = logRunLabel,
                                weightUnit = weightUnit,
                                isCheck = isCheck(item),
                            )
                        } else {
                            PlanItemRow(
                                item = item,
                                isRun = isRun(item),
                                isCheck = isCheck(item),
                                actionLabel = if (pastActionable) planItemActionLabel(item, isRun(item), isCheck(item), logRunLabel) else null,
                                onClick = { if (isRun(item)) onLogRun(s.epoch_day) else onLogItem(item, s.epoch_day) },
                            )
                        }
                    }
                    doneItems.forEach { item ->
                        DonePlanItemRow(
                            item = item,
                            isRun = isRun(item),
                            reopenable = reopenable,
                            onReopen = { onReopen(item, s.epoch_day) },
                            isCheck = isCheck(item),
                        )
                    }
                }
            }
        }
    }
}

/** Compact day-type token for the week strip, from the core's debug session_type
 *  ("Lift(MaxEffort)" / "Run(LongRun)" / "Rest"). Discipline-level so it fits a
 *  7-column strip; the full session title shows in the expanded day card. */
// l10n: output doubles as a comparison key (== "Run"/"Lift"); localizing it would
// break call-site logic. Needs decoupling of display from key before extraction.
internal fun sessionDiscipline(sessionType: String): String = when {
    sessionType.startsWith("Lift") -> "Lift"
    sessionType.startsWith("Run") -> "Run"
    else -> "Rest"
}

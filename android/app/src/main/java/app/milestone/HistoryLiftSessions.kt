package app.milestone

import android.content.res.Resources
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.util.Locale

// ── Lift sessions (History "Lifts" stream) ─────────────────────────────────

/** Local calendar day (days since the epoch in [zone]) of a unix-seconds stamp. */
internal fun localEpochDay(epochSec: Long, zone: ZoneId): Long =
    Instant.ofEpochSecond(epochSec).atZone(zone).toLocalDate().toEpochDay()

/**
 * One lifting session in History: every set logged on one local calendar day.
 * [epochDay] is null for legacy undated sets (observed_at 0), which share one
 * trailing group. [sets] are in logged order (observed_at, stable for ties).
 */
internal data class LiftSession(val epochDay: Long?, val sets: List<LiftResultView>) {
    /** Stable, saveable selection + list key. */
    val key: String = epochDay?.toString() ?: "undated"

    /** Exercises in first-logged order, keyed by [nameKey] (the key the core chains
     *  an exercise's e1RM and PRs on) and named by their first-logged spelling, each
     *  with its own sets in logged order. */
    val exercises: List<Pair<String, List<LiftResultView>>> =
        sets.groupBy { nameKey(it.exercise) }.values.map { it.first().exercise to it }

    /** Stamp of the session's first set (0 when undated): dates the card and its month header. */
    val firstObservedAt: Long get() = sets.firstOrNull()?.observed_at ?: 0L
}

/**
 * Group the set stream into per-local-day sessions, newest day first; undated
 * sets form one trailing group. Bucketing uses the device [zone] (shell-side, so
 * the core stays clock- and zone-free), so a DST shift never splits one day.
 */
internal fun groupLiftSessions(lifts: List<LiftResultView>, zone: ZoneId): List<LiftSession> {
    val byDay = HashMap<Long, MutableList<LiftResultView>>()
    val undated = ArrayList<LiftResultView>()
    for (l in lifts) {
        if (l.observed_at <= 0L) {
            undated += l
        } else {
            byDay.getOrPut(localEpochDay(l.observed_at, zone)) { ArrayList() } += l
        }
    }
    val dated = byDay.entries
        .sortedByDescending { it.key }
        .map { (day, sets) -> LiftSession(day, sets.sortedBy { it.observed_at }) }
    return if (undated.isEmpty()) dated else dated + LiftSession(null, undated)
}

/**
 * Activity-heatmap days: per local day, whether any set was lifted (a lifting day is
 * ONE session, however many sets) and how many runs STARTED that day ([runLocalDay],
 * the day a run's title names). Undated entries are skipped (they have no day to land on).
 */
internal fun historyActivityDays(
    lifts: List<LiftResultView>,
    runs: List<RunResultView>,
    zone: ZoneId,
): Map<Long, DayActivity> {
    val days = HashMap<Long, DayActivity>()
    for (l in lifts) {
        if (l.observed_at <= 0L) continue
        val day = localEpochDay(l.observed_at, zone)
        days[day] = (days[day] ?: DayActivity(lifted = false, runs = 0)).copy(lifted = true)
    }
    for (r in runs) {
        val day = runLocalDay(r, zone) ?: continue
        val prev = days[day] ?: DayActivity(lifted = false, runs = 0)
        days[day] = prev.copy(runs = prev.runs + 1)
    }
    return days
}

/**
 * Per [nameKey], its best e1RM of each dated session in ascending day order: the
 * factual series behind the session sheet's e1RM change line (one point per
 * session, so warm-up sets don't count as a change within a day). Holds and sets
 * without an estimate (e1RM 0) contribute nothing.
 */
internal fun bestE1rmByExerciseDay(sessions: List<LiftSession>): Map<String, List<Pair<Long, Double>>> {
    val out = HashMap<String, MutableList<Pair<Long, Double>>>()
    for (s in sessions.asReversed()) {
        val day = s.epochDay ?: continue
        for ((name, sets) in s.exercises) {
            val best = bestE1rmKg(sets) ?: continue
            out.getOrPut(nameKey(name)) { ArrayList() } += day to best
        }
    }
    return out
}

/** The best e1RM among [sets] (kg), or null when none carries an estimate (holds, 0 reps). */
internal fun bestE1rmKg(sets: List<LiftResultView>): Double? =
    sets.filter { it.hold_sec == null && it.e1rm_kg > 0.0 }.maxOfOrNull { it.e1rm_kg }

/**
 * One exercise's sets as a compact line, consecutive identical sets folded:
 * "3 × 5 @ 120 kg, 1 × 3 @ 125 kg"; a bodyweight set drops the load ("3 × 10");
 * a timed hold reads "2 × 30 s hold" (+ " · +10 kg" when loaded). Never prints an
 * RPE: effort lives on the per-set rows of the session sheet.
 */
internal fun exerciseSetsSummary(res: Resources, sets: List<LiftResultView>, weightUnit: WeightUnit): String {
    val groups = ArrayList<Pair<LiftResultView, Int>>()
    for (s in sets) {
        val last = groups.lastOrNull()
        if (last != null && sameSetShape(last.first, s)) {
            groups[groups.size - 1] = last.first to last.second + 1
        } else {
            groups += s to 1
        }
    }
    return groups.joinToString(res.getString(R.string.history_sets_separator)) { (s, n) ->
        // The same figure the session sheet's set rows show ("121.5 lb"), never a
        // differently rounded one on the card.
        val load = res.getString(R.string.history_load, loadFigure(s.weight_kg, weightUnit), weightSuffix(weightUnit))
        val hold = s.hold_sec
        when {
            hold != null && s.weight_kg > 0.0 -> res.getString(R.string.history_sets_hold_loaded, n, hold, load)
            hold != null -> res.getString(R.string.history_sets_hold, n, hold)
            s.weight_kg > 0.0 -> res.getString(R.string.history_sets_reps_load, n, s.reps, load)
            else -> res.getString(R.string.history_sets_reps, n, s.reps)
        }
    }
}

private fun sameSetShape(a: LiftResultView, b: LiftResultView): Boolean =
    a.hold_sec == b.hold_sec && a.reps == b.reps && a.weight_kg == b.weight_kg

/** Date skeleton of a session title: abbreviated weekday, month and day. */
private const val SESSION_DATE_SKELETON = "EEEMMMd"

/** Session title: the local date ("Mon, Sep 21", device locale), or "Undated sets". */
@Composable
private fun liftSessionTitle(session: LiftSession): String {
    val day = session.epochDay ?: return stringResource(R.string.history_session_undated)
    return localizedDayFormatter(SESSION_DATE_SKELETON).formatEpochDay(day)
}

/** "3 exercises · 12 sets", the count line under a session's date on its card and sheet. */
@Composable
private fun sessionCounts(session: LiftSession): List<String> {
    val exercises = session.exercises.size
    val sets = session.sets.size
    return listOf(
        pluralStringResource(R.plurals.history_exercise_count, exercises, exercises),
        pluralStringResource(R.plurals.history_set_count, sets, sets),
    )
}

/** Card header: date title + "3 exercises · 12 sets", and a "Today" / "Yesterday" chip
 *  on the two most recent days ([todayLocalDay]); older days rely on the date title
 *  alone, so the day is never stated twice. */
@Composable
private fun LiftSessionHeader(session: LiftSession, nowMillis: Long, todayLocalDay: Long) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Md.dp + Space.Xs.dp),
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                liftSessionTitle(session),
                style = Type.CardTitle,
                color = OnBgBody,
            )
            Text(
                sessionCounts(session).joinToString(" · "),
                color = OnBgFaint,
                style = Type.Caption.merge(TabularFigures),
            )
        }
        val day = session.epochDay
        if (day != null && todayLocalDay - day in 0L..1L) {
            DateBadge(formatLogDate(session.firstObservedAt, nowMillis))
        }
    }
}

/** "e1RM 140 kg", plus its glossary "?" on the first e1RM line of a card or sheet
 *  only ([glossary]): the term is explained once, not once per exercise. */
@Composable
private fun BestE1rmRow(e1rmKg: Double, weightUnit: WeightUnit, glossary: Boolean) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Xs.dp),
    ) {
        Text(
            stringResource(R.string.history_session_e1rm, weightFigure(e1rmKg, weightUnit), weightSuffix(weightUnit)),
            color = OnBgFaint,
            style = Type.Caption.merge(TabularFigures),
        )
        if (glossary) GlossaryInfo("e1rm")
    }
}

/** Index of the session's first exercise that carries an e1RM (the one whose e1RM
 *  line holds the glossary "?"), or -1. */
private fun LiftSession.firstE1rmIndex(): Int = exercises.indexOfFirst { bestE1rmKg(it.second) != null }

/** The e1RM change since the exercise's first session, in the display unit, as the
 *  pill text ("+10.2 kg · 3 sessions", "No change · 3 sessions") and its spoken form
 *  ("+10.2 kg over 3 sessions"). */
@Composable
private fun e1rmChangeLabels(deltaKg: Double, sessions: Int, weightUnit: WeightUnit): Pair<String, String> {
    val shown = if (weightUnit == WeightUnit.Lb) kgToLb(deltaKg) else deltaKg
    val rounded = Math.round(Math.abs(shown) * 10.0) / 10.0
    if (rounded == 0.0) {
        return pluralStringResource(R.plurals.history_e1rm_flat_pill, sessions, sessions) to
            pluralStringResource(R.plurals.history_e1rm_flat, sessions, sessions)
    }
    val signed = (if (shown > 0) "+" else "−") + trimDecimal(rounded)
    val unit = weightSuffix(weightUnit)
    return pluralStringResource(R.plurals.history_e1rm_change_pill, sessions, signed, unit, sessions) to
        pluralStringResource(R.plurals.history_e1rm_change, sessions, signed, unit, sessions)
}

/** The day's volume (the sum of the core's per-set [LiftResultView.tonnage_kg]),
 *  whole units in the display unit with [locale] grouping ("4,690"), or null for a
 *  bodyweight-only day. */
private fun sessionVolume(sets: List<LiftResultView>, weightUnit: WeightUnit, locale: Locale): String? {
    val kg = sets.sumOf { it.tonnage_kg }
    if (kg <= 0.0) return null
    val shown = if (weightUnit == WeightUnit.Lb) kgToLb(kg) else kg
    return java.text.NumberFormat.getIntegerInstance(locale).format(Math.round(shown))
}

/** The hairline that keeps a non-safety card readable as a card on a BgElevated sheet
 *  (a safety card keeps its danger border). */
internal val SheetCardBorder: Color @Composable get() = OnBgFaint.copy(alpha = 0.25f)

/** An exercise's name in bold (or "Set" when blank), with "New best" when a set
 *  beat every prior e1RM of the exercise: right after the name, or [endAligned]. */
@Composable
private fun ExerciseNameRow(name: String, sets: List<LiftResultView>, endAligned: Boolean) {
    Row(
        modifier = if (endAligned) Modifier.fillMaxWidth() else Modifier,
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
    ) {
        Text(
            name.ifBlank { stringResource(R.string.main_set) },
            color = OnBgBody,
            style = Type.BodyBold,
            modifier = Modifier.weight(1f, fill = endAligned),
        )
        if (sets.any { it.e1rm_pr }) NewBestChip(name)
    }
}

/**
 * History "Lifts" card for one session (a local day): date header, then one row
 * per exercise with its folded set line and best e1RM. The card face carries no
 * trend line (an axis-less sparkline on a face reads as decoration); the e1RM trend
 * per exercise lives in [LiftSessionSheet]. Tapping opens that sheet.
 */
@Composable
internal fun LiftSessionCard(
    session: LiftSession,
    nowMillis: Long,
    // The screen's local day, bucketed in the same zone as the sessions.
    todayLocalDay: Long,
    weightUnit: WeightUnit,
    onClick: () -> Unit,
) {
    PlainCard(onClick = onClick) {
        LiftSessionHeader(session, nowMillis, todayLocalDay)
        val glossaryAt = session.firstE1rmIndex()
        session.exercises.forEachIndexed { i, (name, sets) ->
            Column(
                modifier = Modifier.fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
            ) {
                ExerciseNameRow(name, sets, endAligned = false)
                Text(
                    exerciseSetsSummary(LocalResources.current, sets, weightUnit),
                    color = OnBgMuted,
                    style = Type.Body.merge(TabularFigures),
                )
                bestE1rmKg(sets)?.let { BestE1rmRow(it, weightUnit, glossary = i == glossaryAt) }
            }
        }
    }
}

/**
 * Session detail sheet: the day as a title with its exercise/set count and volume,
 * then one card per exercise (name, best e1RM with its change pill, and a set table
 * of 48dp rows). Each set row opens the per-set detail (edit/delete via
 * [EntryActionSheets], hosted by the caller through [onOpenSet]). The newest session
 * also carries the core's session notes (lift audit) + e1RM trend. The [ActionRows] footer's
 * "Delete session" removes the whole day behind ONE confirm, emitting one
 * [Event.DeleteEntry] per set.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun LiftSessionSheet(
    session: LiftSession,
    model: ViewModel,
    // Per-exercise best-e1RM-per-session series ([bestE1rmByExerciseDay]).
    e1rmSeries: Map<String, List<Pair<Long, Double>>>,
    weightUnit: WeightUnit,
    nowMillis: Long,
    // Hoisted by the caller so the sheet returns to the set the user just edited.
    scrollState: ScrollState,
    onOpenSet: (LiftResultView) -> Unit,
    onDismiss: () -> Unit,
    onEvent: (Event) -> Unit,
) {
    val deletedText = stringResource(R.string.history_session_deleted)
    val undoLabel = stringResource(R.string.log_undo)
    // Full height: the message host below the list ("Set updated · Undo") must be
    // on screen when the sheet returns from the set editor.
    AppSheet(onDismiss = onDismiss, containerColor = BgElevated) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                // Leaves room below for the message host when the sheet is full height.
                .weight(1f, fill = false)
                .verticalScroll(scrollState)
                .sheetPadding(),
            verticalArrangement = Arrangement.spacedBy(Space.Md.dp),
        ) {
            LiftSessionSheetHeader(session, weightUnit)
            val glossaryAt = session.firstE1rmIndex()
            session.exercises.forEachIndexed { i, (name, sets) ->
                val series = session.epochDay?.let { day ->
                    e1rmSeries[nameKey(name)].orEmpty().filter { it.first <= day }.map { it.second }
                }.orEmpty()
                ExerciseSetsCard(
                    name = name,
                    sets = sets,
                    e1rmSeries = series,
                    glossary = i == glossaryAt,
                    weightUnit = weightUnit,
                    onOpenSet = onOpenSet,
                )
            }
            // Session-level audit + longitudinal trend are core fields scoped to the
            // most recent dated lifting day / the current review, so they attach
            // only to the session holding the newest logged set, never to an older
            // one (which would stamp a stale audit onto it). Prilepin volume +
            // depth-jump gate (lift_audit) and the feedback-027/028/029 trend line.
            val isLatestSession = session.holdsNewestSet(model.lifts)
            if (isLatestSession && (model.trend != null || model.lift_audit.isNotEmpty())) {
                TileOverline(stringResource(R.string.main_lift_session_audit))
                model.trend?.let { t ->
                    EvidenceCard(
                        t.summary, t.grade, t.citation, t.confidence, t.safety_critical, t.contested,
                        border = if (t.safety_critical) null else SheetCardBorder,
                        why = t.why,
                    )
                }
                model.lift_audit.byGuidancePriority().forEach { g ->
                    EvidenceCard(
                        g.summary, g.grade, g.citation, g.confidence, g.safety_critical, g.contested,
                        g.section.takeIf { it.isNotBlank() },
                        border = if (g.safety_critical) null else SheetCardBorder,
                        why = g.why,
                    )
                }
            }
            ActionRows(
                rows = emptyList(),
                destructive = DestructiveAction(
                    label = stringResource(R.string.history_delete_session),
                    icon = painterResource(R.drawable.ic_ui_delete),
                    confirmTitle = stringResource(R.string.history_delete_session_title),
                    confirmMessage = pluralStringResource(
                        R.plurals.history_delete_session_message, session.sets.size, session.sets.size,
                    ),
                    confirmLabel = stringResource(R.string.main_delete),
                    onConfirm = {
                        session.sets.forEach { onEvent(deleteEventFor(HistoryEntry.Lift(it))) }
                        onDismiss()
                        postUndoable(
                            deletedText,
                            undoLabel,
                            session.sets.mapNotNull { restoreEventFor(HistoryEntry.Lift(it), emptyList()) },
                        )
                    },
                    testTag = "lift-session-delete",
                ),
            )
        }
        // The sheet is its own window: a set edited or deleted from it confirms here
        // ("Set updated · Undo"), where the user can still reach Undo. Its inset padding
        // also clears the navigation bar below the scrolling content.
        UiMessageHost(Modifier.fillMaxWidth().navigationBarsPadding())
    }
}

/** The session sheet's title: the day, then "3 exercises · 9 sets · 4,690 lb lifted". */
@Composable
private fun LiftSessionSheetHeader(session: LiftSession, weightUnit: WeightUnit) {
    val volume = sessionVolume(session.sets, weightUnit, LocalConfiguration.current.locales[0])?.let {
        stringResource(R.string.history_session_volume, it, weightSuffix(weightUnit))
    }
    Column(verticalArrangement = Arrangement.spacedBy(Space.Xs.dp)) {
        Text(liftSessionTitle(session), style = Type.Title, color = OnBgBody)
        Text(
            (sessionCounts(session) + listOfNotNull(volume)).joinToString(" · "),
            color = OnBgMuted,
            style = Type.Caption.merge(TabularFigures),
        )
    }
}

/** Width of the set table's index column, so loads line up across every card. */
private val SetIndexWidth = 24.dp

/**
 * One exercise of the session sheet: the name (with "New best" end-aligned when a set
 * beat every prior e1RM), the best e1RM with its change since the exercise's first
 * session ([e1rmSeries], one point per session) as a pill and, when [glossary], the
 * e1RM "?" at the end of the line; then the set table. Each 48dp row (index, load,
 * reps left) opens that set.
 */
@Composable
private fun ExerciseSetsCard(
    name: String,
    sets: List<LiftResultView>,
    e1rmSeries: List<Double>,
    glossary: Boolean,
    weightUnit: WeightUnit,
    onOpenSet: (LiftResultView) -> Unit,
) {
    val res = LocalResources.current
    PlainCard(
        contentPadding = PaddingValues(start = Space.Card.dp, end = Space.Card.dp, top = Space.Card.dp, bottom = Space.Xs.dp),
        spacing = 0.dp,
    ) {
        Column(
            modifier = Modifier.padding(bottom = Space.Md.dp),
            verticalArrangement = Arrangement.spacedBy(Space.Xs.dp),
        ) {
            ExerciseNameRow(name, sets, endAligned = true)
            bestE1rmKg(sets)?.let { best ->
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.Sm.dp),
                ) {
                    Text(
                        stringResource(R.string.history_session_e1rm, weightFigure(best, weightUnit), weightSuffix(weightUnit)),
                        color = OnBgMuted,
                        style = Type.Caption.merge(TabularFigures),
                    )
                    Box(Modifier.weight(1f)) {
                        if (e1rmSeries.size >= 2) {
                            val (pill, spoken) = e1rmChangeLabels(
                                e1rmSeries.last() - e1rmSeries.first(), e1rmSeries.size, weightUnit,
                            )
                            Text(
                                pill,
                                color = OnBgBody,
                                style = Type.Chip.merge(TabularFigures).copy(fontWeight = FontWeight.Bold),
                                modifier = Modifier
                                    .background(AccentTint, ChoiceCellShape)
                                    .padding(horizontal = Space.Sm.dp, vertical = Space.Xs.dp)
                                    .semantics { contentDescription = spoken },
                            )
                        }
                    }
                    if (glossary) GlossaryInfo("e1rm")
                }
            }
        }
        val editLabel = stringResource(R.string.main_edit)
        sets.forEachIndexed { i, s ->
            HorizontalDivider(color = Hairline)
            val load = liftSetLine(res, s.hold_sec, s.weight_kg, s.reps, s.rpe, rpeRecorded = false, weightUnit = weightUnit)
            val left = if (s.hold_sec == null && s.rpe_recorded) repsLeftText(res, s.rpe, short = true) else null
            val spoken = stringResource(
                R.string.history_set_row_cd,
                stringResource(R.string.history_set_n, i + 1),
                liftSetLine(res, s.hold_sec, s.weight_kg, s.reps, s.rpe, s.rpe_recorded, weightUnit),
            )
            // The whole row opens the set (edit/delete). No per-row chevron: a column
            // of › marks reads as a ledger; the button role and "Edit" carry it.
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = ActionMinHeight)
                    .clickable(role = Role.Button, onClickLabel = editLabel) { onOpenSet(s) }
                    .testTag("lift-set-row")
                    .clearAndSetSemantics {
                        contentDescription = spoken
                        role = Role.Button
                        onClick(label = editLabel) { onOpenSet(s); true }
                    },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.Md.dp),
            ) {
                Text(
                    "${i + 1}",
                    color = OnBgFaint,
                    style = Type.Chip.merge(TabularFigures),
                    modifier = Modifier.width(SetIndexWidth),
                )
                Text(load, color = OnBgBody, style = Type.Body.merge(TabularFigures), modifier = Modifier.weight(1f))
                if (left != null) {
                    Text(left, color = OnBgMuted, style = Type.Caption.merge(TabularFigures))
                }
            }
        }
    }
}

/** Whether this session holds the newest logged set of [lifts] (matched by entry id
 *  and stamp, never instance identity: a cached session and a freshly decoded view
 *  hold equal but distinct objects). */
internal fun LiftSession.holdsNewestSet(lifts: List<LiftResultView>): Boolean {
    val latest = lifts.lastOrNull() ?: return false
    return sets.any { it.entry_id == latest.entry_id && it.observed_at == latest.observed_at }
}

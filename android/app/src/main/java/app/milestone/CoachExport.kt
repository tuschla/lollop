package app.milestone

import android.content.Context
import android.content.Intent
import android.content.res.Resources
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.ZoneId
import java.time.temporal.TemporalAdjusters

/** How many local Mon–Sun weeks the coach summary covers, the current one included. */
internal const val CoachSummaryWeeks = 4

/**
 * A readable plain-text summary of the last [weeks] local Mon–Sun weeks (the
 * current one included, up to [today]) for a coach: per week its totals, then per
 * day the lifts (exercise · sets × reps @ load in [weightUnit], holds), the runs
 * (distance in [distanceUnit] · duration · pace) and the other activities (kind ·
 * minutes · effort). A run of weeks with nothing logged folds into one line.
 * Facts only, straight from the logged rows; no coaching.
 * Undated rows (observed_at 0) have no day and are left out. Pure.
 */
internal fun buildCoachSummary(
    res: Resources,
    model: ViewModel,
    today: Long,
    zone: ZoneId,
    weightUnit: WeightUnit,
    distanceUnit: DistanceUnit,
    weeks: Int = CoachSummaryWeeks,
): String {
    val lastMonday = LocalDate.ofEpochDay(today).with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY)).toEpochDay()
    val firstMonday = lastMonday - 7L * (weeks.coerceAtLeast(1) - 1)
    val window = firstMonday..today
    fun dayOf(at: Long): Long? = at.takeIf { it > 0L }?.let { localEpochDay(it, zone) }?.takeIf { it in window }
    val lifts = model.lifts.mapNotNull { s -> dayOf(s.observed_at)?.let { it to s } }.groupBy({ it.first }, { it.second })
    val runs = model.runs.mapNotNull { r -> dayOf(r.observed_at)?.let { it to r } }.groupBy({ it.first }, { it.second })
    val activities = model.activities.mapNotNull { a -> dayOf(a.observed_at)?.let { it to a } }.groupBy({ it.first }, { it.second })

    return buildString {
        appendLine(res.getString(R.string.coach_export_header, epochDayLabel(firstMonday), epochDayLabel(today)))
        var emptyFrom: Long? = null
        fun flushEmpty(lastEmpty: Long) {
            val from = emptyFrom ?: return
            appendLine()
            appendLine(
                if (from == lastEmpty) {
                    res.getString(R.string.coach_export_week_empty, planDateLabel(from))
                } else {
                    res.getString(R.string.coach_export_weeks_empty, planDateLabel(from), planDateLabel(lastEmpty))
                },
            )
            emptyFrom = null
        }
        for (monday in firstMonday..lastMonday step 7L) {
            val days = (monday..minOf(monday + 6L, today)).filter { it in lifts || it in runs || it in activities }
            if (days.isEmpty()) {
                if (emptyFrom == null) emptyFrom = monday
                continue
            }
            flushEmpty(monday - 7L)
            appendLine()
            appendLine(res.getString(R.string.coach_export_week, planDateLabel(monday)))
            appendLine(weekTotals(res, days, lifts, runs, activities, distanceUnit))
            for (day in days) {
                appendLine()
                appendLine(planDateLabel(day))
                lifts[day]?.let { sets -> liftLines(res, sets, weightUnit).forEach { appendLine("- $it") } }
                runs[day]?.sortedBy { it.observed_at }?.forEach { appendLine("- ${runLine(res, it, distanceUnit)}") }
                activities[day]?.sortedBy { it.observed_at }?.forEach { appendLine("- ${activityLine(res, it)}") }
            }
        }
        flushEmpty(lastMonday)
    }.trimEnd() + "\n"
}

/** "Totals: 2 lifting sessions · 1 run, 5.00 km · 72 active minutes from runs …" for
 *  one week; the active minutes name only the sources that have any. */
private fun weekTotals(
    res: Resources,
    days: List<Long>,
    lifts: Map<Long, List<LiftResultView>>,
    runs: Map<Long, List<RunResultView>>,
    activities: Map<Long, List<ActivityResultView>>,
    unit: DistanceUnit,
): String {
    val liftDays = days.count { it in lifts }
    val weekRuns = days.flatMap { runs[it].orEmpty() }
    val weekActivities = days.flatMap { activities[it].orEmpty() }
    val runMin = weekRuns.sumOf { it.duration_min }
    val otherMin = weekActivities.sumOf { it.minutes }
    val activeMin = Math.round(runMin + otherMin).toInt()
    val activeRes = when {
        otherMin <= 0.0 -> R.plurals.coach_export_active_minutes_runs
        runMin <= 0.0 -> R.plurals.coach_export_active_minutes_other
        else -> R.plurals.coach_export_active_minutes
    }
    val parts = buildList {
        if (liftDays > 0) add(res.getQuantityString(R.plurals.coach_export_lift_sessions, liftDays, liftDays))
        if (weekRuns.isNotEmpty()) {
            add(res.getQuantityString(R.plurals.coach_export_runs, weekRuns.size, weekRuns.size, runDistLabel(weekRuns.sumOf { it.distance_km }, unit)))
        }
        if (activeMin > 0) add(res.getQuantityString(activeRes, activeMin, activeMin))
    }
    return res.getString(R.string.coach_export_totals, parts.joinToString(" · "))
}

/** One line per exercise in first-logged order; runs of identical sets fold into
 *  "3 × 5 @ 100 kg". */
private fun liftLines(res: Resources, sets: List<LiftResultView>, unit: WeightUnit): List<String> =
    sets.sortedBy { it.observed_at }.groupBy { it.exercise }.map { (exercise, rows) ->
        val groups = mutableListOf<Pair<LiftResultView, Int>>()
        for (s in rows) {
            val last = groups.lastOrNull()
            if (last != null && sameSet(last.first, s)) groups[groups.lastIndex] = last.first to last.second + 1
            else groups += s to 1
        }
        val text = groups.joinToString(", ") { (s, n) ->
            val hold = s.hold_sec
            when {
                hold != null -> res.getString(R.string.coach_export_sets_hold, n, hold)
                s.weight_kg > 0.0 -> res.getString(
                    R.string.coach_export_sets_load, n, s.reps, "${weightFigure(s.weight_kg, unit)} ${weightSuffix(unit)}",
                )
                else -> res.getString(R.string.coach_export_sets_reps, n, s.reps)
            }
        }
        res.getString(R.string.coach_export_lift, exercise, text)
    }

private fun sameSet(a: LiftResultView, b: LiftResultView): Boolean =
    a.reps == b.reps && a.weight_kg == b.weight_kg && a.hold_sec == b.hold_sec

private fun runLine(res: Resources, r: RunResultView, unit: DistanceUnit): String {
    val line = res.getString(
        R.string.coach_export_run,
        runDistLabel(r.distance_km, unit),
        fmtDurationDigits(minutesToDurationDigits(r.duration_min)),
        runPaceLabel(r.distance_km, r.duration_min, unit),
    )
    return r.session_rpe?.let { "$line · ${res.getString(R.string.coach_export_run_effort, it)}" } ?: line
}

private fun activityLine(res: Resources, a: ActivityResultView): String {
    val effort = res.getString(if (a.vigorous) R.string.activity_effort_vigorous_short else R.string.activity_effort_moderate_short)
    val line = res.getString(R.string.coach_export_activity, res.getString(a.kind.labelRes), Math.round(a.minutes).toInt(), effort)
    return if (a.strengthening) "$line · ${res.getString(R.string.coach_export_strengthening)}" else line
}

/** Display label of an [ActivityKind]. */
internal val ActivityKind.labelRes: Int
    get() = when (this) {
        ActivityKind.Sport -> R.string.activity_kind_sport
        ActivityKind.Cycling -> R.string.activity_kind_cycling
        ActivityKind.Swimming -> R.string.activity_kind_swimming
        ActivityKind.Walking -> R.string.activity_kind_walking
        ActivityKind.Dance -> R.string.activity_kind_dance
        ActivityKind.Strength -> R.string.activity_kind_strength
        ActivityKind.Other -> R.string.activity_kind_other
    }

/** Open the share sheet with [model]'s coach summary as plain text, in the
 *  profile's weight unit and the chosen distance unit. */
internal fun shareCoachSummary(ctx: Context, model: ViewModel) {
    val text = buildCoachSummary(
        ctx.resources,
        model,
        today = model.today(),
        zone = ZoneId.systemDefault(),
        weightUnit = model.weightUnit(),
        distanceUnit = resolveDistanceUnit(AppSettings.distanceUnitOverride.value),
    )
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_SUBJECT, ctx.getString(R.string.coach_export_subject))
        putExtra(Intent.EXTRA_TITLE, ctx.getString(R.string.coach_export_subject))
        putExtra(Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(
        Intent.createChooser(send, ctx.getString(R.string.coach_export_chooser)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        },
    )
}

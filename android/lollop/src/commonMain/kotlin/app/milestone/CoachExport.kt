package app.milestone

import app.milestone.resources.Res
import app.milestone.resources.activity_effort_moderate_short
import app.milestone.resources.activity_effort_vigorous_short
import app.milestone.resources.activity_kind_cycling
import app.milestone.resources.activity_kind_dance
import app.milestone.resources.activity_kind_other
import app.milestone.resources.activity_kind_sport
import app.milestone.resources.activity_kind_strength
import app.milestone.resources.activity_kind_swimming
import app.milestone.resources.activity_kind_walking
import app.milestone.resources.coach_export_active_minutes
import app.milestone.resources.coach_export_active_minutes_other
import app.milestone.resources.coach_export_active_minutes_runs
import app.milestone.resources.coach_export_activity
import app.milestone.resources.coach_export_header
import app.milestone.resources.coach_export_lift_sessions
import app.milestone.resources.coach_export_run
import app.milestone.resources.coach_export_run_effort
import app.milestone.resources.coach_export_runs
import app.milestone.resources.coach_export_strengthening
import app.milestone.resources.coach_export_totals
import app.milestone.resources.coach_export_week
import app.milestone.resources.coach_export_week_empty
import app.milestone.resources.coach_export_weeks_empty
import app.milestone.resources.ui_label_value
import kotlinx.datetime.TimeZone
import org.jetbrains.compose.resources.StringResource

/** How many local Mon–Sun weeks the coach summary covers, the current one included. */
internal const val CoachSummaryWeeks = 4

/**
 * A readable plain-text summary of the last [weeks] local Mon–Sun weeks (the
 * current one included, up to [today]) for a coach: per week its totals, then per
 * day the lifts (exercise · sets × reps @ load in [weightUnit], holds), the runs
 * (distance in [distanceUnit] · duration · pace) and the other activities (kind ·
 * minutes · effort). A run of weeks with nothing logged folds into one line.
 * Facts only, straight from the logged rows; no coaching.
 * Undated rows (observed_at 0) have no day and are left out; a run is filed under
 * the day it STARTED ([runDaySec]), the day its History title names. Pure.
 */
internal fun buildCoachSummary(
    res: PlatformResources,
    model: ViewModel,
    today: Long,
    zone: TimeZone,
    weightUnit: WeightUnit,
    distanceUnit: DistanceUnit,
    weeks: Int = CoachSummaryWeeks,
): String {
    val lastMonday = mondayOf(today)
    val firstMonday = lastMonday - 7L * (weeks.coerceAtLeast(1) - 1)
    val window = firstMonday..today
    fun dayOf(at: Long): Long? = at.takeIf { it > 0L }?.let { localEpochDay(it, zone) }?.takeIf { it in window }
    val lifts = model.lifts.mapNotNull { s -> dayOf(s.observed_at)?.let { it to s } }.groupBy({ it.first }, { it.second })
    val runs = model.runs.mapNotNull { r -> dayOf(runDaySec(r))?.let { it to r } }.groupBy({ it.first }, { it.second })
    val activities = model.activities.mapNotNull { a -> dayOf(a.observed_at)?.let { it to a } }.groupBy({ it.first }, { it.second })

    return buildString {
        appendLine(res.getString(Res.string.coach_export_header, epochDayLabel(firstMonday), epochDayLabel(today)))
        var emptyFrom: Long? = null
        fun flushEmpty(lastEmpty: Long) {
            val from = emptyFrom ?: return
            appendLine()
            appendLine(
                if (from == lastEmpty) {
                    res.getString(Res.string.coach_export_week_empty, planDateLabel(from))
                } else {
                    res.getString(Res.string.coach_export_weeks_empty, planDateLabel(from), planDateLabel(lastEmpty))
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
            appendLine(res.getString(Res.string.coach_export_week, planDateLabel(monday)))
            appendLine(weekTotals(res, days, lifts, runs, activities, distanceUnit))
            for (day in days) {
                appendLine()
                appendLine(planDateLabel(day))
                lifts[day]?.let { sets -> liftLines(res, sets, weightUnit).forEach { appendLine("- $it") } }
                runs[day]?.sortedBy(::runDaySec)?.forEach { appendLine("- ${runLine(res, it, distanceUnit)}") }
                activities[day]?.sortedBy { it.observed_at }?.forEach { appendLine("- ${activityLine(res, it)}") }
            }
        }
        flushEmpty(lastMonday)
    }.trimEnd() + "\n"
}

/** "Totals: 2 lifting sessions · 1 run, 5.00 km · 72 active minutes from runs …" for
 *  one week; the active minutes name only the sources that have any. */
private fun weekTotals(
    res: PlatformResources,
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
    val activeMin = (runMin + otherMin).roundOrZero().toInt()
    val activeRes = when {
        otherMin <= 0.0 -> Res.plurals.coach_export_active_minutes_runs
        runMin <= 0.0 -> Res.plurals.coach_export_active_minutes_other
        else -> Res.plurals.coach_export_active_minutes
    }
    val parts = buildList {
        if (liftDays > 0) add(res.getQuantityString(Res.plurals.coach_export_lift_sessions, liftDays, liftDays))
        if (weekRuns.isNotEmpty()) {
            add(res.getQuantityString(Res.plurals.coach_export_runs, weekRuns.size, weekRuns.size, runDistLabel(weekRuns.sumOf { it.distance_km }, unit)))
        }
        if (activeMin > 0) add(res.getQuantityString(activeRes, activeMin, activeMin))
    }
    return res.getString(Res.string.coach_export_totals, parts.joinToString(" · "))
}

/** One line per exercise in first-logged order, its sets as History summarises
 *  them ([exerciseSetsSummary]): "Bench: 3 × 5 @ 100 kg". */
private fun liftLines(res: PlatformResources, sets: List<LiftResultView>, unit: WeightUnit): List<String> =
    sets.sortedBy { it.observed_at }.groupBy { it.exercise }.map { (exercise, rows) ->
        res.getString(Res.string.ui_label_value, exercise, exerciseSetsSummary(res, rows, unit))
    }

private fun runLine(res: PlatformResources, r: RunResultView, unit: DistanceUnit): String {
    val line = res.getString(
        Res.string.coach_export_run,
        runDistLabel(r.distance_km, unit),
        hmsDisplay(minutesToDurationDigits(r.duration_min)),
        runPaceLabel(r.distance_km, r.duration_min, unit),
    )
    return r.session_rpe?.let { "$line · ${res.getString(Res.string.coach_export_run_effort, it)}" } ?: line
}

private fun activityLine(res: PlatformResources, a: ActivityResultView): String {
    val effort = res.getString(if (a.vigorous) Res.string.activity_effort_vigorous_short else Res.string.activity_effort_moderate_short)
    val line = res.getString(Res.string.coach_export_activity, res.getString(a.kind.labelRes), a.minutes.roundOrZero().toInt(), effort)
    return if (a.strengthening) "$line · ${res.getString(Res.string.coach_export_strengthening)}" else line
}

/** Display label of an [ActivityKind]. */
internal val ActivityKind.labelRes: StringResource
    get() = when (this) {
        ActivityKind.Sport -> Res.string.activity_kind_sport
        ActivityKind.Cycling -> Res.string.activity_kind_cycling
        ActivityKind.Swimming -> Res.string.activity_kind_swimming
        ActivityKind.Walking -> Res.string.activity_kind_walking
        ActivityKind.Dance -> Res.string.activity_kind_dance
        ActivityKind.Strength -> Res.string.activity_kind_strength
        ActivityKind.Other -> Res.string.activity_kind_other
    }

/** [model]'s coach summary in the profile's weight unit and the chosen distance
 *  unit, for the share sheet. */
internal fun coachSummaryText(res: PlatformResources, model: ViewModel): String = buildCoachSummary(
    res,
    model,
    today = model.today(),
    zone = TimeZone.currentSystemDefault(),
    weightUnit = model.weightUnit(),
    distanceUnit = resolveDistanceUnit(AppSettings.distanceUnitOverride.value),
)

/** Open the share sheet with [model]'s coach summary ([coachSummaryText]) as plain text. */
internal expect fun shareCoachSummary(ctx: PlatformContext, model: ViewModel)

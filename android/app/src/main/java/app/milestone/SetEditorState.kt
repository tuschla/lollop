package app.milestone

import android.content.res.Resources
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import java.util.Locale

/** How many quick-pick chips show with nothing (or an exact pick) typed. */
private const val QuickPickBase = 6
/** How many filtered matches show while typing. */
private const val QuickPickFiltered = 8

/**
 * Exercise quick-pick chips. The candidate order is today's open plan items, then
 * the user's own recent exercises (newest first), then the rest of the week's
 * plan, deduplicated case-insensitively, so a busy plan week can't crowd out the
 * lifts the user actually logs. With nothing typed, or a name that already
 * matches a candidate, the first [QuickPickBase] show; otherwise every candidate
 * containing the typed text (case-insensitive) is offered, up to
 * [QuickPickFiltered]. Modality-aware by construction: no hard-coded barbell
 * defaults. Pure.
 */
internal fun quickPickCandidates(
    query: String,
    todayOpen: List<String>,
    recents: List<String>,
    weekPlanned: List<String>,
): List<String> {
    val all = (todayOpen + recents + weekPlanned)
        .map { it.trim() }
        .filter { it.isNotEmpty() }
        .distinctBy { it.lowercase(Locale.ROOT) }
    val q = query.trim()
    if (q.isEmpty() || all.any { it.equals(q, ignoreCase = true) }) return all.take(QuickPickBase)
    return all.filter { it.contains(q, ignoreCase = true) }.take(QuickPickFiltered)
}

/** A typed exercise name as it is saved: trimmed, inner runs of spaces collapsed,
 *  and an all-lower-case entry in sentence case ("dumbbell curl" → "Dumbbell
 *  curl"); a name the user cased themselves ("RDL", "McGill curl-up") is kept.
 *  Pure. */
internal fun exerciseDisplayName(typed: String): String {
    val t = typed.trim().replace(Regex("""\s+"""), " ")
    if (t.isEmpty() || t.any { it.isUpperCase() }) return t
    return t.replaceFirstChar { it.uppercaseChar() }
}

/** The name a set is saved under: the known spelling (plan item / own history)
 *  of a name typed in any case, else [exerciseDisplayName] of what was typed. Pure. */
internal fun resolveExerciseName(typed: String, known: List<String>): String =
    known.firstOrNull { it.trim().equals(typed.trim(), ignoreCase = true) }?.trim() ?: exerciseDisplayName(typed)

/** The new exercise the search offers to add ("+ Add 'Dumbbell curl'"): the
 *  typed name in its saved form when it matches no known exercise; null while
 *  nothing is typed or the name is already known. Pure. */
internal fun newExerciseToAdd(typed: String, known: List<String>): String? {
    if (typed.isBlank() || known.any { it.trim().equals(typed.trim(), ignoreCase = true) }) return null
    return exerciseDisplayName(typed)
}

/** The plan item a set of [exercise] belongs to: a lift or calisthenics item of
 *  that name (case-insensitive). Never a run item, whose exercise is blank, so a
 *  free set opened from the Log chooser on a run day carries no run prescription.
 *  Null while no exercise is named. Pure. */
internal fun planItemFor(items: List<PrescriptionView>, exercise: String): PrescriptionView? {
    val name = exercise.trim()
    if (name.isEmpty()) return null
    return items.firstOrNull { it.exercise.isNotBlank() && it.exercise.trim().equals(name, ignoreCase = true) }
}

/** Whether the set editor's exercise is settled, so its weight, reps and effort
 *  fields show: an edited set, a name the app knows (plan item, own history), the
 *  exercise the editor opened on ([preset]) or one confirmed with "+ Add"
 *  ([added]). A half-typed search term is not. Pure. */
internal fun exerciseChosen(exercise: String, known: List<String>, added: String?, preset: String?, editing: Boolean): Boolean {
    val name = exercise.trim()
    if (name.isEmpty()) return false
    if (editing) return true
    return (known + listOfNotNull(added, preset)).any { it.trim().equals(name, ignoreCase = true) }
}

/** The sets logged today (since [dayStartSec]) for the loop's exercise, oldest
 *  first: the plan item's own logged sets (matched by entry id, so a free-typed
 *  name variant the core attributed to the item still counts) plus any set with
 *  the same name (case-insensitive). Pure. */
internal fun todaySetsFor(
    exercise: String,
    item: PrescriptionView?,
    lifts: List<LiftResultView>,
    dayStartSec: Long,
): List<LiftResultView> {
    val ids = item?.logged_sets.orEmpty().map { it.entry_id }.filter { it != 0L }.toSet()
    val name = exercise.trim()
    return lifts.filter { l ->
        l.observed_at >= dayStartSec && (
            (l.entry_id != 0L && l.entry_id in ids) ||
                (name.isNotEmpty() && l.exercise.trim().equals(name, ignoreCase = true))
            )
    }.sortedBy { it.observed_at }
}

/** Plain "reps left" for an RPE on the RIR-based scale: 10 → "0 reps left",
 *  9 → "1 rep left", 8 → "2 reps left", 6 or below → "4+ reps left", a half
 *  point → a range ("1–2 reps left"). Always says "reps", so it never reads as
 *  sets. The one wording for effort in the editor, the loop and History. */
internal fun repsLeftText(res: Resources, rpe: Double): String {
    val rir = (10.0 - rpe).coerceAtLeast(0.0)
    val n = rir.toInt()
    return when {
        rir >= 4.0 -> res.getString(R.string.log_reps_left_4plus)
        rir % 1.0 == 0.0 -> res.getQuantityString(R.plurals.log_reps_left, n, n)
        else -> res.getString(R.string.log_reps_left_range, n, n + 1)
    }
}

/** What the set loop's pinned primary action does. */
internal enum class LoopPrimary { Save, LogPlannedSet, LogOffPlanSet, LogBlank, LogExtraSet, Next, FinishSession }

/** The loop's primary action. Outside the loop it is a plain Save. While the
 *  plan item has sets to go it logs the next one. Once the item is complete the
 *  big button moves on: "Next: <exercise>", or "Finish session" after the last
 *  open item, so a habitual extra tap never logs a junk set; only a set the user
 *  has started changing ([dirty]) makes it "Log extra set". An off-plan exercise
 *  logs "Set k"; nothing named yet reads as the sheet's "Log set". Pure. */
internal fun loopPrimaryAction(
    loop: Boolean,
    liveItem: PrescriptionView?,
    nextItem: PrescriptionView?,
    exerciseNamed: Boolean,
    dirty: Boolean,
): LoopPrimary = when {
    !loop -> LoopPrimary.Save
    liveItem != null && !liveItem.done -> LoopPrimary.LogPlannedSet
    liveItem != null && dirty -> LoopPrimary.LogExtraSet
    liveItem != null && nextItem != null -> LoopPrimary.Next
    liveItem != null -> LoopPrimary.FinishSession
    exerciseNamed -> LoopPrimary.LogOffPlanSet
    else -> LoopPrimary.LogBlank
}

/** The in-session cue the loop shows for [exercise]: the coach's item-scoped
 *  adjustment for it that changes the load or suggests ending the exercise, else
 *  today's session-review verdict to stop here (it names no exercise, so it
 *  applies to the lift in progress). Adjustments that change nothing in this
 *  session ("next time", a new best) stay on Today. Pure. */
internal fun inSessionCue(
    exercise: String,
    adjustments: List<AdjustmentView>,
    reviewAdjustments: List<AdjustmentView>,
): AdjustmentView? {
    val name = exercise.trim()
    return adjustments.firstOrNull {
        it.exercise?.trim()?.equals(name, ignoreCase = true) == true &&
            (it.suggested_load_kg != null || it.suggests_end_exercise)
    } ?: reviewAdjustments.firstOrNull { it.exercise == null && it.suggests_end_exercise }
}

/** The standard bar the core's plates-per-side breakdown assumes. */
private const val BarKg = 20.0
private const val BarLb = 45.0

/** Where a pull from the floor starts: the bar with one full-size plate a side
 *  (60 kg / 135 lb), the height a deadlift is set up at. */
private const val FloorStartKg = 60.0
private const val FloorStartLb = 135.0

/** The specific warm-up ramp of the knowledge base (warmup-001, STR-WARMUP-001):
 *  about 40% × 5, 60% × 3 and 80% × 2 of the WORKING load, ending on a high-load
 *  set; the exact steps are expert opinion. */
private val WarmUpRamp = listOf(0.4 to 5, 0.6 to 3, 0.8 to 2)

/** One one-tap warm-up step: its load (wire kg) and reps; [bar] marks the empty
 *  bar, labelled "Bar" rather than its figure. */
internal data class WarmUpStep(val kg: Double, val reps: Int, val bar: Boolean = false)

/** Whether [exercise] starts each rep from the floor (a deadlift, a clean or
 *  snatch from the floor, a Pendlay row). A Romanian or stiff-leg deadlift and a
 *  hang variant start from the top. Pure. */
internal fun isFloorLift(exercise: String): Boolean {
    val e = exercise.lowercase(Locale.ROOT)
    if ("romanian" in e || "rdl" in e || "stiff" in e || "hang" in e) return false
    return "deadlift" in e || "clean" in e || "snatch" in e || "pendlay" in e
}

/** One-tap warm-up steps before the first work set at [workKg] of a barbell
 *  lift: a first step (the empty bar × 5, or for a floor pull the plate-loaded
 *  start × 5), then the [WarmUpRamp] on the unit's plate grid (2.5 kg / 5 lb).
 *  A ramp step that rounds to the first step or up to the work load is dropped.
 *  Empty when the work load is at or below the first step. Pure. */
internal fun warmUpSteps(workKg: Double, unit: WeightUnit, exercise: String): List<WarmUpStep> {
    val lb = unit == WeightUnit.Lb
    val floor = isFloorLift(exercise)
    val firstKg = when {
        floor && lb -> lbToKgWire(FloorStartLb)
        floor -> FloorStartKg
        lb -> lbToKgWire(BarLb)
        else -> BarKg
    }
    if (workKg <= firstKg + 1e-6) return emptyList()
    val ramp = WarmUpRamp.map { (f, reps) ->
        val kg = if (lb) lbToKgWire(Math.round(kgToLb(workKg * f) / 5.0) * 5.0) else Math.round(workKg * f / 2.5) * 2.5
        WarmUpStep(kg, reps)
    }.filter { it.kg > firstKg + 1e-6 && it.kg < workKg - 1e-6 }.distinctBy { it.kg }
    return listOf(WarmUpStep(firstKg, 5, bar = !floor)) + ramp
}

/** [kg] in lb rounded to the nearest half pound, without trailing zeros. */
private fun halfPoundFigure(kg: Double): String = fmtLosslessPrefill(Math.round(kgToLb(kg) * 2.0) / 2.0)

/** The weight field's text for a stored [kg]: in lb the nearest half pound
 *  ("290", "292.5"), in kg the exact value without trailing zeros ("101.25",
 *  "100"). Display only: an untouched Save re-emits the exact kg. Pure. */
internal fun weightFieldText(kg: Double, unit: WeightUnit): String =
    if (unit == WeightUnit.Lb) halfPoundFigure(kg) else fmtLosslessPrefill(kg)

/** A load figure for set lines and snackbars: lb in whole or half pounds, kg
 *  to at most two decimals, never a trailing ".0" ("135", "101.25"). Pure. */
internal fun loadFigure(kg: Double, unit: WeightUnit): String =
    if (unit == WeightUnit.Lb) halfPoundFigure(kg) else plateFigure(kg)

/** A ± chip's "%.1f" result without a trailing ".0" ("295.0" → "295"). Pure. */
internal fun trimWeightText(text: String): String =
    text.toDoubleOrNull()?.let { fmtLosslessPrefill(it) } ?: text

/** The timed-hold tap scale, seconds: a prescribed hold band seeds inside it. */
internal val HoldSecRange = 3..120

/** The reps a set opens on when its prescription carries no rep band. */
private const val UnbandedRepsSeed = 5

/** The hold seconds a set opens on with no prescribed or logged hold. */
private const val UnbandedHoldSeed = 20

/** The weight field's ceiling in kg (lb converts it exactly): a plausibility
 *  bound for a typed set load, well above any real lift. */
internal const val SetWeightMaxKg = 400.0

/** The ± chips on a load field, in its display unit: a plate pair (±2.5 kg)
 *  and a bigger jump in kg, the natural ±5/10 lb steps in lb. */
internal fun plateSteps(unit: WeightUnit): List<Double> =
    if (unit == WeightUnit.Lb) PlateStepsLb else PlateStepsKg

private val PlateStepsKg = listOf(-2.5, 2.5, 5.0)
private val PlateStepsLb = listOf(-5.0, 5.0, 10.0)

/** NEW-entry seed from a coach prescription. weightText null → keep the editor's fresh default. */
internal data class SetPrefill(
    val exercise: String,
    val weightText: String?,   // weightFieldText display string; exact kg lives in weightKgVerbatim
    val weightKgVerbatim: Double?, // exact wire kg to emit on an untouched Save (lossless)
    val reps: Int,
    val holdSec: Int,
    // The prescribed effort, shown as a HINT on the effort scale only. Never a
    // field value: effort is the athlete's own per-set rating, so a NEW set opens
    // unrated whatever the target. Null when the item carries no RPE target.
    val targetRpe: Double?,
)

/** A cue load the lifter applied in the loop ("Use 295 lb") for one plan item,
 *  stamped when applied, so a reopened loop keeps it until a set is logged. */
internal data class AppliedCue(val kg: Double, val atSec: Long)

/** The cue loads applied in today's loops, process-scoped like the rest timer:
 *  closing and reopening the sheet must not drop the load the lifter chose. Keyed
 *  by the item's day and exercise; read on the main thread only. */
internal object AppliedCueLoads {
    private val byItem = HashMap<String, AppliedCue>()

    private fun key(item: PrescriptionView): String =
        "${localDayStartSec(AppClock.nowSec())}|${item.exercise.trim().lowercase(Locale.ROOT)}"

    fun record(item: PrescriptionView, kg: Double) {
        byItem[key(item)] = AppliedCue(kg, AppClock.nowSec())
    }

    fun forItem(item: PrescriptionView): AppliedCue? = byItem[key(item)]
}

/** Build a NEW-entry seed from a coach prescription. The weight is the first of
 *  a cue load applied ([appliedCue]) after the item's newest work set today, the
 *  load of that newest work set today (what the lifter is actually using: a
 *  reopened loop never falls back to the plan), `added_kg` (calisthenics "added
 *  load" semantics), `load_kg` (the prescribed load), `suggested_load_kg` (the
 *  core's start weight for an effort target) and the athlete's own `last_set`
 *  weight; with none, the editor's fresh default stays rather than inventing a
 *  load (HARD RULE 1). Reps and hold seconds take the band midpoint; the RPE
 *  target rides along as a hint, never for a ballistic power item. */
internal fun prescriptionPrefill(p: PrescriptionView, weightUnit: WeightUnit, appliedCue: AppliedCue? = null): SetPrefill {
    val lastWork = p.logged_sets.lastOrNull { it.qualifies }
    val cueKg = appliedCue?.takeIf { lastWork == null || it.atSec >= lastWork.observed_at }?.kg
    val kg = cueKg ?: lastWork?.weight_kg ?: p.added_kg ?: p.load_kg ?: p.suggested_load_kg
        ?: p.last_set?.takeIf { it.weight_kg > 0.0 && it.hold_sec == null }?.weight_kg
    // Display uses the field's own format (kg trimmed, lb to the half pound); an
    // untouched Save re-emits the exact `weightKgVerbatim` below, so this text
    // never carries the Save value and its rounding is display-only.
    val text = kg?.let { weightFieldText(it, weightUnit) }
    return SetPrefill(
        exercise = p.exercise,
        weightText = text,
        weightKgVerbatim = kg,
        // Band midpoints.
        reps = if (p.reps_low > 0) Math.round((p.reps_low + maxOf(p.reps_low, p.reps_high)) / 2.0).toInt() else UnbandedRepsSeed,
        holdSec = if (p.hold_low_sec > 0) {
            Math.round((p.hold_low_sec + maxOf(p.hold_low_sec, p.hold_high_sec)) / 2.0).toInt()
                .coerceIn(HoldSecRange.first, HoldSecRange.last)
        } else {
            UnbandedHoldSeed
        },
        targetRpe = if (p.calis?.ballistic == true) null else p.target_rpe,
    )
}

/** Immutable initial field values for [LogSetEditor], resolved from an edited row
 *  ([initial]) or a coach prescription ([rx]); with neither, exercise and weight
 *  seed EMPTY and reps/RPE stay unset (null) so the editor fabricates nothing
 *  (HARD RULE 1). RPE seeds ONLY from an edited row that recorded one: a
 *  prescription's target is never an answer. Pure (no Compose/Android state) so
 *  it is unit-testable. */
internal data class FreshSetSeed(
    val exercise: String,
    val weightText: String,
    val reps: Int?,
    val rpe: Double?,
    val holdSec: Int,
    // Exact wire kg to re-emit on an untouched Save (lossless): the edited row's
    // stored kg, or a prescription's verbatim load. Null when nothing seeded a
    // load. Held as one value so a chip-switch reseed swaps it wholesale and one
    // exercise's load never rides into another.
    val weightKgVerbatim: Double?,
)

internal fun freshSetSeed(
    initial: LiftResultView?,
    rx: SetPrefill?,
    weightUnit: WeightUnit,
): FreshSetSeed {
    val exercise = initial?.exercise ?: rx?.exercise ?: ""
    val weightText = when {
        // An edited row renders its stored kg verbatim (lossless) or the lb display
        // figure; the untouched-Save path re-emits the exact wire kg.
        initial != null -> weightFieldText(initial.weight_kg, weightUnit)
        rx?.weightText != null -> rx.weightText
        else -> ""
    }
    return FreshSetSeed(
        exercise = exercise,
        weightText = weightText,
        reps = (initial?.reps ?: rx?.reps)?.coerceAtLeast(1),
        // 0.0 on the wire means "not recorded": an unrated set stays unrated.
        rpe = initial?.rpe?.takeIf { it > 0.0 },
        holdSec = initial?.hold_sec ?: rx?.holdSec ?: UnbandedHoldSeed,
        weightKgVerbatim = initial?.weight_kg ?: rx?.weightKgVerbatim,
    )
}

/**
 * The set editor's entry fields, held as one saveable value so every re-seed of
 * the gym-floor loop is a pure reducer ([reseededTo], [afterLoggedSet],
 * [withWorkingLoad]). [weightFresh] is the calculator-style flag: the first key
 * after a (re)seed replaces the buffer, and an untouched Save re-emits the exact
 * [weightVerbatim] kg. [reps] null = no count picked; [rpe] null = unrated.
 */
internal data class SetEditorState(
    val exercise: String,
    val weightText: String,
    val weightFresh: Boolean,
    val weightVerbatim: Double?,
    val reps: Int?,
    val holdSec: Int,
    val rpe: Double?,
) {
    companion object {
        fun from(seed: FreshSetSeed) = SetEditorState(
            exercise = seed.exercise,
            weightText = seed.weightText,
            weightFresh = true,
            weightVerbatim = seed.weightKgVerbatim,
            reps = seed.reps,
            holdSec = seed.holdSec,
            rpe = seed.rpe,
        )

        val Saver: Saver<SetEditorState, Any> = listSaver(
            save = { listOf(it.exercise, it.weightText, it.weightFresh, it.weightVerbatim, it.reps, it.holdSec, it.rpe) },
            restore = {
                SetEditorState(
                    exercise = it[0] as String,
                    weightText = it[1] as String,
                    weightFresh = it[2] as Boolean,
                    weightVerbatim = it[3] as Double?,
                    reps = it[4] as Int?,
                    holdSec = it[5] as Int,
                    rpe = it[6] as Double?,
                )
            },
        )
    }
}

/** Every field re-seeded for [name] from its open plan item (in [items]), else
 *  its last logged set (in [recentLifts]), else just renamed, so a chip switch
 *  never carries one exercise's load into another. Effort is never seeded (it is
 *  rated fresh) and the buffer is fresh again. [cueFor] is the cue load the
 *  lifter applied to an item ([AppliedCueLoads.forItem]). Pure. */
internal fun SetEditorState.reseededTo(
    name: String,
    items: List<PrescriptionView>,
    recentLifts: List<LiftResultView>,
    weightUnit: WeightUnit,
    cueFor: (PrescriptionView) -> AppliedCue?,
): SetEditorState {
    val open = planItemFor(items.filter { !it.done }, name)
    val last = if (open == null) recentLifts.lastOrNull { it.exercise.trim().equals(name.trim(), ignoreCase = true) } else null
    val seeded = when {
        open != null -> {
            val rx = prescriptionPrefill(open, weightUnit, cueFor(open))
            copy(
                exercise = open.exercise,
                weightText = rx.weightText ?: if (open.timed_hold || open.added_kg == 0.0) "0" else "",
                weightVerbatim = rx.weightKgVerbatim,
                reps = rx.reps,
                holdSec = rx.holdSec,
            )
        }
        last != null -> copy(
            exercise = last.exercise,
            weightText = weightFieldText(last.weight_kg, weightUnit),
            weightVerbatim = last.weight_kg,
            reps = last.reps.coerceAtLeast(1),
            holdSec = last.hold_sec ?: holdSec,
        )
        // No plan item and no logged set to reseed from (an EDIT sheet, or an
        // unrecognised name): just rename, keeping the entered values.
        else -> copy(exercise = name)
    }
    return seeded.copy(rpe = null, weightFresh = true)
}

/** The fields after the loop logged [kg] under [name]: the load carries over
 *  (an untouched Save repeats the load actually logged), effort resets to
 *  unrated, and the reps (or hold seconds) go back to [liveItem]'s target so one
 *  short set never silently repeats as the next set's count. Pure. */
internal fun SetEditorState.afterLoggedSet(
    name: String,
    kg: Double,
    liveItem: PrescriptionView?,
    holdMode: Boolean,
    weightUnit: WeightUnit,
): SetEditorState {
    val logged = copy(exercise = name, weightVerbatim = kg, weightFresh = true, rpe = null)
    val item = liveItem ?: return logged
    val target = prescriptionPrefill(item, weightUnit)
    return when {
        holdMode -> if (item.hold_low_sec > 0) logged.copy(holdSec = target.holdSec) else logged
        item.reps_low > 0 -> logged.copy(reps = target.reps)
        else -> logged
    }
}

/** The fields with [item]'s working load restored after a warm-up, when the plan
 *  prescribes one (else unchanged); a hold keeps its seconds. Pure. */
internal fun SetEditorState.withWorkingLoad(
    item: PrescriptionView,
    holdMode: Boolean,
    weightUnit: WeightUnit,
    cueFor: (PrescriptionView) -> AppliedCue?,
): SetEditorState {
    val rx = prescriptionPrefill(item, weightUnit, cueFor(item))
    if (rx.weightText == null || rx.weightKgVerbatim == null) return this
    return copy(weightText = rx.weightText, weightVerbatim = rx.weightKgVerbatim, reps = if (holdMode) reps else rx.reps)
}

/** Planned set count for a lift/calisthenics item: at least 1 even when the core
 *  omitted `sets`, so "k of N" and the slot row never render 0 slots. Runs carry
 *  no set count (their completion is distance/time), so callers gate on discipline. */
internal fun plannedSetCount(item: PrescriptionView): Int = maxOf(item.sets, 1)

/** The 1-based set number the next Save logs, clamped to the planned count so a
 *  fully-logged item reads "N of N", never "N+1 of N". */
internal fun nextSetNumber(item: PrescriptionView): Int =
    (item.sets_done + 1).coerceIn(1, plannedSetCount(item))

/** The first not-done item of a session — the "Today's plan" prefill target and
 *  the hero's next-open item. Null when every item is done (next_session keeps
 *  done items, so the shell picks the open one). */
internal fun firstOpenItem(items: List<PrescriptionView>): PrescriptionView? =
    items.firstOrNull { !it.done }

/** The next open lift item to move to once [exercise] completes: the first
 *  not-done named item that is not [exercise] itself (a run item has no name and
 *  is logged elsewhere). Null when nothing else is open. */
internal fun nextOpenItem(items: List<PrescriptionView>, exercise: String): PrescriptionView? =
    items.firstOrNull { !it.done && it.exercise.isNotBlank() && !it.exercise.trim().equals(exercise.trim(), ignoreCase = true) }

/** The wire kg an editor Save emits: 0 for a bodyweight rung; the exact stored
 *  verbatim kg while the weight buffer is untouched ([weightFresh]) so an
 *  untouched Save is lossless and a loop repeat re-emits the load actually saved;
 *  otherwise the displayed buffer, converting lb→kg. Null when the buffer parses
 *  to nothing. Pure so the repeat-last-set invariant is unit-testable. */
internal fun resolveSaveWeightKg(
    bodyweightMode: Boolean,
    weightFresh: Boolean,
    weightVerbatim: Double?,
    weightText: String,
    weightUnit: WeightUnit,
): Double? = when {
    bodyweightMode -> 0.0
    weightFresh && weightVerbatim != null -> weightVerbatim
    else -> parseKeypadNumber(weightText)
        ?.let { if (weightUnit == WeightUnit.Lb) lbToKgWire(it) else it }
}

/** The primary History/line summary for a logged set. A timed hold renders its
 *  seconds ("30 s hold", "30 s hold · +10 kg" when loaded); a reps×load set
 *  renders "100 kg × 5", with " · 2 reps left" appended ONLY when an effort was
 *  recorded. A number and its unit never break across lines. Never fabricates
 *  an effort or a "0 kg × 1" hold. */
internal fun liftSetLine(
    res: Resources,
    holdSec: Int?,
    weightKg: Double,
    reps: Int,
    rpe: Double,
    rpeRecorded: Boolean,
    weightUnit: WeightUnit,
): String {
    val load = res.getString(R.string.log_value_with_unit, loadFigure(weightKg, weightUnit), weightSuffix(weightUnit))
    return if (holdSec != null) {
        if (weightKg > 0.0) {
            res.getString(R.string.log_set_line_hold_loaded, holdSec, load)
        } else {
            res.getString(R.string.log_set_line_hold, holdSec)
        }
    } else {
        val base = res.getString(R.string.log_set_line_reps, load, reps)
        if (rpeRecorded) res.getString(R.string.log_set_line_effort, base, repsLeftText(res, rpe)) else base
    }
}

/** Build the set event for the editor: a fresh [Event.LogSet] when adding, or an
 *  [Event.AmendSet] targeting the logged set when editing. A legacy row (no
 *  entry_id) is matched on its ORIGINAL observed_at, so its stamp is preserved. */
internal fun setEditEvent(
    initial: LiftResultView?,
    exercise: String,
    weightKg: Double,
    reps: Int,
    rpe: Double,
    observedAt: Long,
    holdSec: Int? = null,
): Event = when {
    initial == null -> Event.LogSet(exercise, weightKg, reps, rpe, observedAt, holdSec = holdSec)
    // observedAtFallback is the row's ORIGINAL stamp, so the core replaces the
    // matched row instead of duplicating it; especially a legacy row (id 0).
    initial.entry_id != 0L ->
        Event.AmendSet(initial.entry_id, exercise, weightKg, reps, rpe, observedAt, observedAtFallback = initial.observed_at, holdSec = holdSec)
    else -> Event.AmendSet(0L, exercise, weightKg, reps, rpe, initial.observed_at, observedAtFallback = initial.observed_at, holdSec = holdSec)
}

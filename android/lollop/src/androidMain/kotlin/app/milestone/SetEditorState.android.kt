package app.milestone

import android.content.res.Resources

// The set editor's text (SetEditorState.kt), from the Android string resources.

/** Plain "reps left" for an RPE on the RIR-based scale: 10 → "0 reps left",
 *  9 → "1 rep left", 8 → "2 reps left", 6 or below → "4+ reps left", a half
 *  point → a range ("1–2 reps left"). Always says "reps", so it never reads as
 *  sets. The one wording for effort in the editor, the loop and History; [short]
 *  is History's set-table column ("2 left"), beside a load that already says what
 *  is counted. */
internal fun repsLeftText(res: Resources, rpe: Double, short: Boolean = false): String {
    val rir = repsLeft(rpe)
    val n = rir.toInt()
    return when {
        rir >= 4.0 -> res.getString(if (short) R.string.history_set_left_4plus else R.string.log_reps_left_4plus)
        rir % 1.0 == 0.0 -> res.getQuantityString(if (short) R.plurals.history_set_left else R.plurals.log_reps_left, n, n)
        else -> res.getString(if (short) R.string.history_set_left_range else R.string.log_reps_left_range, n, n + 1)
    }
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

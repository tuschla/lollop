package app.milestone

/** Each of [names] as a match key, in order: the core's `names::exercise_key`
 *  (two spellings of one exercise, "İncline bench press" and "incline bench
 *  press", share a key), asked of the core in one call. */
internal fun nameKeys(names: List<String>): List<String> =
    if (names.isEmpty()) emptyList() else checkNotNull(AppCore.engine.exerciseKeys(names)) { "the core answered no exercise keys" }

internal fun nameKey(name: String): String = nameKeys(listOf(name)).single()

internal fun sameExercise(a: String, b: String): Boolean {
    if (a == b) return true
    val (ka, kb) = nameKeys(listOf(a, b))
    return ka == kb
}

/** The index of the first of [candidates] that names the same exercise as
 *  [name] (by [nameKeys], in one call), or -1. */
internal fun indexOfExercise(candidates: List<String>, name: String): Int {
    if (candidates.isEmpty()) return -1
    val keys = nameKeys(candidates + name)
    return keys.subList(0, candidates.size).indexOf(keys.last())
}

/** How many quick-pick chips show with nothing (or an exact pick) typed. */
private const val QuickPickBase = 6
/** How many filtered matches show while typing. */
private const val QuickPickFiltered = 8

/**
 * Exercise quick-pick chips: today's open plan items, then the user's recent
 * exercises, then the rest of the week's plan, so a busy plan week can't crowd
 * out the lifts the user actually logs.
 */
internal fun quickPickCandidates(
    query: String,
    todayOpen: List<String>,
    recents: List<String>,
    weekPlanned: List<String>,
): List<String> {
    val names = (todayOpen + recents + weekPlanned).map { it.trim() }.filter { it.isNotEmpty() }
    val keys = nameKeys(names + query)
    val q = keys.last()
    val seen = HashSet<String>()
    val all = names.indices.filter { seen.add(keys[it]) }
    val picks = if (q.isEmpty() || all.any { keys[it] == q }) {
        all.take(QuickPickBase)
    } else {
        all.filter { q in keys[it] }.take(QuickPickFiltered)
    }
    return picks.map { names[it] }
}

/** A typed exercise name as it is saved: trimmed, inner runs of spaces collapsed,
 *  and an all-lower-case entry in sentence case ("dumbbell curl" → "Dumbbell
 *  curl"); a name the user cased themselves ("RDL", "McGill curl-up") is kept. */
internal fun exerciseDisplayName(typed: String): String {
    val t = typed.trim().replace(Regex("""\s+"""), " ")
    if (t.isEmpty() || t.any { it.isUpperCase() }) return t
    return t.replaceFirstChar { it.uppercaseChar() }
}

/** The name a set is saved under: the known spelling (plan item / own history)
 *  of a name typed in any case, else [exerciseDisplayName] of what was typed. */
internal fun resolveExerciseName(typed: String, known: List<String>): String =
    known.getOrNull(indexOfExercise(known, typed))?.trim() ?: exerciseDisplayName(typed)

/** The new exercise the search offers to add ("+ Add 'Dumbbell curl'"): the
 *  typed name in its saved form when it matches no known exercise; null while
 *  nothing is typed or the name is already known. */
internal fun newExerciseToAdd(typed: String, known: List<String>): String? {
    if (typed.isBlank() || indexOfExercise(known, typed) >= 0) return null
    return exerciseDisplayName(typed)
}

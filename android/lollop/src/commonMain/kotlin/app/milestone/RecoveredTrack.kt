package app.milestone

/**
 * A run read back from the crash-safe active-run file (Android: ActiveRunStore):
 * its fixes, as a plain list, plus the indices (ascending, ≥ 1) of the fixes that
 * opened a new recording segment live (a Pause/Resume or a long fix gap).
 * [RunSession.restore] breaks the recovered track at exactly these, so it counts
 * no leg the live run did not.
 */
class RecoveredTrack(
    private val points: List<GpsPoint>,
    val segmentStarts: List<Int>,
) : AbstractList<GpsPoint>() {
    override val size: Int get() = points.size
    override fun get(index: Int): GpsPoint = points[index]
}

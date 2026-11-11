package app.milestone

import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized

/** Monotonic per-entry id source. The wall clock ([wallClockMillis]) alone collides
 *  when two entries are created within the same millisecond (a multi-activity
 *  TCX/GPX import builds several runs in a tight loop), which would make a later
 *  DeleteEntry/AmendSet target the wrong row. Threading each id through this
 *  counter guarantees strict uniqueness while staying epoch-millis-shaped. */
object EntryIds : SynchronizedObject() {
    private var last = 0L

    fun next(): Long = synchronized(this) {
        last = maxOf(wallClockMillis(), last + 1)
        last
    }

    /** Never hand out an id at or below [floor]: boot raises it to the largest
     *  restored entry id, so a wall clock moved backwards can't reuse one. */
    fun raiseTo(floor: Long) = synchronized(this) {
        last = maxOf(last, floor)
    }
}

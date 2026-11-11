package app.milestone

import kotlinx.atomicfu.locks.SynchronizedObject
import kotlinx.atomicfu.locks.synchronized
import kotlinx.serialization.json.JsonElement

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

/** The event's wire JSON, the line the durable log stores. */
fun Event.toJson(): JsonElement = coreJson.encodeToJsonElement(EventSerializer, this)

/** Whether this event is written to the durable log when the core accepts it: all
 *  but [Event.SetToday], the shell's clock, re-sent every foreground and kept as one
 *  last-write-wins line in memory only. */
val Event.persist: Boolean get() = this !is Event.SetToday

/** Any of the four overtraining signs ticked. */
val OvertrainingSigns.any: Boolean get() = worsePerformance || lowMood || higherRestingHr || worseSleep

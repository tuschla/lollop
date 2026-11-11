package app.milestone

import kotlinx.serialization.json.JsonElement

/** The event's wire JSON, the line the durable log stores. */
fun Event.toJson(): JsonElement = coreJson.encodeToJsonElement(EventSerializer, this)

/** Whether this event is written to the durable log when the core accepts it: all
 *  but [Event.SetToday], the shell's clock, re-sent every foreground and kept as one
 *  last-write-wins line in memory only. */
val Event.persist: Boolean get() = this !is Event.SetToday

/** Any of the four overtraining signs ticked. */
val OvertrainingSigns.anyTicked: Boolean get() = worsePerformance || lowMood || higherRestingHr || worseSleep

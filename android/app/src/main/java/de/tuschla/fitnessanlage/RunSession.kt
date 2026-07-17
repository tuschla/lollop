package de.tuschla.fitnessanlage

import java.util.Locale
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update

/**
 * In-memory holder the foreground [RunTrackingService] writes and the UI
 * observes. Living here rather than inside the Composable is what lets a run
 * keep recording while the screen is off or the app is backgrounded: the
 * service owns the location stream, the screen just renders this state.
 */
object RunSession {
    val points = MutableStateFlow<List<GpsPoint>>(emptyList())
    val tracking = MutableStateFlow(false)

    // update {} is atomic under concurrent writes from the location callback,
    // so rapid fixes can't clobber each other via read-modify-write races.
    fun add(p: GpsPoint) {
        points.update { it + p }
    }

    fun reset() {
        points.value = emptyList()
        tracking.value = false
    }
}

/**
 * Elapsed run time: `m:ss` under an hour, `h:mm:ss` at or beyond one, so a long
 * run (half/marathon, both goal distances the app supports) reads "1:12:05"
 * rather than a confusing "72:05". Negative spans clamp to zero.
 */
fun formatElapsed(seconds: Long): String {
    val s = seconds.coerceAtLeast(0L)
    return if (s >= 3600) {
        "%d:%02d:%02d".format(Locale.US, s / 3600, (s % 3600) / 60, s % 60)
    } else {
        "%d:%02d".format(Locale.US, s / 60, s % 60)
    }
}

package app.milestone.widget

import android.content.Context
import app.milestone.AppFileNames
import app.milestone.PlatformLog
import app.milestone.todayEpochDay
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.Json

/**
 * The snapshot file ([AppFileNames.WIDGET_SNAPSHOT] in `filesDir`). Writes go through a temp file
 * and a rename, so a reader never sees half a file. [revision] ticks on every
 * write or stale mark, which recomposes a widget session that is still running.
 */
internal object WidgetSnapshotStore {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val lock = Any()
    private val _revision = MutableStateFlow(0L)
    val revision: StateFlow<Long> = _revision.asStateFlow()
    private val _staleMarks = MutableStateFlow(0L)

    /** Ticks when a stale mark leaves the stored snapshot on [markStale]'s day,
     *  so [WidgetSurfaces] derives it again from the model the app process holds:
     *  no new model is published for it. */
    val staleMarks: StateFlow<Long> = _staleMarks.asStateFlow()

    private fun file(ctx: Context) = File(ctx.filesDir, AppFileNames.WIDGET_SNAPSHOT)

    /** The stored snapshot as written, or null when missing, unreadable or of another version. */
    fun read(ctx: Context): WidgetSnapshot? = synchronized(lock) {
        val f = file(ctx)
        if (!f.exists()) return null
        runCatching { json.decodeFromString<WidgetSnapshot>(f.readText()) }
            .onFailure { PlatformLog.w("WidgetSnapshot", "unreadable snapshot", it) }
            .getOrNull()
            ?.takeIf { it.version == WIDGET_SNAPSHOT_VERSION }
    }

    /**
     * The stored snapshot when it describes [today] and is not marked stale; null
     * otherwise, so a snapshot from another day never shows as today.
     */
    fun today(ctx: Context, today: Long = todayEpochDay()): WidgetSnapshot? =
        read(ctx)?.takeIf { !it.stale && it.epochDay == today }

    /** Store [snapshot]; false (and no write) when the file already holds it. */
    fun write(ctx: Context, snapshot: WidgetSnapshot): Boolean = synchronized(lock) {
        if (read(ctx) == snapshot) return false
        val f = file(ctx)
        val tmp = File(f.parentFile, AppFileNames.WIDGET_SNAPSHOT + AppFileNames.TMP_SUFFIX)
        tmp.writeText(json.encodeToString(WidgetSnapshot.serializer(), snapshot))
        if (!tmp.renameTo(f)) {
            tmp.delete()
            error("could not replace $f")
        }
        _revision.value += 1
        true
    }

    /**
     * Mark the stored snapshot stale: always when [force] (a time zone change),
     * else only when it describes a day other than [today] (the midnight tick, a
     * clock change). True when it marked one. The next published model writes a
     * fresh one; while the stored snapshot describes [today] and is stale (a zone
     * change within the day, or a day that came back after a zone or clock change)
     * [staleMarks] ticks, so it is derived again at once from the model the app
     * process holds. Across a day change that model is the old day's as well, so
     * it waits for the app to set the new day.
     */
    fun markStale(ctx: Context, force: Boolean, today: Long = todayEpochDay()): Boolean {
        val (marked, staleToday) = synchronized(lock) {
            val current = read(ctx) ?: return false
            val mark = !current.stale && (force || current.epochDay != today)
            if (mark) write(ctx, current.copy(stale = true))
            mark to (current.epochDay == today && (mark || current.stale))
        }
        if (staleToday) _staleMarks.value += 1
        return marked
    }
}

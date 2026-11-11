package app.milestone

import android.content.Context
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.core.app.ApplicationProvider
import java.io.File
import java.time.LocalDateTime
import java.time.ZoneOffset
import java.util.Locale
import java.util.TimeZone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Run edge cases: a run's title vs its date chip across midnight, and an old crash
 * sidecar without segment marks. (The import's cases, Strava's compound sport types,
 * the GPX logged-at stamp and nested `<type>`s, truncated GPX files, are core tests:
 * `shared/src/import/tests.rs`.)
 */
@RunWith(RobolectricTestRunner::class)
class ReviewRunsTest {

    @get:Rule
    val rule = createComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    private fun utc(y: Int, mo: Int, d: Int, h: Int, mi: Int): Long =
        LocalDateTime.of(y, mo, d, h, mi).toEpochSecond(ZoneOffset.UTC)

    @Test
    fun aRunAcrossMidnightIsNotTitledWithADifferentDayThanItsDate() {
        Locale.setDefault(Locale.US)
        TimeZone.setDefault(TimeZone.getTimeZone("UTC"))
        // Sun 2026-10-18 23:40 → Mon 2026-10-19 00:20 (a tracked run: started_at is
        // its first fix, observed_at its last). Viewed two weeks later, so the chip
        // is an absolute date.
        val run = RunResultView(
            distance_km = 7.0,
            duration_min = 40.0,
            observed_at = utc(2026, 10, 19, 0, 20),
            started_at = utc(2026, 10, 18, 23, 40),
            entry_id = 1,
            has_gpx = true,
        )
        val now = utc(2026, 11, 2, 12, 0) * 1000L
        var title = ""
        var chip = ""
        rule.setContent {
            title = runTitle(run)
            chip = runDateChip(run, now)
        }
        rule.waitForIdle()
        // One label, one day: the title names the START weekday (Sun Oct 18), so the
        // chip beside it must date that same day, never Monday Oct 19 (its end).
        assertEquals(formatLogDate(run.started_at, now), chip)
        assertTrue("chip \"$chip\" dates the start day", "18" in chip)
        assertEquals("Sunday run", title)
    }

    @Test
    fun anOldSidecarWithoutSegmentMarkersStillRecoversByGapRule() {
        // A sidecar from an older app carries no "seg" keys. It still recovers
        // every fix, with boundaries re-derived from fix-time gaps only.
        val f = File(ctx.filesDir, AppFileNames.ACTIVE_RUN)
        ActiveRunStore.start(ctx)
        val lines = buildList {
            add("{\"run\":\"active\"}")
            for (t in 0 until 20) add("{\"lat\":0.0,\"lon\":${t * 3e-5},\"observed_at\":${1_790_000_000 + t},\"accuracy_m\":5.0}")
            // 60 s without a fix (a gap over MAX_FIX_GAP_SEC), then 10 more.
            for (t in 80 until 90) add("{\"lat\":0.0,\"lon\":${t * 3e-5},\"observed_at\":${1_790_000_000 + t},\"accuracy_m\":5.0}")
            add("{\"lat\":0.0,\"lon\":0.1,\"obse") // torn final line
        }
        f.writeText(lines.joinToString("\n"))
        val recovered = ActiveRunStore.recover()
        assertEquals(30, recovered.size)
        assertEquals(emptyList<Int>(), recovered.segmentStarts)
        RunSession.reset()
        RunSession.restore(recovered)
        // The gap leg is a boundary; 28 one-second legs remain moving.
        assertEquals(28L, RunSession.elapsedSec.value)
        RunSession.reset()
        f.delete()
    }
}

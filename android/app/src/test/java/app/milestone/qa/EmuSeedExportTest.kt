package app.milestone.qa

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test

/**
 * Writes the [LongHistorySeed] history as the app's own event-log ndjson, for
 * `scripts/emulator-scenarios.sh` to push onto a device (startup time + heap of a
 * 5-year log on a real Android runtime). Skipped unless `EMU_SEED_OUT` names the
 * output file, so the normal unit suite never writes ~50 MiB.
 *
 *   EMU_SEED_OUT=/path/five-year.ndjson [EMU_SEED_DAYS=1825] [EMU_SEED_FIX_SEC=4] \
 *     ./gradlew :app:testDebugUnitTest --tests app.milestone.qa.EmuSeedExportTest
 */
class EmuSeedExportTest {
    @Test
    fun writeLongHistoryLog() {
        val out = System.getenv("EMU_SEED_OUT")
        assumeTrue("EMU_SEED_OUT not set", !out.isNullOrBlank())
        val days = System.getenv("EMU_SEED_DAYS")?.toIntOrNull() ?: (5 * 365)
        val fixSec = System.getenv("EMU_SEED_FIX_SEC")?.toLongOrNull() ?: 4L
        val history = LongHistorySeed.build(days = days, fixEverySec = fixSec)
        val f = File(out!!)
        f.parentFile?.mkdirs()
        f.bufferedWriter().use { w ->
            history.events.forEach { w.write(it.toJson().toString()); w.write("\n") }
        }
        // Sidecar facts the script checks the device against.
        File("$out.facts").writeText(
            "events=${history.events.size}\nsets=${history.sets.size}\nruns=${history.runs.size}\n" +
                "today_epoch_day=${history.today}\nbytes=${f.length()}\n",
        )
        assertTrue(f.length() > 0)
    }
}

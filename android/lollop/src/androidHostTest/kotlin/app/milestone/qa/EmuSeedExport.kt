package app.milestone.qa

import app.milestone.toJson
import java.io.File

/**
 * Writes the 5-year [LongHistorySeed] history as the app's own event-log ndjson
 * to `args[0]`, plus `<args[0]>.facts` (event/set/run counts, today, size) the
 * script checks the device against. `scripts/emulator-scenarios.sh` pushes it
 * onto a device (startup time + heap of a 5-year log on a real Android runtime):
 *
 *   ./gradlew :lollop:exportLongHistorySeed --args=/path/five-year.ndjson
 */
fun main(args: Array<String>) {
    val out = File(requireNotNull(args.singleOrNull()) { "usage: exportLongHistorySeed --args=OUT.ndjson" })
    val history = LongHistorySeed.build(days = 5 * 365, fixEverySec = 4L)
    out.parentFile?.mkdirs()
    out.bufferedWriter().use { w ->
        history.events.forEach { w.write(it.toJson().toString()); w.write("\n") }
    }
    File("$out.facts").writeText(
        "events=${history.events.size}\nsets=${history.sets.size}\nruns=${history.runs.size}\n" +
            "today_epoch_day=${history.today}\nbytes=${out.length()}\n",
    )
}

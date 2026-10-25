package app.milestone

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Rust ↔ Kotlin compaction differential on generated logs. The committed corpus
 * `shared/tests/fixtures/compaction_parity/<seed>.{in,expected}.ndjson` is written
 * by the Rust test `review_ledger::compaction_parity_corpus_is_current` (re-bless
 * with `BLESS_COMPACTION_PARITY=1`): seeded run/profile/clock-heavy logs (typed-bpm
 * and GPS runs with and without a stamped UTC offset, legacy id-0 stamp
 * collisions, deletes and amends across other runs, trims, dated / undated /
 * same-day timed profile edits, offset-changing SetToday, check-ins, clears)
 * together with the Rust `compact_event_log` output. Each case must compact to
 * the identical line list in [EventLog.compact]. The corpus is a test resource
 * (the test source set includes `../../shared/tests/fixtures`); a missing or
 * empty corpus FAILS, never skips.
 */
class ReviewLedgerTest {

    private fun corpusDir(): File {
        val relative = File("../../shared/tests/fixtures/compaction_parity")
        if (relative.isDirectory) return relative
        val resource = javaClass.classLoader?.getResource("compaction_parity")
            ?: error("compaction_parity corpus is not on the test classpath")
        return File(resource.toURI())
    }

    @Test
    fun kotlinCompactionMatchesRustOnGeneratedLogs() {
        val dir = corpusDir()
        val cases = dir.listFiles { f -> f.name.endsWith(".in.ndjson") }
            ?.map { it.name.removeSuffix(".in.ndjson") }
            ?.sorted()
            .orEmpty()
        assertTrue("no parity corpus in ${dir.absolutePath}", cases.size >= 30)
        val failures = mutableListOf<String>()
        for (case in cases) {
            val input = File(dir, "$case.in.ndjson").readLines().filter { it.isNotBlank() }
            val expected = File(dir, "$case.expected.ndjson").readLines().filter { it.isNotBlank() }
            val actual = EventLog.compact(input)
            if (actual != expected) {
                val i = expected.indices.firstOrNull { it >= actual.size || actual[it] != expected[it] }
                    ?: expected.size
                failures += "$case: rust ${expected.size} lines, kotlin ${actual.size}; first divergence at $i: " +
                    "rust=${expected.getOrNull(i)?.take(160)} kotlin=${actual.getOrNull(i)?.take(160)}"
            }
        }
        assertTrue("${failures.size}/${cases.size} cases diverged:\n" + failures.take(10).joinToString("\n"), failures.isEmpty())
    }
}

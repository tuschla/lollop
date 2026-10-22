package app.milestone.qa

import app.milestone.Core
import app.milestone.Event
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Proves the offline QA harness can drive the REAL Rust core over JNI on the
 * host JVM (the `jni-host` feature): `System.loadLibrary("shared")` resolves the
 * host `libshared.so` gradle put on `java.library.path`, and the test-only
 * [Core.resetForTest] bridge reset works. If this fails, none of the Compose
 * scenario/screenshot tests below can run, so it is the canary.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class CoreHostSmokeTest {

    @Test
    fun realCoreLoadsViaJniAndResets() {
        Core.resetForTest()
        // SetToday is a non-persisted clock event, so it needs no log file: it
        // exercises update() + view() end-to-end through the real core.
        val result = Core.send(Event.SetToday(epochDay = 20_000L, utcOffsetSec = 0))
        assertTrue("core accepted SetToday", result.accepted)
        assertNotNull("view() decoded to a ViewModel", result.view)
        // A fresh core has no plan and is not blocked; the reset gave us a clean slate.
        assertTrue("fresh core is not train-blocked", !result.view.train_blocked)
    }
}

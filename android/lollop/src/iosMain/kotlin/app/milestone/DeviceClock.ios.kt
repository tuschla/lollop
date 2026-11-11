package app.milestone

import kotlinx.cinterop.ExperimentalForeignApi
import platform.Foundation.NSDate
import platform.Foundation.timeIntervalSince1970
import platform.posix.CLOCK_MONOTONIC
import platform.posix.clock_gettime_nsec_np

internal actual fun wallClockMillis(): Long = (NSDate().timeIntervalSince1970 * 1000.0).toLong()

// Darwin's CLOCK_MONOTONIC keeps counting while the device sleeps, as
// elapsedRealtime does (CLOCK_UPTIME_RAW would pause).
@OptIn(ExperimentalForeignApi::class)
internal actual fun deviceMonotonicMillis(): Long = (clock_gettime_nsec_np(CLOCK_MONOTONIC.toUInt()) / 1_000_000uL).toLong()

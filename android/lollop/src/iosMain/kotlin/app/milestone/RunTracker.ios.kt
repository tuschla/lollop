@file:OptIn(ExperimentalForeignApi::class)

package app.milestone

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.useContents
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import platform.AudioToolbox.AudioServicesPlaySystemSound
import platform.AudioToolbox.kSystemSoundID_Vibrate
import platform.CoreLocation.CLAccuracyAuthorization
import platform.CoreLocation.CLActivityTypeFitness
import platform.CoreLocation.CLLocation
import platform.CoreLocation.CLLocationManager
import platform.CoreLocation.CLLocationManagerDelegateProtocol
import platform.CoreLocation.kCLDistanceFilterNone
import platform.CoreLocation.kCLErrorDenied
import platform.CoreLocation.kCLErrorDomain
import platform.CoreLocation.kCLLocationAccuracyBest
import platform.Foundation.NSBundle
import platform.Foundation.NSError
import platform.Foundation.NSNotificationCenter
import platform.Foundation.NSOperationQueue
import platform.Foundation.timeIntervalSince1970
import platform.UIKit.UIApplicationWillTerminateNotification
import platform.darwin.DISPATCH_TIME_NOW
import platform.darwin.NSObject
import platform.darwin.dispatch_after
import platform.darwin.dispatch_get_main_queue
import platform.darwin.dispatch_time
import platform.posix.CLOCK_MONOTONIC
import platform.posix.clock_gettime_nsec_np

internal actual fun platformRunTracker(): RunTracker? = IosRunTracker

/**
 * [RunTracker] over CoreLocation: When-In-Use authorization (precise, or a
 * temporary full-accuracy grant on top of approximate), a best-accuracy fitness
 * stream that keeps running in the background (`UIBackgroundModes` `location`, the
 * blue indicator), the shared [RunRecorder] state machine and the common
 * [ActiveRunStore] sidecar. iOS has no ongoing notification and no battery
 * exemption, so neither is asked for; the run's system surface is the
 * [RunActivity] hook. Main thread only.
 */
internal object IosRunTracker : RunTracker {
    /** The recording stream, made by the first locate / arm / start. */
    var recording: IosRunRecording? = null
        private set

    private fun recording(): IosRunRecording =
        recording ?: IosRunRecording(PlatformContext.INSTANCE, CoreLocationStream()).also { recording = it }

    // The manager the grant, the prompts and the last-known fix go through; its
    // delegate reports every grant or location-services change. Made with the
    // object, on the main thread (the tracker is first reached from composition).
    private val authListeners = mutableListOf<() -> Unit>()
    private val authDelegate = AuthorizationDelegate { authListeners.toList().forEach { it() } }
    private val auth = CLLocationManager().also { it.delegate = authDelegate }

    override fun hasLocationPermission(ctx: PlatformContext): Boolean = preciseGranted(auth)

    override fun hasApproximateLocation(ctx: PlatformContext): Boolean = whenInUseGranted(auth)

    /** Denied or restricted: iOS won't show its prompt again, only Settings can
     *  change it. Before the first ask the status is "not determined", so false. */
    override fun locationPromptBlocked(ctx: PlatformContext): Boolean =
        auth.authorizationStatus.let { it == AUTH_DENIED || it == AUTH_RESTRICTED }

    /** The run posts no notification on iOS (no ongoing notification; the Live
     *  Activity needs no notification grant), so there is nothing to ask for. */
    override fun canPostNotifications(ctx: PlatformContext): Boolean = true

    override fun isLocationEnabled(ctx: PlatformContext): Boolean = CLLocationManager.locationServicesEnabled()

    /** iOS has no per-app battery restriction that stops a foreground-started
     *  background location stream; Low Power Mode doesn't stop it either. */
    override fun isBatteryUnrestricted(ctx: PlatformContext): Boolean = true

    /** Never offered ([isBatteryUnrestricted] is always true); the app's settings
     *  page is the nearest place should a caller ask. */
    override fun requestBatteryUnrestricted(ctx: PlatformContext) = openAppSettings(ctx)

    /** iOS doesn't let an app open the Location Services switch; the app's page in
     *  Settings has its location row. */
    override fun openLocationSettings(ctx: PlatformContext) = openAppSettings(ctx)

    override fun openNotificationSettings(ctx: PlatformContext) = openAppSettings(ctx)

    /** CoreLocation reports a grant change and a location-services toggle the same
     *  way: the manager's authorization callback. */
    override fun observeLocationSettings(ctx: PlatformContext, onChange: () -> Unit): () -> Unit {
        val listener: () -> Unit = { onChange() }
        authListeners += listener
        return { authListeners -= listener }
    }

    @Composable
    override fun rememberPermissionPrompts(
        onLocationResult: (preciseGranted: Boolean) -> Unit,
        onNotificationResult: () -> Unit,
    ): PermissionPrompts {
        val onLocation by rememberUpdatedState(onLocationResult)
        return remember {
            object : PermissionPrompts {
                override fun requestLocation() = askLocation { onLocation(it) }

                // Nothing to ask ([canPostNotifications]).
                override fun requestNotifications() = Unit
            }
        }
    }

    /**
     * The system's location prompts, in Android's order: When-In-Use while it was
     * never asked; with only approximate location, the one-off precise prompt
     * (`requestTemporaryFullAccuracyAuthorization`, purpose key [PRECISE_PURPOSE]);
     * denied or restricted shows nothing. [onResult] gets whether precise location
     * is granted once the prompt is answered (at once when none shows).
     */
    private fun askLocation(onResult: (Boolean) -> Unit) {
        when {
            auth.authorizationStatus == AUTH_NOT_DETERMINED -> {
                lateinit var listener: () -> Unit
                listener = {
                    if (auth.authorizationStatus != AUTH_NOT_DETERMINED) {
                        authListeners -= listener
                        if (whenInUseGranted(auth) && !preciseGranted(auth)) askPrecise(onResult) else onResult(preciseGranted(auth))
                    }
                }
                authListeners += listener
                auth.requestWhenInUseAuthorization()
            }
            whenInUseGranted(auth) && !preciseGranted(auth) -> askPrecise(onResult)
            else -> onResult(preciseGranted(auth))
        }
    }

    private fun askPrecise(onResult: (Boolean) -> Unit) {
        auth.requestTemporaryFullAccuracyAuthorizationWithPurposeKey(PRECISE_PURPOSE) { _ ->
            onResult(preciseGranted(auth))
        }
    }

    override fun lastKnownLocation(ctx: PlatformContext, onResult: (lat: Double, lon: Double) -> Unit) {
        if (!whenInUseGranted(auth)) return
        auth.location?.coordinate?.useContents { onResult(latitude, longitude) }
    }

    override fun locate(ctx: PlatformContext) = recording().recorder.start(record = false)

    override fun arm(ctx: PlatformContext, requirePrecise: Boolean, requireMoving: Boolean) =
        recording().recorder.start(record = false, arming = true, requirePrecise = requirePrecise, requireMoving = requireMoving)

    override fun startNow(ctx: PlatformContext) = recording().recorder.startNow()

    override fun startRecording(ctx: PlatformContext) = recording().recorder.start(record = true)

    override fun stop(ctx: PlatformContext) = recording().recorder.stop()

    override fun hasInterruptedRun(ctx: PlatformContext): Boolean = ActiveRunStore.hasActiveRun(ctx)

    override fun recoverInterruptedRun(ctx: PlatformContext): RecoveredTrack = ActiveRunStore.recover(ctx)

    override fun discardInterruptedRun() = ActiveRunStore.clear()

    override fun discardInterruptedRunSync() = ActiveRunStore.clearSync()
}

/** The Info.plist key under `NSLocationTemporaryUsageDescriptionDictionary` whose
 *  text the one-off precise-location prompt shows. */
private const val PRECISE_PURPOSE = "RunTracking"

// CLAuthorizationStatus values (CLLocationManager.h).
private const val AUTH_NOT_DETERMINED = 0
private const val AUTH_RESTRICTED = 1
private const val AUTH_DENIED = 2
private const val AUTH_ALWAYS = 3
private const val AUTH_WHEN_IN_USE = 4

private fun whenInUseGranted(m: CLLocationManager): Boolean =
    m.authorizationStatus.let { it == AUTH_WHEN_IN_USE || it == AUTH_ALWAYS }

/** Precise location: a grant at full accuracy. Approximate alone can't measure a
 *  route (its fixes are kilometres wide). */
private fun preciseGranted(m: CLLocationManager): Boolean =
    whenInUseGranted(m) && m.accuracyAuthorization == CLAccuracyAuthorization.CLAccuracyAuthorizationFullAccuracy

/** Reports every authorization or location-services change of its manager. */
private class AuthorizationDelegate(private val onChange: () -> Unit) : NSObject(), CLLocationManagerDelegateProtocol {
    override fun locationManagerDidChangeAuthorization(manager: CLLocationManager) = onChange()
}

/** The location stream [IosRunRecording] drives: CoreLocation in the app, a fake in
 *  the simulator tests (which feed the delegate themselves). */
internal interface LocationStream {
    /** Precise When-In-Use location is granted right now. */
    fun preciseGranted(): Boolean

    /** Start delivering fixes to [delegate]. */
    fun start(delegate: CLLocationManagerDelegateProtocol)

    /** Stop delivering; a safe no-op when stopped. */
    fun stop()
}

/** [LocationStream] over one CLLocationManager configured for a run. */
private class CoreLocationStream : LocationStream {
    private val manager: CLLocationManager by lazy {
        CLLocationManager().apply {
            desiredAccuracy = kCLLocationAccuracyBest
            activityType = CLActivityTypeFitness
            // Every fix, as Android's GPS request (no minimum displacement):
            // standing still is the moving-speed floor's job.
            distanceFilter = kCLDistanceFilterNone
            // A runner at a red light must not have the stream paused under them.
            pausesLocationUpdatesAutomatically = false
        }
    }

    override fun preciseGranted(): Boolean = preciseGranted(manager)

    override fun start(delegate: CLLocationManagerDelegateProtocol) {
        manager.delegate = delegate
        // Without the `location` background mode in Info.plist this setter throws;
        // the stream then stops when the app leaves the screen.
        if (hasLocationBackgroundMode) {
            manager.allowsBackgroundLocationUpdates = true
            manager.showsBackgroundLocationIndicator = true
        }
        manager.startUpdatingLocation()
    }

    override fun stop() {
        manager.stopUpdatingLocation()
        if (hasLocationBackgroundMode) manager.allowsBackgroundLocationUpdates = false
    }

    private val hasLocationBackgroundMode: Boolean by lazy {
        (NSBundle.mainBundle.objectForInfoDictionaryKey("UIBackgroundModes") as? List<*>)?.contains("location") == true
    }
}

/** Now on the [monotonicMillis] clock, in nanoseconds. */
private fun monotonicNanos(): Long = clock_gettime_nsec_np(CLOCK_MONOTONIC.toUInt()).toLong()

/**
 * [fix] as the recorder takes it. CoreLocation stamps a fix with the wall clock
 * only, so its monotonic stamp is the delivery's monotonic now less the fix's age
 * ([nowWallMs] − its timestamp, never negative): a wall-clock step between two
 * deliveries can't bend moving time. A negative horizontal accuracy marks an invalid
 * fix, which carries no accuracy.
 */
internal fun liveFixOf(fix: CLLocation, nowWallMs: Long, nowMonoNanos: Long): LiveFix {
    val wallMs = (fix.timestamp.timeIntervalSince1970 * 1000.0).toLong()
    val ageNanos = (nowWallMs - wallMs).coerceAtLeast(0L) * 1_000_000L
    val (lat, lon) = fix.coordinate.useContents { latitude to longitude }
    return LiveFix(
        lat = lat,
        lon = lon,
        wallMs = wallMs,
        monoNanos = nowMonoNanos - ageNanos,
        accuracyM = fix.horizontalAccuracy.takeIf { it >= 0.0 },
    )
}

/**
 * One app's run recording on iOS: [RunRecorder] over a [LocationStream], with the
 * CoreLocation delegate that feeds it ([delegate]), main-queue timers, vibration
 * cues and the [RunActivity] surface. Process-wide; lives as long as the app.
 */
internal class IosRunRecording(private val ctx: PlatformContext, private val stream: LocationStream) {
    private var lastFrame: RunActivityState? = null

    val recorder = RunRecorder(Host())

    /** The stream's delegate: fixes in (oldest first), a revoked grant out. */
    val delegate: CLLocationManagerDelegateProtocol = StreamDelegate()

    init {
        // A swipe away from the app switcher while the app runs in the background
        // ends the process: like Android's onTaskRemoved, a live or armed run
        // flushes its sidecar and keeps it for the resume offer, a locate-only
        // preview just stops.
        NSNotificationCenter.defaultCenter.addObserverForName(
            UIApplicationWillTerminateNotification,
            `object` = null,
            queue = NSOperationQueue.mainQueue,
        ) { _ ->
            if (recorder.recording || recorder.armed) ActiveRunStore.flush() else recorder.stop()
        }
        // An in-app Pause/Resume refreshes the surface, as Android's paused-flow
        // collector re-posts the notification; drop(1) skips the current value.
        // Main.immediate (as Android's) subscribes now, on the creating main thread,
        // so no pause after this is missed.
        MainScope().launch(Dispatchers.Main.immediate) {
            RunSession.paused.drop(1).collect { pushFrame() }
        }
    }

    /** The next refresh pushes a frame even when it looks unchanged. */
    fun invalidateSurface() {
        lastFrame = null
    }

    /** Push the current frame again (a new [RunActivity.sink]). */
    fun resendSurface() {
        invalidateSurface()
        pushFrame()
    }

    private fun pushFrame() {
        if (recorder.stopped) return
        val frame = RunActivityState(
            phase = when {
                recorder.recording && RunSession.paused.value -> RunActivityPhase.Paused
                recorder.recording && recorder.signalLost -> RunActivityPhase.SignalLost
                recorder.recording -> RunActivityPhase.Recording
                recorder.armed -> RunActivityPhase.Armed
                else -> RunActivityPhase.Locating
            },
            movingSec = RunSession.elapsedSec.value,
            distanceKm = RunSession.distanceKm.value,
            paceMinPerKm = RunSession.currentPaceMinPerKm.value,
            miles = resolveDistanceUnit(AppSettings.distanceUnitOverride.value) == DistanceUnit.Mi,
        )
        if (frame == lastFrame) return
        lastFrame = frame
        RunActivity.sink?.update(frame)
    }

    private fun onLocations(locations: List<CLLocation>) {
        val nowWallMs = wallClockMillis()
        val nowMono = monotonicNanos()
        // A delivery can hold several fixes (deferred while the app was busy): oldest
        // first, so the route can't zigzag backwards.
        val batch = locations.sortedBy { it.timestamp.timeIntervalSince1970 }
        for (loc in batch) recorder.onFix(liveFixOf(loc, nowWallMs, nowMono), batch.size)
    }

    private inner class StreamDelegate : NSObject(), CLLocationManagerDelegateProtocol {
        override fun locationManager(manager: CLLocationManager, didUpdateLocations: List<*>) =
            onLocations(didUpdateLocations.filterIsInstance<CLLocation>())

        /** A denied grant ends the stream like Android's async start failure; a
         *  transient "location unknown" just means no fix yet. */
        override fun locationManager(manager: CLLocationManager, didFailWithError: NSError) {
            if (didFailWithError.domain == kCLErrorDomain && didFailWithError.code == kCLErrorDenied.toLong()) {
                recorder.onStreamFailed()
            }
        }

        /** Precise location withdrawn mid-stream (Settings, or the one-off precise
         *  grant lapsing): the stream can't record a route any more. */
        override fun locationManagerDidChangeAuthorization(manager: CLLocationManager) {
            if (!recorder.stopped && !stream.preciseGranted()) recorder.onStreamFailed()
        }
    }

    private inner class Host : RecorderHost {
        override fun nowNanos(): Long = monotonicNanos()

        override fun nowMs(): Long = monotonicMillis()

        override fun startStream(): Boolean {
            if (!stream.preciseGranted()) return false
            lastFrame = null
            stream.start(delegate)
            pushFrame()
            return true
        }

        override fun stopStream() = stream.stop()

        override fun openSidecar() = ActiveRunStore.begin(ctx)

        override fun reassertSurface() = resendSurface()

        override fun refresh() = pushFrame()

        override fun invalidateSurface() = this@IosRunRecording.invalidateSurface()

        /** Vibration only: iOS gives an app one fixed buzz, so the cues differ by
         *  count (one, two, three) like Android's pulse patterns, and a step change
         *  adds the system beep. */
        override fun cue(cue: RecorderCue) {
            val pulses = when (cue) {
                RecorderCue.RecordingStarted -> 1
                RecorderCue.SignalLost -> 2
                RecorderCue.StepChange -> 3
            }
            repeat(pulses) { i -> afterMs(i * PULSE_GAP_MS) { AudioServicesPlaySystemSound(kSystemSoundID_Vibrate) } }
            if (cue == RecorderCue.StepChange) AudioServicesPlaySystemSound(STEP_BEEP_SOUND)
        }

        /** System sounds hold nothing to free. */
        override fun releaseCues() = Unit

        override fun finished(userStop: Boolean) {
            lastFrame = null
            RunActivity.sink?.end()
        }

        override fun wakeTimer(tag: String, action: () -> Unit): RecorderTimer = MainQueueTimer(earliestWins = true, action)

        override fun ticker(action: () -> Unit): RecorderTimer = MainQueueTimer(earliestWins = false, action)
    }
}

/** Gap between vibration pulses: one system buzz lasts about 0.4 s. */
private const val PULSE_GAP_MS = 600L

/** The system's short beep (SystemSoundID 1052), Android's step-change tone. */
private const val STEP_BEEP_SOUND = 1052u

private fun afterMs(delayMs: Long, action: () -> Unit) {
    if (delayMs <= 0L) action() else dispatch_after(dispatch_time(DISPATCH_TIME_NOW, delayMs * 1_000_000), dispatch_get_main_queue(), action)
}

/**
 * A [RecorderTimer] on the main queue. The app keeps running while its location
 * stream is up in the background, so a main-queue deadline fires on time with the
 * screen off. [earliestWins]: while one is pending, a request for the same or a
 * later moment keeps it (Android's WakeTimer); otherwise each request replaces it.
 */
private class MainQueueTimer(private val earliestWins: Boolean, private val action: () -> Unit) : RecorderTimer {
    private var dueAt = 0L

    // Bumped by every cancel and schedule: a block whose token is stale was cancelled.
    private var token = 0L

    override fun schedule(delayMs: Long) {
        val delay = delayMs.coerceAtLeast(0L)
        val at = monotonicMillis() + delay
        if (earliestWins && dueAt != 0L && dueAt <= at) return
        cancel()
        dueAt = at
        val mine = token
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, delay * 1_000_000), dispatch_get_main_queue()) {
            if (mine == token) {
                cancel()
                action()
            }
        }
    }

    override fun cancel() {
        dueAt = 0L
        token++
    }
}

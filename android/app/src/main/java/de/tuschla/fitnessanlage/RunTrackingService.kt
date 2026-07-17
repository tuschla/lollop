package de.tuschla.fitnessanlage

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.google.android.gms.location.FusedLocationProviderClient
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority

/**
 * Foreground service that streams fused-location fixes into [RunSession] with an
 * ongoing notification. Because it runs as a `location`-typed foreground service
 * - not a Composable's callback: tracking continues when the screen turns off
 * or the app is backgrounded. Started/stopped from the UI while it is visible,
 * so foreground-location permission is sufficient (no background-location grant).
 */
class RunTrackingService : Service() {

    private lateinit var fused: FusedLocationProviderClient

    private val callback = object : LocationCallback() {
        override fun onLocationResult(result: LocationResult) {
            // The OS may batch several fixes into one callback (common under
            // screen-off / Doze, exactly when this service earns its keep).
            // lastLocation would keep only the newest and silently drop the
            // intermediate points, tearing the route; append every fix so the
            // logged track stays continuous.
            if (result.locations.isEmpty()) return
            for (loc in result.locations) {
                RunSession.add(
                    GpsPoint(
                        lat = loc.latitude,
                        lon = loc.longitude,
                        observedAt = loc.time / 1000,
                        accuracyM = if (loc.hasAccuracy()) loc.accuracy.toDouble() else 999.0,
                    )
                )
            }
            // Refresh the ongoing notification so a glance at the lockscreen confirms
            // GPS is still capturing. Re-notify with the same NOTIF_ID updates the
            // existing notification in place; IMPORTANCE_LOW keeps it silent every
            // fix. Elapsed + fix count only: route distance stays core-derived.
            getSystemService(NotificationManager::class.java)
                .notify(NOTIF_ID, buildNotification())
        }
    }

    override fun onCreate() {
        super.onCreate()
        fused = LocationServices.getFusedLocationProviderClient(this)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL, "Run tracking", NotificationManager.IMPORTANCE_LOW)
            )
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        // Safety net: if the service is torn down without an ACTION_STOP (e.g. the
        // OS stops the component under memory pressure), unregister so the GPS
        // callback and sensor don't outlive the service. Harmless no-op when
        // stopTracking() already removed it.
        fused.removeLocationUpdates(callback)
        RunSession.tracking.value = false
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startTracking()
            ACTION_STOP -> stopTracking()
        }
        // NOT_STICKY: if the OS kills us mid-run it must not recreate the service
        // with a null intent, that path skips startForeground() and crashes the
        // process on API 26+. A killed run simply ends rather than silently
        // resurrecting without UI/permission context.
        return START_NOT_STICKY
    }

    @SuppressLint("MissingPermission")
    private fun startTracking() {
        // API 34+ requires the typed startForeground for a location FGS; the
        // 2-arg overload throws. ServiceCompat picks the right form per API level.
        ServiceCompat.startForeground(
            this,
            NOTIF_ID,
            buildNotification(),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION
            } else {
                0
            },
        )
        val req = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, 2000L)
            .setMinUpdateIntervalMillis(1000L)
            .build()
        fused.requestLocationUpdates(req, callback, Looper.getMainLooper())
        RunSession.tracking.value = true
    }

    private fun stopTracking() {
        fused.removeLocationUpdates(callback)
        RunSession.tracking.value = false
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun buildNotification(): Notification {
        val tap = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setContentTitle("Tracking run")
            .setContentText(progressText())
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .setContentIntent(tap)
            .build()
    }

    /** Elapsed time + fix count from the captured track, or a pre-first-fix hint. */
    private fun progressText(): String {
        val pts = RunSession.points.value
        if (pts.size < 2) return "Recording your route…"
        val secs = (pts.last().observedAt - pts.first().observedAt).coerceAtLeast(0L)
        return "${formatElapsed(secs)} · ${pts.size} fixes"
    }

    companion object {
        private const val ACTION_START = "de.tuschla.fitnessanlage.action.START"
        private const val ACTION_STOP = "de.tuschla.fitnessanlage.action.STOP"
        private const val CHANNEL = "run_tracking"
        private const val NOTIF_ID = 1

        /**
         * Best-effort last-known fix for centering the map before tracking
         * starts. Caller must already hold a location permission; silently does
         * nothing if none is cached. [onResult] fires on the main thread.
         */
        @SuppressLint("MissingPermission")
        fun lastKnownLocation(ctx: Context, onResult: (lat: Double, lon: Double) -> Unit) {
            LocationServices.getFusedLocationProviderClient(ctx).lastLocation
                .addOnSuccessListener { loc -> loc?.let { onResult(it.latitude, it.longitude) } }
        }

        fun start(ctx: Context) = ContextCompat.startForegroundService(
            ctx,
            Intent(ctx, RunTrackingService::class.java).setAction(ACTION_START),
        )

        fun stop(ctx: Context) {
            // The service may already have self-stopped (e.g. Back pressed after a
            // process restart); delivering ACTION_STOP to a dead service throws on
            // API 26+/31+, so swallow that: there is nothing left to stop.
            try {
                ctx.startService(
                    Intent(ctx, RunTrackingService::class.java).setAction(ACTION_STOP),
                )
            } catch (_: IllegalStateException) {
                RunSession.tracking.value = false
            }
        }
    }
}

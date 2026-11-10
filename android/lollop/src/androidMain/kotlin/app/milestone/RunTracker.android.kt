package app.milestone

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.location.LocationManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat

internal actual fun platformRunTracker(): RunTracker? = ServiceRunTracker

/** [RunTracker] over the [RunTrackingService] foreground service. */
private object ServiceRunTracker : RunTracker {
    override fun hasLocationPermission(ctx: Context): Boolean = hasFineLocation(ctx)

    override fun hasApproximateLocation(ctx: Context): Boolean =
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.ACCESS_COARSE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED

    override fun locationPromptBlocked(ctx: Context): Boolean {
        val act = ctx as? Activity ?: return false
        return !ActivityCompat.shouldShowRequestPermissionRationale(act, Manifest.permission.ACCESS_FINE_LOCATION)
    }

    override fun isLocationEnabled(ctx: Context): Boolean {
        val lm = ctx.getSystemService(Context.LOCATION_SERVICE) as LocationManager
        return LocationManagerCompat.isLocationEnabled(lm)
    }

    override fun openLocationSettings(ctx: Context) = ctx.openFirst(Intent(Settings.ACTION_LOCATION_SOURCE_SETTINGS))

    override fun observeLocationSettings(ctx: Context, onChange: () -> Unit): () -> Unit {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(c: Context?, i: Intent?) = onChange()
        }
        ContextCompat.registerReceiver(
            ctx,
            receiver,
            IntentFilter(LocationManager.PROVIDERS_CHANGED_ACTION),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        return {
            runCatching { ctx.unregisterReceiver(receiver) }
                .onFailure { PlatformLog.w("RunTracking", "location settings receiver already gone", it) }
        }
    }

    @Composable
    override fun rememberLocationPrompt(onResult: (preciseGranted: Boolean) -> Unit): () -> Unit {
        val launcher = rememberLauncherForActivityResult(
            ActivityResultContracts.RequestMultiplePermissions()
        ) { result -> onResult(result[Manifest.permission.ACCESS_FINE_LOCATION] == true) }
        return remember(launcher) {
            { launcher.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)) }
        }
    }

    override val serviceGrants: RunServiceGrants = ServiceGrants

    override fun lastKnownLocation(ctx: Context, onResult: (lat: Double, lon: Double) -> Unit) =
        RunTrackingService.lastKnownLocation(ctx, onResult)

    override fun locate(ctx: Context) {
        RunTrackingService.locate(ctx)
    }

    override fun arm(ctx: Context, requirePrecise: Boolean, requireMoving: Boolean) {
        RunTrackingService.arm(ctx, requirePrecise, requireMoving)
    }

    override fun startNow(ctx: Context) = RunTrackingService.startNow(ctx)

    override fun startRecording(ctx: Context) {
        RunTrackingService.start(ctx)
    }

    override fun stop(ctx: Context) = RunTrackingService.stop(ctx)
}

/** The foreground service's notification grant and battery exemption. */
private object ServiceGrants : RunServiceGrants {
    override fun canPostNotifications(ctx: Context): Boolean = app.milestone.canPostNotifications(ctx)

    override fun openNotificationSettings(ctx: Context) = ctx.openFirst(appNotificationSettingsIntent(ctx))

    @Composable
    override fun rememberNotificationPrompt(onResult: () -> Unit): () -> Unit {
        val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { onResult() }
        return remember(launcher) {
            {
                // Below API 33 posting needs no runtime grant, so there is nothing to ask.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }

    override fun isBatteryUnrestricted(ctx: Context): Boolean {
        val pm = ctx.getSystemService(Context.POWER_SERVICE) as PowerManager
        return pm.isIgnoringBatteryOptimizations(ctx.packageName)
    }

    /** The direct REQUEST_IGNORE_BATTERY_OPTIMIZATIONS dialog (needs the same-named
     *  permission, declared in the manifest, fine for a sideloaded app) is preferred;
     *  if the OEM rejects/omits it, fall back to the general battery-optimization
     *  settings list. */
    @SuppressLint("BatteryLife")
    override fun requestBatteryUnrestricted(ctx: Context) = ctx.openFirst(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).setData(Uri.fromParts("package", ctx.packageName, null)),
        Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
    )
}

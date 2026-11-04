package app.milestone

import android.app.PendingIntent
import android.content.Context
import android.content.Intent

// Android's open intents ([OpenDest] as an explicit activity intent): built by the
// widget, the Quick Settings tile and the notifications, read by [MainActivity].

internal const val ACTION_OPEN = "app.milestone.action.OPEN"
internal const val EXTRA_DEST = "dest"

/** The explicit intent that opens [MainActivity] at [dest]. */
internal fun openIntent(ctx: Context, dest: OpenDest): Intent =
    Intent(ACTION_OPEN)
        .setClass(ctx, MainActivity::class.java)
        .putExtra(EXTRA_DEST, dest.wire)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

/**
 * Immutable activity PendingIntent for [openIntent]. PendingIntents match on the
 * intent without its extras, so each destination gets its own request code.
 */
internal fun openPendingIntent(ctx: Context, dest: OpenDest): PendingIntent =
    PendingIntent.getActivity(
        ctx,
        OPEN_REQUEST_BASE + dest.ordinal,
        openIntent(ctx, dest),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

/** The destination an incoming intent asks for, or null when it isn't an open intent. */
internal fun openDestOf(intent: Intent?): OpenDest? =
    if (intent?.action == ACTION_OPEN) OpenDest.fromWire(intent.getStringExtra(EXTRA_DEST)) else null

// Request codes 0x4F50_0000.. (no other getActivity caller in the app uses this range).
private const val OPEN_REQUEST_BASE = 0x4F500000

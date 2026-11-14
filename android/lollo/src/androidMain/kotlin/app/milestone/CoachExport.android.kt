package app.milestone

import android.content.Intent
import app.milestone.resources.Res
import app.milestone.resources.coach_export_chooser
import app.milestone.resources.coach_export_subject

internal actual fun shareCoachSummary(ctx: PlatformContext, model: ViewModel) {
    val res = ctx.appResources()
    val text = coachSummaryText(res, model)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_SUBJECT, res.getString(Res.string.coach_export_subject))
        putExtra(Intent.EXTRA_TITLE, res.getString(Res.string.coach_export_subject))
        putExtra(Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(
        Intent.createChooser(send, res.getString(Res.string.coach_export_chooser)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        },
    )
}

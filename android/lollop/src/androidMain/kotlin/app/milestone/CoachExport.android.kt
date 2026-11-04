package app.milestone

import android.content.Intent
import app.milestone.resources.Res
import app.milestone.resources.coach_export_chooser
import app.milestone.resources.coach_export_subject

/** Android's share chooser, with the summary built from the Latin-digit resources. */
internal actual fun shareCoachSummary(ctx: PlatformContext, model: ViewModel) {
    val text = coachSummaryText(ctx.latinDigitResources(), model)
    val send = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_SUBJECT, ctx.resources.getString(Res.string.coach_export_subject))
        putExtra(Intent.EXTRA_TITLE, ctx.resources.getString(Res.string.coach_export_subject))
        putExtra(Intent.EXTRA_TEXT, text)
    }
    ctx.startActivity(
        Intent.createChooser(send, ctx.resources.getString(Res.string.coach_export_chooser)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        },
    )
}

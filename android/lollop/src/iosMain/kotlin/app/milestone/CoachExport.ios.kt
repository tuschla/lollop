package app.milestone

/** UIKit's share sheet ([presentShareSheet]) with the summary text. */
internal actual fun shareCoachSummary(ctx: PlatformContext, model: ViewModel) {
    presentShareSheet(listOf(coachSummaryText(ctx.appResources(), model)))
}

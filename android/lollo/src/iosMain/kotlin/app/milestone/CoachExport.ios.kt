package app.milestone

internal actual fun shareCoachSummary(ctx: PlatformContext, model: ViewModel) {
    presentShareSheet(listOf(coachSummaryText(ctx.appResources(), model)))
}

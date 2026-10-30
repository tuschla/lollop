package app.milestone

import android.content.Context
import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** The trim sheet states the run's of-record distance and what a trim removes in
 *  the same figures as the sheet behind its confirm. */
@RunWith(RobolectricTestRunner::class)
class TrimRunSheetTest {

    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun kmWithoutTiles() {
        AppSettings.showRouteMaps.value = false
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.Km
    }

    @After
    fun restore() {
        AppSettings.showRouteMaps.value = true
        AppSettings.distanceUnitOverride.value = DistanceUnitOverride.System
    }

    // 21 fixes 10 s apart, ~37 m each: a 200 s, ~0.745 km raw GPS track.
    private val segments = listOf((0..20).map { GpxFix(48.0, 11.0 + it * 0.0005, 1_700_000_000L + it * 10L, hrBpm = null) })
    private val route = RunRoute(gpx = null, segments = segments, trimmable = true, track = timedTrackFromSegments(segments))

    // The core's QC'd figure differs from the raw track's, as on a real run.
    private val run = RunResultView(distance_km = 0.60, duration_min = 200 / 60.0, entry_id = 1L, observed_at = 1_700_000_200L, has_gpx = true)

    private fun showTrimSheet() {
        rule.setContent { MilestoneTheme { TrimRunSheetContent(run, route, onCancel = {}, onConfirm = { _, _ -> }) } }
        rule.waitForIdle()
    }

    @Test
    fun trimMapDescriptionStatesTheRunsRecordedDistance() {
        showTrimSheet()
        val res = ctx.resources
        val expected = res.getString(R.string.runmap_cd, spokenDistance(res, distanceFigure(run.distance_km, DistanceUnit.Km), DistanceUnit.Km))
        val found = rule.onAllNodes(hasContentDescription(expected), useUnmergedTree = true).fetchSemanticsNodes()
        assertTrue("the trim map reads '$expected', the distance the sheet states", found.isNotEmpty())
    }

    @Test
    fun trimConfirmNeverSaysItRemovesZeroMinutesOfATrimmedStart() {
        showTrimSheet()
        // Cut the first 20 s (two legs, ~75 m) off the start.
        rule.onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.SetProgress))[0]
            .performSemanticsAction(SemanticsActions.SetProgress) { it(20f) }
        rule.waitForIdle()
        // The sheet's readout states what each end removes ("Removes 0.07 km from the start …").
        val tpl = ctx.getString(R.string.trim_removes, "\u0001", "\u0002")
        val (pre, mid, post) = Triple(tpl.substringBefore('\u0001'), tpl.substringAfter('\u0001').substringBefore('\u0002'), tpl.substringAfter('\u0002'))
        val line = rule.onAllNodes(hasText(pre, substring = true)).fetchSemanticsNodes()
            .flatMap { it.config.getOrElse(SemanticsProperties.Text) { emptyList() } }.map { it.text }
            .single { it.startsWith(pre) && it.endsWith(post) }
        val head = line.removePrefix(pre).substringBefore(mid)
        val tail = line.substringAfter(mid).removeSuffix(post)
        assertTrue("a 20 s cut removes a distance, not nothing: $head", head.any { it.isDigit() && it != '0' })
        rule.onAllNodes(hasText(ctx.getString(R.string.main_trim_confirm)) and hasClickAction())[0].performScrollTo().performClick()
        rule.waitForIdle()
        val title = ctx.getString(R.string.main_trim_confirm_title, "", "").substringBefore("(").trim()
        assertTrue("the confirm is up", rule.onAllNodes(hasText(title, substring = true)).fetchSemanticsNodes().isNotEmpty())
        val removes = ctx.getString(R.string.main_trim_confirm_message, head, tail)
        assertTrue(
            "the confirm says '$removes', the figures of the sheet's thumbs",
            rule.onAllNodes(hasText(removes)).fetchSemanticsNodes().isNotEmpty(),
        )
    }
}

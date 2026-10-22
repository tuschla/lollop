package app.milestone.qa

import android.content.Context
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import app.milestone.AppClock
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToNodeAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performScrollToNode
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppSettings
import app.milestone.ConcurrentGoal
import app.milestone.Event
import app.milestone.EventDispatcher
import app.milestone.GoalDistance
import app.milestone.HealthScreen
import app.milestone.LiftGoal
import app.milestone.ProgressionCadence
import app.milestone.R
import androidx.compose.ui.unit.dp
import app.milestone.todayEpochDay
import java.util.Locale
import kotlin.math.abs
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * The owner's illness, plan and safety-card journeys, end to end: real taps on
 * the real screens over the real core, asserting what the user sees and what
 * the durable event log carries. Each scenario is one the owner hit on a device
 * that unit tests of the parts did not catch.
 */
@RunWith(RobolectricTestRunner::class)
class OwnerJourneysTest {

    @get:Rule
    val rule = createEmptyComposeRule()

    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @Before
    fun setUp() {
        Locale.setDefault(Locale.US)
        QaHarness.reset(ctx)
    }

    @After
    fun tearDown() = QaHarness.teardown()

    private val doNotTrain get() = str(R.string.main_do_not_train)

    /** A set-up strength lifter who answered "I'm ill right now", as guided setup leaves them. */
    private fun illProfile(): Event = (QaHarness.strengthProfile() as Event.SetProfile)
        .copy(health = HealthScreen(acute_illness = true), effectiveFromEpochDay = todayEpochDay())

    /** The last SetProfile in the log no longer says "ill right now" (the all-clear
     *  wire omits the health object entirely). */
    private fun lastProfileIsWell(): Boolean {
        val health = journeyLog().last { it.variant == "SetProfile" }.payload["health"] ?: return true
        return !(health as kotlinx.serialization.json.JsonObject)["acute_illness"]!!.jsonPrimitive.boolean
    }

    private fun countOf(variant: String) = journeyLog().count { it.variant == variant }

    /** The post-illness return check's lead ("Return check"), from the core's own
     *  item, and how many places on Today state it. */
    private fun returnCheckStatements(): Pair<String, Int> {
        val item = EventDispatcher.model.value!!.next_session!!.items.first { it.return_check }
        val lead = item.summary.substringBefore(":")
        return lead to rule.onAllNodes(hasText(lead, substring = true)).fetchSemanticsNodes().size
    }

    /** Profile tab: the "I'm ill right now" toggle is off. A while later, as a
     *  user gets there: Today's messages ("Saved", …) have timed out and no
     *  longer sit over the bottom of the list. */
    private fun assertProfileIllnessOff() {
        rule.mainClock.advanceTimeBy(10_000L)
        org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idleFor(java.time.Duration.ofSeconds(10))
        rule.openProfileAnswersAt(illToggle())
        rule.onNode(illToggle()).assertIsOff()
    }

    // ── 1. Illness, profile path: setup "I'm ill" → one resolve on Today ──────
    @Test
    fun freshSetupIllNow_bannerResolveEndsTheHold_withoutAProfileTrip() {
        rule.launchAppExpectingSetup()
        rule.walkLiftSetup(birthdayDigits = "03121991", ill = true)
        rule.finishSetup()

        // Today holds training for the profile's answer and offers its one way back.
        rule.assertTextCountEventually(doNotTrain, 1)
        assertEquals("acute_illness_profile", EventDispatcher.model.value!!.hold_reason)
        val labels = resolveLabels()
        assertEquals("one illness action on the banner: $labels", 1, labels.size)
        // No dead "Plan my training" while the hold stands.
        rule.onAllNodesWithText(str(R.string.main_plan_prompt_cta)).fetchSemanticsNodes()
            .let { assertTrue("no Plan my training during the hold", it.isEmpty()) }

        rule.tapResolveAndConfirm(labels.single())

        // Without visiting Profile: the hold is gone and a session is planned.
        rule.assertTextCountEventually(doNotTrain, 0)
        rule.waitForState { EventDispatcher.model.value?.let { !it.train_blocked && it.next_session != null } == true }
        rule.assertCountEventually(sessionShown(), 1)
        rule.onAllNodesWithText(str(R.string.main_plan_prompt_cta)).fetchSemanticsNodes()
            .let { assertTrue("the plan is back without a second tap", it.isEmpty()) }
        // The one confirm sent ResolveHold and switched the profile answer off.
        assertEquals(1, countOf("ResolveHold"))
        assertTrue("the profile answer is off in the log", lastProfileIsWell())
        assertProfileIllnessOff()
    }

    // ── 2. Illness, check-in path: flu → one resolve → return check, once ─────
    @Test
    fun checkinFlu_holdsTraining_oneResolveTapLeadsIntoTheReturnCheckShownOnce() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.checkInIll(R.string.log_illness_below_neck)
        rule.waitForState { EventDispatcher.model.value?.train_blocked == true }
        rule.assertTextCountEventually(doNotTrain, 1)
        val labels = resolveLabels()
        assertEquals("one illness action: $labels", 1, labels.size)

        // The user recovers later in the day, not in the second they reported it.
        AppClock.offsetMillis += 3 * 3_600_000L
        rule.tapResolveAndConfirm(labels.single())

        rule.assertTextCountEventually(doNotTrain, 0)
        rule.waitForState { EventDispatcher.model.value?.next_session?.items?.any { it.return_check } == true }
        assertEquals(1, countOf("ResolveHold"))
        // The easy return session replaces today's plan, and its advice is stated once.
        val (lead, shown) = returnCheckStatements()
        assertEquals("\"$lead\" is stated once on Today", 1, shown)
        assertTrue(resolveLabels().isEmpty())
    }

    // ── 3. Both illness sources at once → ONE resolve tap ends both ──────────
    @Test
    fun illInProfileAndFluCheckin_oneResolveTapEndsBoth() {
        QaHarness.seedLog(ctx, listOf(illProfile()))
        rule.launchApp()
        rule.checkInIll(R.string.log_illness_below_neck)
        rule.waitForState { EventDispatcher.model.value?.readiness_summary?.any { it.signal == "Illness" } == true }
        rule.assertTextCountEventually(doNotTrain, 1)
        val labels = resolveLabels()
        assertEquals("both illnesses share one action: $labels", 1, labels.size)
        // Exactly one resolve action on screen: no second "I've recovered" beside "I'm better".
        assertEquals(1, rule.onAllNodes(hasText(labels.single()) and hasClickAction()).fetchSemanticsNodes().size)

        // The user recovers later in the day, not in the second they reported it.
        AppClock.offsetMillis += 3 * 3_600_000L
        rule.tapResolveAndConfirm(labels.single())

        rule.assertTextCountEventually(doNotTrain, 0)
        rule.waitForState { EventDispatcher.model.value?.let { !it.train_blocked && it.next_session != null } == true }
        val m = EventDispatcher.model.value!!
        assertTrue("no hold left anywhere", m.hold_resolutions.isEmpty() && m.hold_reason == null)
        assertEquals("the profile answer is off", false, m.profile?.health?.acute_illness)
        assertEquals("one ResolveHold for both", 1, countOf("ResolveHold"))
        assertTrue("the profile answer is off in the log", lastProfileIsWell())
        val (lead, shown) = returnCheckStatements()
        assertEquals("\"$lead\" is stated once on Today", 1, shown)
        assertProfileIllnessOff()
    }

    // ── 4a. Remove plan (This week › ?) → "Plan my training" builds it ───────
    @Test
    fun removePlanThenPlanMyTraining_buildsASession() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.assertCountEventually(sessionShown(), 1)
        val list = rule.onNode(hasScrollToNodeAction())
        list.performScrollToNode(hasTestTag("this-week"))
        rule.onNode(
            hasClickAction() and hasContentDescription(str(R.string.main_details_subject, str(R.string.main_consistency_subject))),
        ).performScrollTo().performClick()
        rule.waitForIdle()
        rule.onNode(hasText(str(R.string.main_remove_plan)) and hasClickAction() and !hasAnyAncestor(isDialog()))
            .performScrollTo().performClick()
        rule.onNode(hasText(str(R.string.main_remove_plan)) and hasAnyAncestor(isDialog())).performClick()

        rule.waitForState { EventDispatcher.model.value?.program == null }
        val cta = str(R.string.main_plan_prompt_cta)
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(hasText(cta))
        rule.assertTextCountEventually(cta, 1)
        assertEquals(0, rule.onAllNodes(sessionShown()).fetchSemanticsNodes().size)

        rule.onNode(hasText(cta) and hasClickAction()).performClick()

        rule.waitForState { EventDispatcher.model.value?.next_session != null }
        rule.assertTextCountEventually(cta, 0)
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(sessionShown())
        rule.assertCountEventually(sessionShown(), 1)
        val log = journeyLog().map { it.variant }
        assertTrue("the tap logged GeneratePlan after the ClearPlan: $log", log.lastIndexOf("GeneratePlan") > log.lastIndexOf("ClearPlan"))
    }

    // ── 4b. A health hold: the plan card is hidden or explains and leads to the cause ─
    @Test
    fun illnessHold_planCardIsNeverADeadButton() {
        QaHarness.seedLog(ctx, listOf(illProfile()))
        rule.launchApp()
        rule.assertTextCountEventually(doNotTrain, 1)
        assertEquals("health_hold", EventDispatcher.model.value!!.plan_withheld)
        rule.onAllNodesWithText(str(R.string.main_plan_prompt_cta)).fetchSemanticsNodes()
            .let { assertTrue("no Plan my training while it can't build anything", it.isEmpty()) }
        val withheld = rule.onAllNodesWithText(str(R.string.main_plan_withheld_health_title)).fetchSemanticsNodes()
        if (withheld.isNotEmpty()) {
            // Shown: it says why, and its action opens Profile at the answer causing it.
            rule.onAllNodesWithText(str(R.string.main_plan_withheld_health_body)).fetchSemanticsNodes()
                .let { assertTrue(it.size == 1) }
            rule.onNode(
                hasText(str(R.string.main_hold_review_health)) and hasClickAction() and
                    hasAnyAncestor(hasText(str(R.string.main_plan_withheld_health_title))),
            ).performScrollTo().performClick()
            rule.waitUntilDisplayed(illToggle())
            rule.onNode(illToggle()).assertIsOn()
        }
        // Either way, the one way back works from Today.
        rule.tapResolveAndConfirm(resolveLabels().single())
        rule.waitForState { EventDispatcher.model.value?.next_session != null }
    }

    // ── 4c. Nothing to plan: says why, never a no-op button ─────────────────
    @Test
    fun nothingToPlan_saysWhyAndItsActionLeadsThroughSetupToASession() {
        QaHarness.seedLog(
            ctx,
            listOf(
                Event.SetProfile(
                    progressionCadence = ProgressionCadence.WeekToWeek,
                    liftGoal = LiftGoal.MaxStrength,
                    goalDistance = GoalDistance.General,
                    concurrentGoal = ConcurrentGoal.Strength,
                    weeklySets = 0,
                    runningDaysPerWeek = 0,
                    runningKmPerWeek = 0.0,
                    advanced = false,
                    bodyweightKg = 80.0,
                    ageYears = 30.0,
                ),
            ),
        )
        rule.launchApp()
        assertEquals("nothing_to_plan", EventDispatcher.model.value!!.plan_withheld)
        val cta = str(R.string.main_plan_withheld_empty_cta)
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(hasText(cta))
        rule.assertTextCountEventually(str(R.string.main_plan_withheld_empty_title), 1)
        rule.assertTextCountEventually(str(R.string.main_plan_withheld_empty_body), 1)
        rule.assertTextCountEventually(str(R.string.main_plan_prompt_cta), 0)

        rule.onNode(hasText(cta) and hasClickAction()).performClick()

        // Its action opens the setup questions (seeded from the profile), whose
        // answers give the plan something to build.
        rule.waitForState { rule.onAllNodesWithText(str(R.string.setup_step_train_q)).fetchSemanticsNodes().isNotEmpty() }
        rule.walkLiftSetup(birthdayDigits = null)
        // About is answered by the profile's stated age; Health needs nothing.
        rule.setupNext()
        rule.setupNext()
        rule.finishSetup()

        rule.waitForState { EventDispatcher.model.value?.let { it.plan_withheld == null && it.next_session != null } == true }
        rule.assertCountEventually(sessionShown(), 1)
        rule.assertTextCountEventually(str(R.string.main_plan_withheld_empty_title), 0)
        val log = journeyLog().map { it.variant }
        assertTrue("setup logged the plan request after the new profile: $log", log.lastIndexOf("GeneratePlan") > log.lastIndexOf("SetProfile"))
    }

    // ── 6. Head cold: the SAFETY card on the theme's surface, its action works ─
    @Test
    fun headColdCheckin_safetyCardSitsOnTheThemeSurfaceAndItsResolveWorks() {
        QaHarness.seedLog(ctx, listOf(QaHarness.strengthProfile()))
        rule.launchApp()
        rule.checkInIll(R.string.log_illness_above_neck)
        rule.waitForState { resolveLabels().isNotEmpty() }
        assertTrue("a head cold does not block training", !EventDispatcher.model.value!!.train_blocked)
        val safety = hasText(str(R.string.main_chip_safety))
        rule.assertCountEventually(safety, 1)

        // The card behind the SAFETY chip is drawn in the active theme's elevated
        // card surface, not a warn wash: walk left from the chip's label across
        // its red fill to the card, then 3 dp further into the card's 14 dp
        // padding (still right of the 4 dp warn rule and the 1 dp outline).
        rule.onNode(hasScrollToNodeAction()).performScrollToNode(safety)
        rule.waitForIdle()
        val chip = rule.onNode(safety, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val image = rule.onAllNodes(isRoot())[0].captureToImage().toPixelMap()
        val y = ((chip.top + chip.bottom) / 2).toInt()
        val reddish = { px: Int -> image[px, y].let { it.red - it.green > 0.2f } }
        var x = chip.left.toInt()
        // Off the label's first glyph onto the red fill, then across the fill.
        while (x > chip.left - with(rule.density) { 6.dp.toPx() } && !reddish(x)) x--
        while (x > 0 && reddish(x)) x--
        x -= with(rule.density) { 3.dp.toPx() }.toInt()
        val actual = image[x, y]
        val expected = AppSettings.theme.value.light.bgElevated
        assertTrue(
            "the SAFETY card's surface at ($x, $y) $actual is the theme's elevated card colour $expected",
            close(actual, expected),
        )

        // Its resolve action ends the eased-illness hold.
        val label = resolveLabels().single()
        rule.tapResolveAndConfirm(label)
        rule.waitForState { resolveLabels().isEmpty() }
        rule.assertTextCountEventually(label, 0)
        assertEquals(1, countOf("ResolveHold"))
    }

    /** Two colours equal within 3/255 per channel (sampling and blending rounding). */
    private fun close(a: Color, b: Color): Boolean =
        abs(a.red - b.red) <= 3 / 255f && abs(a.green - b.green) <= 3 / 255f && abs(a.blue - b.blue) <= 3 / 255f
}

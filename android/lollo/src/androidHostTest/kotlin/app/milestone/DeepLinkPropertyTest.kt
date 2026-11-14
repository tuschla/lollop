package app.milestone

import android.content.Context
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.tileFace
import app.milestone.widget.widgetFace
import kotlin.random.Random
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/**
 * Every open-intent shape decodes, and the widget only offers destinations its day
 * supports. The routing from every navigation state is OpenRoutingTest (common).
 */
@RunWith(RobolectricTestRunner::class)
class DeepLinkPropertyTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()
    private val seed = 20_261_030L
    private val day = 20_400L

    @Test
    fun everyIntentShapeDecodes() {
        val byWire = mapOf("today" to OpenDest.Today, "log" to OpenDest.Log, "track" to OpenDest.Track, "log_set" to OpenDest.LogSet)
        val actions = listOf(ACTION_OPEN, Intent.ACTION_MAIN, Intent.ACTION_VIEW, Intent.ACTION_SEND, null, ACTION_OPEN.uppercase(), "$ACTION_OPEN ", "")
        val wires = byWire.keys.toList() + listOf("", "TODAY", "Today", " log", "log_set\u0000", "settings", "track/../log", "\uD83C\uDFC3", "x".repeat(10_000))
        val extras: List<Any?> = wires + listOf(7, arrayOf("log"), null)
        for (action in actions) for (x in extras) {
            val intent = Intent(action)
            when (x) {
                is String -> intent.putExtra(EXTRA_DEST, x)
                is Int -> intent.putExtra(EXTRA_DEST, x)
                is Array<*> -> intent.putExtra(EXTRA_DEST, x.map { it as String }.toTypedArray())
            }
            val expected = if (action != ACTION_OPEN) null else byWire[x as? String] ?: OpenDest.Today
            assertEquals("$action dest=${x.toString().take(20)}", expected, openDestOf(intent))
        }
    }

    @Test
    fun surfacesOnlyOfferDestinationsTheirDaySupports() {
        val r = Random(seed)
        val res = ctx.resources
        repeat(2_000) { i ->
            val kind = WidgetDayKind.entries[r.nextInt(WidgetDayKind.entries.size)]
            val items = List(r.nextInt(4)) {
                WidgetItem(WidgetItemKind.entries[r.nextInt(3)], name = "E$it", detail = "3 × 5", done = r.nextBoolean())
            }
            val done = r.nextBoolean()
            val s = WidgetSnapshot(
                epochDay = day + r.nextInt(-1, 2), stale = r.nextInt(5) == 0, kind = kind, title = "T", items = items,
                sessionDone = done,
                trackable = (kind == WidgetDayKind.Run || kind == WidgetDayKind.Lift) && !done && items.any { it.kind == WidgetItemKind.Run && !it.done },
            )
            val at = "#$i $s"
            val shownToday = !s.stale && s.epochDay == day

            // Widget: the tile's action first (Track only for today's trackable run,
            // the set editor only for today's open lift session), Log on every face
            // but setup.
            val face = widgetFace(s, day, res)
            val dests = face.actions.map { it.dest }
            if (OpenDest.Track in dests) assertTrue("$at: Track offered", shownToday && s.trackable)
            if (OpenDest.LogSet in dests) assertTrue("$at: Log set offered", shownToday && s.kind == WidgetDayKind.Lift && !s.sessionDone)
            if (shownToday && s.kind != WidgetDayKind.Setup) {
                assertTrue("$at: Log offered", OpenDest.Log in dests)
                assertEquals("$at: same first action as the tile", tileFace(s).dest, dests.first())
            }
        }
    }
}

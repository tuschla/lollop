package app.milestone.qa

import android.content.Context
import android.graphics.Color
import android.os.Looper
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.activity.ComponentActivity
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.glance.appwidget.ExperimentalGlanceRemoteViewsApi
import androidx.glance.appwidget.GlanceRemoteViews
import androidx.test.core.app.ApplicationProvider
import app.milestone.AppTheme
import app.milestone.DarkMode
import app.milestone.widget.TodayWidgetContent
import app.milestone.widget.WidgetFace
import app.milestone.widget.WidgetFixtures
import app.milestone.widget.WidgetSizes
import app.milestone.widget.widgetColors
import app.milestone.widget.widgetFace
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import kotlin.math.pow

/**
 * Renders of the home screen widget as the launcher draws it: the Glance tree
 * composed to RemoteViews for the size step a host picks, applied and laid out at
 * the real cell size, measured on the Pixel launcher: four columns on a 1080 × 2400
 * phone (Android 17 emulator; "2x1" … "4x3") and five on a 1080 × 1920 @ 420 dpi
 * phone (Android 14 emulator; "2x1-5col" …). Named
 * `widget-<state>-<cells>-<theme>-<light|dark>[-font130]` under
 * src/test/screenshots/widget/. The launcher's rounded clip is not drawn here.
 */
@OptIn(ExperimentalGlanceRemoteViewsApi::class)
@RunWith(RobolectricTestRunner::class)
class ScreenshotWidgetTest {
    private val ctx get() = ApplicationProvider.getApplicationContext<Context>()

    @After
    fun resetFont() = RuntimeEnvironment.setFontScale(1f)

    private val cells = linkedMapOf(
        "2x1" to DpSize(179.dp, 95.dp),
        "4x1" to DpSize(373.dp, 95.dp),
        "2x2" to DpSize(179.dp, 202.dp),
        "3x2" to DpSize(276.dp, 202.dp),
        "4x2" to DpSize(373.dp, 202.dp),
        "2x3" to DpSize(179.dp, 308.dp),
        "4x3" to DpSize(373.dp, 308.dp),
        "2x1-5col" to DpSize(144.dp, 89.dp),
        "4x1-5col" to DpSize(296.dp, 89.dp),
        "2x2-5col" to DpSize(144.dp, 193.dp),
        "3x2-5col" to DpSize(221.dp, 193.dp),
        "4x2-5col" to DpSize(296.dp, 193.dp),
        "2x3-5col" to DpSize(144.dp, 296.dp),
        "4x3-5col" to DpSize(296.dp, 296.dp),
    )

    private fun hostStep(size: DpSize): DpSize =
        WidgetSizes.filter { it.width <= size.width && it.height <= size.height }
            .minByOrNull { (size.width - it.width).value.pow(2) + (size.height - it.height).value.pow(2) }
            ?: WidgetSizes.minBy { it.width.value * it.height.value }

    private fun states(): Map<String, WidgetFace> {
        val res = ctx.resources
        return WidgetFixtures.states(res).mapValues { (_, s) -> widgetFace(s.snapshot, WidgetFixtures.DAY, res, s.recording) }
    }

    private fun render(name: String, face: WidgetFace, theme: AppTheme, dark: Boolean, size: DpSize) {
        RuntimeEnvironment.setQualifiers(if (dark) "+night" else "+notnight")
        // Roborazzi captures a view inside an activity window.
        val activity = Robolectric.buildActivity(ComponentActivity::class.java).setup().get()
        val colors = widgetColors(activity, theme, DarkMode.System, dynamic = false)
        val rv = runBlocking { GlanceRemoteViews().compose(activity, hostStep(size)) { TodayWidgetContent(face, colors) }.remoteViews }
        val density = activity.resources.displayMetrics.density
        val pad = (16 * density).toInt()
        val w = (size.width.value * density).toInt()
        val h = (size.height.value * density).toInt()
        val frame = FrameLayout(activity).apply {
            setBackgroundColor(if (dark) Color.rgb(0x20, 0x22, 0x2A) else Color.rgb(0x9A, 0xA3, 0xB5))
            setPadding(pad, pad, pad, pad)
        }
        frame.addView(rv.apply(activity, frame), FrameLayout.LayoutParams(w, h))
        activity.setContentView(frame, ViewGroup.LayoutParams(w + 2 * pad, h + 2 * pad))
        shadowOf(Looper.getMainLooper()).idle()
        frame.captureRoboImage("src/test/screenshots/widget/$name.png")
    }

    /** Every state at every phone size, Paper light (the default theme without the wallpaper accent). */
    @Test
    fun statesAtEverySize() {
        for ((state, face) in states()) for ((cell, size) in cells) {
            render("widget-$state-$cell-paper-light", face, AppTheme.Paper, dark = false, size = size)
        }
    }

    /** The four themes, light and dark, at the 2×1, the default 3×2 and the 4×3. */
    @Test
    fun themes() {
        val s = states()
        for (theme in AppTheme.entries) for (dark in listOf(false, true)) for (cell in listOf("2x1", "3x2", "4x3")) for (state in listOf("lift", "run", "hold")) {
            render("widget-$state-$cell-${theme.name.lowercase()}-${if (dark) "dark" else "light"}", s.getValue(state), theme, dark, cells.getValue(cell))
        }
    }

    /** The system font at 130 %: parts drop out instead of clipping. */
    @Test
    fun largeFont() {
        RuntimeEnvironment.setFontScale(1.3f)
        val s = states()
        for (state in listOf("lift", "run", "rest", "done", "hold")) for ((cell, size) in cells) {
            render("widget-$state-$cell-paper-light-font130", s.getValue(state), AppTheme.Paper, dark = false, size = size)
        }
    }
}

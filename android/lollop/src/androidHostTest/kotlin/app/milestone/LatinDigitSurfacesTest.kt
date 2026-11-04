package app.milestone

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationManager
import android.content.Intent
import android.location.Location
import android.location.LocationManager
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.TextView
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.core.content.IntentCompat
import androidx.glance.ExperimentalGlanceApi
import androidx.glance.appwidget.compose
import androidx.test.core.app.ApplicationProvider
import app.milestone.qa.QaHarness
import app.milestone.widget.TodayWidget
import app.milestone.widget.WidgetDayKind
import app.milestone.widget.WidgetItem
import app.milestone.widget.WidgetItemKind
import app.milestone.widget.WidgetSnapshot
import app.milestone.widget.WidgetSnapshotStore
import java.io.File
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import java.util.TimeZone
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

/**
 * On a phone set to Arabic (Egypt), whose locale writes Arabic-Indic digits, every
 * text the app builds outside its Activity shows its counts in ASCII digits
 * ([latinDigitResources]): the session reminder, the run import result, the coach
 * export, the widget and the run tracker's notification. Each surface is driven
 * through its production entry point and its text read back from what it posted.
 */
@RunWith(RobolectricTestRunner::class)
@Config(qualifiers = "+ar-rEG")
class LatinDigitSurfacesTest {
    private val app: Application = ApplicationProvider.getApplicationContext()
    private val nm get() = shadowOf(app.getSystemService(NotificationManager::class.java))
    private val savedLocale: Locale = Locale.getDefault()
    private val savedZone: TimeZone = TimeZone.getDefault()

    @Before
    fun setUp() {
        QaHarness.reset(app)
        // The JVM default follows the device language, as it does on a phone.
        Locale.setDefault(Locale.forLanguageTag("ar-EG"))
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        File(app.filesDir, WidgetSnapshotStore.FILE_NAME).delete()
    }

    @After
    fun tearDown() {
        SessionReminder.setEnabled(app, false)
        RunSession.reset()
        Locale.setDefault(savedLocale)
        TimeZone.setDefault(savedZone)
        AppClock.offsetMillis = 0L
    }

    /** The Arabic-Indic and Extended Arabic-Indic digits in [text]. */
    private fun nativeDigits(text: CharSequence): String =
        text.filter { it in '\u0660'..'\u0669' || it in '\u06F0'..'\u06F9' }.toString()

    /** [text] has a count in it, and every digit is ASCII. */
    private fun assertLatinDigits(surface: String, text: String) {
        assertTrue("$surface shows a count: '$text'", text.any { it in '0'..'9' })
        assertEquals("$surface: Arabic-Indic digits in '$text'", "", nativeDigits(text))
    }

    @Test
    fun theDeviceLocaleWritesArabicIndicDigits() {
        // The premise of every other test here: the plain resources format "٣".
        assertEquals("ar", app.resources.configuration.locales[0].language)
        assertTrue(app.resources.getQuantityString(R.plurals.widget_more_items, 3, 3).contains('\u0663'))
    }

    // ── Session reminder ─────────────────────────────────────────────────────

    @Test
    fun sessionReminderLineAndActionUseLatinDigits() {
        SessionReminder.setEnabled(app, true)
        val items = listOf("Back squat", "Bench press", "Row").map { WidgetItem(WidgetItemKind.Lift, it, "3 × 5 @ 100 kg") }
        WidgetSnapshotStore.write(
            app,
            WidgetSnapshot(epochDay = todayEpochDay(), kind = WidgetDayKind.Lift, title = "Upper body", items = items, zone = ZoneId.systemDefault().id),
        )
        SessionReminderReceiver().onReceive(app, Intent(SessionReminder.ACTION_FIRE))
        val n = checkNotNull(nm.getNotification(SessionReminder.NOTIF_ID)) { "no reminder posted" }
        val text = n.extras.getCharSequence(Notification.EXTRA_TEXT).toString()
        assertTrue(text, text.startsWith("3 "))
        assertLatinDigits("reminder line", text)
        for (a in n.actions) assertEquals("action '${a.title}'", "", nativeDigits(a.title))
    }

    // ── Run import ───────────────────────────────────────────────────────────

    /** A 20-minute GPX run at 6:00 /km ending [daysAgo] days before today. */
    private fun gpxRun(name: String, daysAgo: Int): android.net.Uri {
        val end = QaHarness.daysAgoAtMidday(daysAgo)
        val doc = buildString {
            append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
            append("<gpx version=\"1.1\" creator=\"test\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n<trk><trkseg>")
            for (i in 0..120) {
                append("<trkpt lat=\"%.7f\" lon=\"0.0\">".format(Locale.US, i * 27.8 / 111_195.0))
                append("<time>").append(Instant.ofEpochSecond(end - (120 - i) * 10L)).append("</time></trkpt>\n")
            }
            append("</trkseg></trk></gpx>\n")
        }
        val f = File(app.cacheDir, "latin-digits/$name").apply { parentFile!!.mkdirs(); writeText(doc) }
        return android.net.Uri.fromFile(f)
    }

    @Test
    fun runImportSummaryUsesLatinDigits() {
        QaHarness.seedLog(app, listOf(QaHarness.strengthProfile()))
        // Boot hydration, which the import waits for.
        EventDispatcher.model.value = QaHarness.reboot(app).view
        var posted: UiMessage? = null
        val collector = CoroutineScope(Dispatchers.Unconfined)
        collector.launch { posted = UiMessages.messages.first() }
        try {
            RunImport.importRuns(app, listOf(gpxRun("a.gpx", daysAgo = 2), gpxRun("b.gpx", daysAgo = 3)))
            QaHarness.pumpMainUntil("the import result", timeoutMs = 20_000) { posted != null }
        } finally {
            collector.cancel()
        }
        val text = posted!!.text
        assertEquals(2, EventDispatcher.model.value!!.runs.size)
        assertTrue(text, text.contains("2"))
        assertLatinDigits("import result", text)
    }

    // ── Coach export ─────────────────────────────────────────────────────────

    @Test
    fun coachExportTextUsesLatinDigits() {
        val today = todayEpochDay()
        val at = today * 86_400L + 43_200L
        val model = ViewModel(
            today_epoch_day = today,
            lifts = List(3) { LiftResultView(exercise = "Back Squat", weight_kg = 100.0, reps = 5, observed_at = at + it * 180L) },
            runs = listOf(RunResultView(distance_km = 8.0, duration_min = 44.0, observed_at = at - 86_400L, session_rpe = 6)),
            activities = listOf(
                ActivityResultView(entry_id = 1, observed_at = at - 2 * 86_400L, kind = ActivityKind.Cycling, minutes = 45.0, vigorous = false),
            ),
        )
        shareCoachSummary(app, model)
        val chooser = shadowOf(app).nextStartedActivity
        val send = checkNotNull(IntentCompat.getParcelableExtra(chooser, Intent.EXTRA_INTENT, Intent::class.java)) { "no share intent" }
        val text = send.getStringExtra(Intent.EXTRA_TEXT)!!
        assertTrue(text, text.contains("45 min") && text.contains("6/10"))
        assertLatinDigits("coach export", text)
        // Dates and weekdays in the app's language, not the device's.
        assertTrue(text, text.none { Character.UnicodeScript.of(it.code) == Character.UnicodeScript.ARABIC || it == '\u200F' })
    }

    // ── Widget ───────────────────────────────────────────────────────────────

    private fun texts(v: View): List<String> = when (v) {
        is TextView -> listOf(v.text.toString())
        is ViewGroup -> (0 until v.childCount).flatMap { texts(v.getChildAt(it)) }
        else -> emptyList()
    }

    @OptIn(ExperimentalGlanceApi::class)
    @Test
    fun widgetFaceUsesLatinDigits() {
        val items = List(8) { WidgetItem(WidgetItemKind.Lift, "Lift ${'A' + it}", "heavy") }
        WidgetSnapshotStore.write(
            app,
            WidgetSnapshot(epochDay = todayEpochDay(), kind = WidgetDayKind.Lift, title = "Upper body", items = items, weekDone = 2, weekPlanned = 4),
        )
        // The tallest layout: six body lines (five items and "3 more items") and the week line.
        val views = runBlocking { TodayWidget().compose(app, size = DpSize(400.dp, 420.dp)) }
        val shown = texts(views.apply(app, FrameLayout(app)))
        val week = shown.single { it.endsWith("sessions done") }
        val more = shown.single { it.endsWith("more items") }
        assertLatinDigits("widget week line", week)
        assertLatinDigits("widget more-items line", more)
    }

    // ── Run tracker notification ─────────────────────────────────────────────

    @Test
    fun trackerStepLineUsesLatinDigits() {
        shadowOf(app).grantPermissions(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION)
        val lm = app.getSystemService(LocationManager::class.java)
        shadowOf(lm).setProviderEnabled(LocationManager.GPS_PROVIDER, true)
        RunSession.plannedRun.value = PrescriptionView(run_structure = RunStructureView(reps = 6, rep_m = 400, recovery_sec = 90))
        fun intent(action: String) = Intent(app, RunTrackingService::class.java).setAction("app.milestone.action.$action")
        val controller = Robolectric.buildService(RunTrackingService::class.java, intent("ARM")).create().startCommand(0, 1)
        controller.withIntent(intent("START_NOW")).startCommand(0, 2)
        val t0 = AppClock.nowMillis()
        for (i in 0..2) {
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(3))
            shadowOf(lm).simulateLocation(
                Location(LocationManager.GPS_PROVIDER).apply {
                    latitude = 52.5 + i * 0.0001
                    longitude = 13.4
                    accuracy = 5f
                    time = t0 + i * 3_000L
                    elapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
                },
            )
            shadowOf(Looper.getMainLooper()).idle()
        }
        val text = nm.getNotification(RunTrackingService.NOTIF_ID).extras.getCharSequence(Notification.EXTRA_TEXT).toString()
        controller.withIntent(intent("STOP")).startCommand(0, 3)
        controller.destroy()
        assertTrue(text, text.startsWith("Rep 1 of 6"))
        assertLatinDigits("tracker step line", text)
    }
}

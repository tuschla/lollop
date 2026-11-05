import SwiftUI
import WidgetKit
import XCTest

/// The widget extension's model and views (no host app): the payload the app
/// wrote in the CI smoke test (LOLLOP_WIDGET_PAYLOAD), the day rule and the
/// midnight roll-over, the line fitting, the App Group lookup, the activity content
/// the Kotlin side builds, and renders of every family, state and activity
/// (LOLLOP_RENDER_DIR, the CI artifact `widget-renders`).
final class LollopWidgetsTests: XCTestCase {
    // MARK: Fixtures

    private static let palette = PaletteSet(
        mode: "system",
        light: WidgetColorsHex(
            ground: "#FFFFFFFF", body: "#FF1A1A1A", muted: "#FF5F5F5F", faint: "#FF9E9E9E", accent: "#FF1A1A1A",
            onAccent: "#FFFFFFFF", accentTint: "#FFE4E4E4", danger: "#FFB3261E", done: "#FF15803D"
        ),
        dark: WidgetColorsHex(
            ground: "#FF16142E", body: "#FFF5F0E6", muted: "#FFB8B2CC", faint: "#FF7D7896", accent: "#FFFFD166",
            onAccent: "#FF16142E", accentTint: "#FF4F3E12", danger: "#FFF87171", done: "#FF4ADE80"
        )
    )

    private static let labels = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]

    private static func week(today: Int = 1) -> [Face.Day] {
        let marks = ["done", "next", "rest", "planned", "planned", "rest", "none"]
        return (0..<7).map { Face.Day(label: labels[$0], mark: marks[$0], today: $0 == today) }
    }

    private static let logSet = Face.Action(label: "Log set", dest: "log_set", face: "log_set")
    private static let startRun = Face.Action(label: "Start run", dest: "track", face: "start_run")
    private static let log = Face.Action(label: "Log", dest: "log", face: "log")

    private static func face(
        title: String, tone: String = "normal", status: String? = nil, brief: String? = nil, lead: [String] = [],
        items: [String] = [], hero: Face.Hero? = nil, heroLead: [String] = [], heroItems: [String] = [],
        week: String? = "1 of 4 sessions done", days: [Face.Day] = LollopWidgetsTests.week(), actions: [Face.Action]
    ) -> Face {
        Face(
            overline: "Today · Tue, Oct 28", date: "Tue, Oct 28", title: title, tone: tone, status: status, brief: brief,
            lead: lead, items: items, hero: hero, heroLead: heroLead, heroItems: heroItems, week: week, days: days,
            actions: actions
        )
    }

    /// The states of Android's widget screenshots (ScreenshotWidgetTest).
    static let faces: [String: Face] = [
        "lift": face(
            title: "Heavy day", status: "4 exercises · Back Squat first", brief: "Back Squat",
            items: ["Back Squat · 5×3 @ 117.5\u{00A0}kg", "Deadlift · 3×5", "Bench press · 3×5", "Cable row · 3×5"],
            actions: [logSet, log]
        ),
        "run": face(
            title: "Easy run", status: "3.4 mi · 9:00–9:40 /mi", brief: "3.4\u{00A0}mi",
            items: ["Run · 3.4\u{00A0}mi · 9:00–9:40 /mi"], hero: Face.Hero(text: "3.4\u{00A0}mi", note: "9:00–9:40 /mi", figure: true),
            actions: [startRun, log]
        ),
        "rest": face(
            title: "Rest day", status: "Next: Heavy volume day, Thu, Oct 30", brief: "Next: Thu",
            lead: ["Next: Heavy volume day, Thu, Oct 30"],
            hero: Face.Hero(text: "Heavy volume day", note: "Next: Thu, Oct 30", figure: false),
            heroItems: ["Back Squat · 4×5 @ 112.5\u{00A0}kg", "Deadlift · 4×5 @ 57.5\u{00A0}kg", "Bench press · 4×5 @ 57.5\u{00A0}kg"],
            actions: [log]
        ),
        "done": face(
            title: "Easy run", status: "Session done", brief: "Session done",
            lead: ["Session done", "Next: Heavy volume day, Thu, Oct 30"], items: ["Run · Done"],
            hero: Face.Hero(text: "Heavy volume day", note: "Next: Thu, Oct 30", figure: false),
            heroLead: ["Session done"], heroItems: ["Back Squat · 4×5 @ 112.5\u{00A0}kg"],
            week: "2 of 4 sessions done", actions: [log]
        ),
        "hold": face(
            title: "Training paused", tone: "paused", status: "Open lollop for details.", brief: "Open lollop for details.",
            lead: ["Open lollop for details."], week: nil, days: [], actions: [log]
        ),
        "noplan": face(
            title: "No plan yet", status: "Open lollop to make a plan.", brief: "Open lollop to make a plan.",
            lead: ["Open lollop to make a plan."], week: nil, days: [], actions: [log]
        ),
        "stale": Face.fallback,
    ]

    private static func payload(_ face: Face, day: Int64, zone: String = TimeZone.current.identifier, stale: Bool = false) -> WidgetPayload {
        WidgetPayload(
            version: WidgetPayload.supportedVersion,
            snapshot: .init(epochDay: day, stale: stale, kind: "Lift", weekDone: 1, weekPlanned: 4, zone: zone),
            recording: false,
            face: face,
            staleFace: Face.fallback,
            moreItems: (1...6).map { $0 == 1 ? "1 more item" : "\($0) more items" },
            control: ControlFace(label: "Log set", subtitle: "Heavy day", dest: "log_set", face: "log_set", active: false),
            staleControl: .fallback,
            palette: palette
        )
    }

    // MARK: The payload the app wrote

    /// The payload the app wrote in the simulator (the CI smoke test copies it
    /// from the App Group container): the extension reads it, it describes today,
    /// and every family draws it.
    @MainActor
    func testThePayloadTheAppWroteReads() throws {
        guard let path = ProcessInfo.processInfo.environment["LOLLOP_WIDGET_PAYLOAD"] else {
            throw XCTSkip("LOLLOP_WIDGET_PAYLOAD not set")
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let payload = try XCTUnwrap(WidgetPayload.decode(data), "the app's payload decodes")
        XCTAssertTrue(payload.isCurrent(at: Date()), "written today, in this zone")
        XCTAssertFalse(payload.face.title.isEmpty)
        XCTAssertFalse(payload.staleFace.title.isEmpty)
        XCTAssertFalse(payload.control.label.isEmpty)
        let entries = TodayProvider(load: { payload }).timeline(at: Date()).entries
        XCTAssertEqual(entries.count, 2)
        XCTAssertEqual(entries[0].face, payload.face)
        XCTAssertEqual(entries[1].face, payload.staleFace)
        for scheme in [ColorScheme.light, .dark] {
            renderAllFamilies(name: "app", face: payload.face, payload: payload, scheme: scheme)
        }
    }

    // MARK: Day rule

    func testTheTimelineRollsOverToTheStaleFaceAtMidnight() throws {
        let now = Date()
        let today = localEpochDay(of: now)
        let lift = try XCTUnwrap(Self.faces["lift"])
        let current = TodayProvider(load: { Self.payload(lift, day: today) }).timeline(at: now)
        XCTAssertEqual(current.entries.map(\.date), [now, nextMidnight(after: now)])
        XCTAssertEqual(current.entries.map(\.face.title), ["Heavy day", Face.fallback.title])
        // Yesterday's snapshot, a stale one, and nothing at all: "Open lollop to update" only.
        for payload in [Self.payload(lift, day: today - 1), Self.payload(lift, day: today, stale: true), nil] {
            let timeline = TodayProvider(load: { payload }).timeline(at: now)
            XCTAssertEqual(timeline.entries.map(\.face.title), [Face.fallback.title])
        }
        // Written in another time zone (Android marks it stale on a zone change).
        let elsewhere = TimeZone.current.identifier == "Pacific/Kiritimati" ? "Etc/GMT+12" : "Pacific/Kiritimati"
        XCTAssertFalse(Self.payload(lift, day: today, zone: elsewhere).isCurrent(at: now))
        XCTAssertTrue(Self.payload(lift, day: today, zone: "").isCurrent(at: now))
    }

    func testTheLocalDayCountsLikeKotlin() {
        let utc = TimeZone(identifier: "UTC")!
        let berlin = TimeZone(identifier: "Europe/Berlin")!
        // 2025-10-28T23:30:00Z: day 20389 in UTC, already the 29th (20390) in Berlin.
        let date = Date(timeIntervalSince1970: 1_761_694_200)
        XCTAssertEqual(localEpochDay(of: date, in: utc), 20389)
        XCTAssertEqual(localEpochDay(of: date, in: berlin), 20390)
        XCTAssertEqual(localEpochDay(of: Date(timeIntervalSince1970: 0), in: utc), 0)
        XCTAssertEqual(localEpochDay(of: Date(timeIntervalSince1970: -1), in: utc), -1)
    }

    func testTheControlShowsTodaysTileFace() {
        let now = Date()
        let lift = Self.faces["lift"]!
        XCTAssertEqual(Self.payload(lift, day: localEpochDay(of: now)).controlFace(at: now).dest, "log_set")
        XCTAssertEqual(Self.payload(lift, day: localEpochDay(of: now)).controlFace(at: now).subtitle, "Heavy day")
        XCTAssertEqual(Self.payload(lift, day: localEpochDay(of: now) - 1).controlFace(at: now), .fallback)
        XCTAssertEqual(ControlFace.symbol(face: "start_run"), "location")
        XCTAssertEqual(ControlFace.symbol(face: "log"), "plus")
    }

    // MARK: Lines and layouts

    /// visibleLines as WidgetFace.kt's: lead first, then items, a cut list ends in
    /// "N more items" unless one item slot is left.
    func testVisibleLinesCutLikeAndroid() {
        let more = (1...6).map { "\($0) more" }
        let items = ["a", "b", "c", "d"]
        XCTAssertEqual(visibleLines(lead: [], items: items, maxLines: 4, moreItems: more), items)
        XCTAssertEqual(visibleLines(lead: [], items: items, maxLines: 3, moreItems: more), ["a", "b", "2 more"])
        XCTAssertEqual(visibleLines(lead: [], items: items, maxLines: 1, moreItems: more), ["a"])
        XCTAssertEqual(visibleLines(lead: ["L"], items: items, maxLines: 2, moreItems: more), ["L", "a"])
        XCTAssertEqual(visibleLines(lead: ["L"], items: items, maxLines: 3, moreItems: more), ["L", "a", "3 more"])
        XCTAssertEqual(visibleLines(lead: ["L", "M"], items: items, maxLines: 1, moreItems: more), ["L"])
        XCTAssertEqual(visibleLines(lead: [], items: items, maxLines: 0, moreItems: more), [])
    }

    func testTheCardCandidatesGoFromRichestToSparsest() throws {
        let run = try XCTUnwrap(Self.faces["run"])
        let wide = CardLayout.candidates(for: run, wide: true)
        XCTAssertEqual(Set(wide).count, wide.count, "no candidate twice")
        XCTAssertEqual(
            wide.first,
            CardLayout(overline: true, hero: true, week: .strip, lines: 0, large: false, cut: false, titleLines: 2, bigTitle: false)
        )
        XCTAssertEqual(wide.map(\.rank), wide.map(\.rank).sorted(by: >))
        // A lone lead line that would wrap to more than two lines gives way to the short one.
        let rest = try XCTUnwrap(Self.faces["rest"])
        XCTAssertEqual(TodayCard.shortLead(rest, textWidth: 80).lead, ["Next: Thu"])
        XCTAssertEqual(TodayCard.shortLead(rest, textWidth: 306).lead, rest.lead)
        XCTAssertEqual(wide.last?.titleLines, 1)
        // A small card never draws the week strip (Android: never two cells wide).
        XCTAssertFalse(CardLayout.candidates(for: run, wide: false).contains { $0.week == .strip })
        // A statement has only its lines to give up.
        let hold = try XCTUnwrap(Self.faces["hold"])
        XCTAssertTrue(CardLayout.candidates(for: hold, wide: true).allSatisfy { !$0.hero && $0.week == .hidden })
    }

    // MARK: App Group

    func testTheProvisionedGroupIsReadFromTheProfile() {
        let plist = """
        <?xml version="1.0" encoding="UTF-8"?>
        <plist version="1.0"><dict><key>Entitlements</key><dict>
        <key>com.apple.security.application-groups</key><array><string>group.app.milestone.lollop.ABCDE12345</string></array>
        </dict></dict></plist>
        """
        let profile = Data([0x30, 0x82, 0x01]) + Data(plist.utf8) + Data([0x00, 0xA0])
        XCTAssertEqual(SharedContainer.applicationGroups(inProfile: profile), ["group.app.milestone.lollop.ABCDE12345"])
        XCTAssertEqual(SharedContainer.applicationGroups(inProfile: Data("no plist".utf8)), [])
        XCTAssertEqual(SharedContainer.candidateGroups().last, SharedContainer.declaredGroup)
    }

    func testScreensMapToTheAppsOpenUrls() {
        XCTAssertEqual(LollopScreen(wire: "log_set").url.absoluteString, "lollop://open/log_set")
        XCTAssertEqual(LollopScreen(wire: "track"), .track)
        XCTAssertEqual(LollopScreen(wire: "nonsense"), .today)
    }

    // MARK: Activities

    /// The JSON the Kotlin side sends (IosRunLive, IosRestLive) decodes, and a
    /// frame that differs only in its clock is the same frame.
    func testTheActivityContentDecodes() throws {
        let runJson = """
        {"kind":"run","phase":"recording","title":"Tracking run","chip":"23:41","text":null,"step":null,
         "movingSec":1421,"time":"23:41","distance":"4.12 km","pace":"5:45 /km","movingLabel":"Moving",
         "distanceLabel":"Distance","paceLabel":"Pace now","canPause":true,"paused":false,"pauseLabel":"Pause",
         "resumeLabel":"Resume","stopLabel":"Stop","accentLight":"#FF1A1A1A","accentDark":"#FFFFD166"}
        """
        let run = try JSONDecoder().decode(RunLive.self, from: Data(runJson.utf8))
        XCTAssertTrue(run.recording)
        var later = run
        later.movingSec = 1422
        later.time = "23:42"
        later.chip = "23:42"
        XCTAssertEqual(run.withoutClock, later.withoutClock)
        later.distance = "4.13 km"
        XCTAssertNotEqual(run.withoutClock, later.withoutClock)

        let restJson = """
        {"kind":"rest","title":"Resting · Back squat","next":"Next: Set 3 of 4 · 100 kg × 5","startMs":1000,
         "endMs":91000,"doneTitle":"Rest done","accentLight":"#FF1A1A1A","accentDark":"#FFFFD166"}
        """
        let rest = try JSONDecoder().decode(RestLive.self, from: Data(restJson.utf8))
        XCTAssertEqual(rest.end.timeIntervalSince(rest.start), 90)
    }

    // MARK: Renders

    @MainActor
    func testEveryStateRendersInEveryFamily() {
        let today = localEpochDay(of: Date())
        for (name, face) in Self.faces.sorted(by: { $0.key < $1.key }) {
            for scheme in [ColorScheme.light, .dark] {
                renderAllFamilies(name: name, face: face, payload: Self.payload(face, day: today), scheme: scheme)
            }
        }
    }

    @MainActor
    func testTheActivitiesRender() throws {
        let base = try JSONDecoder().decode(RunLive.self, from: Data("""
        {"phase":"recording","title":"Tracking run","chip":"23:41","text":null,"step":"Rep 2 of 6 · 320 m left",
         "movingSec":1421,"time":"23:41","distance":"4.12 km","pace":"5:45 /km","movingLabel":"Moving",
         "distanceLabel":"Distance","paceLabel":"Pace now","canPause":true,"paused":false,"pauseLabel":"Pause",
         "resumeLabel":"Resume","stopLabel":"Stop","accentLight":"#FF1A1A1A","accentDark":"#FFFFD166"}
        """.utf8))
        var paused = base
        paused.phase = "paused"
        paused.title = "Run paused"
        paused.chip = "Paused"
        paused.pace = nil
        paused.step = nil
        paused.paused = true
        var locating = base
        locating.phase = "locating"
        locating.title = "Finding GPS…"
        locating.chip = "GPS…"
        locating.text = "Tap Start to record."
        locating.canPause = false
        locating.step = nil
        let anchor = Date().addingTimeInterval(-1421)
        let rest = RestLive(
            title: "Resting · Back squat", next: "Next: Set 3 of 4 · 100 kg × 5",
            startMs: Int64(Date().timeIntervalSince1970 * 1000), endMs: Int64(Date().addingTimeInterval(90).timeIntervalSince1970 * 1000),
            doneTitle: "Rest done", accentLight: "#FF1A1A1A", accentDark: "#FFFFD166"
        )
        for scheme in [ColorScheme.light, .dark] {
            let s = scheme == .dark ? "dark" : "light"
            render("activity-run-recording-\(s)", RunLockScreen(run: base, anchor: anchor), width: 360, scheme: scheme)
            render("activity-run-paused-\(s)", RunLockScreen(run: paused, anchor: nil), width: 360, scheme: scheme)
            render("activity-run-locating-\(s)", RunLockScreen(run: locating, anchor: nil), width: 360, scheme: scheme)
            render("activity-rest-\(s)", RestLockScreen(rest: rest, stale: false), width: 360, scheme: scheme)
            render("activity-rest-done-\(s)", RestLockScreen(rest: rest, stale: true), width: 360, scheme: scheme)
        }
        let island = LollopActivityState(run: base, runAnchor: anchor, rest: nil, stale: false)
        let expanded = ActivityExpanded(state: island)
        render(
            "activity-island-run-expanded",
            VStack(spacing: 8) {
                HStack { expanded.leading; Spacer(); expanded.center; Spacer(); expanded.trailing }
                expanded.bottom(accent: Color(argb: base.accentDark))
            }
            .padding(16)
            .background(Color.black),
            width: 370, scheme: .dark
        )
        render("activity-island-run-compact", HStack { ActivityChip(state: island, minimal: false) }.padding(8).background(Color.black), width: 120, scheme: .dark)
        render(
            "activity-island-rest-compact",
            HStack { ActivityChip(state: LollopActivityState(run: nil, runAnchor: nil, rest: rest, stale: false), minimal: false) }
                .padding(8).background(Color.black),
            width: 120, scheme: .dark
        )
    }

    // MARK: Rendering helpers

    /// Widget sizes on a 6.1" iPhone (points).
    private static let families: [(String, CGSize)] = [
        ("small", CGSize(width: 158, height: 158)),
        ("medium", CGSize(width: 338, height: 158)),
        ("large", CGSize(width: 338, height: 354)),
        ("rectangular", CGSize(width: 160, height: 72)),
        ("circular", CGSize(width: 72, height: 72)),
        ("inline", CGSize(width: 240, height: 24)),
    ]

    @MainActor
    private func renderAllFamilies(name: String, face: Face, payload: WidgetPayload, scheme: ColorScheme) {
        let colors = payload.palette.colors(for: scheme)
        let s = scheme == .dark ? "dark" : "light"
        for (family, size) in Self.families {
            let view: AnyView
            switch family {
            case "small", "medium", "large":
                view = AnyView(
                    TodayCard(face: face, colors: colors, moreItems: payload.moreItems, wide: family != "small", buttons: .drawn)
                        .frame(width: size.width, height: size.height)
                        .background(colors.ground)
                        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                )
            case "rectangular":
                view = AnyView(RectangularView(face: face).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.gray))
            case "circular":
                view = AnyView(CircularView(face: face, week: (done: 1, planned: 4)).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.gray))
            default:
                view = AnyView(InlineView(face: face).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.gray))
            }
            render("widget-\(name)-\(family)-\(s)", view, width: size.width + 16, scheme: scheme)
        }
    }

    @MainActor
    private func render<V: View>(_ name: String, _ view: V, width: CGFloat, scheme: ColorScheme) {
        // The system draws an activity on its own ground; a light or dark one here.
        let ground = scheme == .dark ? Color(white: 0.12) : Color(white: 0.95)
        let renderer = ImageRenderer(content: view.padding(8).background(ground).environment(\.colorScheme, scheme).frame(width: width))
        renderer.scale = 3
        let png = renderer.uiImage?.pngData()
        XCTAssertNotNil(png, "\(name) renders")
        if let dir = ProcessInfo.processInfo.environment["LOLLOP_RENDER_DIR"], let png {
            let url = URL(fileURLWithPath: dir).appendingPathComponent("\(name).png")
            XCTAssertNoThrow(try png.write(to: url))
        }
    }
}

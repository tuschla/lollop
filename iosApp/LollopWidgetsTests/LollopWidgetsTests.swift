import SwiftUI
import WidgetKit
import XCTest

/// The widget extension's model and views (no host app), on what the Kotlin side
/// resolves: fixtures.json (written by android/lollop's GeneratedSourcesTest from
/// the states of Android's widget screenshots, run frames and a rest), the bundled
/// fallback and strings, and the payload the app wrote in the CI smoke test
/// (LOLLOP_WIDGET_PAYLOAD). Renders every family, state and activity
/// (LOLLOP_RENDER_DIR, the CI artifact `widget-renders`).
final class LollopWidgetsTests: XCTestCase {
    // MARK: Fixtures

    private struct Fixture: Decodable {
        var widgets: [String: WidgetPayload]
        var runs: [String: RunLive]
        var rest: RestLive
    }

    private static let fixture: Fixture = {
        let bundle = Bundle(for: LollopWidgetsTests.self)
        guard let url = bundle.url(forResource: "fixtures", withExtension: "json"),
              let data = try? Data(contentsOf: url)
        else { preconditionFailure("the tests lack fixtures.json") }
        do {
            return try JSONDecoder().decode(Fixture.self, from: data)
        } catch {
            preconditionFailure("fixtures.json does not decode: \(error)")
        }
    }()

    private func widget(_ name: String) throws -> WidgetPayload {
        try XCTUnwrap(Self.fixture.widgets[name], "fixture state \(name)")
    }

    /// [payload] current from an hour ago to an hour from [now], in this zone.
    private func current(_ payload: WidgetPayload, around now: Date, zone: String = TimeZone.current.identifier) -> WidgetPayload {
        var p = payload
        p.current = CurrentDay(zone: zone, startMs: Int64((now.timeIntervalSince1970 - 3600) * 1000), endMs: Int64((now.timeIntervalSince1970 + 3600) * 1000))
        return p
    }

    // MARK: What the app writes

    /// Every payload the Kotlin side builds decodes here, in the version this
    /// extension reads.
    func testTheFixtureIsThisVersionsPayload() throws {
        XCTAssertFalse(Self.fixture.widgets.isEmpty)
        for (name, payload) in Self.fixture.widgets {
            let data = try JSONEncoder().encode(payload)
            XCTAssertEqual(WidgetPayload.decode(data), payload, name)
        }
        var other = try widget("lift")
        other.version = widgetPayloadVersion + 1
        XCTAssertNil(WidgetPayload.decode(try JSONEncoder().encode(other)), "another version reads as missing")
    }

    /// The payload the app wrote in the simulator (the CI smoke test copies it
    /// from the App Group container): the extension reads it, it describes today,
    /// and every family draws it.
    @MainActor
    func testThePayloadTheAppWroteReads() throws {
        guard let path = ProcessInfo.processInfo.environment["LOLLOP_WIDGET_PAYLOAD"] else {
            throw XCTSkip("LOLLOP_WIDGET_PAYLOAD not set: only the CI smoke test has an app that wrote one")
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let payload = try XCTUnwrap(WidgetPayload.decode(data), "the app's payload decodes")
        XCTAssertTrue(payload.isCurrent(at: Date()), "written today, in this zone")
        XCTAssertFalse(payload.face.title.isEmpty)
        XCTAssertFalse(payload.staleFace.title.isEmpty)
        XCTAssertFalse(payload.control.label.isEmpty)
        let entries = TodayProvider(load: { payload }).timeline(at: Date()).entries
        XCTAssertEqual(entries.map(\.face), [payload.face, payload.staleFace])
        for scheme in [ColorScheme.light, .dark] {
            renderAllFamilies(name: "app", payload: payload, scheme: scheme)
        }
    }

    /// Before the app has written anything: the bundled faces, the stale face's text.
    func testTheFallbackIsBundled() throws {
        let fallback = WidgetFallback.bundled
        XCTAssertEqual(fallback.face, try widget("lift").staleFace, "the stale face of any snapshot")
        XCTAssertEqual(fallback.control.screen, .log)
        XCTAssertEqual(TodayProvider(load: { nil }).timeline(at: Date()).entries.map(\.face), [fallback.face])
    }

    /// Every string the extension and the intents name by key reads as text.
    func testTheStringKeysResolve() throws {
        let url = try XCTUnwrap(Bundle.lollopWidgets.url(forResource: "Localizable", withExtension: "strings"))
        let table = try XCTUnwrap(NSDictionary(contentsOf: url) as? [String: String])
        XCTAssertFalse(table.isEmpty)
        for (key, value) in table {
            XCTAssertEqual(Bundle.lollopWidgets.localizedString(forKey: key, value: nil, table: nil), value, key)
            XCTAssertNotEqual(value, key)
        }
    }

    // MARK: Day rule

    func testTheTimelineRollsOverToTheStaleFaceWhenTheDayEnds() throws {
        let now = Date()
        let lift = current(try widget("lift"), around: now)
        let timeline = TodayProvider(load: { lift }).timeline(at: now)
        XCTAssertEqual(timeline.entries.map(\.date), [now, lift.current!.end])
        XCTAssertEqual(timeline.entries.map(\.face), [lift.face, lift.staleFace])
        // A stale snapshot ("Open lollop to update" only), and one of another day.
        let stale = try widget("stale")
        XCTAssertNil(stale.current)
        XCTAssertEqual(TodayProvider(load: { stale }).timeline(at: now).entries.map(\.face), [stale.staleFace])
        let yesterday = current(try widget("lift"), around: now.addingTimeInterval(-86_400))
        XCTAssertEqual(TodayProvider(load: { yesterday }).timeline(at: now).entries.map(\.face), [yesterday.staleFace])
        // A day still to come (the clock set back) shows from its start.
        let tomorrow = current(try widget("lift"), around: now.addingTimeInterval(7200))
        XCTAssertEqual(TodayProvider(load: { tomorrow }).timeline(at: now).entries.map(\.face), [tomorrow.staleFace, tomorrow.face, tomorrow.staleFace])
        // Written in another time zone (Android marks it stale on a zone change).
        let elsewhere = TimeZone.current.identifier == "Pacific/Kiritimati" ? "Etc/GMT+12" : "Pacific/Kiritimati"
        XCTAssertFalse(current(try widget("lift"), around: now, zone: elsewhere).isCurrent(at: now))
    }

    func testTheControlShowsTodaysTileFace() throws {
        let now = Date()
        let lift = current(try widget("lift"), around: now)
        XCTAssertEqual(lift.controlFace(at: now), lift.control)
        XCTAssertEqual(lift.controlFace(at: now).screen, .logSet)
        XCTAssertEqual(lift.controlFace(at: now.addingTimeInterval(7200)), lift.staleControl)
    }

    // MARK: Lines and layouts

    /// A card takes the lines the Kotlin side cut for its slot count.
    func testACardShowsTheLinesCutForItsSlots() throws {
        let body = try widget("lift").face.body
        XCTAssertEqual(body.lines(body.total), body.fits.last)
        XCTAssertEqual(body.lines(body.total + 3), body.fits.last, "more slots than lines")
        XCTAssertEqual(body.lines(0), [])
        XCTAssertEqual(body.lines(3), body.fits[3])
        let hold = try widget("hold").face.body
        XCTAssertTrue(hold.leadOnly)
        XCTAssertFalse(body.leadOnly)
    }

    func testTheCardCandidatesGoFromRichestToSparsest() throws {
        let run = try widget("run").face
        let wide = CardLayout.candidates(for: run, wide: true)
        XCTAssertEqual(Set(wide).count, wide.count, "no candidate twice")
        XCTAssertEqual(
            wide.first,
            CardLayout(overline: true, hero: .stacked, week: .strip, lines: 0, large: false, bigTitle: false)
        )
        XCTAssertEqual(wide.map(\.rank), wide.map(\.rank).sorted(by: >))
        // A lone lead line that would wrap to more than two lines gives way to the short one.
        let rest = try widget("rest").face
        let short = try XCTUnwrap(rest.shortBody)
        XCTAssertEqual(TodayCard.shortLead(rest, textWidth: 80).body, short.body)
        XCTAssertEqual(TodayCard.shortLead(rest, textWidth: 306).body, rest.body)
        // The hero ranks above the overline; a wide card can set it beside the buttons.
        XCTAssertEqual(wide.first(where: { $0.hero == .beside })?.overline, true)
        let small = CardLayout.candidates(for: run, wide: false)
        XCTAssertFalse(small.contains { $0.hero == .beside })
        XCTAssertLessThan(
            try XCTUnwrap(small.firstIndex { $0.hero == .stacked && !$0.overline }),
            try XCTUnwrap(small.firstIndex { $0.hero == .hidden && $0.overline })
        )
        // A small card never draws the week strip (Android: never two cells wide).
        XCTAssertFalse(small.contains { $0.week == .strip })
        // A statement has only its lines to give up.
        let hold = try widget("hold").face
        XCTAssertTrue(CardLayout.candidates(for: hold, wide: true).allSatisfy { $0.hero == .hidden && $0.week == .hidden })
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

    /// A frame that differs only in its clock is the same frame.
    func testARunFrameWithoutItsClock() throws {
        let run = try XCTUnwrap(Self.fixture.runs["recording"])
        XCTAssertTrue(run.recording)
        var later = run
        later.movingSec += 1
        later.time = "23:42"
        later.chip = "23:42"
        XCTAssertEqual(run.withoutClock, later.withoutClock)
        later.distance = "4.13 km"
        XCTAssertNotEqual(run.withoutClock, later.withoutClock)
        let rest = Self.fixture.rest
        XCTAssertEqual(rest.end.timeIntervalSince(rest.start), 90)
    }

    // MARK: Renders

    @MainActor
    func testEveryStateRendersInEveryFamily() {
        for (name, payload) in Self.fixture.widgets.sorted(by: { $0.key < $1.key }) {
            for scheme in [ColorScheme.light, .dark] {
                renderAllFamilies(name: name, payload: payload, scheme: scheme)
            }
        }
    }

    @MainActor
    func testTheActivitiesRender() throws {
        var recording = try XCTUnwrap(Self.fixture.runs["recording"])
        recording.step = "Rep 2 of 6 · 320 m left"
        let paused = try XCTUnwrap(Self.fixture.runs["paused"])
        let locating = try XCTUnwrap(Self.fixture.runs["locating"])
        let anchor = Date().addingTimeInterval(-Double(recording.movingSec))
        var rest = Self.fixture.rest
        rest.startMs = Int64(Date().timeIntervalSince1970 * 1000)
        rest.endMs = rest.startMs + 90_000
        for scheme in [ColorScheme.light, .dark] {
            let s = scheme == .dark ? "dark" : "light"
            render("activity-run-recording-\(s)", RunLockScreen(run: recording, anchor: anchor), width: 360, scheme: scheme)
            render("activity-run-paused-\(s)", RunLockScreen(run: paused, anchor: nil), width: 360, scheme: scheme)
            render("activity-run-locating-\(s)", RunLockScreen(run: locating, anchor: nil), width: 360, scheme: scheme)
            render("activity-rest-\(s)", RestLockScreen(rest: rest, stale: false), width: 360, scheme: scheme)
            render("activity-rest-done-\(s)", RestLockScreen(rest: rest, stale: true), width: 360, scheme: scheme)
        }
        let island = LollopActivityState(run: recording, runAnchor: anchor, rest: nil, stale: false)
        let expanded = ActivityExpanded(state: island)
        render(
            "activity-island-run-expanded",
            VStack(spacing: 8) {
                HStack { expanded.leading; Spacer(); expanded.center; Spacer(); expanded.trailing }
                expanded.bottom
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
    private func renderAllFamilies(name: String, payload: WidgetPayload, scheme: ColorScheme) {
        let face = payload.face
        let colors = payload.colors(for: scheme)
        let s = scheme == .dark ? "dark" : "light"
        for (family, size) in Self.families {
            let view: AnyView
            switch family {
            case "small", "medium", "large":
                view = AnyView(
                    TodayCard(face: face, colors: colors, wide: family != "small", buttons: .drawn)
                        .frame(width: size.width, height: size.height)
                        .background(colors.ground)
                        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
                )
            case "rectangular":
                // The lock screen draws these in white on the wallpaper (vibrant rendering).
                view = AnyView(RectangularView(face: face).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.black))
            case "circular":
                view = AnyView(CircularView(face: face).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.black))
            default:
                view = AnyView(InlineView(face: face).frame(width: size.width, height: size.height).foregroundColor(.white).background(Color.black))
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

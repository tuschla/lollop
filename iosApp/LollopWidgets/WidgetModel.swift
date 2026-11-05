import Foundation
import SwiftUI
import UIKit

// The widget payload the app writes to the App Group container on every change
// (IosWidgetPayload in android/lollop's IosSurfaces.kt): the common day snapshot
// and the faces the shared Kotlin code resolved from it, so the widgets and the
// control say what Android's widget and Quick Settings tile say.

struct WidgetPayload: Codable, Equatable {
    /// The payload format this extension reads; a file of another version reads as missing.
    static let supportedVersion = 1

    var version: Int
    var snapshot: SnapshotInfo
    var recording: Bool
    /// The widget for the snapshot's day.
    var face: Face
    /// The widget for any other day: "Open lollop to update".
    var staleFace: Face
    /// "N more items" at index N - 1.
    var moreItems: [String]
    var control: ControlFace
    var staleControl: ControlFace
    var palette: PaletteSet

    /// The parts of the common `WidgetSnapshot` the extension reads itself.
    struct SnapshotInfo: Codable, Equatable {
        var epochDay: Int64
        var stale: Bool
        /// Lift | Run | Rest | Paused | NoPlan | Setup
        var kind: String
        var weekDone: Int
        var weekPlanned: Int
        /// The time zone it was written in; empty in an old file.
        var zone: String
    }

    /// Whether the snapshot describes the local day of [date] in [zone]: not marked
    /// stale, the same day, written in the same zone. Android marks the snapshot
    /// stale at local midnight and on a time zone change; here the widget checks.
    func isCurrent(at date: Date, zone: TimeZone = .current) -> Bool {
        !snapshot.stale && snapshot.epochDay == localEpochDay(of: date, in: zone)
            && (snapshot.zone.isEmpty || snapshot.zone == zone.identifier)
    }

    func shownFace(at date: Date, zone: TimeZone = .current) -> Face {
        isCurrent(at: date, zone: zone) ? face : staleFace
    }

    func controlFace(at date: Date, zone: TimeZone = .current) -> ControlFace {
        isCurrent(at: date, zone: zone) ? control : staleControl
    }

    /// The payload in [data], or nil when it is unreadable or of another version.
    static func decode(_ data: Data) -> WidgetPayload? {
        guard let payload = try? JSONDecoder().decode(WidgetPayload.self, from: data),
              payload.version == supportedVersion
        else { return nil }
        return payload
    }

    /// The payload the app last wrote, or nil (no App Group container, nothing
    /// written yet, another version).
    static func load(from url: URL? = SharedContainer.payloadURL) -> WidgetPayload? {
        guard let url, let data = try? Data(contentsOf: url) else { return nil }
        return decode(data)
    }
}

/// Days since 1970-01-01 of [date]'s local date in [zone], as kotlinx-datetime's
/// `LocalDate.toEpochDays()` counts the snapshot's day.
func localEpochDay(of date: Date, in zone: TimeZone = .current) -> Int64 {
    let local = date.timeIntervalSince1970 + Double(zone.secondsFromGMT(for: date))
    return Int64((local / 86_400).rounded(.down))
}

/// The next local midnight after [date] in the current calendar and zone.
func nextMidnight(after date: Date, calendar: Calendar = .current) -> Date {
    let start = calendar.startOfDay(for: date)
    return calendar.date(byAdding: .day, value: 1, to: start) ?? date.addingTimeInterval(86_400)
}

/// What the widget shows (the common `WidgetFace`).
struct Face: Codable, Equatable {
    var overline: String
    /// The short date alone: a narrow card's overline.
    var date: String?
    var title: String
    /// normal | paused | muted
    var tone: String
    var status: String?
    var brief: String?
    var lead: [String]
    var items: [String]
    var hero: Hero?
    var heroLead: [String]
    var heroItems: [String]
    var week: String?
    var days: [Day]
    var actions: [Action]

    struct Hero: Codable, Equatable {
        var text: String
        var note: String?
        /// A figure (a run's distance, set large) rather than a session name.
        var figure: Bool
    }

    struct Day: Codable, Equatable {
        var label: String
        /// done | next | planned | missed | rest | none
        var mark: String
        var today: Bool
    }

    struct Action: Codable, Equatable {
        var label: String
        var dest: String
        /// recording | start_run | log_set | log: the tile face whose icon it shows.
        var face: String

        var screen: LollopScreen { LollopScreen(wire: dest) }
    }

    /// Only a title and its line (a hold, no plan, open to update): the statement card.
    var isStatement: Bool { hero == nil && week == nil && items.isEmpty }

    /// Shown before the app has written anything. The app's own strings
    /// (`widget_overline_today`, `widget_open_to_update`, `tile_log` in
    /// android/lollop/src/androidMain/res/values) for the one case it can't supply them.
    static let fallback = Face(
        overline: "Today", date: nil, title: "Open lollop to update", tone: "muted", status: nil, brief: nil,
        lead: [], items: [], hero: nil, heroLead: [], heroItems: [], week: nil, days: [],
        actions: [Action(label: "Log", dest: "log", face: "log")]
    )
}

/// The Control Center control's face (Android's Quick Settings tile).
struct ControlFace: Codable, Equatable {
    var label: String
    var subtitle: String?
    var dest: String
    var face: String
    var active: Bool

    var screen: LollopScreen { LollopScreen(wire: dest) }

    /// The SF Symbol for the face: a control shows symbols only.
    var symbol: String { Self.symbol(face: face) }

    static func symbol(face: String) -> String {
        switch face {
        case "recording": return "location.fill"
        case "start_run": return "location"
        case "log_set": return "dumbbell"
        default: return "plus"
        }
    }

    /// Shown before the app has written anything (`tile_log`).
    static let fallback = ControlFace(label: "Log", subtitle: nil, dest: "log", face: "log", active: false)
}

/// The lines a card shows in [maxLines] slots (`visibleLines`, WidgetFace.kt): every
/// lead line first, then items; when items are cut, the last slot reads "N more
/// items", unless it is the only item slot, which shows the first item.
func visibleLines(lead: [String], items: [String], maxLines: Int, moreItems: [String]) -> [String] {
    let shownLead = Array(lead.prefix(max(maxLines, 0)))
    let room = maxLines - shownLead.count
    if room <= 0 || items.isEmpty { return shownLead }
    if items.count <= room || room == 1 { return shownLead + items.prefix(room) }
    let shown = Array(items.prefix(room - 1))
    let cut = items.count - shown.count
    let more = moreItems.indices.contains(cut - 1) ? moreItems[cut - 1] : "+\(cut)"
    return shownLead + shown + [more]
}

// MARK: Colours

struct PaletteSet: Codable, Equatable {
    /// system | light | dark: the app's dark mode choice.
    var mode: String
    var light: WidgetColorsHex
    var dark: WidgetColorsHex

    func colors(for scheme: ColorScheme) -> WidgetColors {
        let dark: Bool
        switch mode {
        case "light": dark = false
        case "dark": dark = true
        default: dark = scheme == .dark
        }
        return WidgetColors(dark ? self.dark : light)
    }
}

/// The theme's colours as "#AARRGGBB" (the app's palette roles).
struct WidgetColorsHex: Codable, Equatable {
    var ground: String
    var body: String
    var muted: String
    var faint: String
    var accent: String
    var onAccent: String
    var accentTint: String
    var danger: String
    var done: String
}

struct WidgetColors {
    var ground: Color
    var body: Color
    var muted: Color
    var faint: Color
    var accent: Color
    var onAccent: Color
    var accentTint: Color
    var danger: Color
    var done: Color

    init(_ hex: WidgetColorsHex) {
        ground = Color(argb: hex.ground)
        body = Color(argb: hex.body)
        muted = Color(argb: hex.muted)
        faint = Color(argb: hex.faint)
        accent = Color(argb: hex.accent)
        onAccent = Color(argb: hex.onAccent)
        accentTint = Color(argb: hex.accentTint)
        danger = Color(argb: hex.danger)
        done = Color(argb: hex.done)
    }

    init(ground: Color, body: Color, muted: Color, faint: Color, accent: Color, onAccent: Color, accentTint: Color, danger: Color, done: Color) {
        self.ground = ground
        self.body = body
        self.muted = muted
        self.faint = faint
        self.accent = accent
        self.onAccent = onAccent
        self.accentTint = accentTint
        self.danger = danger
        self.done = done
    }

    /// Before the app has written a palette: the system's.
    static let system = WidgetColors(
        ground: Color(UIColor.systemBackground), body: .primary, muted: .secondary, faint: Color(UIColor.tertiaryLabel),
        accent: .primary, onAccent: Color(UIColor.systemBackground), accentTint: Color(UIColor.tertiarySystemFill),
        danger: .red, done: .green
    )

    func title(tone: String) -> Color {
        switch tone {
        case "paused": return danger
        case "muted": return muted
        default: return body
        }
    }
}

extension Color {
    /// "#AARRGGBB" or "#RRGGBB".
    init(argb: String) {
        var hex = argb.hasPrefix("#") ? String(argb.dropFirst()) : argb
        if hex.count == 6 { hex = "FF" + hex }
        let v = UInt32(hex, radix: 16) ?? 0xFF00_0000
        self.init(
            .sRGB,
            red: Double((v >> 16) & 0xFF) / 255,
            green: Double((v >> 8) & 0xFF) / 255,
            blue: Double(v & 0xFF) / 255,
            opacity: Double((v >> 24) & 0xFF) / 255
        )
    }
}

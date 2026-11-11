import Foundation
import SwiftUI
import UIKit

// The widget payload the app writes to the App Group container on every change
// (WidgetPayload, generated in Shared/SurfacePayload.swift from android/lollop's
// IosSurfacePayload.kt). The app resolves every text, line cut, glyph, icon and
// colour; the extension picks the face for the moment it draws and lays it out.

extension WidgetPayload {
    /// Whether [current] covers [date] in [zone]: the snapshot's local day, written
    /// in the zone the reader is in (Android marks it stale on a zone change).
    func isCurrent(at date: Date, zone: TimeZone = .current) -> Bool {
        guard let current else { return false }
        return current.zone == zone.identifier && current.start <= date && date < current.end
    }

    func shownFace(at date: Date, zone: TimeZone = .current) -> WidgetFace {
        isCurrent(at: date, zone: zone) ? face : staleFace
    }

    func controlFace(at date: Date, zone: TimeZone = .current) -> ControlFace {
        isCurrent(at: date, zone: zone) ? control : staleControl
    }

    /// The moments after [now] at which the shown face changes: the start and the
    /// end of the snapshot's day.
    func changes(after now: Date) -> [Date] {
        guard let current else { return [] }
        return [current.start, current.end].filter { $0 > now }
    }

    func colors(for scheme: ColorScheme) -> WidgetColors {
        WidgetColors(scheme == .dark ? dark : light)
    }

    /// The payload in [data], or nil when it is unreadable or of another version.
    static func decode(_ data: Data) -> WidgetPayload? {
        guard let payload = try? JSONDecoder().decode(WidgetPayload.self, from: data),
              payload.version == widgetPayloadVersion
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

extension CurrentDay {
    var start: Date { Date(timeIntervalSince1970: Double(startMs) / 1000) }
    var end: Date { Date(timeIntervalSince1970: Double(endMs) / 1000) }
}

extension WidgetFallback {
    /// What the widgets and the control show before the app has written a payload
    /// (fallback.json, which the app's code resolved at build time).
    static let bundled: WidgetFallback = {
        guard let url = Bundle.lollopWidgets.url(forResource: "fallback", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let fallback = try? JSONDecoder().decode(WidgetFallback.self, from: data)
        else { preconditionFailure("the extension lacks a readable fallback.json") }
        return fallback
    }()
}

extension WidgetLines {
    /// Every line, the most a card can show.
    var total: Int { fits.count - 1 }

    /// Only lead lines: nothing to cut but the lead.
    var leadOnly: Bool { total == lead }

    /// The lines [slots] line slots show.
    func lines(_ slots: Int) -> [String] { fits[min(max(slots, 0), total)] }
}

extension WidgetAction {
    var screen: LollopScreen { LollopScreen(wire: dest) }
}

extension ControlFace {
    var screen: LollopScreen { LollopScreen(wire: dest) }
}

// MARK: Colours

/// One appearance's colours, per [WidgetInk].
struct WidgetColors {
    private let resolve: (WidgetInk) -> Color

    init(_ palette: WidgetPalette) {
        resolve = { Color(argb: palette.hex($0)) }
    }

    private init(resolve: @escaping (WidgetInk) -> Color) {
        self.resolve = resolve
    }

    subscript(ink: WidgetInk) -> Color { resolve(ink) }

    var ground: Color { self[.ground] }
    var body: Color { self[.body] }
    var muted: Color { self[.muted] }
    var faint: Color { self[.faint] }
    var accent: Color { self[.accent] }
    var onAccent: Color { self[.onAccent] }
    var accentTint: Color { self[.accentTint] }

    /// Before the app has written a palette: the system's.
    static let system = WidgetColors { ink in
        switch ink {
        case .ground, .onAccent: return Color(UIColor.systemBackground)
        case .body, .accent: return .primary
        case .muted: return .secondary
        case .faint: return Color(UIColor.tertiaryLabel)
        case .accentTint: return Color(UIColor.tertiarySystemFill)
        case .danger: return .red
        case .done: return .green
        }
    }
}

extension WidgetPalette {
    func hex(_ ink: WidgetInk) -> String {
        switch ink {
        case .ground: return ground
        case .body: return body
        case .muted: return muted
        case .faint: return faint
        case .accent: return accent
        case .onAccent: return onAccent
        case .accentTint: return accentTint
        case .danger: return danger
        case .done: return done
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

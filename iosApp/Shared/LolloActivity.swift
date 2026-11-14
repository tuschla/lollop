import Foundation
#if canImport(ActivityKit)
import ActivityKit
#endif

// What a Live Activity shows, shared by the app (which starts, updates and ends
// the activities, iosApp/iosApp/Surfaces.swift) and the widget extension (which
// draws them, LolloWidgets/LiveActivityViews.swift). The app decodes the frames
// the Kotlin side resolves (RunLive, RestLive in SurfacePayload.swift) and passes
// them on.

extension RunLive {
    /// The deep link of a tap on the run's activity: the tracker, as Android's notification.
    static let openURL = LolloScreen.track.url

    /// This frame with its clock taken out: two frames that differ only in the
    /// moving time look the same while the timer text counts on its own.
    var withoutClock: RunLive {
        var copy = self
        copy.movingSec = 0
        copy.time = ""
        if recording { copy.chip = "" }
        return copy
    }
}

extension RestLive {
    /// The deep link of a tap on the rest's activity: the set editor, as Android's notification.
    static let openURL = LolloScreen.logSet.url

    var start: Date { Date(timeIntervalSince1970: Double(startMs) / 1000) }
    var end: Date { Date(timeIntervalSince1970: Double(endMs) / 1000) }
}

#if canImport(ActivityKit)
/// The one activity type, a run's or a rest's, so one extension widget draws both.
@available(iOS 16.1, *)
struct LolloActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var run: RunLive?
        /// When the moving time was zero, for the counting timer text.
        var runAnchor: Date?
        var rest: RestLive?
    }
}
#endif

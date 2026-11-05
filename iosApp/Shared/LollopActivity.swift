import Foundation
#if canImport(ActivityKit)
import ActivityKit
#endif

// What a Live Activity shows, shared by the app (which starts, updates and ends
// the activities, iosApp/iosApp/Surfaces.swift) and the widget extension (which
// draws them, LollopWidgets/LiveActivityViews.swift). The app decodes the JSON the
// Kotlin side builds (IosRunLive / IosRestLive in android/lollop's IosSurfaces.kt)
// into these and passes them on; every text is the app's.

/// A run's frame: Android's run notification and its status bar chip.
struct RunLive: Codable, Hashable {
    /// locating | armed | recording | paused | signal_lost
    var phase: String
    var title: String
    /// A state word, or the moving time while recording.
    var chip: String
    /// The fixed line Android shows in place of the figures (locating, waiting,
    /// signal lost, before the track has two fixes).
    var text: String?
    /// A guided run's step line.
    var step: String?
    var movingSec: Int64
    var time: String
    var distance: String?
    var pace: String?
    var movingLabel: String
    var distanceLabel: String
    var paceLabel: String
    /// Pause/Resume shows (a recording run).
    var canPause: Bool
    var paused: Bool
    var pauseLabel: String
    var resumeLabel: String
    var stopLabel: String
    var accentLight: String
    var accentDark: String

    var recording: Bool { phase == "recording" }

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

/// A rest's countdown: Android's rest countdown notification.
struct RestLive: Codable, Hashable {
    var title: String
    var next: String?
    /// Wall clock, epoch milliseconds.
    var startMs: Int64
    var endMs: Int64
    /// The title once the countdown ran out while the app could not end it.
    var doneTitle: String
    var accentLight: String
    var accentDark: String

    var start: Date { Date(timeIntervalSince1970: Double(startMs) / 1000) }
    var end: Date { Date(timeIntervalSince1970: Double(endMs) / 1000) }
}

#if canImport(ActivityKit)
/// The one activity type: a run or a rest ([kind]), so one extension widget draws both.
@available(iOS 16.1, *)
struct LollopActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var run: RunLive?
        /// When the moving time was zero, for the counting timer text.
        var runAnchor: Date?
        var rest: RestLive?
    }

    /// run | rest
    var kind: String
}
#endif

extension RunLive {
    /// The deep link of a tap on the run's activity: the tracker, as Android's notification.
    static let openURL = URL(string: "lollop://open/track")!
}

extension RestLive {
    /// The deep link of a tap on the rest's activity: the set editor, as Android's notification.
    static let openURL = URL(string: "lollop://open/log_set")!
}

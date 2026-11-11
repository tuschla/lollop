import ActivityKit
import Foundation
import Lollop
import WidgetKit

/// The app's side of its surfaces outside the window (`SurfaceHost` in the Kotlin
/// framework, android/lollop-ios's LollopSurfaces.kt): it writes the widget payload
/// to the App Group container and reloads the widgets and the control, and runs
/// the run's and the rest's Live Activities. The Kotlin side calls it on the main
/// thread whenever what they show changes.
final class Surfaces: NSObject, SurfaceHost {
    static let shared = Surfaces()

    /// Hand the surfaces to the Kotlin app. Once, at launch, before the UI.
    func install() {
        if #available(iOS 16.2, *) {
            LiveActivities.shared.endLeftovers()
        }
        LollopSurfaces.shared.install(host: self)
    }

    /// The app came to the front: a rest that ran out while it was suspended ends.
    func appBecameActive() {
        if #available(iOS 16.2, *) {
            LiveActivities.shared.endFinishedRest()
        }
    }

    func publishWidget(json: String) {
        guard let url = SharedContainer.payloadURL else {
            NSLog("lollop: no App Group container; the widgets show \"Open lollop to update\"")
            return
        }
        do {
            try Data(json.utf8).write(to: url, options: .atomic)
        } catch {
            NSLog("lollop: widget payload not written: \(error)")
            return
        }
        WidgetCenter.shared.reloadAllTimelines()
        if #available(iOS 18.0, *) {
            ControlCenter.shared.reloadAllControls()
        }
    }

    func updateRunActivity(json: String) {
        if #available(iOS 16.2, *), let run = decode(RunLive.self, json) {
            LiveActivities.shared.showRun(run)
        }
    }

    func endRunActivity() {
        if #available(iOS 16.2, *) {
            LiveActivities.shared.endRun()
        }
    }

    func updateRestActivity(json: String) {
        if #available(iOS 16.2, *), let rest = decode(RestLive.self, json) {
            LiveActivities.shared.showRest(rest)
        }
    }

    func endRestActivity() {
        if #available(iOS 16.2, *) {
            LiveActivities.shared.endRest()
        }
    }

    private func decode<T: Decodable>(_ type: T.Type, _ json: String) -> T? {
        do {
            return try JSONDecoder().decode(type, from: Data(json.utf8))
        } catch {
            NSLog("lollop: activity content not read: \(error)")
            return nil
        }
    }
}

/// The run's and the rest's Live Activities: started while the app is in front
/// (a run starts from the tracker, a rest from the set loop), updated from the
/// app, ended with the run or the rest. No push updates: a free Apple Account has
/// no push entitlement.
@available(iOS 16.2, *)
final class LiveActivities {
    static let shared = LiveActivities()

    private typealias LollopActivity = Activity<LollopActivityAttributes>

    private var run: LollopActivity?
    private var lastRun: RunLive?
    private var lastAnchor: Date?
    private var rest: LollopActivity?
    private var restEnd: Date?
    private var restEndWork: DispatchWorkItem?

    /// End the activities a previous run of the app left behind (it was ended by
    /// the system or crashed): a resumed run or rest starts its own.
    func endLeftovers() {
        let leftovers = LollopActivity.activities
        Task {
            for activity in leftovers {
                await activity.end(nil, dismissalPolicy: .immediate)
            }
        }
    }

    func showRun(_ frame: RunLive) {
        let anchor = Date().addingTimeInterval(-Double(frame.movingSec))
        // The timer text counts the moving time on its own: an update only for
        // what it can't show, or when the moving time stood still (or the clock
        // jumped) by more than a second.
        if let last = lastRun, let lastAnchor, last.withoutClock == frame.withoutClock,
           !frame.recording || abs(anchor.timeIntervalSince(lastAnchor)) < 1.5 {
            return
        }
        lastRun = frame
        lastAnchor = anchor
        let content = ActivityContent(state: LollopActivityAttributes.ContentState(run: frame, runAnchor: anchor, rest: nil), staleDate: nil)
        if let run {
            Task { await run.update(content) }
        } else {
            run = request(content)
        }
    }

    func endRun() {
        lastRun = nil
        lastAnchor = nil
        guard let activity = run else { return }
        run = nil
        Task { await activity.end(nil, dismissalPolicy: .immediate) }
    }

    /// Show [frame]'s rest; it reads "Rest done" from its end (its stale date) and
    /// is ended at the end while the app runs, or when it next comes to the front.
    func showRest(_ frame: RestLive) {
        guard frame.end > Date() else {
            endRest()
            return
        }
        let content = ActivityContent(state: LollopActivityAttributes.ContentState(run: nil, runAnchor: nil, rest: frame), staleDate: frame.end)
        if let rest {
            Task { await rest.update(content) }
        } else {
            rest = request(content)
        }
        restEnd = frame.end
        restEndWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.endFinishedRest() }
        restEndWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + frame.end.timeIntervalSinceNow, execute: work)
    }

    func endRest() {
        restEndWork?.cancel()
        restEndWork = nil
        restEnd = nil
        guard let activity = rest else { return }
        rest = nil
        Task { await activity.end(nil, dismissalPolicy: .immediate) }
    }

    /// End the rest's activity once its countdown is over (Android takes the
    /// countdown notification down at the rest end; the rest-done alert follows).
    func endFinishedRest() {
        if let restEnd, restEnd <= Date() {
            endRest()
        }
    }

    private func request(_ content: ActivityContent<LollopActivityAttributes.ContentState>) -> LollopActivity? {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return nil }
        do {
            return try LollopActivity.request(attributes: LollopActivityAttributes(), content: content, pushType: nil)
        } catch {
            NSLog("lollop: Live Activity not started: \(error)")
            return nil
        }
    }
}

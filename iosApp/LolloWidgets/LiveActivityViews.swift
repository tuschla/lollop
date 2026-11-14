import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

// The run's and the rest's Live Activities (visual-design-system §10, "Run tracking
// notification" and "Rest timer notification"): Android's Live Update card on the
// lock screen, its status bar chip in the Dynamic Island. The views take plain
// values, so the tests draw them without an activity.

/// The run on the lock screen: the title and chip, the moving time (counting on
/// its own while recording), distance and current pace, or Android's fixed line,
/// a guided run's step, and Pause/Resume and Stop.
struct RunLockScreen: View {
    let run: RunLive
    let anchor: Date?
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let accent = Color(argb: scheme == .dark ? run.accentDark : run.accentLight)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                SurfaceIcon.mark.image.resizable().scaledToFit().frame(width: 18, height: 18).foregroundColor(accent)
                Text(run.title).font(.headline).lineLimit(1)
                Spacer(minLength: 0)
                if !run.recording {
                    Text(run.chip).font(.subheadline.weight(.semibold)).foregroundColor(accent).lineLimit(1)
                }
            }
            if let step = run.step {
                Text(step).font(.subheadline).lineLimit(2)
            }
            if let text = run.text {
                Text(text).font(.subheadline).foregroundColor(.secondary).lineLimit(2)
            } else {
                HStack(alignment: .top, spacing: 20) {
                    RunStat(label: run.movingLabel) { RunTime(run: run, anchor: anchor) }
                    if let distance = run.distance {
                        RunStat(label: run.distanceLabel) { Text(distance) }
                    }
                    if let pace = run.pace {
                        RunStat(label: run.paceLabel) { Text(pace) }
                    }
                }
            }
            RunButtons(run: run, accent: accent)
        }
        .padding(16)
    }
}

struct RunTime: View {
    let run: RunLive
    let anchor: Date?

    var body: some View {
        if run.recording, let anchor {
            Text(timerInterval: anchor...Date.distantFuture, countsDown: false)
                .monospacedDigit()
        } else {
            Text(run.time).monospacedDigit()
        }
    }
}

struct RunStat<Value: View>: View {
    let label: String
    @ViewBuilder let value: () -> Value

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(label.uppercased()).font(.caption2.weight(.semibold)).foregroundColor(.secondary).lineLimit(1)
            value().font(.title3.weight(.bold)).lineLimit(1)
        }
    }
}

/// Pause/Resume while a run records (an App Intent the system runs in the app,
/// iOS 17+), and Stop, which opens the tracker: the run is saved or discarded
/// there, never from the lock screen (Android's Stop does the same).
struct RunButtons: View {
    let run: RunLive
    let accent: Color

    var body: some View {
        HStack(spacing: 8) {
            if run.canPause {
                if #available(iOS 17.0, *) {
                    Button(intent: SetRunPausedIntent(paused: !run.paused)) {
                        pill(run.paused ? run.resumeLabel : run.pauseLabel, icon: run.paused ? .play : .pause)
                    }
                    .buttonStyle(.plain)
                }
            }
            Link(destination: RunLive.openURL) {
                pill(run.stopLabel, icon: .stop)
            }
        }
    }

    private func pill(_ label: String, icon: SurfaceIcon) -> some View {
        HStack(spacing: 6) {
            icon.image.resizable().scaledToFit().frame(width: 14, height: 14)
            Text(label).font(.subheadline.weight(.semibold)).lineLimit(1)
        }
        .foregroundColor(accent)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Capsule().fill(accent.opacity(0.18)))
    }
}

/// The rest on the lock screen: "Resting · <exercise>", the next set, and the
/// countdown to the rest end; "Rest done" once the countdown ran out.
struct RestLockScreen: View {
    let rest: RestLive
    let stale: Bool
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let accent = Color(argb: scheme == .dark ? rest.accentDark : rest.accentLight)
        HStack(alignment: .center, spacing: 12) {
            SurfaceIcon.mark.image.resizable().scaledToFit().frame(width: 18, height: 18).foregroundColor(accent)
            VStack(alignment: .leading, spacing: 2) {
                Text(stale ? rest.doneTitle : rest.title).font(.headline).lineLimit(2)
                if let next = rest.next {
                    Text(next).font(.subheadline).foregroundColor(.secondary).lineLimit(2)
                }
            }
            Spacer(minLength: 0)
            RestCountdown(rest: rest, stale: stale)
                .font(.largeTitle.weight(.bold))
                .foregroundColor(accent)
                .frame(maxWidth: 120, alignment: .trailing)
        }
        .padding(16)
    }
}

/// The countdown to the rest end, stopped at zero.
struct RestCountdown: View {
    let rest: RestLive
    let stale: Bool

    var body: some View {
        if stale || rest.end <= rest.start {
            Text(verbatim: "0:00").monospacedDigit()
        } else {
            Text(timerInterval: rest.start...rest.end, countsDown: true)
                .monospacedDigit()
                .multilineTextAlignment(.trailing)
        }
    }
}

/// The Dynamic Island's compact and minimal parts: the route mark and Android's
/// chip (the moving time, a state word, the rest countdown).
struct ActivityChip: View {
    let state: LolloActivityState
    let minimal: Bool

    var body: some View {
        if let run = state.run {
            if run.recording {
                RunTime(run: run, anchor: state.runAnchor)
                    .frame(maxWidth: minimal ? 40 : 56)
            } else if minimal {
                SurfaceIcon.mark.image.resizable().scaledToFit().frame(width: 16, height: 16)
            } else {
                Text(run.chip).lineLimit(1)
            }
        } else if let rest = state.rest {
            RestCountdown(rest: rest, stale: state.stale)
                .frame(maxWidth: minimal ? 40 : 56)
        }
    }
}

/// An activity's content as the views read it, without ActivityKit's context.
struct LolloActivityState {
    var run: RunLive?
    var runAnchor: Date?
    var rest: RestLive?
    var stale: Bool

    var accentLight: Color? { (run?.accentLight ?? rest?.accentLight).map(Color.init(argb:)) }
    var accentDark: Color? { (run?.accentDark ?? rest?.accentDark).map(Color.init(argb:)) }
    var openURL: URL { run != nil ? RunLive.openURL : RestLive.openURL }
}

struct ActivityLockScreen: View {
    let state: LolloActivityState

    var body: some View {
        if let run = state.run {
            RunLockScreen(run: run, anchor: state.runAnchor)
        } else if let rest = state.rest {
            RestLockScreen(rest: rest, stale: state.stale)
        }
    }
}

/// The expanded Dynamic Island's parts.
struct ActivityExpanded {
    let state: LolloActivityState

    @ViewBuilder
    var leading: some View {
        if let run = state.run {
            RunStat(label: run.movingLabel) { RunTime(run: run, anchor: state.runAnchor) }
                .padding(.leading, 4)
        } else if state.rest != nil {
            SurfaceIcon.mark.image.resizable().scaledToFit().frame(width: 20, height: 20).padding(.leading, 4)
        }
    }

    @ViewBuilder
    var trailing: some View {
        if let run = state.run {
            if let distance = run.distance {
                RunStat(label: run.distanceLabel) { Text(distance) }.padding(.trailing, 4)
            } else {
                Text(run.chip).font(.headline).lineLimit(1).padding(.trailing, 4)
            }
        } else if let rest = state.rest {
            RestCountdown(rest: rest, stale: state.stale)
                .font(.title2.weight(.bold))
                .frame(maxWidth: 100, alignment: .trailing)
                .padding(.trailing, 4)
        }
    }

    @ViewBuilder
    var center: some View {
        if let run = state.run {
            Text(run.title).font(.headline).lineLimit(1)
        } else if let rest = state.rest {
            Text(state.stale ? rest.doneTitle : rest.title).font(.headline).lineLimit(1)
        }
    }

    @ViewBuilder
    var bottom: some View {
        if let run = state.run {
            VStack(alignment: .leading, spacing: 6) {
                if let step = run.step {
                    Text(step).font(.subheadline).lineLimit(1)
                }
                if let text = run.text {
                    Text(text).font(.subheadline).foregroundColor(.secondary).lineLimit(2)
                } else if let pace = run.pace {
                    Text(verbatim: "\(run.paceLabel) \(pace)").font(.subheadline).lineLimit(1)
                }
                RunButtons(run: run, accent: Color(argb: run.accentDark))
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else if let rest = state.rest, let next = rest.next {
            Text(next).font(.subheadline).foregroundColor(.secondary).lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// The run's and the rest's Live Activity.
@available(iOS 16.2, *)
struct LolloActivityWidget: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: LolloActivityAttributes.self) { context in
            let state = LolloActivityState(context.state, stale: context.isStale)
            ActivityLockScreen(state: state)
                .activitySystemActionForegroundColor(state.accentLight)
                .widgetURL(state.openURL)
        } dynamicIsland: { context in
            let state = LolloActivityState(context.state, stale: context.isStale)
            let accent = state.accentDark
            let expanded = ActivityExpanded(state: state)
            return DynamicIsland {
                DynamicIslandExpandedRegion(.leading) { expanded.leading }
                DynamicIslandExpandedRegion(.trailing) { expanded.trailing }
                DynamicIslandExpandedRegion(.center) { expanded.center }
                DynamicIslandExpandedRegion(.bottom) { expanded.bottom }
            } compactLeading: {
                SurfaceIcon.mark.image.resizable().scaledToFit().frame(width: 16, height: 16).foregroundColor(accent)
            } compactTrailing: {
                ActivityChip(state: state, minimal: false).foregroundColor(accent)
            } minimal: {
                ActivityChip(state: state, minimal: true).foregroundColor(accent)
            }
            .keylineTint(accent)
            .widgetURL(state.openURL)
        }
    }
}

@available(iOS 16.1, *)
extension LolloActivityState {
    init(_ content: LolloActivityAttributes.ContentState, stale: Bool) {
        self.init(run: content.run, runAnchor: content.runAnchor, rest: content.rest, stale: stale)
    }
}

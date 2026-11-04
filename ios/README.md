# lollop on iOS

Status: the Rust core builds for iOS and is callable from Kotlin/Native, and the
app's core engine (event log, replay, compaction) runs on iOS over it. Most of
the Compose UI is shared Kotlin now, but there is no iOS app target that shows
it yet. The iOS app is the Android app ported to Compose Multiplatform: the
screens live in shared Kotlin, and only platform services (location,
notifications, widgets, files) are written per platform. The app's Kotlin lives
in the multiplatform module `android/lollop`, which builds for Android and iOS.
Its `commonMain` holds the core engine and event log, the core's wire types and
events, the serial event queue (`EventDispatcher`), run tracking state
(`RunSession`, `RunSteps`), units, settings, the theme, and the Compose UI: the
design system, cards and charts, the log editors, guided setup and the profile
editor, Today and the tracker's controls and stats. They reach the platform
through `expect` seams with Android and iOS actuals (see below).

## Architecture

```
shared/ (Rust core, pure update/view)
  └─ shared/src/ffi.rs            platform-neutral bridge API (one implementation)
       ├─ android module          JNI exports Java_app_milestone_Core_*  → libshared.so (Android)
       └─ ffi/c_abi.rs            C ABI lollop_*  (feature c-abi)        → libshared.a  (iOS)
                                    header: shared/include/lollop.h (cbindgen)
kmp/ (Gradle build of the bridge alone)
  └─ core-bridge/
       ├─ commonMain  CoreBridge: the calls the core engine makes (update, view,
       │              parseFit, exportGpx, trackFigures, compactLog), same JSON wire
       ├─ nativeMain  NativeCore: CoreBridge over cinterop + lollop.h
       └─ nativeTest  bridge tests against the core's own golden fixtures
android/ (Gradle build of the app; includes kmp/core-bridge as :core-bridge)
  ├─ app/        application shell: application id, signing, R8, jniLibs
  ├─ lollop/     the app as a Kotlin Multiplatform module (namespace app.milestone)
  │    ├─ commonMain       Kotlin + Compose UI for both platforms, generated CMP
  │    │                   resources; the core engine DurableCore, EventLog,
  │    │                   the LogFiles seam
  │    ├─ androidMain      everything Android (the activity root, file pickers,
  │    │                   the map, run tracking service, rest timer, widget, Core.kt)
  │    ├─ iosMain          iOS actuals (POSIX event-log files, NSLog, …)
  │    ├─ commonTest       engine, formatting and date tests: JVM and iOS simulator
  │    └─ androidHostTest  JVM + Robolectric suite
  └─ lollop-ios/ the iOS framework `Lollop`: :lollop + :core-bridge + the
                 entry point LollopApp.start(); iosTest runs the engine on the
                 real core
```

`DurableCore` (commonMain) is the engine both platforms run: it sends events
through a `CoreBridge`, appends the accepted ones to the event log, replays the
log at boot (`restore`, with compaction and torn-tail repair in `EventLog`),
rebuilds the core after a `panic_reset`, and keeps the last good view when a
view fails. Android's engine is `object Core : DurableCore` with the JNI
natives; iOS's is a `BridgeCore` over `kmp/core-bridge`'s `NativeCore`, which
`LollopApp.start()` installs (`installCoreEngine`). The event queue sends to
it on `CoreDispatcher` (one dedicated thread, `newSingleThreadContext`).

`:lollop` doesn't depend on the cinterop module. Its commonMain declares
`CoreBridge` with the same calls as `kmp/core-bridge`'s, and `:lollop-ios`
adapts one to the other. So `:lollop`'s iOS klibs compile on Linux; only
`:lollop-ios`, which pulls in the cinterop module, needs macOS.

The other seams (`expect` in commonMain, actual per platform):

| Seam | Android | iOS |
|---|---|---|
| `PlatformLog` | `android.util.Log` | `NSLog` |
| `wallClockMillis`, `monotonicMillis` | `System.currentTimeMillis`, `SystemClock.elapsedRealtime` | `NSDate`, `clock_gettime_nsec_np(CLOCK_MONOTONIC)` (counts during sleep) |
| `formatFixed` (`%.Nf`) | `String.format(Locale.US, …)` | `halfUpFixed`, a Kotlin port of Java's half-up rounding of the shortest digits, held to `String.format` by the host tests |
| `appLocale`, `localizedDayFormatter`, `weekdayName`, `formatMediumDate`, `relativeDayLabel` | java.time + ICU best patterns, ICU `RelativeDateTimeFormatter` | `NSDateFormatter` (`dateFormatFromTemplate`, Gregorian, UTC wall clock), `NSRelativeDateTimeFormatter`; app locale = first English preferred language, else English + device region, `@numbers=latn` |
| `is24HourClock` | `DateFormat.is24HourFormat` | the locale's `j` hour pattern has no `a` |
| `KeyValueStore`, `PlatformContext` | `SharedPreferences`, `Context` | `NSUserDefaults(suiteName)`, one process-wide context |
| `toRadians` | `Math.toRadians` | multiply by π/180 |
| `platformCoreEngine` | `Core` | the `BridgeCore` `installCoreEngine` made |
| `LogPath`, `LogFiles` (event log files) | `java.io.File`: `appendText`, a buffered read, `FileOutputStream` + `fd.sync()`, `renameTo` | POSIX `open`/`read`/`write`/`rename`; the rewrite is flushed with `fcntl(F_FULLFSYNC)` (plain `fsync` leaves it in the drive cache on Apple hardware), `fsync` where that's unsupported |
| `eventLogPath`, `isDebuggable` | `filesDir/event-log.ndjson`, `FLAG_DEBUGGABLE` | `Application Support/event-log.ndjson`, `Platform.isDebugBinary` |
| `PlatformResources`, `getString` / `getQuantityString` / `getStringArray` (`Res` handles outside composition) | `android.content.res.Resources`, read through the `R` id of the same name (generated map) | CMP resources read synchronously in the system resource environment |
| `platformOkLabel` | `android.R.string.ok` | the app's "OK" |
| `BackHandler` | activity-compose `BackHandler` | CMP's `BackHandler` (UIKit back gesture) |
| `DialogResizesForKeyboard`, `SheetNavBarScrim` | dialog window `SOFT_INPUT_ADJUST_RESIZE`; no nav-bar contrast scrim | nothing to set: a CMP dialog already avoids the keyboard; iOS has no nav bar |
| `platformAccent` | Material You primary (Android 12+) | none: the theme's own accent |
| `currentAppLocale`, `screenHeightDp`, `currentPlatformContext` | `appLocale(LocalConfiguration.locales)`, `Configuration.screenHeightDp`, `LocalContext` | `appLocale()`, the Compose view's height, the process context |
| `localInstantSeconds` | `java.util.Calendar` (lenient across a DST gap) | `NSCalendar` (same rule) |
| `shareCoachSummary` | `ACTION_SEND` chooser | `UIActivityViewController` (popover on iPad) |

`commonTest` pins the formatting and date output to Android's (`FormattingTest`,
`DateFormatsTest`) and drives the engine on a scripted core and a real log file
(`DurableCoreTest`: replay, append, torn tail, compaction, panic reset,
degraded log, stale view). It runs in the host tests and, in the `bridge-macos`
CI job, in an iOS simulator, where the engine runs on the POSIX file seam.
`:lollop-ios`'s `CoreEngineTest` runs the engine on the real Rust core in the
simulator (restart replay, torn tail, the core's compaction, a real
`panic_reset`).

Both bindings call the same Rust functions in `shared::ffi` (`process_event`,
`current_view`, `parse_fit_json`, `track_figures_json`, `export_gpx`,
`compact_log_json`). Neither binding has logic of its own: they convert
pointers or JNI arrays to byte slices and back. The panic firewall (a core
panic becomes an `{"error":{"kind":"panic_reset",...}}` object and the core is
replaced) lives in `shared::ffi`, so both platforms get it.

The C ABI:

| C function | Android JNI equivalent | Answer |
|---|---|---|
| `lollop_update(event, len)` | `Core.update` | effect-request array, empty for a dropped event, or an error object |
| `lollop_view()` | `Core.view` | `ViewModel` JSON or an error object |
| `lollop_parse_fit(data, len)` | `Core.parseFit` | tracker-import JSON or `{"error":…}` |
| `lollop_track_figures(event, len)` | `Core.trackFigures` | `{"distance_km":…,"duration_min":…}` or `{"error":…}` |
| `lollop_export_gpx(entry_id, observed_at)` | `Core.exportGpx` | GPX text, empty when the run has no route |
| `lollop_compact_log(lines, count)` | `Core.compactLog` | compaction verdict JSON, empty when the core can't answer |
| `lollop_bytes_free(bytes)` | (JVM GC) | releases any buffer above |
| `lollop_reset_for_test()`, `lollop_panic_for_test(reset)` | `Core.resetForTest` | test builds only (feature `test-hooks`) |

Every answer is a `LollopBytes {data, len}` the caller owns and frees once;
`{NULL, 0}` is the empty answer. All calls are safe from any thread. The core's
derived-view memos are per thread, so the shell keeps `update`/`view` on one
thread, as Android does with its `core-jni` dispatcher.

### Why a C ABI and not UniFFI

- The JSON wire stays the single contract. Kotlin on Android already decodes
  the `ViewModel` JSON (`DurableCore`); the iOS app runs the same Kotlin, so typed
  bindings would only add a second model of the same data.
- No new crate dependencies: the C ABI is about 200 lines over `shared::ffi`.
  UniFFI would add its runtime crate and proc-macros to the core, and its
  Kotlin output targets the JVM (JNA), not Kotlin/Native.
- Kotlin/Native reads a C header directly through cinterop.
- Everything except the final iOS link works on Linux: cbindgen, the iOS
  staticlibs and the Kotlin/Native bridge tests (`linuxX64`).

## Building

### On Linux (no Apple SDK)

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim   # x86_64-apple-ios for Intel simulators
scripts/build-ios-core.sh             # release libshared.a for device + arm64 simulator
scripts/build-ios-core.sh x86_64-apple-ios
cd kmp && ./gradlew :core-bridge:linuxX64Test   # bridge tests on the host core
cd android && ./gradlew :lollop:compileKotlinIosArm64 :lollop:compileKotlinIosSimulatorArm64
```

`build-ios-core.sh` runs `cargo rustc -p shared --lib --crate-type staticlib
--features c-abi`: a staticlib is an archive, so no Apple linker or SDK is
needed. Measured 2026-11-03 (rustc 1.96.1, release profile `opt-level="z"`,
LTO): `aarch64-apple-ios/release/libshared.a` is 10.0 MB on disk. Of that,
1.38 MB is machine code (`__text`) and 0.41 MB constants; the rest is
embedded LLVM bitcode (1.38 MB), debug info and unwind tables. How much of
the archive ends up in the app depends on the final link and strip, which
needs a Mac and is not measured yet. For comparison, the Android arm64
`libshared.so` is 2.86 MB.

Regenerate the header after changing `shared/src/ffi/c_abi.rs`:

```sh
cbindgen --quiet --config shared/cbindgen.toml --crate shared --output shared/include/lollop.h
```

CI (`.github/workflows/ios.yml`) checks the header with `--verify`.

Kotlin/Native links Apple targets only on a macOS host; on Linux the link
tasks are skipped (`kotlin.native.ignoreDisabledTargets=true`). Compiling iOS
klibs works on Linux as long as the module has no cinterop: `:lollop`'s
`compileKotlinIos*` run in CI (`lollop-ios` job in `ci.yml`), while the iOS
compiles of `:core-bridge` and `:lollop-ios` are skipped on Linux because of
the cinterop. The engine can't get a `linuxX64` target of its own: `:lollop`'s
commonMain depends on Compose UI, which publishes no Linux native artifacts.
xtool (https://github.com/xtool-org/xtool) builds Swift packages for iOS on
Linux, but it can't run the Kotlin/Native compiler for Apple targets, so it
can't build this app.

### On macOS

```sh
cd android
./gradlew :lollop-ios:linkReleaseFrameworkIosArm64   # Lollop.framework, device
./gradlew :core-bridge:iosSimulatorArm64Test :lollop:iosSimulatorArm64Test :lollop-ios:iosSimulatorArm64Test
```

The Gradle build calls `scripts/build-ios-core.sh` itself: test binaries link a
debug core with the test hooks, frameworks link the release core without them
(`linkRustCore` in `kmp/core-bridge/build.gradle.kts`, which `:lollop-ios`
reuses). `-Plollop.iosSimulator=<udid>` picks the simulator. `Lollop.framework`
is dynamic and contains the Rust core; the Swift app calls
`LollopApp.shared.start()` at launch.

### Without a Mac: CI build + sideloading

1. The `bridge-macos` job in `.github/workflows/ios.yml` runs on a GitHub macOS
   runner (free for this public repo) and links `Lollop.framework`. Once the
   Xcode app exists, the same job builds it with `xcodebuild -sdk iphoneos
   CODE_SIGNING_ALLOWED=NO`, zips `Payload/lollop.app` into an unsigned `.ipa`
   and uploads it as an artifact.
2. Install SideStore (or AltStore) on the iPhone once, signed in with a free
   Apple ID.
3. Open the `.ipa` in SideStore; it signs it with the free Apple ID and
   installs it.
4. A free Apple ID signature expires after 7 days. SideStore refreshes it on
   the phone; otherwise reinstall. A refresh re-signs the installed app, so
   its data should stay (not tested yet).

Free Apple IDs limit how many app IDs one account can register per week, and
each app extension (widget, Live Activity, control) needs its own. Check
SideStore's current limits before adding extensions. App Groups, which the
widget needs to read the app's snapshot, also depend on what SideStore's
signing supports. Not tested yet.

## What the iOS app has to implement

The Rust core and, after the port, the Compose screens are shared. These are
the Android platform parts and their iOS counterparts. Swift is required where
Apple offers no other API (extensions).

| Android (file) | iOS |
|---|---|
| JNI core, `Core.kt` on the `core-jni` thread | `BridgeCore` over `kmp/core-bridge`'s `NativeCore`, installed by `LollopApp.start()`, on one dedicated thread |
| Event log `filesDir/event-log.ndjson`, replay on boot, compaction (`EventLog.kt`, `DurableCore.restore`) | Done: the same NDJSON file in Application Support through the same commonMain code, so an exported log can be imported on either platform |
| Crash-safe active run file (`ActiveRunStore.kt`) | Same file format in Application Support |
| GPS run tracking, foreground service with a location type (`RunTrackingService.kt`) | CoreLocation with the `location` background mode: `CLLocationUpdate.liveUpdates` + `CLBackgroundActivitySession` (iOS 17+) or `CLLocationManager.allowsBackgroundLocationUpdates`. "Always" permission is not needed while the run is active. |
| Live Update notifications for run and rest (`LiveUpdates.kt`, promoted notifications) | Live Activities (ActivityKit, Swift widget extension), Dynamic Island |
| Rest-done alert, exact alarm (`RestAlarm.kt`) | AlarmKit (iOS 26) for an alert that sounds in silent mode; else a time-interval local notification |
| Session reminder, boot receiver (`SessionReminder.kt`) | Calendar-trigger local notification (UserNotifications); iOS keeps it across reboots, no boot hook |
| Home screen widget (`widget/`, Glance) | WidgetKit extension (SwiftUI) reading a snapshot the app writes to an App Group container (Android: `WidgetSnapshot.kt`) |
| Quick Settings tile (`QuickTileService.kt`) | Control Center control (`ControlWidget`, iOS 18) in the widget extension |
| Deep links `OpenDest` (`OpenIntents.kt`): today, log, track, log_set | URL scheme `lollop://open/<dest>` or App Intents (`OpenIntent`) from widgets, controls and Live Activities |
| Open/share GPX, TCX, FIT into the app (`RunImport.kt`, VIEW/SEND intent filters) | Document types (`CFBundleDocumentTypes` + UTTypes for GPX/TCX/FIT) so Files and "Open in" work; a share extension for the Share sheet |
| Import a log, export a log/GPX/coach summary (`ProfileScreen.kt`, `Export.kt`, `CoachExport.kt`) | `UIDocumentPickerViewController` (import, export) and `UIActivityViewController` (share; the coach summary's is done, `CoachExport.ios.kt`) |
| Battery optimisation exemption prompt | None needed |
| Paper theme with the system (Material You) accent | iOS has no wallpaper palette: Paper uses its own accent (`platformAccent` is null); hiding the accent switch on iOS is an owner decision |
| Per-app language (`AppLocale.kt`) | Per-app language in iOS Settings (`CFBundleLocalizations`) |

## Next steps

1. Move the remaining screens to commonMain, each behind a real iOS
   implementation of what keeps it on Android: the activity root
   (`AppScaffold`, `CoachSession`, step 7 of the port), the rest timer
   (`RestAlarm`: `SetEditor`'s set loop), file pickers and SAF
   (`HistoryScreen`, `LogSheet`, `ProfileScreen`), the osmdroid map (`RunMap`,
   `TrackerMap` and the screens that show it: `RunDetailSheet`, `HistoryDetail`,
   `TrimRunSheet`, `TrackerDialogs`) and location permission + the tracking
   service (`RunTracking`).
2. Add the iOS app target (Compose `ComposeUIViewController` in a SwiftUI host)
   and the unsigned `.ipa` step to `bridge-macos`.
3. The Swift extensions: widget, Live Activity, control.

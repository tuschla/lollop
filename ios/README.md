# lollop on iOS

Status: the iOS app (`iosApp/`) is the Android app ported to Compose
Multiplatform: a SwiftUI host around the shared Compose UI, the core engine
(event log, replay, compaction) over the Rust core's C ABI. CI builds it as an
unsigned `.ipa` to sideload and launches it in a simulator. The app's Kotlin
lives in the multiplatform module `android/lollop`, which builds for Android
and iOS. Its `commonMain` holds the app root and boot, every screen (Today,
History, the Log sheet and its editors, Profile, guided setup, the tracker, run
detail and trim, the maps), the core engine and event log, the core's wire
types and events, the serial event queue (`EventDispatcher`), run tracking
(`RunSession`, `RunRecorder`, the active-run store), GPX/TCX parsing, units,
settings and the theme. They reach the platform through `expect` seams with
Android and iOS actuals (see below). Android-only so far: the home screen
widget, the Quick Settings tile and the Live Update notifications; their iOS
counterparts are Swift extensions (Next steps).

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
       │              parseFit, exportGpx, trackFigures, compactLog, reset), same JSON wire
       ├─ nativeMain  NativeCore: CoreBridge over cinterop + lollop.h
       └─ nativeTest  bridge tests against the core's own golden fixtures
android/ (Gradle build of the app; includes kmp/core-bridge as :core-bridge)
  ├─ app/        application shell: application id, signing, R8, jniLibs
  ├─ lollop/     the app as a Kotlin Multiplatform module (namespace app.milestone)
  │    ├─ commonMain       the app for both platforms: root, screens, core engine
  │    │                   (DurableCore, EventLog), run tracking, generated CMP
  │    │                   resources
  │    ├─ androidMain      Android actuals and what has no iOS twin yet (the
  │    │                   activity, osmdroid, location service, widget, tile,
  │    │                   Live Updates, Core.kt)
  │    ├─ iosMain          iOS actuals (LollopIosRoot, POSIX files, MapKit,
  │    │                   CoreLocation, UserNotifications, document pickers, …)
  │    ├─ commonTest       engine, formatting, date, map and run recorder tests:
  │    │                   JVM and iOS simulator
  │    └─ androidHostTest  JVM + Robolectric suite
  └─ lollop-ios/ the iOS framework `Lollop`: :lollop + :core-bridge + the
                 entry points LollopApp.start() and MainViewController();
                 iosTest runs the engine on the real core
iosApp/ (the iOS app: XcodeGen spec, SwiftUI host, Info.plist, assets)
  ├─ project.yml            target `lollop`, bundle id app.milestone.lollop; a
  │                         build phase runs :lollop-ios:embedAndSignAppleFrameworkForXcode
  ├─ iosApp/LollopIosApp.swift  @main SwiftUI app showing MainViewController()
  ├─ iosApp/Info.plist      document types (GPX/TCX/FIT), lollop:// URLs, location
  │                         When In Use + background mode, launch screen, orientations
  ├─ iosApp/Assets.xcassets app icon (light/dark/tinted) and launch logo
  └─ make-assets.py         renders those PNGs from the Android vector drawables
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
| `RouteLayer`, `FullScreenMapDialog` (RunMap.kt); `LiveMapSurface` via `newLiveMapSurface` (TrackerMap.kt) | osmdroid `MapView` with the android.graphics route renderer (RouteRenderer.android.kt); the full-screen map's dialog window with transparent bars | MKMapView in a `UIKitView`: OSM tiles through `OsmTileOverlay`/`OsmTileRenderer` (User-Agent = bundle identifier, 2 connections, a 128 MB disk cache that honours the server's cache headers, dark themes desaturate + invert each tile with Android's colour matrix, "© OpenStreetMap" caption); the route drawn in Compose over a passive map (`ComposeRoutePainter`, same `RouteLayout` as Android, the app owns the camera so tiles and route never part); the tracker's route as MKPolyline casing + accent lines, blip and start ring as annotations |

`commonTest` pins the formatting and date output to Android's (`FormattingTest`,
`DateFormatsTest`) and drives the engine on a scripted core and a real log file
(`DurableCoreTest`: replay, append, torn tail, compaction, panic reset,
degraded log, stale view), and the maps' route framing, layout and dark tile
filter (`RouteGeometryTest`, `MapTilesTest`, `FrameLongitudesTest`). It runs
in the host tests and, in the `bridge-macos`
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
| `lollop_reset()` | (process restart) | empties the core for the in-process data import |
| `lollop_panic_for_test(reset)` | — | test builds only (feature `test-hooks`) |

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
cd .. && brew install xcodegen && xcodegen generate --spec iosApp/project.yml
open iosApp/lollop.xcodeproj                         # run the `lollop` scheme
```

The Gradle build calls `scripts/build-ios-core.sh` itself: test binaries link a
debug core with the test hooks, frameworks link the release core without them
(`linkRustCore` in `kmp/core-bridge/build.gradle.kts`, which `:lollop-ios`
reuses). `-Plollop.iosSimulator=<udid>` picks the simulator. `Lollop.framework`
is dynamic and contains the Rust core.

The Xcode project is generated from `iosApp/project.yml` and not committed. Its
build phase runs `./gradlew :lollop-ios:embedAndSignAppleFrameworkForXcode`,
which builds `Lollop.framework` for the configuration and SDK Xcode is building
(Debug/Release, device/simulator) and copies it into the app. The Swift host
(`iosApp/iosApp/LollopIosApp.swift`) shows `MainViewController()`, which installs
the core engine (`LollopApp.start()`) and composes the app's root
(`LollopIosRoot()`, which restores the event log and sends SetToday), and hands
URLs the app is opened with (a GPX, TCX or FIT file from Files or the share
sheet) to `openUrl` (`LollopIosOpen`). The icon and launch logo PNGs are
rendered from the Android launcher icon and splash marks by
`iosApp/make-assets.py` (rsvg-convert + Pillow, runs on Linux); rerun it after
changing those drawables.

### Without a Mac: CI build + sideloading

`.github/workflows/ios-app.yml` runs on GitHub macOS runners (free for this
public repo) on every push to `main` that touches the app, and by hand
(`gh workflow run ios-app.yml`). Two jobs:

- **Unsigned .ipa**: `xcodegen generate`, `xcodebuild -configuration Release
  -sdk iphoneos CODE_SIGNING_ALLOWED=NO`, then `Payload/lollop.app` zipped into
  `lollop-unsigned.ipa`, uploaded as the artifact `lollop-unsigned-ipa` (kept
  30 days).
- **Launch smoke test**: a Debug build for an iPhone simulator, installed and
  launched; the job fails if the app writes a crash report or isn't running
  30 s after launch. The artifact `simulator-smoke` holds a screenshot and the
  app's log.

Installing on an iPhone with SideStore and a free Apple Account (the
"Install" page of https://docs.sidestore.io has the current steps):

1. Once, on a computer (Linux works): install [iloader](https://github.com/nab138/iloader),
   connect the iPhone by USB, sign in with the Apple Account (it doesn't have
   to be the one on the phone; a separate one is fine), select the phone and
   "Install SideStore (Stable)". On the phone: Settings → General → VPN &
   Device Management → trust the Apple Account; Settings → Privacy & Security
   → Developer Mode on (iOS 16+; the phone restarts). Install LocalDevVPN from
   the App Store and connect it, open SideStore, sign in, and in My Apps tap
   the "7 DAYS" counter next to SideStore to finish its setup.
2. Download the app: on the phone, open the latest successful "iOS app" run
   under the repo's Actions tab (signed in to GitHub: artifact downloads need
   an account even on a public repo) and download `lollop-unsigned-ipa`. Files
   unzips it to `lollop-unsigned.ipa` when tapped. From a computer:
   `gh run download --repo tuschla/lollop -n lollop-unsigned-ipa`, then copy
   the `.ipa` to the phone (or install it with iloader).
3. With LocalDevVPN connected and on Wi-Fi, SideStore → My Apps → `+` → pick
   `lollop-unsigned.ipa`. SideStore signs it with the Apple Account and installs
   it. Installing a newer `.ipa` the same way updates the app in place.
4. A free account's signature expires after 7 days; an expired app doesn't
   open until it is refreshed. SideStore refreshes in the background, or tap
   the app's day counter in My Apps (LocalDevVPN connected, Wi-Fi). A refresh
   re-signs the installed app in place, which keeps its data (not checked on
   this app yet). If refreshing stops working
   after an iOS update or reset, the pairing file has expired: connect to the
   computer and run iloader again.

Limits of a free Apple Account:

- 3 sideloaded apps installed at once, SideStore included: lollop leaves one
  slot free.
- 10 app IDs per 7 days. The app and each of its extensions register an app
  ID of their own, so a later widget extension (it can hold the widget, the
  Live Activity and the control) makes every fresh install cost two. When an
  `.ipa` has extensions, SideStore asks whether to keep them.
- App Groups (the widget reading the app's snapshot) need SideStore to
  register a group for the account; check that before building the widget.

Info.plist asks for location "When In Use" only
(`NSLocationWhenInUseUsageDescription`, plus
`NSLocationTemporaryUsageDescriptionDictionary` → `RunTracking` for precise
location when the user granted approximate), and declares the `location`
background mode so a run keeps recording with the screen locked. No "Always"
permission: the run is active while it records.

## What the iOS app has to implement

The Rust core and, after the port, the Compose screens are shared. These are
the Android platform parts and their iOS counterparts. Swift is required where
Apple offers no other API (extensions).

| Android (file) | iOS |
|---|---|
| JNI core, `Core.kt` on the `core-jni` thread | `BridgeCore` over `kmp/core-bridge`'s `NativeCore`, installed by `LollopApp.start()`, on one dedicated thread |
| Event log `filesDir/event-log.ndjson`, replay on boot, compaction (`EventLog.kt`, `DurableCore.restore`) | Done: the same NDJSON file in Application Support through the same commonMain code, so an exported log can be imported on either platform |
| Crash-safe active run file (`ActiveRunStore.kt`) | Done: the same commonMain store and NDJSON format in Application Support, over the `LogFiles` seam (POSIX) |
| GPS run tracking, foreground service with a location type (`RunTrackingService.kt`) | Done: `RunTracker.ios.kt`, a `CLLocationManager` (best accuracy, fitness activity, no distance filter, no auto-pause, `allowsBackgroundLocationUpdates` with the blue indicator) feeding the common `RunRecorder` (the same fix QC, armed start, pause, signal-loss watchdog and step ticker as Android's service) and the sidecar file; When In Use plus a one-off precise-location prompt; a run resumes after a relaunch from the sidecar. The Live Activity hook (`RunActivity`: sink + `setPaused`) is in iosMain, not yet exported by `:lollop-ios`. |
| Live Update notifications for run and rest (`LiveUpdates.kt`, promoted notifications) | Live Activities (ActivityKit, Swift widget extension), Dynamic Island |
| Rest-done alert, exact alarm (`RestAlarm.kt`) | Done: a time-interval local notification (`UNTimeIntervalNotificationTrigger`) and the in-app countdown; no exact-alarm notice. AlarmKit (iOS 26) would sound in silent mode. |
| Session reminder, boot receiver (`SessionReminder.kt`) | Done: a dated calendar-trigger local notification (`UNCalendarNotificationTrigger`) per upcoming session day; iOS keeps them across reboots, no boot hook |
| Home screen widget (`widget/`, Glance) | WidgetKit extension (SwiftUI) reading a snapshot the app writes to an App Group container (Android: `WidgetSnapshot.kt`) |
| Quick Settings tile (`QuickTileService.kt`) | Control Center control (`ControlWidget`, iOS 18) in the widget extension |
| Deep links `OpenDest` (`OpenIntents.kt`): today, log, track, log_set | URL scheme `lollop://open/<dest>` or App Intents (`OpenIntent`) from widgets, controls and Live Activities |
| Open/share GPX, TCX, FIT into the app (`RunImport.kt`, VIEW/SEND intent filters) | Done: document types in `iosApp/iosApp/Info.plist` (`CFBundleDocumentTypes` + imported UTTypes `com.topografix.gpx`, `com.garmin.tcx`, `com.garmin.fit`), so Files' "Open in" and the share sheet's app row copy the file into the app, whose `onOpenURL` hands it to `LollopIosOpen` |
| Import a log, export a log/GPX/coach summary (`ProfileScreen.kt`, `Export.kt`, `CoachExport.kt`) | Done: the backup NDJSON (`lollop-YYYY-MM-DD.ndjson`) is written to the temp dir and shared with `UIActivityViewController` (Save to Files, AirDrop; "Exported" only when the sheet completes); import picks one file with `UIDocumentPickerViewController` (streamed validation, 256 MiB cap) → confirm → log swap → in-process reboot (`DurableCore.resetForReboot` over `lollop_reset`); a run's GPX is shared as `run-YYYY-MM-DD-HHmm.gpx` (`Export.ios.kt`); the coach summary through `CoachExport.ios.kt` |
| Battery optimisation exemption prompt | None needed |
| Paper theme with the system (Material You) accent | iOS has no wallpaper palette: Paper uses its own accent (`platformAccent` is null) and Profile hides the "Use system accent" row (`platformHasDynamicAccent` = false, `ProfilePlatform.kt`), as on Android < 12. Profile's Live Updates notice is hidden too. |
| Per-app language (`AppLocale.kt`) | Per-app language in iOS Settings (`CFBundleLocalizations`) |

## Next steps

1. The Swift extensions (one widget extension): the home screen widget
   (WidgetKit, reading a snapshot the app writes to an App Group container),
   the Live Activity for run and rest (ActivityKit; the app side needs
   `RunActivity` exported from `:lollop-ios`), and the Control Center control.
   Each costs a free Apple Account an app ID (see the sideloading limits).

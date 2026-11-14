# lollo on iOS

Status: the iOS app (`iosApp/`) is the Android app ported to Compose
Multiplatform: a SwiftUI host around the shared Compose UI, the core engine
(event log, replay, compaction) over the Rust core's C ABI, and one Swift
extension with the home and lock screen widget, the Live Activities of a run
and a rest, and the Control Center control. CI builds it as an `.ipa` to
sideload and launches it in a simulator. The app's Kotlin
lives in the multiplatform module `android/lollo`, which builds for Android
and iOS. Its `commonMain` holds the app root and boot, every screen (Today,
History, the Log sheet and its editors, Profile, guided setup, the tracker, run
detail and trim, the maps), the core engine and event log, the core's wire
types and events, the serial event queue (`EventDispatcher`), run tracking
(`RunSession`, `RunRecorder`, the active-run store), the run import batch over
the core's `ParseRunFile` op (the core reads GPX, TCX and FIT), units,
settings, the theme, and what the widget and the tile show (`WidgetSnapshot`,
`widgetFace`, `TileFace`). They reach the platform through `expect` seams with
Android and iOS actuals (see below). The surfaces outside the app are drawn by
each platform (Glance and Android notifications; WidgetKit and ActivityKit in
Swift) from text the shared code resolves (see "Widgets, control and Live
Activities").

## Architecture

```
shared/ (Rust core, pure update/view)
  ├─ shared/src/ffi/ops.rs        every bridge op declared once (`Op`)
  ├─ shared/src/ffi/bridge.rs     answers an op on the core's own thread (one implementation)
  │    ├─ ffi/jni.rs              JNI exports Java_app_milestone_core_JniCore_{call,callLines} → libshared.so (Android)
  │    └─ ffi/c_abi.rs            C ABI lollo_call, lollo_call_lines (feature c-abi)       → libshared.a  (iOS)
  │                                header: shared/include/lollo.h (cbindgen)
  └─ shared/wiregen/              generates the Kotlin wire types (lollo's WireTypes.kt) and
                                  CoreBridge from the Rust types and the op table
android/ (the Gradle build of the app, both platforms)
  ├─ core-bridge/
  │    ├─ commonMain  CoreBridge (generated): one method per op in shared/src/ffi/ops.rs
  │    │              (update, view, parseRunFile, runTrack, draftFacts, …), same JSON wire
  │    ├─ jvmMain     JniCore: CoreBridge over the JNI entry points
  │    ├─ nativeMain  NativeCore: CoreBridge over cinterop + lollo.h
  │    └─ nativeTest  bridge tests against the core's own golden fixtures (linuxX64 + iOS)
  ├─ build-logic/ RustCore: every cargo build of the core (profiles and triples
  │               from scripts/lib/rust-core.env)
  ├─ app/        application shell: application id, signing, R8, the core's jniLibs
  ├─ lollo/      the app as a Kotlin Multiplatform module (namespace app.milestone)
  │    ├─ commonMain       the app for both platforms: root, screens, core engine
  │    │                   (DurableCore, EventLog), run tracking, generated CMP
  │    │                   resources
  │    ├─ androidMain      Android actuals and what has no iOS twin (the
  │    │                   activity, osmdroid, location service, Glance widget,
  │    │                   tile service, Live Updates, Core.kt)
  │    ├─ iosMain          iOS actuals (LolloIosRoot, POSIX files, MapKit,
  │    │                   CoreLocation, UserNotifications, document pickers, …)
  │    │                   and IosSurfaces.kt: hands the widget payload and the
  │    │                   Live Activity frames (commonMain widget/IosSurfacePayload.kt) to Swift
  │    ├─ commonTest       engine, formatting, date, map and run recorder tests:
  │    │                   JVM and iOS simulator
  │    ├─ iosTest          the iOS tracker, the surface payloads and the engine on
  │    │                   the real core (simulator)
  │    └─ androidHostTest  JVM + Robolectric suite
  └─ lollo-ios/  the iOS framework `Lollo`: :lollo (with :core-bridge) + the
                 entry points LolloApp.start(), MainViewController() and
                 LolloSurfaces (the Swift host of the widgets and activities)
iosApp/ (the iOS app: XcodeGen spec, SwiftUI host, extension, Info.plists, assets)
  ├─ project.yml            the identifiers once (LOLLO_BUNDLE_ID app.milestone.lollo,
  │                         LOLLO_APP_GROUP group.app.milestone.lollo; the
  │                         entitlements and Info.plists reference them); targets
  │                         `lollo` (a build phase runs
  │                         :lollo-ios:embedAndSignAppleFrameworkForXcode),
  │                         `LolloWidgets` (the extension, <bundle id>.widgets,
  │                         embedded in PlugIns/) and `LolloWidgetsTests`
  ├─ iosApp/LolloIosApp.swift   @main SwiftUI app showing MainViewController()
  ├─ iosApp/Surfaces.swift  writes the widget payload, runs the Live Activities
  ├─ iosApp/Info.plist      document types (GPX/TCX/FIT), lollo:// URLs, location
  │                         When In Use + background mode, Live Activities,
  │                         launch screen, orientations
  ├─ iosApp/Assets.xcassets app icon (light/dark/tinted) and launch logo
  ├─ LolloWidgets/          the widget extension (Swift only): widget, Live
  │                         Activities, control, icons (Assets.xcassets)
  ├─ LolloWidgetsTests/     the extension's tests and renders (simulator)
  ├─ Shared/                compiled into the app and the extension: the App Group
  │                         lookup, the activity content, the App Intents
  ├─ Entitlements/          the App Group of the app and the extension
  └─ make-assets.py         renders the PNGs from the Android vector drawables
```

`DurableCore` (commonMain) is the engine both platforms run: it sends events
through a `CoreBridge`, appends the accepted ones to the event log, replays the
log at boot (`restore`, with compaction and torn-tail repair in `EventLog`),
rebuilds the core after a `panic_reset`, and keeps the last good view when a
view fails. Android's engine is `object Core : DurableCore(JniCore)`; iOS's is
`DurableCore(NativeCore)` (`platformCoreEngine` in `AppCore.ios.kt`). The
event queue sends to it on `CoreDispatcher` (one dedicated thread,
`newSingleThreadContext`).

`AppRuntime` (commonMain) is the app's process: its coroutine scope, the engine,
the clock (`AppClock`), the boot ledger and every singleton's process state
(`ProcessLocal`). `AppRuntime.start(ctx)` runs once per process: it loads
`AppSettings` and starts each `AppFeature` (the common ones and
`platformFeatures()`: on iOS the notifications delegate and reminders, and the
widget sync). Android calls it from every component the system may start the
process for; iOS from `LolloApp.start()`. Host tests start each scenario from
`AppRuntime.resetForTest()`.

`:lollo` depends on `:core-bridge` on every target: its Android code runs on
`JniCore`, its iOS code on `NativeCore`. Kotlin compiles the iOS klibs,
cinterop included, on Linux; linking an iOS binary (the tests, `:lollo-ios`'s
framework) needs macOS and the core's staticlib.

The other seams (`expect` in commonMain, actual per platform):

| Seam | Android | iOS |
|---|---|---|
| `PlatformLog` | `android.util.Log` | `NSLog` |
| `wallClockMillis`, `deviceMonotonicMillis` (read through `AppClock`) | `System.currentTimeMillis`, `SystemClock.elapsedRealtime` | `NSDate`, `clock_gettime_nsec_np(CLOCK_MONOTONIC)` (counts during sleep) |
| `formatFixed` (`%.Nf`) | `String.format(Locale.US, …)` | `halfUpFixed`, a Kotlin port of Java's half-up rounding of the shortest digits, held to `String.format` by the host tests |
| `appLocale`, `localizedDayFormatter`, `weekdayName`, `formatMediumDate`, `relativeDayLabel` | java.time + ICU best patterns, ICU `RelativeDateTimeFormatter` | `NSDateFormatter` (`dateFormatFromTemplate`, Gregorian, UTC wall clock), `NSRelativeDateTimeFormatter`; app locale = first English preferred language, else English + device region, `@numbers=latn` |
| `is24HourClock` | `DateFormat.is24HourFormat` | the locale's `j` hour pattern has no `a` |
| `KeyValueStore`, `PlatformContext` | `SharedPreferences`, `Context` | `NSUserDefaults(suiteName)`, one process-wide context |
| `platformCoreEngine` (read through `AppRuntime.engine`) | `Core` | `DurableCore(NativeCore)` |
| `AppPath`, `AppFiles` (the app's files) | `java.io.File`: `appendText`, a buffered read, `FileOutputStream` + `fd.sync()`, `renameTo` | POSIX `open`/`read`/`write`/`rename`; the rewrite is flushed with `fcntl(F_FULLFSYNC)` (plain `fsync` leaves it in the drive cache on Apple hardware), `fsync` where that's unsupported |
| `eventLogPath`, `isDebuggable` | `filesDir/event-log.ndjson`, `FLAG_DEBUGGABLE` | `Application Support/event-log.ndjson`, `Platform.isDebugBinary` |
| `PlatformResources`, `getString` / `getQuantityString` / `getStringArray` (`Res` handles outside composition) | `android.content.res.Resources`, read through the `R` id of the same name (generated map) | CMP resources read synchronously in the system resource environment |
| `platformOkLabel` | `android.R.string.ok` | the app's "OK" |
| `BackHandler` | activity-compose `BackHandler` | CMP's `BackHandler` (UIKit back gesture) |
| `DialogResizesForKeyboard`, `SheetNavBarScrim` | dialog window `SOFT_INPUT_ADJUST_RESIZE`; no nav-bar contrast scrim | nothing to set: a CMP dialog already avoids the keyboard; iOS has no nav bar |
| `platformAccent` | Material You primary (Android 12+) | none: the theme's own accent |
| `currentAppLocale`, `screenHeightDp`, `currentPlatformContext` | `appLocale(LocalConfiguration.locales)`, `Configuration.screenHeightDp`, `LocalContext` | `appLocale()`, the Compose view's height, the process context |
| `localInstantSeconds` | `java.util.Calendar` (lenient across a DST gap) | `NSCalendar` (same rule) |
| `shareCoachSummary` | `ACTION_SEND` chooser | `UIActivityViewController` (popover on iPad) |
| `RouteLayer`, `FullScreenMapDialog` (RunMap.kt); `LiveMapSurface` via `newLiveMapSurface` (TrackerMap.kt) | osmdroid `MapView` with the common Compose `RoutePainter` (RoutePainter.kt) as its overlay; the full-screen map's dialog window with transparent bars | MKMapView in a `UIKitView`: OSM tiles through `OsmTileOverlay`/`OsmTileRenderer` (User-Agent = bundle identifier, 2 connections, a 128 MB disk cache that honors the server's cache headers, dark themes desaturate + invert each tile with Android's color matrix, "© OpenStreetMap" caption); the route drawn in Compose over a passive map (the same `RoutePainter` as Android, the app owns the camera so tiles and route never part); the tracker's route as MKPolyline casing + accent lines, blip and start ring as annotations |

`commonTest` pins the formatting and date output to Android's (`FormattingTest`,
`DateFormatsTest`) and drives the engine on a scripted core and a real log file
(`DurableCoreTest`: replay, append, torn tail, compaction, panic reset,
degraded log, stale view), and the maps' route framing, layout and dark tile
filter (`RouteGeometryTest`, `MapTilesTest`, `FrameLongitudesTest`). It runs
in the host tests and, in the `bridge-macos`
CI job, in an iOS simulator, where the engine runs on the POSIX file seam.
`:lollo`'s iOS `CoreEngineTest` runs the engine on the real Rust core in the
simulator (restart replay, torn tail, the core's compaction, a real
`panic_reset`).

Every bridge op is declared once, in `shared/src/ffi/ops.rs` (`Op`), and
answered by `shared/src/ffi/bridge.rs` on the core's own thread
(`lollo-core`), so the core's view memos stay on one thread and any shell
thread may call. The two bindings take an op's code with its input and have no
logic of their own: they convert pointers or JNI arrays to bytes and back.
`shared/wiregen` generates the Kotlin `CoreBridge` (one method per op) and
`CoreCalls` (each method calls its op's code) from the same table, so adding an
op is one `Op` entry, its arm in `bridge.rs`, a regeneration and its callers;
the header and the JNI exports stay the same. The panic firewall and the one
error envelope (`{"error":{"kind":…,"context":…,"message":…}}`; a core panic in
`update` answers `panic_reset` and replaces the core) live in the bridge, so
both platforms get them.

The C ABI:

| C function | Android JNI equivalent | Answer |
|---|---|---|
| `lollo_call(op, input, len)` | `JniCore.call(op, input)` | op `op`'s answer to the input bytes (none for `view` and `reset`): its payload, no bytes, or the error envelope |
| `lollo_call_lines(op, lines, count)` | `JniCore.callLines(op, lines)` | the same for an op that takes lines (`compactLog`, the event log); a line that is not UTF-8 fails the op |
| `lollo_bytes_free(bytes)` | (JVM GC) | releases any buffer above |
| `lollo_panic_for_test(reset)` | — | test builds only (feature `test-hooks`) |

Every answer is a `LolloBytes {data, len}` the caller owns and frees once;
`{NULL, 0}` is the empty answer. All calls are safe from any thread.

### Why a C ABI and not UniFFI

- The JSON wire stays the single contract. Kotlin on Android already decodes
  the `ViewModel` JSON (`DurableCore`); the iOS app runs the same Kotlin, so typed
  bindings would only add a second model of the same data.
- No new crate dependencies: the C ABI is about 130 lines over `shared::ffi`.
  UniFFI would add its runtime crate and proc-macros to the core, and its
  Kotlin output targets the JVM (JNA), not Kotlin/Native.
- Kotlin/Native reads a C header directly through cinterop.
- Everything except the final iOS link works on Linux: cbindgen, the iOS
  staticlibs and the Kotlin/Native bridge tests (`linuxX64`).

## Building

### On Linux (no Apple SDK)

```sh
scripts/build-ios-core.sh             # release libshared.a for device + arm64 simulator
cd android && ./gradlew :core-bridge:linuxX64Test   # bridge tests on the host core
cd android && ./gradlew :lollo:compileKotlinIosArm64 :lollo:compileKotlinIosSimulatorArm64
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
cbindgen --quiet --config shared/cbindgen.toml --crate shared --output shared/include/lollo.h
```

CI (`.github/workflows/ios.yml`) checks the header with `--verify`.

Kotlin/Native compiles and links Apple targets only on a macOS host; on Linux
those tasks are skipped (`kotlin.native.ignoreDisabledTargets=true`), including
`:lollo`'s `compileKotlinIos*`, because `:lollo` depends on `:core-bridge`,
whose cinterop can't be cross-compiled. Every iOS compile and test therefore
runs in `ios.yml`'s macOS job. The engine can't get a `linuxX64` target of its
own: `:lollo`'s commonMain depends on Compose UI, which publishes no Linux
native artifacts.
xtool (https://github.com/xtool-org/xtool) builds Swift packages for iOS on
Linux, but it can't run the Kotlin/Native compiler for Apple targets, so it
can't build this app.

### On macOS

```sh
cd android
./gradlew :lollo-ios:linkReleaseFrameworkIosArm64    # Lollo.framework, device
./gradlew :core-bridge:iosSimulatorArm64Test :lollo:iosSimulatorArm64Test
cd .. && brew install xcodegen && xcodegen generate --spec iosApp/project.yml
open iosApp/lollo.xcodeproj                          # run the `lollo` scheme
```

rustup installs the toolchain and the iOS targets from `rust-toolchain.toml`.
The Gradle build calls `scripts/build-ios-core.sh` itself: test binaries link a
debug core with the test hooks, frameworks link the release core without them
(`RustCore.link` in `android/build-logic`, used by `:core-bridge` and
`:lollo-ios`). `-Plollo.iosSimulator=<udid>` picks the simulator.
`Lollo.framework` is dynamic and contains the Rust core. The app version
(`MARKETING_VERSION`, `CURRENT_PROJECT_VERSION`) comes from
`app-version.xcconfig`, which the Android build reads too.

The Xcode project is generated from `iosApp/project.yml` and not committed. Its
build phase runs `./gradlew :lollo-ios:embedAndSignAppleFrameworkForXcode`,
which builds `Lollo.framework` for the configuration and SDK Xcode is building
(Debug/Release, device/simulator) and copies it into the app. The Swift host
(`iosApp/iosApp/LolloIosApp.swift`) starts the app once at launch
(`LolloApp.start()`: the core engine and `AppRuntime.start`), then shows
`MainViewController()`, which composes the app's root (`LolloIosRoot()`, which
restores the event log and sends SetToday), and hands
URLs the app is opened with (a GPX, TCX or FIT file from Files or the share
sheet) to `openUrl` (`LolloIosOpen`). Before that it hands the widgets and
activities to `Surfaces.swift` (`LolloSurfaces.install`). The icon, launch logo
and widget icon PNGs are rendered from the Android drawables by
`iosApp/make-assets.py` (rsvg-convert + Pillow, runs on Linux); rerun it after
changing those drawables.

### Without a Mac: CI build + sideloading

`.github/workflows/ios-app.yml` runs on GitHub macOS runners (free for this
public repo) on every push to `main` that touches the app, and by hand
(`gh workflow run ios-app.yml`). Three jobs:

- `.ipa`: `xcodegen generate`, `xcodebuild -configuration Release
  -sdk iphoneos CODE_SIGNING_ALLOWED=NO`, checks that `PlugIns/LolloWidgets.appex`
  is embedded with a widget extension Info.plist, signs the framework, the
  extension and the app ad hoc (`codesign -s -`) with the App Group entitlement
  (`iosApp/Entitlements/`), then `Payload/lollo.app` zipped into
  `lollo-unsigned.ipa`, uploaded as the artifact `lollo-unsigned-ipa` (kept
  30 days). The ad-hoc signature only carries the entitlement: SideStore reads an
  app's App Groups from the signed binaries and re-signs everything with the
  Apple Account (see below).
- Widget extension tests: `LolloWidgetsTests` in an iPhone simulator,
  without the Kotlin framework: the day rule and the midnight roll-over, the
  line fitting, the App Group lookup, the activity content, and renders of every
  widget family and state and of the activities (artifact `widget-renders`).
- Launch smoke test: a Debug build for an iPhone simulator, installed and
  launched; the job fails if the app writes a crash report or isn't running
  30 s after launch, if the extension isn't embedded, or if the app didn't write
  the widget payload to its App Group container. The extension's code then
  reads that payload (`testThePayloadTheAppWroteReads`) and draws it in every
  family. The artifact `simulator-smoke` holds a screenshot, the app's log, the
  payload and its renders.

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
2. Download the app: on the phone, open the latest release
   (https://github.com/tuschla/lollo/releases) and download
   `lollo-<version>-unsigned.ipa`. Between releases, the latest successful
   "iOS app" run under the repo's Actions tab has the current build as the
   artifact `lollo-unsigned-ipa` (signed in to GitHub: artifact downloads need
   an account even on a public repo; Files unzips it when tapped). From a
   computer: `gh release download --repo tuschla/lollo --pattern '*.ipa'`,
   then copy the `.ipa` to the phone (or install it with iloader).
3. With LocalDevVPN connected and on Wi-Fi, SideStore → My Apps → `+` → pick
   the `.ipa`. SideStore asks what to do with the app extension:
   pick "Keep App Extensions (Register App ID for Each Extension)" (without the
   extension there are no widgets, no Live Activities and no control; "Use Main
   Profile" signs the extension with the app's profile, which WidgetKit may
   refuse). SideStore signs it with the Apple Account and installs it.
   Installing a newer `.ipa` the same way updates the app in place.
4. A free account's signature expires after 7 days; an expired app doesn't
   open until it is refreshed. SideStore refreshes in the background, or tap
   the app's day counter in My Apps (LocalDevVPN connected, Wi-Fi). A refresh
   re-signs the installed app in place, which keeps its data (not checked on
   this app yet). If refreshing stops working
   after an iOS update or reset, the pairing file has expired: connect to the
   computer and run iloader again.

Limits of a free Apple Account:

- 3 sideloaded apps installed at once, SideStore included: lollo leaves one
  slot free.
- 10 app IDs per 7 days. The app and its extension each register one
  (SideStore asks per extension; the prompt names the 10-per-week limit), so
  every fresh install of lollo costs two. One extension holds the widget, the
  Live Activities and the control, so they cost one app ID together.
- App Groups work on a free account: SideStore registers the groups the signed
  binaries ask for (SideSign `Feature.freeFeatures` = App Groups and Inter-App
  Audio,
  https://github.com/SideStore/SideSign/blob/752d1ef602aa792a432ffa1e71646dc1ecf7363c/Sources/Models/Feature.swift#L78-L81;
  groups read from the Mach-O signature, none from an unsigned binary,
  https://github.com/SideStore/SideStore/blob/6032424a0e56c1c319762e786099bdd9186a238b/SideStore/Core/Operations/PipelineOperations/FetchProvisioningProfilesOperation.swift#L297-L301).
  It renames them for the account: `group.app.milestone.lollo` becomes
  `group.app.milestone.lollo.<TEAMID>` (same file, L428), and the bundle ids
  become `app.milestone.lollo.<TEAMID>` and `app.milestone.lollo.<TEAMID>.widgets`
  (L143-L150). The app and the extension find the group at runtime
  (`Shared/SharedContainer.swift`): `ALTAppGroups` in Info.plist (SideStore 0.6
  and 0.7 releases write it), else the groups in `embedded.mobileprovision`
  (every version, nightly included), else the declared group. SideStore's own
  widget extension shares its data the same way. Push notifications are not
  available to a free account, so the Live Activities are updated by the app only.

Info.plist asks for location "When In Use" only
(`NSLocationWhenInUseUsageDescription`, plus
`NSLocationTemporaryUsageDescriptionDictionary` → `RunTracking` for precise
location when the user granted approximate), and declares the `location`
background mode so a run keeps recording with the screen locked. No "Always"
permission: the run is active while it records.

## Widgets, control and Live Activities

One extension, `iosApp/LolloWidgets` (Swift, no Kotlin), draws what the app
hands it; the text and the decisions come from the same common Kotlin as
Android's (`widgetSnapshotOf`, `widgetFace`, `tileFace`, `tileSubtitle`,
`countdownText`, the notification strings).

- Data: the common `WidgetSurfaces` feature hands every change of the
  widget inputs (published model, distance unit, theme, dark mode, recording,
  time zone or clock change) to `IosWidgetSurface` (iosMain), which builds the
  payload of commonMain `widget/IosSurfacePayload.kt`: the widget faces for the
  snapshot's day and for any other day ("Open lollo to update"), each with its
  body lines already cut for every slot count, the short form of a long lead
  line with its width in em, the week strip's glyphs and color roles, the
  buttons' icons; the control's faces with their SF Symbols; the day the faces
  hold (start and end in its time zone); and the colors for the light and dark
  appearance. `Surfaces.swift` writes it to `widget-today.json` in the App Group
  container and reloads the widgets and controls (`WidgetCenter`,
  `ControlCenter`). Without a group container (the extension dropped at install)
  the widgets show the bundled `fallback.json` ("Open lollo to update").
- Generated contract: the Swift payload types and payload version
  (`Shared/SurfacePayload.swift`), the type and spacing tokens
  (`LolloWidgets/WidgetTheme.swift`), the English `Shared/en.lproj/Localizable.strings`
  the widget, control and App Intents name by resource key, `fallback.json`, the
  Swift tests' `fixtures.json`, the launch background and `widget-icons.json`
  (make-assets.py's icon list) are written from the Kotlin by android/lollo's
  `GeneratedSourcesTest`, which fails when a committed file differs. Regenerate:
  `LOLLO_WRITE_GENERATED=1 ./gradlew :lollo:testAndroidHostTest --tests app.milestone.GeneratedSourcesTest --rerun`.
- Day rule: as Android's. The timeline shows the snapshot's face while the
  reader's clock is within its day and in its time zone, and the stale face
  after (an entry at the day's end); a snapshot marked stale holds no day. The
  app writes a new one when it publishes a model for the new day.

| Surface | Families / parts | Taps |
|---|---|---|
| Home screen widget (Android's widget) | small (Android's 2×2: date overline, full-width primary), medium and large (Android's 4×2 and 4×3: "Today · date", week strip, Log beside the primary); the richest layout whose parts fit at the reader's text size, in Android's order (`ViewThatFits` over `CardLayout.candidates`); the theme's palette, light/dark per the app's dark mode | the widget opens Today, a button its screen (`Link`; a small widget's button runs `OpenLolloIntent` from iOS 17, before that the small widget opens the primary's screen) |
| Lock screen widgets | rectangular: the title over the status line (Android's one-row widget without buttons); circular: the week's sessions done of planned, else the primary's icon; inline: title · short line | Today |
| Control Center control (iOS 18, the Quick Settings tile) | the tile's face and the widget's title under it (`ControlValueProvider` reads the payload); SF Symbols, as controls show symbols only | `OpenLolloIntent` (an `OpenIntent`, run in the app) opens the tile's screen |
| Run Live Activity (the run Live Update) | lock screen: title, chip word, moving time (a timer text from the moving time's anchor while recording), distance, pace now, or Android's fixed line; a guided run's step; Pause/Resume and Stop. Dynamic Island: route mark + moving time or chip (compact, minimal), the figures and buttons (expanded) | the tracker; Pause/Resume runs `SetRunPausedIntent` (a `LiveActivityIntent`, iOS 17+, in the app) → `RunActivity.setPaused`; Stop opens the tracker |
| Rest Live Activity (the rest countdown) | "Resting · <exercise>", the next set, the countdown to the end; ended at the rest end while the app runs, else it reads "Rest done" from its stale date and ends when the app comes to the front | the set editor |

The activities start while the app is in front (the tracker, the set loop; the
system hides an activity of the app in front) and are updated by the app only:
a run's frames come from the tracker (`IosSurfaces.showRun`), a rest's from `RestAlarm`.
An update is sent only for what the timer text can't show. Live Activities need
iOS 16.2 and the user's Live Activities setting; widgets work from iOS 16.

## What the iOS app has to implement

The Rust core and, after the port, the Compose screens are shared. These are
the Android platform parts and their iOS counterparts. Swift is required where
Apple offers no other API (extensions).

| Android (file) | iOS |
|---|---|
| JNI core, `Core.kt` (`DurableCore(JniCore)`) on the `core-jni` thread | `DurableCore(NativeCore)` over `android/core-bridge`'s C ABI binding, on one dedicated thread |
| Event log `filesDir/event-log.ndjson`, replay on boot, compaction (`EventLog.kt`, `DurableCore.restore`) | Done: the same NDJSON file in Application Support through the same commonMain code, so an exported log can be imported on either platform |
| Crash-safe active run file (`ActiveRunStore.kt`) | Done: the same commonMain store and NDJSON format in Application Support, over the `AppFiles` seam (POSIX) |
| GPS run tracking, foreground service with a location type (`RunTrackingService.kt`) | Done: `RunTracker.ios.kt`, a `CLLocationManager` (best accuracy, fitness activity, no distance filter, no auto-pause, `allowsBackgroundLocationUpdates` with the blue indicator) feeding the common `RunRecorder` (the same fix QC, armed start, pause, signal-loss watchdog and step ticker as Android's service) and the sidecar file; When In Use plus a one-off precise-location prompt; a run resumes after a relaunch from the sidecar. Its frames go to the run's Live Activity (`IosSurfaces.showRun`). |
| Live Update notifications for run and rest (`LiveUpdates.kt`, promoted notifications) | Done: Live Activities with the Dynamic Island (see above) |
| Rest-done alert, exact alarm (`RestAlarm.kt`) | Done: a time-interval local notification (`UNTimeIntervalNotificationTrigger`) and the in-app countdown; no exact-alarm notice. AlarmKit (iOS 26) would sound in silent mode. |
| Session reminder, boot receiver (`SessionReminder.kt`) | Done: a dated calendar-trigger local notification (`UNCalendarNotificationTrigger`) per upcoming session day; iOS keeps them across reboots, no boot hook |
| Home screen widget (`widget/`, Glance) | Done: WidgetKit widget, home and lock screen (see above) |
| Quick Settings tile (`QuickTileService.kt`) | Done: Control Center control (`ControlWidget`, iOS 18) in the widget extension |
| Deep links `OpenDest` (`OpenIntents.kt`): today, log, track, log_set | Done: URL scheme `lollo://open/<dest>` (widget links, activities) and `OpenLolloIntent` (control, small widget's button) |
| Open/share GPX, TCX, FIT into the app (`RunImport.kt`, VIEW/SEND intent filters) | Done: document types in `iosApp/iosApp/Info.plist` (`CFBundleDocumentTypes` + imported UTTypes `com.topografix.gpx`, `com.garmin.tcx`, `com.garmin.fit`), so Files' "Open in" and the share sheet's app row copy the file into the app, whose `onOpenURL` hands it to `LolloIosOpen` |
| Import a log, export a log/GPX/coach summary (`ProfileScreen.kt`, `Export.kt`, `CoachExport.kt`) | Done: the backup NDJSON (`lollo-YYYY-MM-DD.ndjson`) is written to the temp dir and shared with `UIActivityViewController` (Save to Files, AirDrop; "Exported" only when the sheet completes); import picks one file with `UIDocumentPickerViewController` (streamed validation, 256 MiB cap) → confirm → log swap → in-process reboot (`AppRuntime.requestReboot`; the boot turn resets the core through the `reset` op), as on Android; a run's GPX is shared as `run-YYYY-MM-DD-HHmm.gpx` (`Export.ios.kt`); the coach summary through `CoachExport.ios.kt` |
| Battery optimization exemption prompt | None needed |
| Paper theme with the system (Material You) accent | iOS has no wallpaper palette: Paper uses its own accent (`platformAccent` is null) and Profile hides the "Use system accent" row (`platformHasDynamicAccent` = false, `ProfilePlatform.kt`), as on Android < 12. Profile's Live Updates notice is hidden too. |
| Per-app language (`AppLocale.kt`) | Per-app language in iOS Settings (`CFBundleLocalizations`) |

## Next steps

1. On a phone: the widgets, the control and the Live Activities under a
   SideStore free-account signature (the simulator tests cover the app's
   payload and the extension's code; adding a widget, the control and a Live
   Activity needs a device). See "Widgets, control and Live Activities".

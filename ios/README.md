# lollop on iOS

Status: the Rust core builds for iOS and is callable from Kotlin/Native. There
is no iOS UI yet. The iOS app will be the Android app ported to Compose
Multiplatform: the screens move to shared Kotlin, and only platform services
(location, notifications, widgets, files) are written per platform.

## Architecture

```
shared/ (Rust core, pure update/view)
  └─ shared/src/ffi.rs            platform-neutral bridge API (one implementation)
       ├─ android module          JNI exports Java_app_milestone_Core_*  → libshared.so (Android)
       └─ ffi/c_abi.rs            C ABI lollop_*  (feature c-abi)        → libshared.a  (iOS)
                                    header: shared/include/lollop.h (cbindgen)
kmp/ (Kotlin Multiplatform build)
  └─ core-bridge/
       ├─ commonMain  CoreBridge: the calls Core.kt makes (update, view, parseFit,
       │              exportGpx, trackFigures, compactLog), same JSON wire
       ├─ nativeMain  NativeCore: CoreBridge over cinterop + lollop.h
       └─ nativeTest  bridge tests against the core's own golden fixtures
```

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
  the `ViewModel` JSON (`Core.kt`); the iOS app runs the same Kotlin, so typed
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

Kotlin/Native compiles and links Apple targets only on a macOS host. On Linux
the `iosArm64` / `iosSimulatorArm64` targets are skipped with a warning
(`kotlin.native.ignoreDisabledTargets=true`). xtool (https://github.com/xtool-org/xtool)
builds Swift packages for iOS on Linux, but it can't run the Kotlin/Native
compiler for Apple targets, so it can't build this app.

### On macOS

```sh
cd kmp
./gradlew :core-bridge:linkReleaseFrameworkIosArm64   # LollopCore.framework, device
./gradlew :core-bridge:iosSimulatorArm64Test          # bridge tests in a simulator
```

The Gradle build calls `scripts/build-ios-core.sh` itself: test binaries link a
debug core with the test hooks, frameworks link the release core without them.
`-Plollop.iosSimulator=<udid>` picks the simulator.

### Without a Mac: CI build + sideloading

1. The `bridge-macos` job in `.github/workflows/ios.yml` runs on a GitHub macOS
   runner (free for this public repo). Once the app module exists, the same job
   builds the app with `xcodebuild -sdk iphoneos CODE_SIGNING_ALLOWED=NO`, zips
   `Payload/lollop.app` into an unsigned `.ipa` and uploads it as an artifact.
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
| JNI core, `Core.kt` on the `core-jni` thread | `CoreBridge` (`kmp/core-bridge`) on one dedicated thread |
| Event log `filesDir/event-log.ndjson`, replay on boot, compaction (`EventLog.kt`, `Core.restore`) | Same NDJSON file in Application Support; same replay and compaction code once `Core.kt` moves to commonMain, so an exported log can be imported on either platform |
| Crash-safe active run file (`ActiveRunStore.kt`) | Same file format in Application Support |
| GPS run tracking, foreground service with a location type (`RunTrackingService.kt`) | CoreLocation with the `location` background mode: `CLLocationUpdate.liveUpdates` + `CLBackgroundActivitySession` (iOS 17+) or `CLLocationManager.allowsBackgroundLocationUpdates`. "Always" permission is not needed while the run is active. |
| Live Update notifications for run and rest (`LiveUpdates.kt`, promoted notifications) | Live Activities (ActivityKit, Swift widget extension), Dynamic Island |
| Rest-done alert, exact alarm (`RestAlarm.kt`) | AlarmKit (iOS 26) for an alert that sounds in silent mode; else a time-interval local notification |
| Session reminder, boot receiver (`SessionReminder.kt`) | Calendar-trigger local notification (UserNotifications); iOS keeps it across reboots, no boot hook |
| Home screen widget (`widget/`, Glance) | WidgetKit extension (SwiftUI) reading a snapshot the app writes to an App Group container (Android: `WidgetSnapshot.kt`) |
| Quick Settings tile (`QuickTileService.kt`) | Control Center control (`ControlWidget`, iOS 18) in the widget extension |
| Deep links `OpenDest` (`OpenIntents.kt`): today, log, track, log_set | URL scheme `lollop://open/<dest>` or App Intents (`OpenIntent`) from widgets, controls and Live Activities |
| Open/share GPX, TCX, FIT into the app (`RunImport.kt`, VIEW/SEND intent filters) | Document types (`CFBundleDocumentTypes` + UTTypes for GPX/TCX/FIT) so Files and "Open in" work; a share extension for the Share sheet |
| Import a log, export a log/GPX/coach summary (`ProfileScreen.kt`, `Export.kt`, `CoachExport.kt`) | `UIDocumentPickerViewController` (import, export) and `UIActivityViewController` (share) |
| Battery optimisation exemption prompt | None needed |
| Paper theme with the system (Material You) accent | iOS has no wallpaper palette: Paper uses its own accent; owner decision |
| Per-app language (`AppLocale.kt`) | Per-app language in iOS Settings (`CFBundleLocalizations`) |

## Next steps

1. Move `Core.kt`'s Kotlin-only logic (event types, replay, compaction, the
   wire decoders) into `kmp` commonMain with an Android `actual` over the
   existing JNI exports, then the Compose screens, after the current Android UX
   work lands.
2. Add the iOS app target (Compose `ComposeUIViewController` in a SwiftUI host)
   and the unsigned `.ipa` step to `bridge-macos`.
3. The Swift extensions: widget, Live Activity, control.

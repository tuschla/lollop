# lollop

Offline training coach for strength, running and hybrid athletes. Every
recommendation shows its evidence grade and citation.

- `shared/`: Rust core (crux). Deterministic: the same event log always gives the same view.
- `android/`: the app's Gradle build. `android/lollop` is the app (Compose Multiplatform, for Android and iOS),
  `android/app` the Android shell, `android/lollop-ios` the iOS framework, `android/core-bridge` the core's
  bridge (`CoreBridge`: JNI on Android, the C ABI on iOS). Bridge ops are declared once in `shared/src/ffi/ops.rs`.
- `iosApp/`: the iOS app around that framework. See `ios/README.md`.

## Install

[Releases](https://github.com/tuschla/lollop/releases) carry the Android APK
(arm64, updates install over earlier releases) and the iOS `.ipa` for SideStore
(`ios/README.md`). `scripts/release.sh 0.2.0` cuts one: it sets the version in
`app-version.xcconfig`, bumps the build number, tags `v0.2.0` and pushes; the
tag runs `.github/workflows/release.yml`.

## Build

```sh
scripts/build.sh              # cargo test, Android tests, debug + release APKs (Gradle builds the core's .so files)
scripts/build.sh --no-tests   # build only
```

Needs rustup (it installs the toolchain and targets in `rust-toolchain.toml`), cargo-ndk,
the Android NDK, the JDK in `.java-version` and the Android SDK (`scripts/lib/env.sh`
finds them). Plugin, library and SDK versions are in `android/gradle/libs.versions.toml`,
the app version in `app-version.xcconfig`. Set `MILESTONE_KEYSTORE`, `MILESTONE_KS_PASS`
and `MILESTONE_KS_ALIAS` to sign a release (`scripts/sign-release.sh`); without them the
release APK is signed with the debug key. Releases are signed with the release key, which
lives in the repo secrets and, for the owner, in `~/.local/share/lollop/`. Android only
updates an app signed with the same key, so that key must never be lost or replaced. The
app id and package are still `app.milestone`.

## Test

```sh
cargo test                                                    # core
cd android && ./gradlew :lollop:testAndroidHostTest           # JVM + Robolectric UI tests on the real core
cd android && ./gradlew :lollop:verifyRoborazziAndroidHostTest # screenshots (baselines are local, not in git)
cd android && ./gradlew :lollop:compileKotlinIosArm64         # the shared module's iOS build (macOS only; skipped on Linux)
scripts/qa-emulator.sh                                        # on-device smoke: seeded log, set flow, monkey (CI's emulator job)
scripts/emulator-scenarios.sh                                 # on-device scenarios
scripts/emu-scenarios/surfaces.sh                             # widget, QS tile, open intents, Live Updates, session reminder
cd android && ./gradlew :core-bridge:linuxX64Test             # Kotlin/Native bridge to the core's C ABI (iOS)
```

The device scripts boot AVD `e37` (API 37) on console port 5582; `--avd` and
`--port` pick another.

## Privacy

Everything stays on the device. The only network use is OpenStreetMap tiles for
run maps, which can be turned off in Profile.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

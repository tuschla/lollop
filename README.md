# lollop

Offline training coach for strength, running and hybrid athletes. Every
recommendation shows its evidence grade and citation.

- `shared/`: Rust core (crux). Deterministic: the same event log always gives the same view.
- `android/`: the app's Gradle build. `android/lollop` is the app (Compose Multiplatform, for Android and iOS),
  `android/app` the Android shell, `android/lollop-ios` the iOS framework, `android/core-bridge` the core's
  C ABI binding for Kotlin/Native. Android talks to the core over JNI (`shared/src/ffi.rs`).
- `iosApp/`: the iOS app around that framework. See `ios/README.md`.

## Build

```sh
scripts/build.sh              # cargo test, Android tests, debug + release APKs (Gradle builds the core's .so files)
scripts/build.sh --no-tests   # build only
```

Needs rustup (it installs the toolchain and targets in `rust-toolchain.toml`), cargo-ndk,
the Android NDK, the JDK in `.java-version` and the Android SDK (`scripts/lib/env.sh`
finds them). Plugin, library and SDK versions are in `android/gradle/libs.versions.toml`,
the app version in `app-version.xcconfig`. Set `MILESTONE_KEYSTORE`, `MILESTONE_KS_PASS`
and `MILESTONE_KS_ALIAS` to sign a release; without them the release APK is signed with
the debug key. The app id and package are still `app.milestone`.

## Test

```sh
cargo test                                                    # core
cd android && ./gradlew :lollop:testAndroidHostTest           # JVM + Robolectric UI tests on the real core
cd android && ./gradlew :lollop:verifyRoborazziAndroidHostTest # screenshots (baselines are local, not in git)
cd android && ./gradlew :lollop:compileKotlinIosArm64         # the shared module's iOS build (any host)
scripts/emulator-scenarios.sh                                 # on-device scenarios (AVD e34)
scripts/emu-scenarios/surfaces.sh                             # widget, QS tile, open intents, Live Updates, session reminder (AVD e37)
cd android && ./gradlew :core-bridge:linuxX64Test             # Kotlin/Native bridge to the core's C ABI (iOS)
```

## Privacy

Everything stays on the device. The only network use is OpenStreetMap tiles for
run maps, which can be turned off in Profile.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

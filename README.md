# lollop

Offline training coach for strength, running and hybrid athletes. Every
recommendation shows its evidence grade and citation.

- `shared/`: Rust core (crux). Deterministic: the same event log always gives the same view.
- `android/`: Jetpack Compose shell, talks to the core over JNI (`shared/src/ffi.rs`).
- `kmp/`, `ios/`: iOS port (Compose Multiplatform), so far the core's C ABI and its Kotlin/Native bridge. See `ios/README.md`.

## Build

```sh
scripts/build.sh              # cargo test, native libs, Android tests, debug + release APKs
scripts/build.sh --no-tests   # build only
```

Needs Rust (edition 2024), cargo-ndk, Android NDK r30, JDK 21 and the Android SDK
(platform 37). Set `MILESTONE_KEYSTORE`, `MILESTONE_KS_PASS` and `MILESTONE_KS_ALIAS`
to sign a release; without them the release APK is signed with the debug key.
The app id and package are still `app.milestone`. `android/app` is the application
shell; the app's code, resources and tests live in `android/lollop`, a Kotlin
Multiplatform module that also compiles for iOS.

## Test

```sh
cargo test                                                    # core
cd android && ./gradlew :lollop:testAndroidHostTest           # JVM + Robolectric UI tests on the real core
cd android && ./gradlew :lollop:verifyRoborazziAndroidHostTest # screenshots (baselines are local, not in git)
cd android && ./gradlew :lollop:compileKotlinIosArm64         # the shared module's iOS build (any host)
scripts/emulator-scenarios.sh                                 # on-device scenarios (AVD e34)
scripts/emu-scenarios/surfaces.sh                             # widget, QS tile, open intents, Live Updates, session reminder (AVD e37 / e34)
cd kmp && ./gradlew :core-bridge:linuxX64Test                 # Kotlin/Native bridge to the core's C ABI (iOS)
```

## Privacy

Everything stays on the device. The only network use is OpenStreetMap tiles for
run maps, which can be turned off in Profile.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

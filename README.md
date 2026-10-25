# lollop

Offline training coach for strength, running and hybrid athletes. Every
recommendation shows its evidence grade and citation.

- `shared/`: Rust core (crux). Deterministic: the same event log always gives the same view.
- `android/`: Jetpack Compose shell, talks to the core over JNI (`shared/src/ffi.rs`).

## Build

```sh
scripts/build.sh              # cargo test, native libs, Android tests, debug + release APKs
scripts/build.sh --no-tests   # build only
```

Needs Rust (edition 2024), cargo-ndk, Android NDK r30, JDK 21 and the Android SDK
(platform 37). Set `MILESTONE_KEYSTORE`, `MILESTONE_KS_PASS` and `MILESTONE_KS_ALIAS`
to sign a release; without them the release APK is signed with the debug key.
The app id and package are still `app.milestone`.

## Test

```sh
cargo test                                    # core
cd android && ./gradlew testDebugUnitTest     # JVM + Robolectric UI tests on the real core
cd android && ./gradlew verifyRoborazziDebug  # screenshots (baselines are local, not in git)
scripts/emulator-scenarios.sh                 # on-device scenarios (AVD e34)
```

## Privacy

Everything stays on the device. The only network use is OpenStreetMap tiles for
run maps, which can be turned off in Profile.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

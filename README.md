# lollop

lollop is an offline, evidence-cited training coach for strength, running and
hybrid athletes. It turns a local log of workouts and readiness signals into
daily prescriptions and session reviews, and every number it shows carries the
evidence grade and the citation that produced it, so the athlete can always see
how strongly a recommendation is backed.

## Architecture

The engine is a deterministic Rust core (built on [crux](https://github.com/redbadger/crux))
that holds no clock, no randomness and no IO: time enters only as event data, so
the same event log always replays to the same view. An Android Compose shell
renders the views the core produces and forwards user events back to it. Core
and shell talk over a JSON FFI bridge (`shared/src/ffi.rs`): events serialize in,
view models serialize out. The core lives in `shared/`; the shell in `android/`.

Three rules are structural, not conventional (see `shared/src/lib.rs`):

- The knowledge base is authoritative — nothing enters the engine the KB does
  not state.
- Every recommendation travels as `Recommended<T>`, backed by a registered claim
  (`ClaimId`) that carries its evidence grade, citation and confidence.
- Safety overrides goals — safety gates run before and above any goal logic.

## Build

```sh
scripts/build.sh             # the entry point: cargo test → cargo-ndk jniLibs →
                             # gradle unit tests → debug APK + lollop-release.apk
scripts/build.sh --no-tests  # quick rebuild, skips cargo + gradle unit tests
```

The individual steps, for targeted work:

```sh
cargo build && cargo test    # core only
cargo ndk -o android/app/src/main/jniLibs -t arm64-v8a -t x86_64 build -p shared --release
cd android && JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew assembleDebug
```

Toolchain: edition 2024, Rust 1.90 (crux MSRV), crux 0.20, cargo-ndk, NDK r30,
JDK 21, the Android SDK with platform 37 (the Gradle 9.7.1 wrapper builds with
AGP 9.3.3 and Kotlin 2.4.21), and `MILESTONE_KEYSTORE`/`MILESTONE_KS_PASS`/
`MILESTONE_KS_ALIAS` for a distributable release signature (the build falls back
to the debug key, which is not distributable, when they are unset). The variable
names, the application id and the Kotlin package `app.milestone` predate the
rename to lollop and are unchanged.

## Quality assurance

Three layers, all runnable from a clean checkout:

**Rust core** — `cargo test` (unit + schema + behaviour tests) and, for the JNI
bridge on a non-Android host, `cargo test -p shared --features jni-host`. Three
tests pin the wire output so refactors cannot change it silently:
`shared/tests/view_snapshot.rs` compares the `ViewModel` JSON of replayed
fixture logs byte for byte (re-bless an intended change with
`BLESS_VIEW_SNAPSHOT=1`); `view_equivalence` (`shared/src/app/tests/`) pins the
JSON hash and length of a 420-day synthetic history at four dates and checks a
cold view equals a memoized one (`VIEW_EQ_DUMP=<dir>` writes the JSON for
diffing); `compaction_golden` checks event-log compaction against fixtures and
that each compacted log renders the same view as its full input.
An ignored probe times the view path:
`cargo test --release -p shared --lib view_timing -- --ignored --nocapture`.

**Offline UI/UX QA (no device)** — Robolectric drives the *real* Jetpack Compose
UI over the *real* Rust core on the plain JVM. The core is the host
`libshared.so` built from the `shared` crate's `jni-host` feature; the gradle
test task builds it and puts it on `java.library.path` automatically, so the UI
tests always load a real core (never a mock). It runs under the normal unit-test
task:

```sh
cd android && ./gradlew testDebugUnitTest    # wire-shape + UI/UX QA suite
```

The suite (`android/app/src/test/java/app/milestone/qa/`) pins the app clock to
a fixed Monday, seeds a scenario event log (the same ndjson the app persists),
launches `MainActivity`, and interacts through Compose semantics. It covers the
daily journeys (fresh-install setup, the strength plan hero, logging a set,
run/lift/hold history) and per-set completion (slots fill, "Log set k of N"
advances, warm-ups stay below target, End exercise / Reopen, the Log chooser's
"Today's plan" tile, pound loads on the plate grid). A reusable UX invariant
checker (`UxInvariants`) fails on `NaN`/`null`/`@ RPE 0`-style leaks, unlabeled
or sub-48 dp tap targets, duplicate in-card actions, and actions clipped at
fontScale 1.3.

**Screenshot regression (Roborazzi)** — about 270 baselines (light + dark) live under
`android/app/src/test/screenshots/`, local only (not in git, not checked in CI): Today (plan, run, rest
day, holds, session done, calisthenics, hypertrophy), the Log sheet and
editors, History and Runs, the run tracker, guided setup, Profile, and each
colour theme. Captures hide Compose ripple hosts (`captureWithoutRipples` in
`qa/QaUi.kt`), because Robolectric's hardware renderer animates ripples on
wall-clock time and made repeat runs differ.

```sh
cd android && ./gradlew verifyRoborazziDebug   # fail on any unexpected pixel diff
cd android && ./gradlew recordRoborazziDebug   # re-record after an INTENTIONAL UI change
```

**On-device smoke (emulator)** — `scripts/qa-emulator.sh` boots the local AVD
headless on its own port (default `e34`, SDK from `android/local.properties`),
pins the device clock to a Monday, installs the debug APK, seeds a scenario,
screenshots the key screens into `android/app/build/qa/`, drives the per-set
logging flow (log a set from Today, keep the editor open, check the first slot
fills), runs a `monkey` stress pass (failing on a Java crash, native abort or
ANR in the app), runs `connectedDebugAndroidTest`, and shuts the emulator down.
`scripts/build.sh --qa` runs it after a normal build; `scripts/qa-emulator.sh
--check` dry-runs the non-emulator wiring. CI runs an instrumented + monkey
smoke on an emulator weekly and on manual dispatch.

## Privacy

Routes, logs and health data stay on the device. The only network egress is
map tiles fetched from openstreetmap.org to draw the area of a run; that fetch
is toggleable in Profile → Show route maps (on by default), and tiles are cached
under the app's cache directory, never backed up.

## Evidence policy

Every recommendation carries an evidence grade, a citation and a confidence.
Claim ids resolve against an evidence-graded knowledge base that is maintained
privately and is not distributed with this repository.

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

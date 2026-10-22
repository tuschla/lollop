plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
    // Screenshot regression for the QA suite (recordRoborazziDebug /
    // verifyRoborazziDebug). Runs on the same host-JVM Robolectric harness.
    id("io.github.takahirom.roborazzi") version "1.76.0"
}

android {
    namespace = "app.milestone"
    compileSdk = 37
    buildToolsVersion = "36.1.0"

    defaultConfig {
        applicationId = "app.milestone"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        // Only ABIs the Rust core (libshared.so) is built for; without a filter
        // androidx transitive .so's would add armeabi-v7a/x86 slices that install
        // on those devices and crash at the first JNI call.
        // Release ships phones only (arm64-v8a, ~3 MB smaller); debug adds x86_64
        // so it runs on the emulator.
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    buildTypes {
        debug {
            ndk {
                abiFilters += listOf("x86_64")
            }
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    // Distribution shape:
    //   • F-Droid + GitHub direct-download → `./gradlew assembleRelease` = one
    //     arm64-v8a APK. F-Droid builds APKs from source (no AAB support).
    //   • Play Store → `./gradlew bundleRelease` = an AAB; Play splits it
    //     per-device (density + language), no manual ABI splits needed.
    // Any published build MUST be signed with a real keystore supplied through
    // MILESTONE_KEYSTORE (see scripts/build.sh); the debug-key fallback is for
    // local sideload/testing only and is not distributable.

    buildFeatures {
        compose = true
        buildConfig = true
    }

    testOptions {
        unitTests {
            // The monotonic-clock run tracking calls android.os.SystemClock in
            // RunSession.restore(); in plain JVM unit tests (no Robolectric) those
            // framework stubs throw unless unmocked calls return defaults. The
            // distance/segment assertions don't depend on the clock value, so a 0
            // default is fine and keeps the suite runnable on the JVM.
            isReturnDefaultValues = true
            // Robolectric (the offline UI tests) needs the merged Android
            // resources + manifest to inflate the real theme, strings, and the
            // MainActivity Compose tree on the JVM.
            isIncludeAndroidResources = true
        }
    }

    sourceSets {
        getByName("test") {
            // The compaction golden corpus is one shared directory the Rust core
            // (shared/tests/fixtures/compaction) and this shell both grade against;
            // exposing it as a test resource lets the JVM tests read it regardless
            // of the runner's working directory.
            resources.directories += "../../shared/tests/fixtures"
        }
    }

    compileOptions {
        // Core-library desugaring backports the java.time API (used at startup in
        // MainActivity + in Export.kt) to the minSdk-24 range. Without it, an
        // Android 7 (API 24/25) device hits NoClassDefFoundError on the first
        // frame: java.time.* only exists natively from API 26.
        isCoreLibraryDesugaringEnabled = true
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.19.1")
    // Backported SplashScreen API: paints the theme-aware launch frame
    // (white/dark logo in light, black/white logo in dark) before Compose.
    implementation("androidx.core:core-splashscreen:1.2.0")
    implementation("androidx.activity:activity-compose:1.13.0")
    // collectAsStateWithLifecycle + LocalLifecycleOwner; declared rather than relied on
    // transitively through activity-compose.
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("org.osmdroid:osmdroid-android:6.1.20")
    implementation("com.google.android.gms:play-services-location:21.4.0")
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Backports java.time (+ other Java 8 APIs) to minSdk 24 so the app doesn't
    // crash on API 24/25 launch. 2.1.x is compatible with AGP 9.
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")

    // JVM unit tests (no device): compaction + event JSON wire shapes.
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    // runTest + controllable dispatchers for the M1 run-save critical-section
    // cancellation tests (drives real coroutine cancellation the UI can't).
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.11.0")

    // Offline UI/UX QA: Robolectric runs the REAL Compose UI and the
    // REAL Rust core (host `libshared.so`, built from the shared crate's
    // `jni-host` feature) on the plain JVM — no device, no emulator. These are
    // testImplementation (not androidTest) so they run under `testDebugUnitTest`.
    testImplementation("org.robolectric:robolectric:4.17")
    testImplementation("androidx.test:core-ktx:1.7.0")
    testImplementation("androidx.test.ext:junit:1.3.0")
    testImplementation(platform("androidx.compose:compose-bom:2026.09.00"))
    testImplementation("androidx.compose.ui:ui-test-junit4")
    testImplementation("androidx.compose.ui:ui-test-manifest")
    // Roborazzi screenshot capture/verify for the key screens (light + dark).
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.76.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.76.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-junit-rule:1.76.0")

    // Instrumented Compose UI tests (on device/emulator).
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation(platform("androidx.compose:compose-bom:2026.09.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}

// ---------------------------------------------------------------------------
// QA host core: build libshared.so for the HOST JVM so the
// Robolectric UI/UX tests drive the REAL Rust core over JNI instead of a mock.
// `testDebugUnitTest` depends on this and puts the fresh .so on java.library.path.
// The device build still gets its core from cargo-ndk (scripts/build.sh); this
// task is host-only and never feeds the APK.
// ---------------------------------------------------------------------------
val repoRoot = rootDir.parentFile

// Honor CARGO_TARGET_DIR (the QA sandbox points it at a shared target dir); the
// cdylib always lands in <target>/debug/libshared.so on a `cargo build` (no
// --release) of the jni-host feature.
val cargoTargetDir: File =
    System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: File(repoRoot, "target")

val qaJniDir: Provider<Directory> = layout.buildDirectory.dir("qa-jni")

val buildHostCore = tasks.register<Exec>("buildHostCore") {
    group = "verification"
    description = "Build libshared.so (jni-host) for host-JVM UI/UX QA tests."
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "shared", "--features", "jni-host")
    // Rebuild only when the core sources or its manifests change.
    inputs.dir(File(repoRoot, "shared/src"))
    inputs.file(File(repoRoot, "shared/Cargo.toml"))
    inputs.file(File(repoRoot, "Cargo.toml"))
    val outSo = qaJniDir.map { it.file("libshared.so") }
    outputs.file(outSo)
    doLast {
        val built = File(cargoTargetDir, "debug/libshared.so")
        if (!built.exists()) {
            throw GradleException(
                "host core not found at $built after cargo build " +
                    "(CARGO_TARGET_DIR=${System.getenv("CARGO_TARGET_DIR") ?: "<unset>"})",
            )
        }
        val dest = outSo.get().asFile
        dest.parentFile.mkdirs()
        built.copyTo(dest, overwrite = true)
    }
}

tasks.withType<Test>().configureEach {
    dependsOn(buildHostCore)
    // Prepend the freshly-built host core so System.loadLibrary("shared") in
    // Core's static init resolves it on the JVM (Robolectric loads it once for
    // the whole test JVM).
    val libDir = qaJniDir.get().asFile.absolutePath
    val existing = System.getProperty("java.library.path") ?: ""
    systemProperty("java.library.path", libDir + File.pathSeparator + existing)
    // Roborazzi records real pixels; NATIVE graphics also makes Compose layout
    // deterministic on the JVM.
    systemProperty("robolectric.graphicsMode", "NATIVE")
    // The native renderer writes its own logs straight to the terminal; ripple
    // animations started without a render thread emit harmless per-tap
    // "OpenGLRenderer W ... without an animation handle" lines. Show only its
    // errors; everything else keeps warning level.
    environment("ANDROID_LOG_TAGS", "OpenGLRenderer:E *:W")
    // Give the forked test JVM headroom: the real core + Compose + Robolectric.
    maxHeapSize = "2g"
}

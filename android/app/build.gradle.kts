// The application shell: packages :lollop (all code, resources and manifest
// components) with the Rust core's jniLibs into the APK. Application id,
// signing inputs, ABI filters, minification and versioning live here.
plugins {
    id("com.android.application")
}

android {
    // The library owns `app.milestone` (its R class and every FQCN); the shell
    // only needs a namespace of its own.
    namespace = "app.milestone.shell"
    compileSdk = 37
    buildToolsVersion = "36.1.0"

    defaultConfig {
        applicationId = "app.milestone"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"

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
            // QA only: `-PqaX86Release` (or `=true`) adds the x86_64 core so the
            // minified release APK runs on the x86_64 emulator
            // (scripts/emulator-scenarios.sh --apk); `=false` leaves it out. The
            // shipped default stays arm64-v8a alone.
            if (providers.gradleProperty("qaX86Release").map { it != "false" }.getOrElse(false)) {
                ndk {
                    abiFilters += listOf("x86_64")
                }
            }
            isMinifyEnabled = true
            isShrinkResources = true
            // The app's own keep rules ship with :lollop as consumer rules
            // (lollop/consumer-rules.pro).
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
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
    implementation(project(":lollop"))

    // Backports java.time (+ other Java 8 APIs) to minSdk 24 so the app doesn't
    // crash on API 24/25 launch. 2.1.x is compatible with AGP 9.
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}

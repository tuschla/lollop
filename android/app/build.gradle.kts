plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "app.milestone"
    compileSdk = 36
    buildToolsVersion = "36.1.0"

    defaultConfig {
        applicationId = "app.milestone"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        // Only the ABIs the Rust core (libshared.so) is built for. Without this
        // the APK/AAB would also ship armeabi-v7a/x86 slices from androidx
        // transitive .so's, install on those devices, and crash at the first JNI
        // call. Applies to BOTH the universal APK (F-Droid/GitHub) and the AAB
        // (Play splits per-device from these ABIs).
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
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
    //     UNIVERSAL APK (both ABIs, ~6 MB). F-Droid builds APKs from source (no
    //     AAB support); a single universal file is the simplest sideloadable.
    //   • Play Store → `./gradlew bundleRelease` = an AAB. Play splits it
    //     per-device (ABI + density + language) automatically, so a phone
    //     downloads only its arm64 slice (~4.3 MB), no manual ABI splits needed.
    // (No `splits { abi }` block: manual per-ABI APKs are the pre-AAB technique,
    // redundant with the AAB for Play and worse for the single-file F-Droid/GitHub
    // case.)

    buildFeatures {
        compose = true
    }

    testOptions {
        unitTests {
            // The monotonic-clock run tracking calls android.os.SystemClock in
            // RunSession.restore(); in plain JVM unit tests (no Robolectric) those
            // framework stubs throw unless unmocked calls return defaults. The
            // distance/segment assertions don't depend on the clock value, so a 0
            // default is fine and keeps the suite runnable on the JVM.
            isReturnDefaultValues = true
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

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    // Backported SplashScreen API: paints the theme-aware launch frame
    // (white/dark logo in light, black/white logo in dark) before Compose.
    implementation("androidx.core:core-splashscreen:1.0.1")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation(platform("androidx.compose:compose-bom:2024.10.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.osmdroid:osmdroid-android:6.1.20")
    implementation("com.google.android.gms:play-services-location:21.3.0")
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Backports java.time (+ other Java 8 APIs) to minSdk 24 so the app doesn't
    // crash on API 24/25 launch. 2.1.x is compatible with AGP 8.9.
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.4")

    // JVM unit tests (no device): compaction + event JSON wire shapes.
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

    // Instrumented Compose UI tests (on device/emulator).
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2024.10.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}

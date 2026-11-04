import java.time.Duration
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// The whole app (UI, platform code, manifest components, resources) as a Kotlin
// Multiplatform library; :app only packages it. Namespace app.milestone keeps
// `app.milestone.R` and every FQCN the manifest, JNI and tests name.
//   commonMain   platform-free code, compiled for Android and iOS
//   androidMain  everything else (Compose UI, services, widget, the JNI core)
//   androidHostTest  the JVM + Robolectric suite (`:lollop:testAndroidHostTest`)
plugins {
    id("org.jetbrains.kotlin.multiplatform")
    id("com.android.kotlin.multiplatform.library")
    // Lint report tasks for the KMP Android library (`:lollop:lint`).
    id("com.android.lint")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
    // Screenshot regression for the QA suite (recordRoborazziAndroidHostTest /
    // verifyRoborazziAndroidHostTest). Runs on the same host-JVM Robolectric harness.
    id("io.github.takahirom.roborazzi")
}

// Compose Multiplatform 1.12.2 is built on Jetpack Compose 1.12.1, the version the
// BOM below pins: on Android both resolve to the same Google artifacts.
val cmpVersion = "1.12.2"
val composeBom = "androidx.compose:compose-bom:2026.09.00"

kotlin {
    android {
        namespace = "app.milestone"
        compileSdk = 37
        buildToolsVersion = "36.1.0"
        minSdk = 24
        // java.time on API 24/25: :app desugars; this tells lint and the library's
        // D8 that the APIs are backported.
        enableCoreLibraryDesugaring = true

        androidResources { enable = true }

        compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }

        // The R8 keep rules ship to :app's release build as consumer rules.
        optimization {
            consumerKeepRules.publish = true
            consumerKeepRules.file("consumer-rules.pro")
        }

        withHostTest {
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

    // iOS: the static framework the Xcode app links (`embedAndSignAppleFrameworkForXcode`).
    // Kotlin compiles iOS klibs on any host (`compileKotlinIosArm64` runs on Linux CI);
    // linking needs macOS and is skipped elsewhere (kotlin.native.ignoreDisabledTargets).
    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.framework {
            baseName = "Lollop"
            isStatic = true
        }
    }

    compilerOptions {
        // expect/actual classes (JvmSerializable) are Beta; this acknowledges it.
        freeCompilerArgs.add("-Xexpect-actual-classes")
    }

    sourceSets {
        commonMain.dependencies {
            implementation("org.jetbrains.compose.runtime:runtime:$cmpVersion")
            implementation("org.jetbrains.compose.runtime:runtime-saveable:$cmpVersion")
            implementation("org.jetbrains.compose.ui:ui:$cmpVersion")
            implementation("org.jetbrains.compose.components:components-resources:$cmpVersion")
            implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
            // Dates and time zones in common code; on Android it wraps java.time
            // (desugared below API 26), so dates read exactly as before.
            implementation("org.jetbrains.kotlinx:kotlinx-datetime:0.8.0")
            // SynchronizedObject + synchronized(): a real lock on Kotlin/Native, where
            // @Synchronized does nothing; on the JVM it is the object's monitor.
            implementation("org.jetbrains.kotlinx:atomicfu:0.33.0")
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
        // On iOS, CMP 1.12.2's ui depends on androidx lifecycle 2.11.0 and on
        // JetBrains' lifecycle port 2.9.6: the KLIB loader found every lifecycle
        // module twice ("same unique_name"). JetBrains' 2.11.0 redirects to the
        // androidx klibs. iOS only: Android keeps its androidx 2.10.0.
        iosMain.dependencies {
            implementation("org.jetbrains.androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
        }
        androidMain.dependencies {
            implementation("androidx.core:core-ktx:1.19.1")
            // Backported SplashScreen API: paints the theme-aware launch frame
            // (white/dark logo in light, black/white logo in dark) before Compose.
            implementation("androidx.core:core-splashscreen:1.2.0")
            implementation("androidx.activity:activity-compose:1.13.0")
            // collectAsStateWithLifecycle + LocalLifecycleOwner; declared rather than relied on
            // transitively through activity-compose.
            implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
            implementation(project.dependencies.platform(composeBom))
            implementation("androidx.compose.ui:ui")
            implementation("androidx.compose.material3:material3")
            // Window size classes (currentWindowAdaptiveInfoV2): the navigation rail, the
            // two-pane layouts and the content width on landscape phones, foldables and
            // tablets (Adaptive.kt).
            implementation("androidx.compose.material3.adaptive:adaptive")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
            implementation("org.osmdroid:osmdroid-android:6.1.20")
            implementation("com.google.android.gms:play-services-location:21.4.0")
            // Home screen widget (widget/TodayWidget.kt).
            implementation("androidx.glance:glance-appwidget:1.2.0")
        }
        getByName("androidHostTest") {
            // The core's test fixtures (device FIT files) as test resources, so the
            // JVM tests read them regardless of the runner's working directory.
            resources.srcDir("../../shared/tests/fixtures")
            dependencies {
                // JVM unit tests (no device): compaction + event JSON wire shapes.
                implementation("junit:junit:4.13.2")
                // runTest + controllable dispatchers for the M1 run-save critical-section
                // cancellation tests (drives real coroutine cancellation the UI can't).
                implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.11.0")
                // Offline UI/UX QA: Robolectric runs the REAL Compose UI and the
                // REAL Rust core (host `libshared.so`, built from the shared crate's
                // `jni-host` feature) on the plain JVM — no device, no emulator.
                implementation("org.robolectric:robolectric:4.17")
                implementation("androidx.test:core-ktx:1.7.0")
                implementation(project.dependencies.platform(composeBom))
                implementation("androidx.compose.ui:ui-test-junit4")
                // Declares the empty ComponentActivity that createComposeRule() launches;
                // the test manifest Robolectric reads must contain it.
                implementation("androidx.compose.ui:ui-test-manifest")
                // Roborazzi screenshot capture/verify for the key screens (light + dark).
                implementation("io.github.takahirom.roborazzi:roborazzi:1.76.0")
                // Glance widget content tests (runGlanceAppWidgetUnitTest), same version as the widget.
                implementation("androidx.glance:glance-appwidget-testing:1.2.0")
            }
        }
    }
}

// enableCoreLibraryDesugaring needs the library on this module's configuration
// too (AGP checks it when writing the AAR metadata).
dependencies {
    "coreLibraryDesugaring"("com.android.tools:desugar_jdk_libs:2.1.5")
}

// ---------------------------------------------------------------------------
// Strings for Compose Multiplatform resources, generated from the Android ones.
// `src/androidMain/res/values/strings*.xml` stays the only source; this writes
// their CMP twin (`values/strings.xml` of the commonMain compose resources) into
// the build directory. CmpResourceParityTest (host tests) checks every string,
// plural and array reads the same through both.
//
// Android resolves escapes, quotes and whitespace when it compiles the XML and
// runs String.format; CMP takes the text as written and substitutes only
// `%N$s` / `%N$d` with `arg.toString()`. So the generator:
//   • resolves what aapt2 resolves: `\'` `\"` `\\` `\@` `\?` `\#` `\n` `\t` `\uXXXX`,
//     unquoted whitespace runs collapse to one space and unquoted leading/trailing
//     whitespace goes, `"…"` quoting is stripped (its whitespace kept);
//   • in a string with format arguments: bare `%d` / `%s` become positional
//     (`%1$d`, in order, as java.util.Formatter numbers them) and `%%` becomes `%`;
//   • fails on any other format specifier (`%.1f`, `%,d`, `%02d`, …): CMP would
//     print it literally, so the number has to be formatted in Kotlin instead;
//     also on inline markup and on a literal backslash, which CMP can't carry.
// `formatted="false"` strings are literal text: escapes resolved, no format step.
// ---------------------------------------------------------------------------
abstract class GenerateComposeStrings : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val sources: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = StringBuilder("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n")
            .append("<!-- Generated by :lollop:generateComposeStrings from res/values/strings*.xml. Do not edit. -->\n")
            .append("<resources>\n")
        val errors = mutableListOf<String>()
        val factory = javax.xml.parsers.DocumentBuilderFactory.newInstance()
        for (file in sources.files.sortedBy { it.name }) {
            val root = factory.newDocumentBuilder().parse(file).documentElement
            val nodes = root.childNodes
            for (i in 0 until nodes.length) {
                val el = nodes.item(i) as? org.w3c.dom.Element ?: continue
                val name = el.getAttribute("name")
                val literal = el.getAttribute("formatted") == "false"
                fun convert(item: org.w3c.dom.Element, where: String): String? = try {
                    val text = flatten(item)
                    escapeXml(if (literal) text else formatForCmp(text))
                } catch (e: IllegalArgumentException) {
                    errors += "${file.name}: $where: ${e.message}"
                    null
                }
                when (el.tagName) {
                    "string" -> convert(el, "string $name")?.let {
                        out.append("    <string name=\"").append(name).append("\">").append(it).append("</string>\n")
                    }
                    "plurals", "string-array" -> {
                        val items = el.getElementsByTagName("item")
                        out.append("    <").append(el.tagName).append(" name=\"").append(name).append("\">\n")
                        for (j in 0 until items.length) {
                            val item = items.item(j) as org.w3c.dom.Element
                            val quantity = item.getAttribute("quantity")
                            val value = convert(item, "${el.tagName} $name ${quantity.ifEmpty { "#$j" }}") ?: continue
                            out.append("        <item")
                            if (quantity.isNotEmpty()) out.append(" quantity=\"").append(quantity).append('"')
                            out.append('>').append(value).append("</item>\n")
                        }
                        out.append("    </").append(el.tagName).append(">\n")
                    }
                    else -> errors += "${file.name}: unsupported <${el.tagName}> $name"
                }
            }
        }
        if (errors.isNotEmpty()) {
            throw GradleException("Compose resources can't express these strings:\n  " + errors.joinToString("\n  "))
        }
        out.append("</resources>\n")
        val dir = outputDir.get().asFile
        dir.deleteRecursively()
        File(dir, "values").apply { mkdirs() }.resolve("strings.xml").writeText(out.toString())
    }

    /** The string aapt2 compiles [el]'s text to (ResourceUtils StringBuilder). */
    private fun flatten(el: org.w3c.dom.Element): String {
        val children = el.childNodes
        val raw = StringBuilder()
        for (i in 0 until children.length) {
            val n = children.item(i)
            when (n.nodeType) {
                org.w3c.dom.Node.TEXT_NODE, org.w3c.dom.Node.CDATA_SECTION_NODE -> raw.append(n.nodeValue)
                org.w3c.dom.Node.COMMENT_NODE -> Unit
                else -> throw IllegalArgumentException("markup <${n.nodeName}> is not supported")
            }
        }
        // Each output char with whether it came from inside quotes (kept on trim).
        val chars = StringBuilder()
        val quoted = mutableListOf<Boolean>()
        fun emit(c: Char, q: Boolean) { chars.append(c); quoted += q }
        var quote = false
        var lastSpace = false
        var i = 0
        val s = raw.toString()
        while (i < s.length) {
            val c = s[i++]
            // aapt2 collapses ASCII isspace() only; NBSP and thin spaces are text.
            if (!quote && c in " \t\n\r\u000B\u000C") {
                if (!lastSpace) emit(' ', false)
                lastSpace = true
                continue
            }
            lastSpace = false
            when {
                c == '\\' && i < s.length -> when (val e = s[i++]) {
                    't' -> emit('\t', quote)
                    'n' -> emit('\n', quote)
                    'u' -> {
                        val hex = s.substring(i, minOf(i + 4, s.length))
                        require(hex.length == 4 && hex.all { it in "0123456789abcdefABCDEF" }) { "bad \\u escape in «$s»" }
                        emit(hex.toInt(16).toChar(), quote)
                        i += 4
                    }
                    else -> emit(e, quote)
                }
                c == '"' -> quote = !quote
                else -> emit(c, quote)
            }
        }
        var start = 0
        var end = chars.length
        while (start < end && chars[start] == ' ' && !quoted[start]) start++
        while (end > start && chars[end - 1] == ' ' && !quoted[end - 1]) end--
        return chars.substring(start, end)
    }

    /** [text] as CMP's formatter needs it to read like Android's String.format. */
    private fun formatForCmp(text: String): String {
        // %[index$][flags][width][.precision]conversion (java.util.Formatter); the
        // space flag is left out so prose like "% of max" is not a specifier.
        val spec = Regex("""%(\d+\$)?([-#+0,(<]*)(\d*)(\.\d+)?([a-zA-Z%])""")
        val specs = spec.findAll(text).toList()
        for (m in specs) {
            val (_, flags, width, precision, conversion) = m.destructured
            val supported = conversion == "%" && m.value == "%%" ||
                (conversion == "d" || conversion == "s") && flags.isEmpty() && width.isEmpty() && precision.isEmpty()
            require(supported) { "format specifier ${m.value} in «$text» (CMP supports only %N\$s / %N\$d)" }
        }
        // Only a string with arguments is formatted; anything else reads raw on Android too.
        if (specs.none { it.groupValues[5] != "%" }) return text
        var ordinary = 0
        return spec.replace(text) { m ->
            when {
                m.value == "%%" -> "%"
                m.groupValues[1].isEmpty() -> "%${++ordinary}\$${m.groupValues[5]}"
                else -> m.value
            }
        }
    }

    private fun escapeXml(text: String): String {
        // CMP would read a backslash as the start of an escape of its own.
        require('\\' !in text) { "a literal backslash in «$text»" }
        return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace("\n", "\\n").replace("\t", "\\t")
    }
}

val generateComposeStrings = tasks.register<GenerateComposeStrings>("generateComposeStrings") {
    group = "build"
    description = "Generate the CMP strings resource from the Android res/values/strings*.xml."
    sources.from(fileTree("src/androidMain/res/values") { include("strings*.xml") })
    outputDir.set(layout.buildDirectory.dir("generated/composeStrings"))
}

compose.resources {
    packageOfResClass = "app.milestone.resources"
    publicResClass = false
    generateResClass = always
    // The generated strings are commonMain's compose resources; the provider
    // carries the task dependency into every resource task.
    customDirectory(
        sourceSetName = "commonMain",
        directoryProvider = generateComposeStrings.flatMap { it.outputDir },
    )
}

// ---------------------------------------------------------------------------
// QA host core: build libshared.so for the HOST JVM so the
// Robolectric UI/UX tests drive the REAL Rust core over JNI instead of a mock.
// `testAndroidHostTest` depends on this and puts the fresh .so on java.library.path.
// The device build still gets its core from cargo-ndk (scripts/build.sh, into
// :app's jniLibs); this task is host-only and never feeds the APK.
// ---------------------------------------------------------------------------
val repoRoot: File = rootDir.parentFile

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
    // Native code in the Robolectric runtime logs straight to the terminal.
    // android-base's logger reads ANDROID_LOG_TAGS and accepts only
    // `*:<lowercase level>` (any other spec logs "unsupported ... in
    // ANDROID_LOG_TAGS"), so this sets one floor for every native tag.
    environment("ANDROID_LOG_TAGS", "*:w")
    // Two test JVMs, each with headroom for the real core + Compose +
    // Robolectric: 2 × 2 GB fits the 16 GB CI runner.
    maxParallelForks = 2
    maxHeapSize = "2g"
    // Robolectric's Android 16 (SDK 36) sandbox reaches into jdk.internal.access to
    // set up shared memory; without this export every @Config(sdk = [36]) test fails
    // at setup with "Failed to interact with raw FileDescriptor internals".
    jvmArgs("--add-exports=java.base/jdk.internal.access=ALL-UNNAMED")
    // A hang fails the task instead of holding CI until its job timeout: the full
    // suite runs in well under this.
    timeout.set(Duration.ofMinutes(30))
}

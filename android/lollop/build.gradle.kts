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

kotlin {
    android {
        namespace = "app.milestone"
        compileSdk = libs.versions.android.compileSdk.get().toInt()
        buildToolsVersion = libs.versions.android.buildTools.get()
        minSdk = libs.versions.android.minSdk.get().toInt()
        // java.time on API 24/25: :app desugars; this tells lint and the library's
        // D8 that the APIs are backported.
        enableCoreLibraryDesugaring = true

        androidResources { enable = true }

        compilerOptions { jvmTarget.set(JvmTarget.fromTarget(libs.versions.jvm.target.get())) }

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

    // iOS klibs: Kotlin compiles them on any host (`compileKotlinIosArm64` runs on
    // Linux CI). The framework the Xcode app links is :lollop-ios's, which adds the
    // Rust core; this module's iOS code reaches the core only through CoreBridge.
    iosArm64()
    iosSimulatorArm64()

    compilerOptions {
        // expect/actual classes (JvmSerializable) are Beta; this acknowledges it.
        freeCompilerArgs.add("-Xexpect-actual-classes")
    }

    sourceSets {
        commonMain.dependencies {
            implementation(libs.compose.runtime)
            implementation(libs.compose.runtime.saveable)
            implementation(libs.compose.ui)
            implementation(libs.compose.foundation)
            implementation(libs.compose.material3)
            // Window size classes (currentWindowAdaptiveInfoV2, Adaptive.kt).
            implementation(libs.compose.material3.adaptive)
            implementation(libs.compose.components.resources)
            // collectAsStateWithLifecycle, LocalLifecycleOwner.
            implementation(libs.jetbrains.lifecycle.runtime.compose)
            implementation(libs.kotlinx.serialization.json)
            implementation(libs.kotlinx.coroutines.core)
            // Dates and time zones in common code; on Android it wraps java.time
            // (desugared below API 26), so dates read exactly as before.
            implementation(libs.kotlinx.datetime)
            // SynchronizedObject + synchronized(): a real lock on Kotlin/Native, where
            // @Synchronized does nothing; on the JVM it is the object's monitor.
            implementation(libs.atomicfu)
            // Pure-Kotlin XML pull reader (KtXmlReader) for GPX/TCX run import on iOS
            // (ImportXml.kt); Android reads with the platform's xmlpull parser.
            implementation(libs.xmlutil.core)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            // runTest for the boot sequence's suspend turns (AppBootTest).
            implementation(libs.kotlinx.coroutines.test)
        }
        iosMain.dependencies {
            implementation(libs.jetbrains.lifecycle.runtime.compose.ios)
            // BackHandler (PlatformUi.ios.kt): the UIKit back gesture.
            implementation(libs.compose.ui.backhandler)
        }
        androidMain.dependencies {
            implementation(libs.androidx.core.ktx)
            // Backported SplashScreen API: paints the theme-aware launch frame
            // (white/dark logo in light, black/white logo in dark) before Compose.
            implementation(libs.androidx.core.splashscreen)
            implementation(libs.androidx.activity.compose)
            // Declared rather than relied on transitively through activity-compose.
            implementation(libs.androidx.lifecycle.runtime.compose)
            implementation(project.dependencies.platform(libs.androidx.compose.bom))
            implementation(libs.androidx.compose.ui)
            implementation(libs.androidx.compose.material3)
            implementation(libs.androidx.compose.material3.adaptive)
            implementation(libs.kotlinx.coroutines.android)
            implementation(libs.osmdroid.android)
            implementation(libs.play.services.location)
            implementation(libs.androidx.glance.appwidget)
        }
        getByName("androidHostTest") {
            // The core's test fixtures (device FIT files) as test resources, so the
            // JVM tests read them regardless of the runner's working directory.
            resources.srcDir("../../shared/tests/fixtures")
            dependencies {
                implementation(libs.junit)
                // runTest + controllable dispatchers for the M1 run-save critical-section
                // cancellation tests (drives real coroutine cancellation the UI can't).
                implementation(libs.kotlinx.coroutines.test)
                // Offline UI/UX QA: Robolectric runs the REAL Compose UI and the
                // REAL Rust core (host `libshared.so`, built from the shared crate's
                // `jni-host` feature) on the plain JVM — no device, no emulator.
                implementation(libs.robolectric)
                implementation(libs.androidx.test.core.ktx)
                implementation(project.dependencies.platform(libs.androidx.compose.bom))
                implementation(libs.androidx.compose.ui.test.junit4)
                // Declares the empty ComponentActivity that createComposeRule() launches;
                // the test manifest Robolectric reads must contain it.
                implementation(libs.androidx.compose.ui.test.manifest)
                implementation(libs.roborazzi)
                // Glance widget content tests (runGlanceAppWidgetUnitTest).
                implementation(libs.androidx.glance.appwidget.testing)
            }
        }
    }
}

// enableCoreLibraryDesugaring needs the library on this module's configuration
// too (AGP checks it when writing the AAR metadata).
dependencies {
    "coreLibraryDesugaring"(libs.desugar.jdk.libs)
}

// ---------------------------------------------------------------------------
// Compose Multiplatform resources, generated from the Android ones.
// `src/androidMain/res` stays the only source; this writes its CMP twin (the
// commonMain compose resources) into the build directory:
//   • `values/strings.xml` from `res/values/strings*.xml`. CmpResourceParityTest
//     (host tests) checks every string, plural and array reads the same through both.
//   • `drawable/` from the UI vectors in `res/drawable` (everything but the
//     launcher, splash and notification icons, which only Android draws).
//   • an androidMain Kotlin file mapping each resource key to its `R` id
//     (`androidStringId` …), so text read outside composition on Android
//     (`PlatformResources.getString`, Strings.kt) is Android's own getString.
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
abstract class GenerateComposeResources : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val sources: ConfigurableFileCollection

    @get:InputFiles
    @get:PathSensitive(PathSensitivity.NAME_ONLY)
    abstract val drawables: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:OutputDirectory
    abstract val idsDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = StringBuilder("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n")
            .append("<!-- Generated by :lollop:generateComposeResources from res/values/strings*.xml. Do not edit. -->\n")
            .append("<resources>\n")
        val names = sortedMapOf("string" to sortedSetOf<String>(), "plurals" to sortedSetOf(), "string-array" to sortedSetOf())
        val errors = mutableListOf<String>()
        val factory = javax.xml.parsers.DocumentBuilderFactory.newInstance()
        for (file in sources.files.sortedBy { it.name }) {
            val root = factory.newDocumentBuilder().parse(file).documentElement
            val nodes = root.childNodes
            for (i in 0 until nodes.length) {
                val el = nodes.item(i) as? org.w3c.dom.Element ?: continue
                val name = el.getAttribute("name")
                names[el.tagName]?.add(name)
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
        val drawableDir = File(dir, "drawable").apply { mkdirs() }
        for (file in drawables.files) file.copyTo(drawableDir.resolve(file.name))
        writeIds(names)
    }

    private fun writeIds(names: Map<String, Set<String>>) {
        val kt = StringBuilder("// Generated by :lollop:generateComposeResources from res/values/strings*.xml. Do not edit.\n")
            .append("package app.milestone\n")
        for ((tag, function, rClass) in listOf(
            Triple("string", "androidStringId", "string"),
            Triple("plurals", "androidPluralsId", "plurals"),
            Triple("string-array", "androidArrayId", "array"),
        )) {
            kt.append("\n/** The `R.$rClass` id of the resource named [key]. */\n")
                .append("internal fun $function(key: String): Int = when (key) {\n")
            for (name in names.getValue(tag)) kt.append("    \"").append(name).append("\" -> R.").append(rClass).append('.').append(name).append('\n')
            kt.append("    else -> throw IllegalArgumentException(\"no $tag resource \$key\")\n}\n")
        }
        val dir = idsDir.get().asFile
        dir.deleteRecursively()
        File(dir, "app/milestone").apply { mkdirs() }.resolve("AndroidResourceIds.kt").writeText(kt.toString())
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

val generateComposeResources = tasks.register<GenerateComposeResources>("generateComposeResources") {
    group = "build"
    description = "Generate the CMP resources (strings, UI vectors) and the R id map from the Android res."
    sources.from(fileTree("src/androidMain/res/values") { include("strings*.xml") })
    drawables.from(
        fileTree("src/androidMain/res/drawable") {
            include("ic_*.xml")
            exclude("ic_launcher_*.xml", "ic_stat_mark.xml")
        },
    )
    outputDir.set(layout.buildDirectory.dir("generated/composeResources"))
    idsDir.set(layout.buildDirectory.dir("generated/androidResourceIds"))
}

kotlin.sourceSets.getByName("androidMain").kotlin.srcDir(generateComposeResources.flatMap { it.idsDir })

compose.resources {
    packageOfResClass = "app.milestone.resources"
    publicResClass = false
    generateResClass = always
    // The generated resources are commonMain's compose resources; the provider
    // carries the task dependency into every resource task.
    customDirectory(
        sourceSetName = "commonMain",
        directoryProvider = generateComposeResources.flatMap { it.outputDir },
    )
}

// The Robolectric UI/UX tests drive the real Rust core over JNI: the test JVM
// loads the host build of the core's JNI exports (the APK's core is :app's).
val qaJniDir: Provider<Directory> = layout.buildDirectory.dir("qa-jni")

val buildHostCore = rustCore.hostJniLibrary(project, "buildHostCore", qaJniDir)

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

package app.milestone

import java.io.File
import java.io.StringReader
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element
import org.xml.sax.InputSource

/**
 * The copies of the app's identifiers that live where Kotlin can't be referenced
 * (Android XML, the iOS Info.plists, entitlements and Swift, the iOS CI) match their
 * one definition: [AppFileNames], [RunFileType], [OpenDest] / [OPEN_URL_SCHEME] and
 * the identifiers in iosApp/project.yml.
 */
class AppIdentifiersTest {

    private val repo = File(REPO_ROOT)
    private val android = File(repo, "android/lollo/src/androidMain")
    private val ios = File(repo, "iosApp")

    /** Every file and store backup and device transfer leave out, as (domain, path). */
    private val backupExcluded: Set<Pair<String, String>> = buildSet {
        add("file" to AppFileNames.EVENT_LOG)
        for (suffix in AppFileNames.LOG_SUFFIXES) add("file" to AppFileNames.EVENT_LOG + suffix)
        add("file" to AppFileNames.ACTIVE_RUN)
        add("file" to AppFileNames.WIDGET_SNAPSHOT)
        add("file" to AppFileNames.WIDGET_SNAPSHOT + AppFileNames.TMP_SUFFIX)
        add("sharedpref" to AppFileNames.SETUP_STORE + ".xml")
        // Map tiles an old install may have left on external storage; the app no longer writes there.
        add("external" to "osmdroid/")
    }

    @Test
    fun autoBackupLeavesOutTheRegisteredFiles() {
        val legacy = xml(File(android, "res/xml/backup_rules.xml"))
        assertEquals("backup_rules.xml", backupExcluded, rules(legacy))
        val modern = xml(File(android, "res/xml/data_extraction_rules.xml"))
        for (section in listOf("cloud-backup", "device-transfer")) {
            val e = children(modern).single { it.tagName == section }
            assertEquals("data_extraction_rules.xml $section", backupExcluded, rules(e))
        }
    }

    /** The (domain, path) of each `exclude` under [e]; no `include` (that would back up only those). */
    private fun rules(e: Element): Set<Pair<String, String>> {
        val kids = children(e)
        assertTrue("${e.tagName}: only excludes", kids.all { it.tagName == "exclude" })
        return kids.map { it.getAttribute("domain") to it.getAttribute("path") }.toSet()
    }

    @Test
    fun androidImportFiltersAcceptTheRunFileTypes() {
        val manifest = xml(File(android, "AndroidManifest.xml"))
        val filters = manifest.getElementsByTagName("intent-filter").let { list ->
            (0 until list.length).map { list.item(it) as Element }
        }.filter { f -> children(f).any { it.tagName == "data" && it.hasAttribute("android:mimeType") } }
        val actions = filters.flatMap { f -> children(f).filter { it.tagName == "action" }.map { it.getAttribute("android:name") } }
        assertEquals(
            listOf("android.intent.action.VIEW", "android.intent.action.SEND", "android.intent.action.SEND_MULTIPLE"),
            actions,
        )
        for (f in filters) {
            val mimes = children(f).filter { it.tagName == "data" && it.hasAttribute("android:mimeType") }.map { it.getAttribute("android:mimeType") }
            assertEquals(RunFileType.IMPORT_MIME_TYPES, mimes)
        }
    }

    @Test
    fun iosDocumentTypesAreTheRunFileTypes() {
        val info = plist(File(ios, "iosApp/Info.plist"))
        val opened = list(info["CFBundleDocumentTypes"]).flatMap { list(dict(it)["LSItemContentTypes"]) }
        assertEquals(RunFileType.entries.map { it.uti }, opened)
        val declared = list(info["UTImportedTypeDeclarations"]).map(::dict).associate { d ->
            val tags = dict(d["UTTypeTagSpecification"])
            d["UTTypeIdentifier"] to (list(tags["public.filename-extension"]) to list(tags["public.mime-type"]))
        }
        assertEquals(RunFileType.entries.associate { it.uti to (listOf(it.extension) to it.mimeTypes) }, declared)
    }

    @Test
    fun iosOpenUrlsAreOpenDests() {
        val info = plist(File(ios, "iosApp/Info.plist"))
        assertEquals(listOf(OPEN_URL_SCHEME), list(info["CFBundleURLTypes"]).flatMap { list(dict(it)["CFBundleURLSchemes"]) })

        val intents = File(ios, "Shared/LolloIntents.swift").readText()
        val screen = checkNotNull(Regex("""(?s)enum LolloScreen: String, AppEnum \{(.*?)\n\s*static """).find(intents)) { "no enum LolloScreen" }
        val wires = Regex("""case (\w+)(?:\s*=\s*"([^"]+)")?""").findAll(screen.groupValues[1])
            .map { m -> m.groupValues[2].ifEmpty { m.groupValues[1] } }.toList()
        assertEquals(OpenDest.entries.map { it.wire }, wires)

        // LolloScreen.url is the one place Swift spells an open URL.
        val url = "\"$OPEN_URL_SCHEME://open/\\(rawValue)\""
        assertTrue("LolloScreen.url builds $url", intents.contains(url))
        val spelled = swiftSources().filter { it.readText().contains("\"$OPEN_URL_SCHEME://") }
        assertEquals(listOf(File(ios, "Shared/LolloIntents.swift").canonicalFile), spelled)
    }

    @Test
    fun iosIdentifiersComeFromTheProjectSpec() {
        val spec = File(ios, "project.yml").readText()
        fun setting(name: String) =
            Regex("""(?m)^\s+$name:\s*(\S+)\s*$""").findAll(spec).map { it.groupValues[1] }.single()
        val group = setting("LOLLO_APP_GROUP")
        val bundle = setting("LOLLO_BUNDLE_ID")

        for (name in listOf("lollo", "LolloWidgets")) {
            val groups = list(plist(File(ios, "Entitlements/$name.entitlements"))["com.apple.security.application-groups"])
            assertEquals("$name.entitlements", listOf("\$(LOLLO_APP_GROUP)"), groups)
        }
        for (path in listOf("iosApp/Info.plist", "LolloWidgets/Info.plist", "LolloWidgetsTests/Info.plist")) {
            assertEquals(path, "\$(LOLLO_APP_GROUP)", plist(File(ios, path))["LolloAppGroup"])
        }
        // No other copy of the group in the app's sources, and none of either id in the CI.
        val copies = swiftSources().filter { it.readText().contains(group) }
        assertEquals(emptyList<File>(), copies)
        val ci = File(repo, ".github/workflows/ios-app.yml").readText()
        assertTrue("ios-app.yml reads the ids from the build", !ci.contains(group) && !ci.contains(bundle))
    }

    /** The app's and the extension's Swift (not the extension's tests, whose fixtures spell values). */
    private fun swiftSources(): List<File> =
        listOf("iosApp", "Shared", "LolloWidgets").flatMap { dir ->
            File(ios, dir).walkTopDown().filter { it.isFile && it.extension == "swift" }.map { it.canonicalFile }.toList()
        }.sorted()

    private fun xml(f: File): Element {
        val builder = DocumentBuilderFactory.newInstance().newDocumentBuilder()
        // A property list names Apple's DTD by URL; don't fetch it.
        builder.setEntityResolver { _, _ -> InputSource(StringReader("")) }
        return builder.parse(f).documentElement
    }

    private fun children(e: Element): List<Element> =
        (0 until e.childNodes.length).mapNotNull { e.childNodes.item(it) as? Element }

    private fun plist(f: File): Map<String, Any> = dict(value(children(xml(f)).single()))

    private fun value(e: Element): Any = when (e.tagName) {
        "dict" -> children(e).chunked(2).associate { (k, v) -> k.textContent to value(v) }
        "array" -> children(e).map(::value)
        "true" -> true
        "false" -> false
        else -> e.textContent
    }

    @Suppress("UNCHECKED_CAST")
    private fun dict(v: Any?): Map<String, Any> = v as Map<String, Any>

    private fun list(v: Any?): List<Any> = v as List<Any>
}

package app.milestone

import nl.adaptivity.xmlutil.EventType
import nl.adaptivity.xmlutil.core.KtXmlReader

// The XML pull reader the GPX and TCX import readers ([parseGpxRuns],
// [parseTcxActivities], through [pullImportTags]) walk: only what they use. Android
// reads with the platform's xmlpull parser (`android.util.Xml`), iOS with
// [XmlUtilImportReader], xmlutil's pure-Kotlin reader. Namespace processing is off in
// effect: a tag's [ImportXmlReader.name] keeps its prefix (`gpxtpx:hr`) and an
// attribute is found by its name as written.

/** The events an import reads; text, comments and the like are passed over. */
internal enum class ImportXmlEvent { StartTag, EndTag, EndDocument }

/** A forward-only reader over one XML document. Every call throws
 *  [ImportXmlException] on malformed XML. */
internal interface ImportXmlReader {
    /** Advance to the next start tag, end tag or the document's end. */
    fun next(): ImportXmlEvent

    /** The current tag's nesting depth: the root element is 1, and an end tag has its
     *  start tag's depth. */
    val depth: Int

    /** The current tag's name as written, prefix included (`gpxtpx:hr`). */
    val name: String

    /** The current start tag's attribute named exactly [name] (no prefix), or null. */
    fun attribute(name: String): String?

    /** The current start tag's text content, leaving the reader on its end tag. A
     *  child element inside it is malformed for this call and throws. */
    fun nextText(): String
}

/** Malformed import XML: the reader's parse error, or a document refused by
 *  [pullImportTags] (too deep, cut off). The import maps it to [ImportFailure.Damaged]. */
internal class ImportXmlException(message: String, cause: Throwable? = null) : Exception(message, cause)

/** The platform's pull reader over [text]: xmlpull on Android, [XmlUtilImportReader] on iOS. */
internal expect fun importXmlReader(text: String): ImportXmlReader

/**
 * [ImportXmlReader] over xmlutil's [KtXmlReader] (pure Kotlin, any platform; iOS's
 * reader, and JVM-tested against Android's in the host tests). Entities are expanded:
 * the predefined ones and character references read as text, an undefined one is
 * malformed (as in the xmlpull parser). It follows the xmlpull parser's `next` /
 * `nextText` contract: text split by entities, CDATA sections and comments reads as one.
 * Well-formed documents read exactly as through xmlpull (`ImportXmlParityTest`); on
 * malformed ones the two parsers' tolerance differs at the edges: KtXmlReader
 * resolves prefixes, so an undeclared one is malformed here, and it refuses a second
 * root element or a stray character in a name, while xmlpull refuses a `<` inside an
 * attribute value that KtXmlReader reads. Any failure of the reader on its in-memory
 * input is a malformed document ([ImportXmlException]).
 */
internal class XmlUtilImportReader(text: String) : ImportXmlReader {
    private val reader = reading { KtXmlReader(text, expandEntities = true, relaxed = false) }
    private var current: ImportXmlEvent? = null

    override fun next(): ImportXmlEvent = reading { advance() }

    private fun advance(): ImportXmlEvent {
        while (true) {
            val event = when (reader.next()) {
                EventType.START_ELEMENT -> ImportXmlEvent.StartTag
                EventType.END_ELEMENT -> ImportXmlEvent.EndTag
                EventType.END_DOCUMENT -> ImportXmlEvent.EndDocument
                else -> continue
            }
            current = event
            return event
        }
    }

    override val depth: Int get() = reader.depth

    override val name: String
        get() = reading {
            val prefix = reader.prefix
            if (prefix.isEmpty()) reader.localName else "$prefix:${reader.localName}"
        }

    // A duplicated attribute (malformed, but the xmlpull parser reads it) reads as its last value there.
    override fun attribute(name: String): String? = reading {
        (0 until reader.attributeCount)
            .lastOrNull { reader.getAttributePrefix(it).isEmpty() && reader.getAttributeLocalName(it) == name }
            ?.let { reader.getAttributeValue(it) }
    }

    override fun nextText(): String = reading { readText() }

    private fun readText(): String {
        if (current != ImportXmlEvent.StartTag) throw ImportXmlException("precondition: START_TAG")
        val text = StringBuilder()
        while (true) {
            when (reader.next()) {
                EventType.TEXT, EventType.CDSECT, EventType.ENTITY_REF, EventType.IGNORABLE_WHITESPACE ->
                    text.append(reader.text)
                EventType.COMMENT, EventType.PROCESSING_INSTRUCTION -> Unit
                EventType.END_ELEMENT -> {
                    current = ImportXmlEvent.EndTag
                    return text.toString()
                }
                else -> throw ImportXmlException("END_TAG expected")
            }
        }
    }

    private inline fun <T> reading(block: () -> T): T = try {
        block()
    } catch (e: ImportXmlException) {
        throw e
    } catch (e: Exception) {
        throw ImportXmlException(e.message ?: "malformed XML", e)
    }
}

package app.milestone

import android.util.Xml
import org.xmlpull.v1.XmlPullParser
import org.xmlpull.v1.XmlPullParserException
import java.io.StringReader

internal actual fun importXmlReader(text: String): ImportXmlReader = PullImportXmlReader(text)

/** [ImportXmlReader] over the platform's xmlpull parser with namespace processing off
 *  (so `gpxtpx:hr` is the tag's name). Its parse errors throw as [ImportXmlException];
 *  anything else it throws passes through unchanged. */
private class PullImportXmlReader(text: String) : ImportXmlReader {
    private val parser = pulling {
        Xml.newPullParser().apply {
            setFeature(XmlPullParser.FEATURE_PROCESS_NAMESPACES, false)
            setInput(StringReader(text))
        }
    }

    override fun next(): ImportXmlEvent = pulling {
        var event: ImportXmlEvent?
        do {
            event = when (parser.next()) {
                XmlPullParser.START_TAG -> ImportXmlEvent.StartTag
                XmlPullParser.END_TAG -> ImportXmlEvent.EndTag
                XmlPullParser.END_DOCUMENT -> ImportXmlEvent.EndDocument
                else -> null
            }
        } while (event == null)
        event
    }

    override val depth: Int get() = parser.depth

    override val name: String get() = parser.name

    override fun attribute(name: String): String? = parser.getAttributeValue(null, name)

    override fun nextText(): String = pulling { parser.nextText() }

    private inline fun <T> pulling(block: () -> T): T = try {
        block()
    } catch (e: XmlPullParserException) {
        throw ImportXmlException(e.message.orEmpty(), e)
    }
}

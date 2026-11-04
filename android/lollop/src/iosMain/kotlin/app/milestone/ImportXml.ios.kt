package app.milestone

internal actual fun importXmlReader(text: String): ImportXmlReader = XmlUtilImportReader(text)

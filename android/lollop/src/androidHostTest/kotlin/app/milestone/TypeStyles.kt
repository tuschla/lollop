package app.milestone

import androidx.compose.ui.text.TextStyle

/**
 * Every text style [Type] declares, by property name, read off the object itself
 * so a style added later is checked without anyone extending a test's list. JVM
 * reflection over the public getters, hence host-test only.
 */
internal fun typeStyles(): Map<String, TextStyle> =
    Type::class.java.declaredMethods
        .filter { it.parameterCount == 0 && it.returnType == TextStyle::class.java && it.name.startsWith("get") }
        .associate { it.name.removePrefix("get") to it.invoke(Type) as TextStyle }
        .toSortedMap()
        .also { check("Body" in it && "Caption" in it) { "Type's styles not found by reflection: ${it.keys}" } }

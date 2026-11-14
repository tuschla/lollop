package app.milestone

import androidx.compose.runtime.saveable.Saver
import kotlinx.serialization.KSerializer

/** Keeps a draft in a saveable bundle as its JSON, so it survives rotation and
 *  process death without the type being `java.io.Serializable`. */
internal fun <T> jsonSaver(serializer: KSerializer<T>): Saver<T, String> = Saver(
    save = { coreJson.encodeToString(serializer, it) },
    restore = { coreJson.decodeFromString(serializer, it) },
)

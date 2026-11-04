package app.milestone

/** `java.io.Serializable` on the JVM, so a type can ride in a rememberSaveable
 *  bundle (the profile draft); a plain marker elsewhere. */
expect interface JvmSerializable

package app.milestone

import app.milestone.core.JniCore

/** Android's core engine: [DurableCore] over the JNI bridge to the Rust core
 *  (`libshared.so`, loaded on first touch). */
object Core : DurableCore(JniCore) {
    override val loadError: Throwable? get() = JniCore.loadError
}

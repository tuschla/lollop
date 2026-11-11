package app.milestone.core

/**
 * [CoreBridge] over the JNI entry points of `libshared.so`
 * (`Java_app_milestone_core_JniCore_*`, shared/src/ffi/jni.rs).
 */
object JniCore : CoreCalls() {
    /** Why `libshared.so` failed to load (a missing or ABI-incompatible library),
     *  or null. Caught instead of letting the initializer throw, which would make
     *  every later touch of this object a NoClassDefFoundError: the engine reads
     *  it at boot and shows the core-unavailable state instead. */
    val loadError: Throwable? = try {
        System.loadLibrary("shared")
        null
    } catch (e: UnsatisfiedLinkError) {
        e
    } catch (e: SecurityException) {
        e
    }

    external override fun call(op: Int, input: ByteArray): ByteArray?

    external override fun callLines(op: Int, lines: Array<String>): ByteArray?
}

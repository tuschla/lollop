package app.milestone

import org.robolectric.config.AndroidConfigurer
import org.robolectric.internal.bytecode.InstrumentationConfiguration
import org.robolectric.internal.bytecode.Interceptors
import org.robolectric.internal.bytecode.ShadowProviders

/**
 * Robolectric's sandbox configuration for every test runner (registered in
 * `META-INF/services/org.robolectric.config.AndroidConfigurer`): the core's JNI
 * binding (`app.milestone.core`) is loaded by the test JVM's own class loader,
 * not by each sandbox. A JNI library loads into one class loader per JVM, so the
 * SDK 34 sandbox, an SDK 36 sandbox and a plain-JVM test in the same test JVM all
 * reach the one loaded core through that class loader.
 */
class SharedCoreConfigurer(shadowProviders: ShadowProviders) : AndroidConfigurer(shadowProviders) {
    override fun configure(builder: InstrumentationConfiguration.Builder, interceptors: Interceptors) {
        super.configure(builder, interceptors)
        builder.doNotAcquirePackage("app.milestone.core.")
    }
}

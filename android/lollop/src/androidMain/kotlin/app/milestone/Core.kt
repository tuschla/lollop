package app.milestone

/**
 * Android's core engine: [DurableCore] over the JNI bridge to the Rust/crux core
 * (`libshared.so`). The natives bind to `Java_app_milestone_Core_*` in
 * `shared/src/ffi.rs`, so this object keeps its name and their signatures.
 */
object Core : DurableCore() {
    /** Why `libshared.so` failed to load (a missing or ABI-incompatible library),
     *  or null. Caught instead of letting the initializer throw, which would make
     *  every later touch of [Core] a NoClassDefFoundError crash: boot reads it via
     *  [probe] and shows the core-unavailable state instead. */
    override val loadError: Throwable? = try {
        System.loadLibrary("shared")
        null
    } catch (e: UnsatisfiedLinkError) {
        e
    } catch (e: SecurityException) {
        e
    }

    private external fun update(event: ByteArray): ByteArray?
    private external fun view(): ByteArray?

    /** The JNI exports as the platform-neutral [CoreBridge] the engine calls. */
    override val bridge: CoreBridge = object : CoreBridge {
        override fun update(event: ByteArray): ByteArray? = this@Core.update(event)
        override fun view(): ByteArray? = this@Core.view()
        override fun parseFit(bytes: ByteArray): String? = this@Core.parseFit(bytes)
        override fun exportGpx(entryId: Long, observedAt: Long): String? = this@Core.exportGpx(entryId, observedAt)
        override fun trackFigures(event: ByteArray): String? = this@Core.trackFigures(event)
        override fun compactLog(lines: Array<String>): String? = this@Core.compactLog(lines)
    }

    /** TEST-ONLY bridge reset: drops the native core's global
     *  bridge and rebuilds it empty so a host-JVM UI/UX QA scenario starts from
     *  a clean core. Present only in the `jni-host` host build the QA gradle task
     *  links (`Java_app_milestone_Core_resetForTest`); it is never compiled into a
     *  device APK and never called on device, so the missing device symbol is
     *  inert (JNI resolves natives lazily, at first call). */
    external fun resetForTest()

    /** Pure FIT-file parse in the Rust core (fitparser), stateless, touches no
     *  model/log. Returns `{"segments":[[{lat,lon,time_sec,hr_bpm?},…],…]}` or
     *  `{"error":"…"}`; the shell converts into its LogRunTrack import path.
     *  Nullable: the hardened JNI returns a null jstring if the result string
     *  can't be allocated (double alloc-failure), the decode site treats null
     *  like the error envelope. */
    external fun parseFit(bytes: ByteArray): String?

    /** Build the GPX 1.1 document for ONE logged run, matched by the same (entry_id,
     *  observed_at) identity as delete/amend. Returns "" for a hand-entered run or
     *  an unknown id; for a GPS run QC left unmeasurable (gps_unmeasurable) it is
     *  the recorded fixes, read only by Undo (no route to map or share). null only
     *  if the JNI string couldn't be allocated (the caller treats null like ""). Reads
     *  the registry the last view populated, so call it after the run list rendered.
     *  Heavy (builds + returns the whole document): call OFF the main thread. */
    external fun exportGpx(entryId: Long, observedAt: Long): String?

    /** Pure core query (no model/log): `{"distance_km":…,"duration_min":…}` for a
     *  `LogRunTrack` event's wire JSON, or `{"error":…}`; null only if the JNI
     *  string couldn't be allocated. See [savedFigures]. */
    private external fun trackFigures(event: ByteArray): String?

    /** Pure core query (no model/log): the [LogCompaction] JSON for the event-log
     *  [lines], or null. See [compaction]. */
    private external fun compactLog(lines: Array<String>): String?
}

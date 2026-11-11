package app.milestone

/** The iOS app's entry point: the Swift host calls [start] once at launch, before
 *  the UI and the surfaces. */
object LollopApp {
    /** Start the app ([startIosApp]); its core engine runs over the Rust core's C ABI. */
    fun start() = startIosApp()
}

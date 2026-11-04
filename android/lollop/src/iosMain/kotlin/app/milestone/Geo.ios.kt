package app.milestone

import kotlin.math.PI

// What java.lang.Math.toRadians computes on current JDKs: one multiply by the
// rounded constant π/180.
private const val DegreesToRadians = PI / 180.0

internal actual fun toRadians(degrees: Double): Double = degrees * DegreesToRadians

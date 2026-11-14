package app.milestone

// Java rounds the value's shortest decimal digits half up (0.15 → "0.2", where
// the binary value is 0.1499…); Foundation's formatters don't document that rule,
// so iOS runs the Kotlin port that the host tests hold to String.format.
internal actual fun formatFixed(value: Double, decimals: Int): String = halfUpFixed(value, decimals)

package app.milestone

// The map tiles every map in the app shows, and how the app may fetch them. Android
// reads them through osmdroid's MAPNIK source (the same server, zoom range and
// policy); iOS through MapKit's MKTileOverlay (OsmTiles.ios.kt). The OpenStreetMap
// Foundation's tile usage policy (operations.osmfoundation.org/policies/tiles/)
// asks for a User-Agent that names the app (Android: the package name; iOS: the
// bundle identifier), honouring the server's caching headers, at most two
// connections, no bulk or preventive downloads, and a visible attribution.

/** OSM's standard tile layer ("Mapnik"), 256 px PNG tiles. */
internal const val OSM_TILE_URL_TEMPLATE = "https://tile.openstreetmap.org/{z}/{x}/{y}.png"

/** The deepest zoom the OSM standard layer serves; deeper views upscale its tiles. */
internal const val OSM_MAX_ZOOM = 19

/** Concurrent tile downloads the OSM policy allows (osmdroid's MAPNIK policy: 2). */
internal const val OSM_MAX_CONNECTIONS = 2

/** Desaturation the dark tile filter applies before inverting. */
internal const val DARK_TILE_SATURATION = 0.3f

/**
 * The 4×5 colour matrix (android.graphics.ColorMatrix layout: rows R, G, B, A of
 * `[r g b a offset]`, channels 0..255) a dark theme puts over the always-light OSM
 * tiles: desaturate to [DARK_TILE_SATURATION] with ColorMatrix.setSaturation's
 * luminance weights, then invert, so the map reads as a dark ground instead of a
 * bright block. Android builds the same matrix from ColorMatrix (RunMap.android.kt).
 */
internal fun darkTileMatrix(): FloatArray {
    val sat = DARK_TILE_SATURATION
    val invSat = 1 - sat
    val r = 0.213f * invSat
    val g = 0.715f * invSat
    val b = 0.072f * invSat
    return floatArrayOf(
        -(r + sat), -g, -b, 0f, 255f,
        -r, -(g + sat), -b, 0f, 255f,
        -r, -g, -(b + sat), 0f, 255f,
        0f, 0f, 0f, 1f, 0f,
    )
}

/** [matrix] applied to one opaque-or-not pixel's straight (unpremultiplied) RGB, each
 *  channel rounded and clamped to 0..255; alpha is kept. */
internal fun applyColorMatrix(matrix: FloatArray, r: Int, g: Int, b: Int, out: IntArray) {
    for (row in 0..2) {
        val v = matrix[row * 5] * r + matrix[row * 5 + 1] * g + matrix[row * 5 + 2] * b + matrix[row * 5 + 4]
        out[row] = (v + 0.5f).toInt().coerceIn(0, 255)
    }
}

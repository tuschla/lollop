package app.milestone

import kotlin.test.Test
import kotlin.test.assertContentEquals

/** The dark-theme tile filter iOS applies itself (Android's osmdroid ColorMatrix): desaturate to 0.3, then invert. */
class MapTilesTest {
    private fun dark(r: Int, g: Int, b: Int) = IntArray(3).also { applyColorMatrix(darkTileMatrix(), r, g, b, it) }

    @Test
    fun whiteGroundTurnsBlackAndBlackTurnsWhite() {
        assertContentEquals(intArrayOf(0, 0, 0), dark(255, 255, 255))
        assertContentEquals(intArrayOf(255, 255, 255), dark(0, 0, 0))
    }

    @Test
    fun greyStaysGreyInverted() {
        // Saturation keeps luminance, so a grey only inverts.
        assertContentEquals(intArrayOf(155, 155, 155), dark(100, 100, 100))
    }

    @Test
    fun colourIsMostlyDrainedThenInverted() {
        // ColorMatrix.setSaturation(0.3) on pure red: (0.213·0.7 + 0.3)·255 = 114.5 red,
        // 0.213·0.7·255 = 38.0 green and blue; inverted: 140.5 → 140, 217, 217.
        assertContentEquals(intArrayOf(140, 217, 217), dark(255, 0, 0))
        // OSM's park green (#C8FACC) lands on a dark muted green.
        assertContentEquals(intArrayOf(30, 15, 29), dark(0xC8, 0xFA, 0xCC))
    }
}

package app.milestone

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * WCAG 2.2 SC 1.4.3 contrast for every palette token pair the app draws text
 * with, across every theme in both light and dark: each on-ground text tone on
 * each ground it sits on, and the on-accent text tone on an accent fill. Text in
 * the app is 12–16 sp (below "large text"), so every pair needs 4.5:1.
 */
class ThemeContrastTest {

    private fun variants(): List<Pair<String, Palette>> = AppTheme.entries.flatMap { t ->
        listOf("${t.label} dark" to t.dark, "${t.label} light" to t.light)
    }

    private fun assertAa(variant: String, pair: String, fg: Color, bg: Color) {
        val ratio = contrastRatio(fg, bg)
        assertTrue(
            "$variant: $pair is %.2f:1, needs ≥ %.1f:1".format(ratio, WCAG_AA_NORMAL),
            ratio >= WCAG_AA_NORMAL,
        )
    }

    @Test
    fun onGroundTextTones_clearAaOnEveryGround() {
        for ((variant, p) in variants()) {
            val grounds = listOf("bgTop" to p.bgTop, "bgElevated" to p.bgElevated, "bgSunken" to p.bgSunken)
            val texts = listOf("onBgBody" to p.onBgBody, "onBgMuted" to p.onBgMuted, "onBgFaint" to p.onBgFaint)
            for ((tName, t) in texts) {
                for ((gName, g) in grounds) assertAa(variant, "$tName on $gName", t, g)
            }
        }
    }

    @Test
    fun accentText_clearsAaOnEveryGround() {
        // Accent is a text colour too (section overlines, active tab label, every
        // TextAction, and the one "?" glyph, drawn on the bare ground inside a ring).
        for ((variant, p) in variants()) {
            assertAa(variant, "accent on bgTop", p.accent, p.bgTop)
            assertAa(variant, "accent on bgElevated", p.accent, p.bgElevated)
            assertAa(variant, "accent on bgSunken", p.accent, p.bgSunken)
        }
    }

    @Test
    fun onAccentText_clearsAaOnAccentFill() {
        // OnAccent is the palette's bgTop at full opacity (primary action
        // labels, the "Today's plan" row, selected choice cells/chips).
        for ((variant, p) in variants()) {
            assertAa(variant, "onAccent (bgTop) on accent", p.bgTop, p.accent)
        }
    }

    @Test
    fun selectedSegmentLabel_clearsAaOnSolidAccentSegment() {
        // The selected segment of every segmented control (Lifts | Runs, kg | lb,
        // Focus, …) is a SOLID Accent fill with an OnAccent label, never a tint
        // or an outline ring.
        for ((variant, p) in variants()) {
            assertAa(variant, "onAccent label on selected segment fill", p.bgTop, p.accent)
        }
    }

    @Test
    fun warnText_clearsAaOnEveryGround() {
        // WarnText is the warn ochre used AS text (the outlined CONTESTED marker
        // inside the "?" panel), which can sit on any of the three grounds.
        for ((variant, p) in variants()) {
            assertAa(variant, "warnText on bgTop", p.warnText, p.bgTop)
            assertAa(variant, "warnText on bgElevated", p.warnText, p.bgElevated)
            assertAa(variant, "warnText on bgSunken", p.warnText, p.bgSunken)
        }
    }

    @Test
    fun dangerText_clearsAaOnEveryGround() {
        // DangerText is the danger red used AS text (a Delete TextAction, a Discard
        // dialog button) on any of the three grounds; the fixed danger fill measures
        // under 3:1 on the dark grounds.
        for ((variant, p) in variants()) {
            assertAa(variant, "dangerText on bgTop", p.dangerText, p.bgTop)
            assertAa(variant, "dangerText on bgElevated", p.dangerText, p.bgElevated)
            assertAa(variant, "dangerText on bgSunken", p.dangerText, p.bgSunken)
        }
    }

    @Test
    fun doneMark_clearsNonTextContrastOnEveryGround() {
        // The green done ✓ and the week strip's done mark sit straight on the
        // ground: a graphical object (WCAG 1.4.11), ≥3:1 on every ground of its mode.
        for (t in AppTheme.entries) {
            for ((mode, p, done) in listOf(
                Triple("dark", t.dark, DarkStatusColors.done),
                Triple("light", t.light, StatusColors().done),
            )) {
                for ((name, ground) in listOf("bgTop" to p.bgTop, "bgElevated" to p.bgElevated, "bgSunken" to p.bgSunken)) {
                    val ratio = contrastRatio(done, ground)
                    assertTrue(
                        "${t.label} $mode: done mark on $name is %.2f:1, needs ≥ 3.0:1".format(ratio),
                        ratio >= 3.0,
                    )
                }
            }
        }
    }

    @Test
    fun warnMark_clearsNonTextContrastOnEveryGround() {
        // The ring of an `Adjusted` StatusCard (ui-sweep §0.3) is the card's caution
        // signal, drawn straight on the theme's card surface (no tinted wash): a
        // graphical object (WCAG 1.4.11), ≥3:1 on every ground of its mode.
        for (t in AppTheme.entries) {
            for ((mode, p, mark) in listOf(
                Triple("dark", t.dark, DarkStatusColors.warnMark),
                Triple("light", t.light, StatusColors().warnMark),
            )) {
                for ((name, ground) in listOf("bgTop" to p.bgTop, "bgElevated" to p.bgElevated, "bgSunken" to p.bgSunken)) {
                    val ratio = contrastRatio(mark, ground)
                    assertTrue(
                        "${t.label} $mode: warn mark on $name is %.2f:1, needs ≥ 3.0:1".format(ratio),
                        ratio >= 3.0,
                    )
                }
            }
        }
    }

    @Test
    fun warnTile_clearsNonTextContrastOnBgElevated_andItsGlyphOnTheTile() {
        // The solid 40dp mark tile of an `Adjusted` StatusCard sits on the card
        // (bgElevated) and holds a glyph: tile vs card ≥3:1, glyph vs tile ≥3:1, on
        // all 8 palettes. Light: the warn fill with a white glyph; dark: the lighter
        // on-ground ochre with the ground's ink (bgTop) as the glyph.
        for (t in AppTheme.entries) {
            for ((mode, p, tile) in listOf(
                Triple("dark", t.dark, DarkStatusColors.warnMark),
                Triple("light", t.light, StatusColors().warnMark),
            )) {
                val glyph = if (tile == StatusColors().warn) Color.White else p.bgTop
                val onCard = contrastRatio(tile, p.bgElevated)
                assertTrue("${t.label} $mode: warn tile on bgElevated is %.2f:1, needs ≥ 3.0:1".format(onCard), onCard >= 3.0)
                val glyphRatio = contrastRatio(glyph, tile)
                assertTrue("${t.label} $mode: glyph on the warn tile is %.2f:1, needs ≥ 3.0:1".format(glyphRatio), glyphRatio >= 3.0)
            }
        }
    }

    @Test
    fun choiceControlText_clearsAaOnItsFill() {
        // Unselected choice cells/chips and the segment track: OnBgBody on a 4%
        // OnBgBody wash, over whichever ground hosts them. Tonal secondary action,
        // the selected option-list row, nav pill and icon tiles: OnBgBody on the
        // opaque AccentTint. (Selected cells and segments use onAccent on accent,
        // above.)
        for ((variant, p) in variants()) {
            for ((name, ground) in listOf("bgTop" to p.bgTop, "bgElevated" to p.bgElevated, "bgSunken" to p.bgSunken)) {
                val cell = p.onBgBody.copy(alpha = ChoiceCellFillAlpha).compositeOver(ground)
                assertAa(variant, "onBgBody on choice cell / segment track over $name", p.onBgBody, cell)
            }
            assertAa(variant, "onBgBody on accent tint", p.onBgBody, p.accentTint())
        }
    }

    @Test
    fun dangerSubtext_clearsAaOnDangerSurface() {
        assertAa("all themes", "DangerOn on danger", DangerOn, StatusColors().danger)
    }

    @Test
    fun safetyBannerText_clearsAaOnDangerSurface() {
        // The DO-NOT-TRAIN banner's title, actions and summary are white; its
        // captions (reason, "paused" line, grade, citation) are DangerOn.
        val danger = StatusColors().danger
        assertAa("all themes", "white on danger", Color.White, danger)
        assertAa("all themes", "DangerOn on danger", DangerOn, danger)
    }

    @Test
    fun translucentWhiteCaption_failsAaOnDangerSurface() {
        // Why banner captions use DangerOn: white at 0.7–0.8 alpha composited on
        // the danger fill drops below 4.5:1.
        val danger = StatusColors().danger
        for (alpha in listOf(0.7f, 0.8f)) {
            val fg = Color.White.copy(alpha = alpha).compositeOver(danger)
            assertTrue("white @ $alpha on danger unexpectedly clears AA", contrastRatio(fg, danger) < WCAG_AA_NORMAL)
        }
    }

    @Test
    fun typeScale_isReadableAtDefaultFontSize() {
        // The 4.5:1 pairs above assume the documented sizes: running text (Body,
        // Caption) ≥ 14sp, and no style (labels, overlines, legends) under 12sp.
        val styles = mapOf(
            "Display" to Type.Display, "Title" to Type.Title, "Section" to Type.Section,
            "Body" to Type.Body, "Caption" to Type.Caption, "Chip" to Type.Chip,
            "Overline" to Type.Overline,
        )
        for ((name, style) in styles) {
            assertTrue("$name is ${style.fontSize}, floor is 12sp", style.fontSize.value >= 12f)
        }
        assertTrue("Body is ${Type.Body.fontSize}", Type.Body.fontSize.value >= 16f)
        assertTrue("Caption is ${Type.Caption.fontSize}", Type.Caption.fontSize.value >= 14f)
    }
}

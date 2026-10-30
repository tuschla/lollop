package app.milestone

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/** Tone lint over every string, plural item and array item in `res/values/strings*.xml`.
 *  Owner, 2026-10-25: copy reads neutral and factual, like a tool, not a product selling
 *  or motivating. The rules are objective markers of that voice: exclamation marks,
 *  emojis, "we/us/our", a hype/AI word list, rhetorical question openers ("First time?
 *  Pick…"), stock phrases ("Let's", "Ready to", "It's important to note") and em dashes
 *  in sentences (a lone `—` no-value placeholder is fine). The core twin is
 *  `shared/src/app/tests/copy_tone.rs`; keep the two rule sets in step.
 *
 *  A real exception goes in [ALLOW] with its reason; an entry nothing matches any more
 *  fails [allowlistIsReasonedAndLive]. */
class CopyToneTest {

    /** A sanctioned exception: [rule] (a rule-name prefix) on resource [name]. */
    private data class Allow(val name: String, val rule: String, val reason: String)

    private val ALLOW: List<Allow> = listOf()

    /** One resource string: its file, `name` (`name[quantity]` / `name[index]` for
     *  plural and array items) and its text as the app shows it. */
    private data class Res(val file: String, val name: String, val text: String)

    @Test
    fun theLintFlagsEachRuleAndPassesCleanCopy() {
        for (clean in listOf(
            "Squat 5×3 @ 85 kg · ~2 reps left",
            "Held: resting heart rate 8 bpm above your usual. Easy run instead.",
            "—",
            "Feeling ill?",
            "Pace 4:50–5:05 /km → 150–162 bpm",
            "US spelling; elevated resting heart rate; Expert opinion",
            "Lighter week: the plan keeps %1\$d sessions. Log them by hand if needed.",
        )) {
            assertEquals(clean, emptyList<String>(), violations(clean))
        }
        for ((s, rule) in listOf(
            "Nice run!" to "exclamation",
            "Done \uD83D\uDCAA" to "emoji",
            "Rest ☀" to "emoji",
            "We hold this day" to "`we`",
            "We’ll lower the weight" to "`we`",
            "Tell us how it felt" to "`us`",
            "Our plan adapts" to "`our`",
            "This unlocks the next block" to "`unlock`",
            "Level up your squat" to "`level up`",
            "Personalised targets" to "`personalized`",
            "A plan tailored to you" to "`tailored`",
            "Welcome back, Sam" to "`Welcome back`",
            "Well-paced run" to "`Well paced`",
            "Don’t worry about a missed day" to "`Don't worry`",
            "Simply log a set" to "`simply`",
            "Research backs this" to "`research shows`",
            "Studies show it works" to "`studies show`",
            "Experts say rest" to "`experts say`",
            "Start when you're ready" to "`when you're ready`",
            "First time? Pick a plan" to "question opener",
            "Is it sore? Then skip it" to "question opener",
            "Done. Feeling ill? Hold today" to "question opener",
            "Let's go" to "`Let's`",
            "Ready to train" to "`Ready to`",
            "It's important to note the cap" to "`It's important to note`",
            "Easy run — keep it short" to "em dash",
            "Training is on hold" to "`on hold`",
        )) {
            val v = violations(s)
            assertTrue("`$s` must flag $rule: $v", v.any { it.contains(rule) })
        }
        // Resource escapes are undone before the rules run.
        assertEquals("Don't \"x\"\ny", unescape("""  Don\'t   \"x\"\ny """))
        assertEquals("a—b", unescape("""a\u2014b"""))
    }

    @Test
    fun allowlistIsReasonedAndLive() {
        val all = resources()
        for (a in ALLOW) {
            assertTrue("$a needs a reason", a.reason.isNotBlank())
            val r = all.firstOrNull { it.name == a.name }
            assertTrue("$a names no string resource; remove it", r != null)
            assertTrue(
                "$a: `${r!!.text}` no longer breaks ${a.rule}; remove it",
                violations(r.text).any { it.startsWith(a.rule) },
            )
        }
    }

    @Test
    fun everyStringReadsNeutral() {
        val bad = resources().flatMap { r ->
            violations(r.text)
                .filter { v -> ALLOW.none { it.name == r.name && v.startsWith(it.rule) } }
                .map { v -> "${r.file} ${r.name}: $v · `${r.text}`" }
        }
        assertTrue("${bad.size} tone violations:\n" + bad.joinToString("\n"), bad.isEmpty())
    }

    // ── Rules ───────────────────────────────────────────────────────────────

    private fun violations(s: String): List<String> {
        val v = mutableListOf<String>()
        val straight = s.replace('\u2019', '\'')
        val n = straight.lowercase()
        if ('!' in s) v += "exclamation mark"
        if (s.codePoints().anyMatch(::isEmoji)) v += "emoji"
        for (w in listOf("we", "our")) if (phrase(w).containsMatchIn(n)) v += "first-person plural `$w`"
        // Lowercase or sentence-case only: `US` is the country.
        if (Regex("""\b[Uu]s\b""").containsMatchIn(s)) v += "first-person plural `us`"
        for ((label, forms) in HYPE) if (forms.any { phrase(it).containsMatchIn(n) }) v += "hype `$label`"
        if (QUESTION_OPENER.containsMatchIn(s)) v += "rhetorical question opener"
        if (phrase("let's").containsMatchIn(n)) v += "stock phrase `Let's`"
        if (Regex("""\bReady to\b""").containsMatchIn(straight)) v += "stock phrase `Ready to`"
        if (phrase("it's important to note").containsMatchIn(n) || phrase("it is important to note").containsMatchIn(n)) {
            v += "stock phrase `It's important to note`"
        }
        if ('—' in s && s.trim() != "—") v += "em dash in a sentence"
        for ((term, say) in TERMS) if (phrase(term).containsMatchIn(n)) v += "term `$term` (say $say)"
        return v
    }

    /** writing.md Terms: the word not to use to the app's word. */
    private val TERMS = listOf("on hold" to "paused")

    /** A whole-word phrase in lowercase, straight-apostrophe text. */
    private fun phrase(p: String) = Regex("""(?<![\p{L}\p{N}])""" + Regex.escape(p) + """(?![\p{L}\p{N}])""")

    /** Pictographic emoji blocks plus the emoji joiner and presentation selector. Arrows
     *  (`→`, `↔`) and math symbols are typography, not emoji. */
    private fun isEmoji(c: Int) = c in 0x1F000..0x1FAFF || c in 0x2600..0x27BF || c in 0x2B05..0x2B55 ||
        c in 0x231A..0x231B || c in 0x23E9..0x23FA || c == 0xFE0F || c == 0x200D

    /** A sentence (string start, after `. ` or a newline) that opens with a short question
     *  and goes on. A lone question (a field label) is fine. */
    private val QUESTION_OPENER = Regex("""(?:^|\. |\n)[A-Z][^?]{0,40}\? \S""")

    /** Hype and AI-voice words: label to the forms that count. "elevated" is not listed:
     *  "elevated resting heart rate" is the clinical term. */
    private val HYPE: List<Pair<String, List<String>>> = listOf(
        "unlock" to listOf("unlock", "unlocks", "unlocked", "unlocking"),
        "crush" to listOf("crush", "crushes", "crushed", "crushing"),
        "journey" to listOf("journey", "journeys"),
        "level up" to listOf("level up", "levels up", "leveled up", "leveling up", "level-up"),
        "boost" to listOf("boost", "boosts", "boosted", "boosting"),
        "supercharge" to listOf("supercharge", "supercharges", "supercharged", "supercharging"),
        "seamless" to listOf("seamless", "seamlessly"),
        "effortless" to listOf("effortless", "effortlessly"),
        "empower" to listOf("empower", "empowers", "empowered", "empowering", "empowerment"),
        "elevate" to listOf("elevate", "elevates", "elevating"),
        "delve" to listOf("delve", "delves", "delved", "delving"),
        "robust" to listOf("robust", "robustly"),
        "vibrant" to listOf("vibrant"),
        "tapestry" to listOf("tapestry"),
        "testament" to listOf("testament"),
        "game-changer" to listOf("game-changer", "game changer", "game-changing"),
        "insights" to listOf("insight", "insights"),
        "personalized" to listOf("personalized", "personalised", "personalize", "personalization"),
        "tailored" to listOf("tailor", "tailors", "tailored", "tailoring"),
        "Welcome back" to listOf("welcome back"),
        "Great job" to listOf("great job"),
        "Nice work" to listOf("nice work"),
        "Well done" to listOf("well done"),
        "Well paced" to listOf("well paced", "well-paced"),
        "You've got this" to listOf("you've got this"),
        "Keep it up" to listOf("keep it up"),
        "Don't worry" to listOf("don't worry"),
        "No pressure" to listOf("no pressure"),
        "simply" to listOf("simply"),
        "when you're ready" to listOf("when you're ready"),
        "studies show" to listOf("studies show", "studies suggest", "studies have shown"),
        "research shows" to listOf("research shows", "research backs", "research suggests", "research has shown"),
        "experts say" to listOf("experts say", "experts agree", "experts recommend"),
    )

    // ── Resources ───────────────────────────────────────────────────────────

    /** The unit-test working directory is the app module. */
    private fun resources(): List<Res> {
        val dir = File("src/main/res/values")
        val files = dir.listFiles { f -> f.name.startsWith("strings") && f.name.endsWith(".xml") }
            .orEmpty().sortedBy { it.name }
        assertTrue("no strings*.xml under ${dir.absolutePath}", files.isNotEmpty())
        val out = mutableListOf<Res>()
        for (f in files) {
            val root = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(f).documentElement
            val kids = root.childNodes
            for (i in 0 until kids.length) {
                val e = kids.item(i) as? Element ?: continue
                val name = e.getAttribute("name")
                when (e.tagName) {
                    "string" -> out += Res(f.name, name, unescape(e.textContent))
                    "plurals", "string-array" -> {
                        val items = e.getElementsByTagName("item")
                        for (j in 0 until items.length) {
                            val item = items.item(j) as Element
                            val tag = item.getAttribute("quantity").ifEmpty { "$j" }
                            out += Res(f.name, "$name[$tag]", unescape(item.textContent))
                        }
                    }
                }
            }
        }
        return out
    }

    /** Android resource text as shown: whitespace runs collapse, unescaped quotes drop,
     *  and `\'` `\"` `\n` `\t` `\@` `\?` `\\` `\uXXXX` resolve. */
    private fun unescape(raw: String): String {
        val s = raw.trim().replace(Regex("""\s+"""), " ")
        val b = StringBuilder()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            when {
                c == '\\' && i + 1 < s.length -> {
                    val d = s[i + 1]
                    when (d) {
                        'n' -> b.append('\n')
                        't' -> b.append('\t')
                        'u' -> {
                            b.append(s.substring(i + 2, i + 6).toInt(16).toChar())
                            i += 4
                        }
                        else -> b.append(d)
                    }
                    i += 2
                }
                c == '"' -> i++
                else -> { b.append(c); i++ }
            }
        }
        return b.toString()
    }
}

package app.milestone

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/** Tone lint over every string, plural item and array item in `res/values/strings*.xml`.
 *  Owner, 2026-10-25: copy reads neutral and factual, like a tool, not a product selling
 *  or motivating. The rules are objective markers of that voice: exclamation marks,
 *  emojis, "we/us/our", a hype and AI-vocabulary word list, rhetorical question openers
 *  ("First time? Pick…"), stock phrases ("Let's", "Ready to", "It's important to note",
 *  "serves as", "plays a role", "I hope this helps"), placeholder text ("[insert",
 *  "TODO"), "not just X but Y" parallels and em dashes in sentences (a lone `—` no-value
 *  placeholder is fine).
 *
 *  The word lists (hype, stock phrases, placeholders, parallels, terms) and the lint's own
 *  clean/flagged cases are `shared/tests/fixtures/copy_tone.json`, which the core's twin
 *  `shared/src/app/tests/copy_tone.rs` reads too. */
class CopyToneTest {

    /** One resource string: its file, `name` (`name[quantity]` / `name[index]` for
     *  plural and array items) and its text as the app shows it. */
    private data class Res(val file: String, val name: String, val text: String)

    /** A label and the lowercase, straight-apostrophe forms that count as it. */
    private class Phrase(val label: String, val forms: List<String>)

    /** A negative parallel: a [first] phrase with a [then] word later in the same sentence. */
    private class Parallel(val label: String, val first: List<String>, val then: List<String>)

    private val fixture: JsonObject =
        Json.parseToJsonElement(File(REPO_ROOT, "shared/tests/fixtures/copy_tone.json").readText()).jsonObject

    private fun strings(key: String): List<String> = fixture[key]!!.jsonArray.map { it.jsonPrimitive.content }

    private fun phrases(key: String): List<Phrase> = fixture[key]!!.jsonArray.map { e ->
        val o = e.jsonObject
        Phrase(o["label"]!!.jsonPrimitive.content, (o["forms"] as JsonArray).map { it.jsonPrimitive.content })
    }

    /** Hype and AI-vocabulary words. */
    private val hype = phrases("hype")

    /** Stock phrases matched case-insensitively ("Ready to" is case-sensitive, in [violations]). */
    private val stockPhrases = phrases("stock_phrases")

    /** Template and placeholder text left unfilled. */
    private val placeholders = strings("placeholders")

    private val parallels: List<Parallel> = fixture["parallels"]!!.jsonArray.map { e ->
        val o = e.jsonObject
        fun list(k: String) = o[k]!!.jsonArray.map { it.jsonPrimitive.content }
        Parallel(o["label"]!!.jsonPrimitive.content, list("first"), list("then"))
    }

    /** writing.md Terms: the word not to use to the app's word. */
    private val terms: List<Pair<String, String>> = fixture["terms"]!!.jsonArray.map {
        it.jsonObject["term"]!!.jsonPrimitive.content to it.jsonObject["say"]!!.jsonPrimitive.content
    }

    @Test
    fun theLintFlagsEachRuleAndPassesCleanCopy() {
        val clean = strings("clean")
        assertTrue("the fixture lists clean copy", clean.isNotEmpty())
        for (s in clean) {
            assertEquals(s, emptyList<String>(), violations(s))
        }
        val flagged = fixture["flagged"]!!.jsonArray.map { it.jsonObject["text"]!!.jsonPrimitive.content to it.jsonObject["rule"]!!.jsonPrimitive.content }
        assertTrue("the fixture lists flagged copy", flagged.isNotEmpty())
        for ((s, rule) in flagged) {
            val v = violations(s)
            assertTrue("`$s` must flag $rule: $v", v.any { it.contains(rule) })
        }
        // Resource escapes are undone before the rules run.
        assertEquals("Don't \"x\"\ny", unescape("""  Don\'t   \"x\"\ny """))
        assertEquals("a—b", unescape("""a\u2014b"""))
    }

    @Test
    fun everyStringReadsNeutral() {
        val bad = resources().flatMap { r ->
            violations(r.text).map { v -> "${r.file} ${r.name}: $v · `${r.text}`" }
        }
        assertTrue("${bad.size} tone violations:\n" + bad.joinToString("\n"), bad.isEmpty())
    }

    private fun violations(s: String): List<String> {
        val v = mutableListOf<String>()
        val straight = s.replace('\u2019', '\'')
        val n = straight.lowercase()
        if ('!' in s) v += "exclamation mark"
        if (s.codePoints().anyMatch(::isEmoji)) v += "emoji"
        for (w in listOf("we", "our")) if (phrase(w).containsMatchIn(n)) v += "first-person plural `$w`"
        // Lowercase or sentence-case only: `US` is the country.
        if (Regex("""\b[Uu]s\b""").containsMatchIn(s)) v += "first-person plural `us`"
        for (p in hype) if (p.forms.any { phrase(it).containsMatchIn(n) }) v += "hype `${p.label}`"
        if (QUESTION_OPENER.containsMatchIn(s)) v += "rhetorical question opener"
        for (p in stockPhrases) if (p.forms.any { phrase(it).containsMatchIn(n) }) v += "stock phrase `${p.label}`"
        for (p in placeholders) if (phrase(p).containsMatchIn(n)) v += "placeholder `$p`"
        for (p in parallels) if (hasParallel(n, p)) v += "parallel `${p.label}`"
        if (Regex("""\bReady to\b""").containsMatchIn(straight)) v += "stock phrase `Ready to`"
        if ('—' in s && s.trim() != "—") v += "em dash in a sentence"
        for ((term, say) in terms) if (phrase(term).containsMatchIn(n)) v += "term `$term` (say $say)"
        return v
    }

    /** Whether lowercase [n] has a `first` phrase of [p] followed by one of its `then` words
     *  before the sentence ends (`. `, a newline or the end). */
    private fun hasParallel(n: String, p: Parallel) = p.first.any { f ->
        phrase(f).findAll(n).any { m ->
            val rest = n.substring(m.range.last + 1)
            val end = listOf(rest.indexOf(". "), rest.indexOf('\n')).filter { it >= 0 }.minOrNull() ?: rest.length
            p.then.any { phrase(it).containsMatchIn(rest.substring(0, end)) }
        }
    }

    /** A whole-word phrase in lowercase, straight-apostrophe text. */
    private fun phrase(p: String) = Regex("""(?<![\p{L}\p{N}])""" + Regex.escape(p) + """(?![\p{L}\p{N}])""")

    /** Pictographic emoji blocks plus the emoji joiner and presentation selector. Arrows
     *  (`→`, `↔`) and math symbols are typography, not emoji. */
    private fun isEmoji(c: Int) = c in 0x1F000..0x1FAFF || c in 0x2600..0x27BF || c in 0x2B05..0x2B55 ||
        c in 0x231A..0x231B || c in 0x23E9..0x23FA || c == 0xFE0F || c == 0x200D

    /** A sentence (string start, after `. ` or a newline) that opens with a short question
     *  and goes on. A lone question (a field label) is fine. */
    private val QUESTION_OPENER = Regex("""(?:^|\. |\n)[A-Z][^?]{0,40}\? \S""")

    private fun resources(): List<Res> {
        val dir = File(REPO_ROOT, "android/lollop/src/androidMain/res/values")
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

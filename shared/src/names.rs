//! Exercise-name folding: the one rule by which two typed spellings name the
//! same exercise. The core keys e1RM chains, the lift audit and `SetExerciseEnded`
//! compaction on [`exercise_key`], and compares movement tokens with
//! [`fold_eq`]; the shell's `nameKey` (SetEditorState.kt) folds the same way,
//! pinned by `tests/fixtures/names/fold.tsv`. ASCII is only lower-cased, so an
//! ASCII name keys exactly as `trim().to_lowercase()` did.

/// One character folded: a single char, or the ASCII letters of a Latin letter.
enum Folded {
    One(char),
    Letters(&'static str),
}

/// `c` folded: ASCII lower-cased; a Latin letter with a diacritic, a ligature or
/// the Turkish İ/ı as its ASCII letters ([`ascii_fold`]); any other character
/// upper-cased, then lower-cased, one char at a time (a step that would yield
/// more than one char keeps the char), as Kotlin's `uppercaseChar()` /
/// `lowercaseChar()`.
fn fold_char(c: char) -> Folded {
    if c.is_ascii() {
        return Folded::One(c.to_ascii_lowercase());
    }
    if let Some(letters) = ascii_fold(c) {
        return Folded::Letters(letters);
    }
    let up = single(c.to_uppercase()).unwrap_or(c);
    Folded::One(single(up.to_lowercase()).unwrap_or(up))
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

fn folded_chars(s: &str) -> impl Iterator<Item = char> + '_ {
    s.chars().flat_map(|c| {
        let (one, letters) = match fold_char(c) {
            Folded::One(c) => (Some(c), ""),
            Folded::Letters(l) => (None, l),
        };
        one.into_iter().chain(letters.chars())
    })
}

/// `s` folded character by character ([`fold_char`]).
pub(crate) fn fold(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    folded_chars(s).collect()
}

/// Whether `a` and `b` fold to the same text; `eq_ignore_ascii_case` for ASCII.
pub(crate) fn fold_eq(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    folded_chars(a).eq(folded_chars(b))
}

/// The key two spellings of one exercise share: the trimmed name, folded
/// ("İncline bench press" and "incline bench press" → "incline bench press").
pub(crate) fn exercise_key(name: &str) -> String {
    fold(name.trim())
}

/// The lowercase ASCII spelling of a Latin-1 / Latin Extended-A letter with a
/// diacritic, a ligature, the capital sharp s or the Turkish dotted/dotless i
/// (İ, ı → "i"; é → "e"; ß, ẞ → "ss"); None for every other character,
/// including ASCII.
pub(crate) fn ascii_fold(c: char) -> Option<&'static str> {
    Some(match c {
        'À'..='Å' | 'à'..='å' | 'Ā'..='ą' => "a",
        'Æ' | 'æ' => "ae",
        'Ç' | 'ç' | 'Ć'..='č' => "c",
        'Ð' | 'ð' | 'Ď'..='đ' => "d",
        'È'..='Ë' | 'è'..='ë' | 'Ē'..='ě' => "e",
        'Ĝ'..='ģ' => "g",
        'Ĥ'..='ħ' => "h",
        'Ì'..='Ï' | 'ì'..='ï' | 'Ĩ'..='ı' => "i",
        'Ĳ' | 'ĳ' => "ij",
        'Ĵ' | 'ĵ' => "j",
        'Ķ'..='ĸ' => "k",
        'Ĺ'..='ł' => "l",
        'Ñ' | 'ñ' | 'Ń'..='ŋ' => "n",
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' | 'Ō'..='ő' => "o",
        'Œ' | 'œ' => "oe",
        'Ŕ'..='ř' => "r",
        'Ś'..='š' | 'ſ' | 'Ș' | 'ș' => "s",
        'ß' | 'ẞ' => "ss",
        'Ţ'..='ŧ' | 'Ț' | 'ț' => "t",
        'Þ' | 'þ' => "th",
        'Ù'..='Ü' | 'ù'..='ü' | 'Ũ'..='ų' => "u",
        'Ŵ' | 'ŵ' => "w",
        'Ý' | 'ý' | 'ÿ' | 'Ŷ'..='Ÿ' => "y",
        'Ź'..='ž' => "z",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of the fixture the shell's `nameKey` test reads too: the core
    /// and the shell key each spelling the same.
    #[test]
    fn exercise_key_matches_the_shared_fixture() {
        let fixture = include_str!("../tests/fixtures/names/fold.tsv");
        let mut rows = 0;
        for line in fixture
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (name, key) = line.split_once('\t').expect("name<TAB>key");
            assert_eq!(exercise_key(name), key, "{name:?}");
            rows += 1;
        }
        assert!(rows >= 10, "{rows} rows");
    }

    #[test]
    fn ascii_folds_exactly_as_before() {
        for name in ["  Back Squat ", "PULL-UPS", "Bench press 2", "rdl", ""] {
            assert_eq!(exercise_key(name), name.trim().to_lowercase(), "{name:?}");
            assert_eq!(fold(name), name.to_ascii_lowercase());
            assert!(fold_eq(name, &name.to_uppercase()));
        }
        assert!(!fold_eq("Squat", "Squats"));
    }

    #[test]
    fn fold_eq_agrees_with_fold() {
        for (a, b) in [
            ("İncline", "incline"),
            ("Inclıne", "INCLINE"),
            ("Straße", "STRASSE"),
            ("STRAẞE", "strasse"),
            ("Crème", "creme"),
            ("ΣΚΟΥΑΤ", "σκουας"),
            ("Жим", "жим"),
        ] {
            assert_eq!(fold_eq(a, b), fold(a) == fold(b), "{a:?} {b:?}");
        }
        assert!(fold_eq("İncline", "incline"));
        assert!(fold_eq("STRAẞE", "Strasse"));
        assert!(fold_eq("Жим", "жим"));
    }
}

//! Exercise-name folding: the one rule by which two typed spellings name the
//! same exercise. The core keys e1RM chains, the lift audit and `SetExerciseEnded`
//! compaction on [`exercise_key`], and compares movement tokens with
//! [`fold_eq`]; the shell's `nameKey` (SetEditorState.kt) folds the same way,
//! pinned by `tests/fixtures/names/fold.tsv`. ASCII is only lower-cased.

use std::borrow::Cow;

/// `c` folded onto `out`: ASCII lower-cased; any other character upper-cased,
/// then lower-cased, one char at a time (a step that would yield more than one
/// char keeps the char), as Kotlin's `uppercaseChar()` / `lowercaseChar()`; a
/// Latin letter with a diacritic, a ligature or the Turkish İ/ı, before or
/// after that step, becomes its ASCII letters ([`ascii_fold`]). Folding the
/// result again changes nothing (U+212B Å → å → "a", like U+00C5 Å).
fn push_folded(c: char, out: &mut String) {
    if c.is_ascii() {
        out.push(c.to_ascii_lowercase());
        return;
    }
    let up = single(c.to_uppercase()).unwrap_or(c);
    let low = single(up.to_lowercase()).unwrap_or(up);
    match ascii_fold(c).or_else(|| ascii_fold(low)) {
        Some(letters) => out.push_str(letters),
        None => out.push(low),
    }
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// `s` folded character by character ([`push_folded`]).
pub(crate) fn fold(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        push_folded(c, &mut out);
    }
    out
}

/// Whether `a` and `b` fold to the same text: `eq_ignore_ascii_case` (a length
/// check first), then [`fold`] on both only when either is not ASCII.
pub(crate) fn fold_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || (!(a.is_ascii() && b.is_ascii()) && fold(a) == fold(b))
}

/// A name folded once, to compare with many ASCII words ([`FoldKey::is`]): an
/// ASCII name is borrowed as is, so each compare is one `eq_ignore_ascii_case`.
pub(crate) struct FoldKey<'a>(Cow<'a, str>);

impl<'a> FoldKey<'a> {
    pub(crate) fn new(name: &'a str) -> Self {
        FoldKey(if name.is_ascii() {
            Cow::Borrowed(name)
        } else {
            Cow::Owned(fold(name))
        })
    }

    /// Whether the name folds to `word`, an ASCII or already folded word.
    pub(crate) fn is(&self, word: &str) -> bool {
        self.0.eq_ignore_ascii_case(word)
    }
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
fn ascii_fold(c: char) -> Option<&'static str> {
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
            assert!(fold_eq(name, &name.to_uppercase()));
            assert!(FoldKey::new(name).is(&name.to_uppercase()));
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
            assert_eq!(
                FoldKey::new(a).is(&fold(b)),
                fold(a) == fold(b),
                "{a:?} {b:?}"
            );
        }
    }

    /// Folding a folded name changes nothing, for every char; so the Ångström
    /// sign U+212B keys like the letter U+00C5, the pair the pre-fold
    /// compaction key (`trim().to_lowercase()`) merged too.
    #[test]
    fn fold_is_idempotent() {
        for c in (0..=u32::from(char::MAX)).filter_map(char::from_u32) {
            let once = fold(c.encode_utf8(&mut [0; 4]));
            assert_eq!(fold(&once), once, "{c:?} U+{:04X}", u32::from(c));
        }
        let (sign, letter) = ("\u{212B}ngstr\u{f6}m row", "\u{c5}ngstr\u{f6}m row");
        assert_eq!(sign.to_lowercase(), letter.to_lowercase());
        assert!(fold_eq(sign, letter));
    }

    /// [`FoldKey::is`] compares with ASCII words only: every legacy catalog
    /// label `hypertrophy::renamed` matches is ASCII.
    #[test]
    fn legacy_labels_are_ascii() {
        for (old, _) in crate::hypertrophy::LEGACY_NAMES
            .iter()
            .chain(crate::calisthenics::LEGACY_NAMES)
        {
            assert!(old.is_ascii(), "{old:?}");
        }
    }

    /// The name classifiers' hot path: `same_movement` and `movement_key`
    /// compare every token with the equipment words and every name with the
    /// legacy labels.
    #[test]
    #[ignore = "timing probe; run in release with --ignored --nocapture"]
    fn name_classifier_timing() {
        use std::hint::black_box;
        let names = [
            "Back squat",
            "Barbell back squat",
            "Bench press",
            "Incline dumbbell curl",
            "Cable row",
            "Row (barbell/cable/machine)",
            "Romanian deadlift",
            "Pull-up",
            "Overhead press",
            "Smith machine back squat",
            "Lat pulldown",
            "Chest fly",
        ];
        let mut n = 0usize;
        let t = std::time::Instant::now();
        for i in 0..200_000usize {
            let (a, b) = (names[i % names.len()], names[(i * 7 + 3) % names.len()]);
            n += usize::from(crate::plan::same_movement(black_box(a), black_box(b)));
        }
        eprintln!("same_movement ×200k: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        for i in 0..200_000usize {
            n += crate::plan::movement_key(black_box(names[i % names.len()])).len();
        }
        eprintln!("movement_key ×200k: {:?} ({n})", t.elapsed());
    }
}

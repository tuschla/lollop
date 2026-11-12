//! Text and numbers as the import has always read them (the JVM's
//! `Char.isWhitespace`, `trim` and `Double.valueOf`), shared by every reader.

/// Whitespace as the import has always trimmed it (Kotlin's `Char.isWhitespace`
/// on the JVM): Unicode space separators, line and paragraph separators, and
/// the ASCII controls TAB, LF, VT, FF, CR and U+001C–U+001F.
pub(super) fn kt_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{B}' | '\u{C}' | '\r' | '\u{1C}'..='\u{1F}'
    ) || (c.is_whitespace() && c != '\u{85}')
}

pub(super) fn kt_trim(s: &str) -> &str {
    s.trim_matches(kt_is_whitespace)
}

/// A number as the JVM's `Double.valueOf` reads one (the grammar the import
/// has always accepted): characters at or below U+0020 trimmed, an optional
/// sign, then `NaN`, `Infinity`, a decimal with optional exponent, or a hex
/// float (`0x1.8p3`), each optionally suffixed `f`/`F`/`d`/`D`. `None` for
/// anything else.
pub(super) fn kt_to_double(raw: &str) -> Option<f64> {
    let s = raw.trim_matches(|c: char| c <= ' ');
    let (negative, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let magnitude = match body {
        "NaN" => f64::NAN,
        "Infinity" => f64::INFINITY,
        _ => {
            let body = body.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(body);
            if let Some(hex) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X")) {
                hex_float(hex)?
            } else {
                decimal(body)?
            }
        }
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// `digits[.digits][(e|E)[sign]digits]` with at least one mantissa digit.
fn decimal(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let int_digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if exp_digits == 0 {
            return None;
        }
        i += exp_digits;
    }
    if i != b.len() {
        return None;
    }
    s.parse().ok()
}

/// The part of a Java hex float after `0x`: `hex[.hex](p|P)[sign]digits`, the
/// binary exponent required. Exact for mantissas of up to 53 significant bits.
fn hex_float(s: &str) -> Option<f64> {
    let (mantissa, exponent) = s.split_once(['p', 'P'])?;
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.len() + frac.len() == 0
        || !int
            .chars()
            .chain(frac.chars())
            .all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let exp_digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
    if exp_digits.is_empty() || !exp_digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let exponent: i64 = exponent.parse().unwrap_or(if exponent.starts_with('-') {
        i64::MIN / 2
    } else {
        i64::MAX / 2
    });
    let mut value = 0.0f64;
    for c in int.chars().chain(frac.chars()) {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let shift = exponent
        .saturating_sub(4 * frac.len() as i64)
        .clamp(-2200, 2200) as i32;
    Some(value * 2f64.powi(shift / 2) * 2f64.powi(shift - shift / 2))
}

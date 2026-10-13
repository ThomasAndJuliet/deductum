//! The few Python built-in semantics the original stylesheet code relies on: `str.isspace`,
//! `str.split()`/`strip()`, `float(str)`, `repr(str)`, `min`/`max` and `f"{x:.4f}"`.

/// Python's `str.isspace`: Unicode White_Space plus the separators U+001C…U+001F.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}
/// Python's `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}
/// Python's `" ".join(s.split())`.
pub fn collapse(s: &str) -> String {
    s.split(is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
/// Python's word character in `re` (`\w` of a str pattern).
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `max(a, b)`: the first unless the second is strictly greater (so NaN never replaces).
pub fn py_max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}
/// `min(a, b)`: the first unless the second is strictly less.
pub fn py_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Python's `repr` of a string.
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c)
            }
            c if (c as u32) < 0x20 || (0x7f..=0xa0).contains(&(c as u32)) || c == '\u{ad}' => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if matches!(
                c,
                '\u{2028}' | '\u{2029}' | '\u{200b}'..='\u{200f}' | '\u{feff}'
            ) || (c.is_whitespace() && c != ' ') =>
            {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn digits_with_underscores(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('_')
        && !s.ends_with('_')
        && !s.contains("__")
        && s.chars().all(|c| c.is_ascii_digit() || c == '_')
}

/// Python's `float(str)`: surrounding whitespace, a sign, digits with single underscores
/// between them, a fraction, an exponent, `inf`/`infinity`/`nan` in any case. (Non-ASCII
/// decimal digits, which Python also accepts, are refused.) The error is Python's message.
pub fn float(s: &str) -> Result<f64, String> {
    let fail = || format!("could not convert string to float: {}", repr(s));
    let t = strip(s);
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    let lower = body.to_ascii_lowercase();
    if matches!(lower.as_str(), "inf" | "infinity" | "nan") {
        let v = if lower == "nan" {
            f64::NAN
        } else {
            f64::INFINITY
        };
        return Ok(if t.starts_with('-') { -v } else { v });
    }
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (mantissa, None),
    };
    let ok_int = int.is_empty() || digits_with_underscores(int);
    let ok_frac = frac.is_none_or(|f| f.is_empty() || digits_with_underscores(f));
    let has_digits = !int.is_empty() || frac.is_some_and(|f| !f.is_empty());
    let ok_exp =
        exponent.is_none_or(|e| digits_with_underscores(e.strip_prefix(['+', '-']).unwrap_or(e)));
    if !(ok_int && ok_frac && has_digits && ok_exp) {
        return Err(fail());
    }
    t.replace('_', "").parse::<f64>().map_err(|_| fail())
}

/// figkit `style._num`: a number as CSS writes it, at most four decimals, no trailing zeros.
pub fn num(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    let s = format!("{x:.4}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s.as_str()
    };
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    }
}

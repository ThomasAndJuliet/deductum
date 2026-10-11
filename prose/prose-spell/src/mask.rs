//! Hides Discord syntax from the checker.

/// `text` with code, links, mentions, custom emoji and `:shortcodes:` replaced
/// by spaces. The result has exactly as many chars as `text`, so char offsets
/// into it are char offsets into `text`.
pub fn mask_discord_syntax(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = chars.clone();
    let n = chars.len();
    let mut i = 0;
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in &mut out[from..to.min(n)] {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };
    while i < n {
        let c = chars[i];
        // Fenced code block.
        if starts_with(&chars, i, "```") {
            let end = find(&chars, i + 3, "```").map(|e| e + 3).unwrap_or(n);
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // Inline code.
        if c == '`' {
            let end = chars[i + 1..].iter().position(|&d| d == '`').map(|p| i + 1 + p + 1).unwrap_or(n);
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // Links.
        if (i == 0 || !chars[i - 1].is_alphanumeric()) && (starts_with_ci(&chars, i, "https://") || starts_with_ci(&chars, i, "http://")) {
            let end = run_until_space(&chars, i);
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        // <@id>, <#id>, <@&id>, <:name:id>, <a:name:id>, <t:123:R>, </cmd:id>, <https://…>
        if c == '<'
            && let Some(len) = angle_token(&chars[i..])
        {
            blank(&mut out, i, i + len);
            i += len;
            continue;
        }
        // :shortcode:
        if c == ':'
            && let Some(len) = shortcode(&chars[i..])
        {
            blank(&mut out, i, i + len);
            i += len;
            continue;
        }
        // @name typed before it becomes a mention.
        if c == '@' && (i == 0 || chars[i - 1].is_whitespace()) {
            let end = run_until_space(&chars, i);
            blank(&mut out, i, end);
            i = end;
            continue;
        }
        i += 1;
    }
    out.into_iter().collect()
}

fn starts_with(chars: &[char], at: usize, pat: &str) -> bool {
    let mut j = at;
    for p in pat.chars() {
        if chars.get(j) != Some(&p) {
            return false;
        }
        j += 1;
    }
    true
}

fn starts_with_ci(chars: &[char], at: usize, pat: &str) -> bool {
    let mut j = at;
    for p in pat.chars() {
        match chars.get(j) {
            Some(c) if c.eq_ignore_ascii_case(&p) => {}
            _ => return false,
        }
        j += 1;
    }
    true
}

fn find(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    (from..chars.len()).find(|&j| starts_with(chars, j, pat))
}

fn run_until_space(chars: &[char], from: usize) -> usize {
    chars[from..].iter().position(|c| c.is_whitespace()).map(|p| from + p).unwrap_or(chars.len())
}

fn angle_token(s: &[char]) -> Option<usize> {
    let close = s.iter().take(200).position(|&c| c == '>')?;
    let inner = &s[1..close];
    if inner.is_empty() || inner.iter().any(|c| c.is_whitespace()) {
        return None;
    }
    let first = inner[0];
    let ok = matches!(first, '@' | '#' | ':' | '/')
        || (first == 'a' && inner.get(1) == Some(&':'))
        || (first == 't' && inner.get(1) == Some(&':'))
        || starts_with_ci(inner, 0, "http");
    ok.then_some(close + 1)
}

fn shortcode(s: &[char]) -> Option<usize> {
    let mut j = 1;
    while j < s.len() && j <= 40 {
        let c = s[j];
        if c == ':' {
            return (j > 1).then_some(j + 1);
        }
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-') {
            return None;
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::mask_discord_syntax as m;

    #[test]
    fn keeps_char_count() {
        for t in ["héllo `cöde` <@1> :smile: https://x.y/é", "```rs\nfn main() {}\n``` after", "a < b > c", "@everyone hi"] {
            assert_eq!(m(t).chars().count(), t.chars().count(), "{t}");
        }
    }

    #[test]
    fn masks_syntax() {
        assert_eq!(m("see `codez` now"), "see         now");
        assert_eq!(m("hi <@!42> there"), "hi        there");
        assert_eq!(m("go https://a.b/c ok"), "go               ok");
        assert_eq!(m("lol :kekw: yes"), "lol        yes");
        assert_eq!(m("x <:pepe:99> y"), "x            y");
        assert_eq!(m("@bob hello"), "     hello");
    }

    #[test]
    fn leaves_prose() {
        assert_eq!(m("a < b and c > d"), "a < b and c > d");
        assert_eq!(m("time 10:30 ok"), "time 10:30 ok");
        assert_eq!(m("email me@x.com"), "email me@x.com");
    }

    #[test]
    fn code_block_keeps_newlines() {
        assert_eq!(m("```\nab\n```"), "   \n  \n   ");
    }
}

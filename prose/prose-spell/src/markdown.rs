//! Hides Markdown syntax from the checker: only prose is checked.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// `text` with everything but its prose replaced by spaces: markup, code,
/// maths, HTML, link targets, front matter, and URLs in the text. Prose is
/// the text of paragraphs, headings, list items, table cells, link text and
/// image descriptions, where it is written out as it reads (an entity such
/// as `&amp;` is not). The result has exactly as many chars as `text`, so
/// char offsets into it are char offsets into `text`.
pub fn mask_markdown(text: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut keep = vec![false; text.len()];
    // Inside a code block, an HTML block or front matter, nothing is prose
    let mut verbatim = 0usize;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_)) => verbatim += 1,
            Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock | TagEnd::MetadataBlock(_)) => {
                verbatim = verbatim.saturating_sub(1)
            }
            Event::Text(t) if verbatim == 0 && text.get(range.clone()) == Some(&*t) => {
                keep[range].fill(true);
            }
            _ => {}
        }
    }
    for range in latex_maths(text) {
        keep[range].fill(false);
    }
    let mut out: String = text
        .char_indices()
        .map(|(i, c)| if keep[i] || c == '\n' { c } else { ' ' })
        .collect();
    blank_urls(&mut out);
    out
}

/// Maths CommonMark does not know but Terminal Agents' threads and Cerea
/// read: `\\(…\\)`, `\\[…\\]`, and `\\begin{align}`…`\\end{align}` and the like
fn latex_maths(text: &str) -> Vec<std::ops::Range<usize>> {
    const ENVIRONMENTS: [&str; 8] = ["align", "align*", "equation", "equation*", "gather", "gather*", "multline", "eqnarray"];
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' {
            let close = match bytes[i + 1] {
                b'(' => Some("\\)"),
                b'[' => Some("\\]"),
                _ => None,
            };
            if let Some(close) = close
                && let Some(end) = text[i + 2..].find(close)
            {
                out.push(i..i + 2 + end + 2);
                i += 2 + end + 2;
                continue;
            }
            if let Some(env) = ENVIRONMENTS.iter().find(|env| text[i..].starts_with(&format!("\\begin{{{env}}}"))) {
                let end_tag = format!("\\end{{{env}}}");
                let end = text[i..].find(&end_tag).map_or(text.len(), |e| i + e + end_tag.len());
                out.push(i..end);
                i = end;
                continue;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

/// Bare URLs, which Markdown leaves as text
fn blank_urls(text: &mut String) {
    let mut chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let starts = |prefix: &str| {
            let p: Vec<char> = prefix.chars().collect();
            i + p.len() <= n && chars[i..i + p.len()].iter().zip(&p).all(|(a, b)| a.eq_ignore_ascii_case(b))
        };
        if (i == 0 || !chars[i - 1].is_alphanumeric()) && (starts("https://") || starts("http://") || starts("www.")) {
            while i < n && !chars[i].is_whitespace() {
                chars[i] = ' ';
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    *text = chars.into_iter().collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_prose_and_its_length() {
        let text = "# Tïtle\n\nSome **bold** and `cöde`, a [link](http://x.y) &amp; $x^2$.\n\n```rust\nlet x = 1;\n```\n";
        let masked = mask_markdown(text);
        assert_eq!(masked.chars().count(), text.chars().count());
        assert!(masked.contains("Tïtle"));
        assert!(masked.contains("Some   bold   and"));
        assert!(!masked.contains("cöde"));
        assert!(masked.contains(" link "));
        assert!(!masked.contains("http"));
        assert!(!masked.contains("amp"));
        assert!(!masked.contains("x^2"));
        assert!(!masked.contains("let x"));
        assert_eq!(masked.lines().count(), text.lines().count());
    }

    #[test]
    fn latex_delimited_maths_is_hidden() {
        let text = "See \\(xqz\\) and\n\\[\n\\hat f(\\xi) = qwzx\n\\]\n\\begin{align}\nzzq &= 1\n\\end{align}\nfine words.\n";
        let masked = mask_markdown(text);
        assert!(!masked.contains("xqz") && !masked.contains("qwzx") && !masked.contains("zzq") && !masked.contains("hat"));
        assert!(masked.contains("See") && masked.contains("fine words."));
        assert_eq!(masked.chars().count(), text.chars().count());
    }

    #[test]
    fn front_matter_and_bare_urls_are_hidden() {
        let text = "---\ntitle: Notes\n---\n\nSee https://exmple.org/qq or www.exmple.org today.\n";
        let masked = mask_markdown(text);
        assert!(!masked.contains("title"));
        assert!(!masked.contains("exmple"));
        assert!(masked.contains("See"));
        assert!(masked.contains("today."));
    }
}

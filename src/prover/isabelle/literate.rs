// SPDX-License-Identifier: GPL-3.0-only

//! Isabelle document text, as the literate view shows it (design C4, provers
//! §8.1): the body of a `text` or `section` turned into the Markdown that
//! prose-render sets, LaTeX maths included. This is a presentation only; edits
//! always go to the source span (`theory::Block`), never back through it.
//!
//! Formal antiquotations (`@{term …}`, `\<^const>‹…›`) are shown as code with
//! their symbols decoded; literal LaTeX maths is passed through for RaTeX;
//! common LaTeX text commands become Markdown emphasis.

use crate::codec::Codec;

fn starts(chars: &[char], i: usize, s: &str) -> bool {
    let mut j = i;
    for c in s.chars() {
        if chars.get(j) != Some(&c) {
            return false;
        }
        j += 1;
    }
    true
}

/// The end of the cartouche starting at `i` (after its closing delimiter)
fn cartouche_end(chars: &[char], i: usize) -> usize {
    let mut depth = 0;
    let mut j = i;
    while j < chars.len() {
        if starts(chars, j, "\\<open>") {
            depth += 1;
            j += 7;
        } else if chars[j] == '‹' {
            depth += 1;
            j += 1;
        } else if starts(chars, j, "\\<close>") {
            depth -= 1;
            j += 8;
            if depth == 0 {
                return j;
            }
        } else if chars[j] == '›' {
            depth -= 1;
            j += 1;
            if depth == 0 {
                return j;
            }
        } else {
            j += 1;
        }
    }
    chars.len()
}

/// A cartouche's content, if one starts at `i`, and where it ends
fn cartouche(chars: &[char], i: usize) -> Option<(String, usize)> {
    if starts(chars, i, "\\<open>") {
        let end = cartouche_end(chars, i);
        let inner = end.saturating_sub(8).max(i + 7);
        Some((chars[i + 7..inner].iter().collect(), end))
    } else if chars.get(i) == Some(&'‹') {
        let end = cartouche_end(chars, i);
        let inner = end.saturating_sub(1).max(i + 1);
        Some((chars[i + 1..inner].iter().collect(), end))
    } else {
        None
    }
}

/// A balanced `{…}` group starting at `i`
fn braces(chars: &[char], i: usize) -> Option<(String, usize)> {
    if chars.get(i) != Some(&'{') {
        return None;
    }
    let mut depth = 0;
    let mut j = i;
    while j < chars.len() {
        match chars[j] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((chars[i + 1..j].iter().collect(), j + 1));
                }
            }
            '\\' => j += 1,
            _ => {}
        }
        j += 1;
    }
    None
}

fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix("\\<open>").and_then(|r| r.strip_suffix("\\<close>")) {
        return inner.to_string();
    }
    if let Some(inner) = t.strip_prefix('‹').and_then(|r| r.strip_suffix('›')) {
        return inner.to_string();
    }
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        return t[1..t.len() - 1].to_string();
    }
    t.to_string()
}

fn code(table: &dyn Codec, s: &str) -> String {
    let text = table.decode(s.trim()).text.replace('`', "'").replace('\n', " ");
    if text.is_empty() { String::new() } else { format!("`{text}`") }
}

/// The character a symbol (`\<forall>`) stands for, if the codec knows it
fn char_of(table: &dyn Codec, symbol: &str) -> Option<char> {
    let decoded = table.decode(symbol).text;
    let mut chars = decoded.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c != '\\' => Some(c),
        _ => None,
    }
}

fn escape(c: char, out: &mut String) {
    if matches!(c, '*' | '_' | '#' | '`' | '[' | ']' | '<' | '>' | '|') {
        out.push('\\');
    }
    out.push(c);
}

/// Markdown for the source of a document text
pub fn to_markdown(source: &str, table: &dyn Codec) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len() + 16);
    convert(&chars, table, &mut out);
    join_lines(&out)
}

/// The maths (`$…$`, `$$…$$`) or antiquotation (`@{…}`) around a character offset of prose, if the
/// offset is in one: what to show rendered while the prose is edited as source
pub fn snippet_at(text: &str, at: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i] == '$' {
            let double = chars.get(i + 1) == Some(&'$');
            let open = if double { 2 } else { 1 };
            let mut j = i + open;
            while j < n && !(chars[j] == '$' && (!double || chars.get(j + 1) == Some(&'$'))) {
                j += 1;
            }
            let end = (j + open).min(n);
            if i <= at && at <= end && j < n {
                return Some(chars[i..end].iter().collect());
            }
            i = end;
            continue;
        }
        if chars[i] == '@' && chars.get(i + 1) == Some(&'{') {
            let mut depth = 0;
            let mut j = i + 1;
            while j < n {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if i <= at && at <= j + 1 && j < n {
                return Some(chars[i..=j].iter().collect());
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    None
}

/// A paragraph's lines joined: the source breaks lines where its author's editor did, and the
/// page wraps them again (the renderer keeps a single line break, as chats do). Blank lines, list
/// items, display maths, code and hard breaks (`\\`) stay
fn join_lines(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut fenced = false;
    let mut display = false;
    let mut prev_open = false;
    for line in md.split('\n') {
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
        }
        let maths_edge = t.starts_with("$$") || t.trim_end().ends_with("$$");
        let block_start = t.is_empty()
            || t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with('#')
            || t.starts_with('>')
            || t.starts_with('|')
            || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let join = prev_open && !fenced && !display && !block_start && !maths_edge && !out.ends_with("  ");
        if join {
            out.push(' ');
            out.push_str(t);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
        if t.starts_with("$$") && !(t.len() > 2 && t.trim_end().ends_with("$$")) {
            display = !display;
        }
        prev_open = !t.is_empty() && !fenced && !display && !maths_edge;
    }
    out
}

fn convert(chars: &[char], table: &dyn Codec, out: &mut String) {
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if let Some((marker, len)) = script_marker(chars, i)
            && marker != "\\<^bold>"
            && let Some(end) = script_end(chars, i, marker, len)
        {
            let source: String = chars[i..end].iter().collect();
            let decoded = table.decode(&source);
            if decoded.text != source {
                for ch in decoded.text.chars() { escape(ch, out); }
                i = end;
                continue;
            }
        }
        // Literal maths: passed through
        if c == '$' {
            let double = chars.get(i + 1) == Some(&'$');
            let open = if double { 2 } else { 1 };
            let mut j = i + open;
            while j < n && !(chars[j] == '$' && (!double || chars.get(j + 1) == Some(&'$'))) {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + open).min(n);
            let body: String = chars[i + open..j.min(n)].iter().collect();
            let body = math_symbols(&body);
            if double {
                out.push_str(&format!("\n\n$${body}$$\n\n"));
            } else {
                out.push_str(&format!("${body}$"));
            }
            i = end;
            continue;
        }
        if starts(chars, i, "\\[") || starts(chars, i, "\\(") {
            let close = if chars[i + 1] == '[' { "\\]" } else { "\\)" };
            let mut j = i + 2;
            while j < n && !starts(chars, j, close) {
                j += 1;
            }
            let body: String = chars[i + 2..j.min(n)].iter().collect();
            let body = math_symbols(&body);
            if close == "\\]" {
                out.push_str(&format!("\n\n$${body}$$\n\n"));
            } else {
                out.push_str(&format!("${body}$"));
            }
            i = (j + 2).min(n);
            continue;
        }
        // Antiquotation @{name args}
        if c == '@' && chars.get(i + 1) == Some(&'{') {
            if let Some((inner, end)) = braces(chars, i + 1) {
                let inner = inner.trim();
                let name_end = inner.find(|c: char| c.is_whitespace() || c == '[').unwrap_or(inner.len());
                let name = &inner[..name_end];
                let mut rest = inner[name_end..].trim_start();
                if rest.starts_with('[')
                    && let Some(close) = rest.find(']')
                {
                    rest = rest[close + 1..].trim_start();
                }
                out.push_str(&antiquotation(name, &strip_quotes(rest), table));
                i = end;
                continue;
            }
        }
        // Control symbols \<^name> with a cartouche argument
        if starts(chars, i, "\\<^") {
            let mut j = i + 3;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if chars.get(j) == Some(&'>') {
                let name: String = chars[i + 3..j].iter().collect();
                let after = j + 1;
                match name.as_str() {
                    "item" | "descr" | "enum" => {
                        out.push_str(if name == "enum" { "\n\n1. " } else { "\n\n- " });
                        i = after;
                        while i < n && (chars[i] == ' ' || chars[i] == '\t') {
                            i += 1;
                        }
                        continue;
                    }
                    "noindent" => {
                        i = after;
                        continue;
                    }
                    "medskip" | "smallskip" | "bigskip" => {
                        out.push_str("\n\n");
                        i = after;
                        continue;
                    }
                    _ => {}
                }
                if let Some((arg, end)) = cartouche(chars, after) {
                    let arg_chars: Vec<char> = arg.chars().collect();
                    match name.as_str() {
                        "emph" => {
                            out.push('*');
                            convert(&arg_chars, table, out);
                            out.push('*');
                        }
                        "bold" => {
                            out.push_str("**");
                            convert(&arg_chars, table, out);
                            out.push_str("**");
                        }
                        "footnote" => {
                            out.push_str(" (");
                            convert(&arg_chars, table, out);
                            out.push(')');
                        }
                        "cite" => out.push_str(&format!("[{}]", arg.trim())),
                        "url" => out.push_str(arg.trim()),
                        _ => out.push_str(&antiquotation(&name, &arg, table)),
                    }
                    i = end;
                    continue;
                }
            }
        }
        // LaTeX text commands
        if c == '\\' && i + 1 < n && chars[i + 1].is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < n && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            let cmd: String = chars[i + 1..j].iter().collect();
            let mut k = j;
            // optional argument
            if chars.get(k) == Some(&'[') {
                while k < n && chars[k] != ']' {
                    k += 1;
                }
                k = (k + 1).min(n);
            }
            let arg = braces(chars, k);
            let wrapped = |out: &mut String, l: &str, r: &str, arg: &str| {
                out.push_str(l);
                convert(&arg.chars().collect::<Vec<_>>(), table, out);
                out.push_str(r);
            };
            match (cmd.as_str(), arg) {
                ("emph" | "textit" | "textsl", Some((a, end))) => {
                    wrapped(out, "*", "*", &a);
                    i = end;
                    continue;
                }
                ("textbf", Some((a, end))) => {
                    wrapped(out, "**", "**", &a);
                    i = end;
                    continue;
                }
                ("texttt" | "isa" | "verb", Some((a, end))) => {
                    out.push_str(&code(table, &a));
                    i = end;
                    continue;
                }
                ("cite" | "citep" | "citet" | "autocite", Some((a, end))) => {
                    out.push_str(&format!("[{a}]"));
                    i = end;
                    continue;
                }
                ("label" | "index", Some((_, end))) => {
                    i = end;
                    continue;
                }
                ("ref" | "eqref" | "autoref" | "cref" | "Cref" | "url" | "textrm" | "textsf" | "mbox", Some((a, end))) => {
                    wrapped(out, "", "", &a);
                    i = end;
                    continue;
                }
                ("footnote", Some((a, end))) => {
                    wrapped(out, " (", ")", &a);
                    i = end;
                    continue;
                }
                ("ldots" | "dots", _) => {
                    out.push('…');
                    i = j;
                    continue;
                }
                ("LaTeX", _) => {
                    out.push_str("LaTeX");
                    i = j;
                    continue;
                }
                ("TeX", _) => {
                    out.push_str("TeX");
                    i = j;
                    continue;
                }
                _ => {}
            }
        }
        if starts(chars, i, "\\\\") {
            out.push_str("  \n");
            i += 2;
            continue;
        }
        if starts(chars, i, "---") {
            out.push('—');
            i += 3;
            continue;
        }
        if starts(chars, i, "--") {
            out.push('–');
            i += 2;
            continue;
        }
        if starts(chars, i, "``") {
            out.push('“');
            i += 2;
            continue;
        }
        if starts(chars, i, "''") {
            out.push('”');
            i += 2;
            continue;
        }
        if c == '~' {
            out.push('\u{a0}');
            i += 1;
            continue;
        }
        // Isabelle symbols
        if c == '\\' && chars.get(i + 1) == Some(&'<') {
            let mut j = i + 2;
            while j < n && chars[j] != '>' && !chars[j].is_whitespace() {
                j += 1;
            }
            if j < n && chars[j] == '>' {
                let sym: String = chars[i..=j].iter().collect();
                if let Some(ch) = char_of(table, &sym) {
                    escape(ch, out);
                    i = j + 1;
                    continue;
                }
            }
        }
        escape(c, out);
        i += 1;
    }
}

/// End of a single-character or balanced Isabelle script control.
fn script_marker(chars: &[char], start: usize) -> Option<(&'static str, usize)> {
    for (named, unicode) in [("\\<^sup>", '⇧'), ("\\<^sub>", '⇩'), ("\\<^bsup>", '⇗'), ("\\<^esup>", '⇖'), ("\\<^bsub>", '⇘'), ("\\<^esub>", '⇙'), ("\\<^bold>", '❙')] {
        if chars.get(start) == Some(&unicode) { return Some((named, 1)); }
        if starts(chars, start, named) { return Some((named, named.chars().count())); }
    }
    None
}

fn script_end(chars: &[char], start: usize, marker: &str, len: usize) -> Option<usize> {
    let from = start + len;
    match marker {
        "\\<^sup>" | "\\<^sub>" | "\\<^bold>" => {
            if chars.get(from) == Some(&'\\') && chars.get(from + 1) == Some(&'<') {
                Some(from + chars[from..].iter().position(|c| *c == '>')? + 1)
            } else {
                chars.get(from).map(|_| from + 1)
            }
        }
        "\\<^bsup>" | "\\<^bsub>" => {
            let close = if marker == "\\<^bsup>" { "\\<^esup>" } else { "\\<^esub>" };
            (from..chars.len()).find_map(|j| script_marker(chars, j).filter(|(name, _)| *name == close).map(|(_, len)| j + len))
        }
        _ => None,
    }
}

/// Text inside an Isabelle script is literal, including TeX's special characters.
fn math_script_content(chars: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'<')
            && let Some(end) = chars[i..].iter().position(|c| *c == '>')
        {
            let token: String = chars[i..=i + end].iter().collect();
            out.push_str(&math_symbols(&token));
            i += end + 1;
            continue;
        }
        match chars[i] {
            '_' | '#' | '%' | '&' | '{' | '}' | '$' => { out.push('\\'); out.push(chars[i]); }
            '\\' => out.push_str("\\backslash "),
            '^' => out.push_str("\\text{\\textasciicircum}"),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

fn antiquotation(name: &str, arg: &str, table: &dyn Codec) -> String {
    match name {
        "cite" => format!("[{}]", arg.trim()),
        "url" | "doc" | "file" | "path" | "dir" => format!("`{}`", arg.trim()),
        "emph" => format!("*{}*", table.decode(arg).text),
        "bold" => format!("**{}**", table.decode(arg).text),
        _ => code(table, arg),
    }
}

/// Isabelle symbols inside LaTeX maths, as LaTeX commands where obvious
fn math_symbols(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some((marker, len)) = script_marker(&chars, i)
            && let Some(end) = script_end(&chars, i, marker, len)
        {
            let sup = marker == "\\<^sup>" || marker == "\\<^bsup>";
            let inner_start = i + len;
            let close_start = if marker == "\\<^bsup>" || marker == "\\<^bsub>" {
                (inner_start..end).find(|&j| script_marker(&chars, j).is_some_and(|(m, _)| m == if sup { "\\<^esup>" } else { "\\<^esub>" })).unwrap_or(end)
            } else { end };
            if marker == "\\<^bold>" { out.push_str("\\mathbf"); } else { out.push(if sup { '^' } else { '_' }); }
            out.push('{');
            out.push_str(&math_script_content(&chars[inner_start..close_start]));
            out.push('}');
            i = end;
            continue;
        }
        if starts(&chars, i, "\\<")
            && let Some(len) = chars[i..].iter().position(|c| *c == '>')
        {
            let name: String = chars[i + 2..i + len].iter().collect();
            let latex = match name.as_str() {
                "forall" => "\\forall ",
                "exists" => "\\exists ",
                "and" => "\\land ",
                "or" => "\\lor ",
                "not" => "\\neg ",
                "longrightarrow" | "rightarrow" => "\\to ",
                "Longrightarrow" | "Rightarrow" => "\\Rightarrow ",
                "le" => "\\le ",
                "ge" => "\\ge ",
                "noteq" => "\\neq ",
                "in" => "\\in ",
                "subseteq" => "\\subseteq ",
                "union" => "\\cup ",
                "inter" => "\\cap ",
                "lambda" => "\\lambda ",
                "times" => "\\times ",
                "circ" => "\\circ ",
                "sqsubseteq" => "\\sqsubseteq ",
                other => {
                    out.push('\\');
                    out.push_str(other);
                    out.push(' ');
                    i += len + 1;
                    continue;
                }
            };
            out.push_str(latex);
            i += len + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::symbols::Table;
    use super::*;

    #[test]
    fn converts_document_text() {
        let table = Table::fallback();
        let md = to_markdown(
            "We prove @{term \"\\<forall>x. P x\"} using \\<^emph>\\<open>simp\\<close> and $x^2 \\<le> y$. See \\cite{Foo}.\n\n\\<^item> one\n\\<^item> two",
            &table,
        );
        assert!(md.contains("`∀x. P x`"), "{md}");
        assert!(md.contains("*simp*"), "{md}");
        assert!(md.contains("$x^2 \\le  y$"), "{md}");
        assert!(md.contains("[Foo]"), "{md}");
        assert!(md.contains("\n\n- one"), "{md}");
        assert_eq!(snippet_at("so $x + 0 = x$ for", 5).as_deref(), Some("$x + 0 = x$"));
        assert_eq!(snippet_at("so @{term \"a + b\"} here", 8).as_deref(), Some("@{term \"a + b\"}"));
        assert_eq!(snippet_at("plain text", 3), None);
        let joined = to_markdown("We prove\n  that it holds.\n\nNext\nline", &table);
        assert!(joined.contains("We prove that it holds.") && joined.contains("\n\nNext line"), "{joined}");
    }

    #[test]
    fn escapes_markdown_in_prose() {
        let table = Table::fallback();
        assert_eq!(to_markdown("a_b *c*", &table), "a\\_b \\*c\\*");
    }

    #[test]
    fn scripts_in_prose_antiquotation_and_maths_keep_their_forms() {
        let table = Table::fallback();
        let md = to_markdown("x\\<^sup>2 y\\<^sub>\\<alpha> z\\<^bsup>n+1\\<^esup> @{term \"x\\<^sup>2\"} $a\\<^sup>2 + b\\<^sub>\\<alpha>$ $$c\\<^bsup>n+1\\<^esup>$$", &table);
        let sup2 = super::super::symbols::script_glyph(true, '2').unwrap();
        let sub_alpha = super::super::symbols::script_glyph(false, 'α').unwrap();
        assert!(md.contains(sup2) && md.contains(sub_alpha), "{md:?}");
        assert!(md.contains(&format!("`x{sup2}`")), "{md:?}");
        assert!(md.contains("$a^{2} + b_{\\alpha }$"), "{md:?}");
        assert!(md.contains("$$c^{n+1}$$"), "{md:?}");
        let unicode = to_markdown("x⇧2 y⇩α z⇗n+1⇖ $a⇧2 + b⇩\\<alpha>$ $$c⇗n+1⇖$$", &table);
        assert!(unicode.contains(sup2) && unicode.contains(sub_alpha), "{unicode:?}");
        assert!(unicode.contains("$a^{2} + b_{\\alpha }$") && unicode.contains("$$c^{n+1}$$"), "{unicode:?}");
        let malformed = to_markdown("unfinished \\<^bsup>abc and $x\\<^sup>$", &table);
        assert!(!malformed.is_empty());
        assert_eq!(math_symbols("x\\<^sup>_ + y\\<^sup># + z\\<^sup>{"), "x^{\\_} + y^{\\#} + z^{\\{}");
        for c in ['_', '#', '%', '&', '{', '}', '^', '\\'] {
            let latex = math_symbols(&format!("x\\<^sup>{c}"));
            assert!(prose_render::math::parses(&latex), "{c:?} produced invalid maths {latex:?}");
        }
        for source in ["x\\<^bold>v", "x\\<^bold>\\<alpha>", "x❙v"] {
            let latex = math_symbols(source);
            assert!(latex.contains("\\mathbf{") && prose_render::math::parses(&latex), "{source:?}: {latex:?}");
        }
    }
}

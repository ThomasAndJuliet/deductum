// SPDX-License-Identifier: GPL-3.0-only

//! Isabelle document text, as the literate view shows it (design C4): the
//! body of a `text` or `section` turned into the Markdown the thread view's
//! renderer sets, LaTeX maths included. This is a presentation only; edits
//! always go to the source span (`theory::Block`), never back through it.
//!
//! Formal antiquotations (`@{term …}`, `\<^const>‹…›`) are shown as code with
//! their symbols decoded; literal LaTeX maths is passed through for RaTeX;
//! common LaTeX text commands become Markdown emphasis.

use super::symbols::Table;

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

fn code(table: &Table, s: &str) -> String {
    let text = table.decode(s.trim()).text.replace('`', "'").replace('\n', " ");
    if text.is_empty() { String::new() } else { format!("`{text}`") }
}

fn escape(c: char, out: &mut String) {
    if matches!(c, '*' | '_' | '#' | '`' | '[' | ']' | '<' | '>' | '|') {
        out.push('\\');
    }
    out.push(c);
}

/// Markdown for the source of a document text
pub fn to_markdown(source: &str, table: &Table) -> String {
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

fn convert(chars: &[char], table: &Table, out: &mut String) {
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
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
                // Sub- and superscripts, as jEdit shows them
                if (sym == "\\<^sub>" || sym == "\\<^sup>")
                    && let Some(&next) = chars.get(j + 1)
                    && let Some(sc) = super::symbols::unicode_script(sym == "\\<^sup>", next)
                {
                    out.push(sc);
                    i = j + 2;
                    continue;
                }
                if let Some(ch) = table.char_of(&sym) {
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

fn antiquotation(name: &str, arg: &str, table: &Table) -> String {
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
}

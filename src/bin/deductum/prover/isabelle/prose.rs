// SPDX-License-Identifier: GPL-3.0-only

//! Isabelle prose while it is edited (design "Prose a paragraph at a time",
//! provers §8.2): markup reads as it renders unless the caret is in it. Maths
//! is set in Unicode through Isabelle's symbol names (`$x \le y^2$` as
//! `x ≤ y²`), `\<^bold>‹…›`, `\<^emph>‹…›` and `\<^verbatim>‹…›` as they read,
//! and antiquotations (`@{term …}`) as what they hold. One shown character
//! stands for each decoded one, so offsets stay the source's.

use std::collections::HashMap;

use crate::codec::{Codec, Decoded, Style};
use crate::editor::{Hl, Markup, Sem, ZERO_WIDTH, shown_text};
use crate::prover::PaletteEntry;

/// What text editing can add around the selection (or at the caret, the caret then between):
/// a label, what goes before, what goes after
pub const TEMPLATES: &[(&str, &str, &str)] = &[
    ("emphasis", "\\<^emph>‹", "›"),
    ("bold", "\\<^bold>‹", "›"),
    ("code", "\\<^verbatim>‹", "›"),
    ("term", "@{term \"", "\"}"),
    ("theorem", "@{thm ", "}"),
    ("constant", "@{const ", "}"),
    ("type", "@{typ \"", "\"}"),
    ("maths", "$", "$"),
    ("cartouche", "‹", "›"),
    ("url", "\\<^url>‹", "›"),
    ("citation", "\\cite{", "}"),
    ("footnote", "\\footnote{", "}"),
];

/// The palette's list entries for prose whose caret line is `line` (decoded): in a list
/// (`\<^item>` or `\<^enum>`, as written or as the symbols show them), its next item and a
/// nested one at the line's indent; elsewhere a list to begin
pub fn lists(line: &str, table: &dyn Codec) -> Vec<PaletteEntry> {
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let rest = line.trim_start();
    let marker = ["\\<^item>", "\\<^enum>"].into_iter().find(|m| {
        let shown = table.decode(m).text;
        rest.starts_with(m) || rest.starts_with(shown.as_str())
    });
    match marker {
        Some(marker) => vec![
            PaletteEntry::new("next item", &format!("\n{indent}{marker} "), ""),
            PaletteEntry::new("nested item", &format!("\n{indent}  {marker} "), ""),
        ],
        None => vec![PaletteEntry::new("bullet list", "\n\\<^item> ", ""), PaletteEntry::new("numbered list", "\n\\<^enum> ", "")],
    }
}

/// Whether the text around a prose block's body is only its command and delimiters
pub fn bare(around: &str) -> bool {
    around.replace("\\<open>", " ").replace("\\<close>", " ").split_whitespace().all(|w| {
        matches!(w, "text" | "txt" | "section" | "subsection" | "subsubsection" | "paragraph" | "subparagraph" | "chapter" | "text_raw")
    })
}

/// Prose as the editor shows it, the caret at `caret` (a decoded offset): [`shown_text`], and its
/// maths set in Unicode where it can be (`$x \le y^2$` as `x ≤ y²`), one shown character for one
/// source character (the rest zero-width spaces), except the formula the caret is in, which is
/// its source, to edit
pub fn shown_text_at(text: &str, caret: Option<usize>, table: &dyn Codec) -> String {
    let base = shown_text(text, true);
    if !text.contains('$') {
        return base;
    }
    let src: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = base.chars().collect();
    let mut i = 0;
    while i < src.len() {
        if src[i] != '$' {
            i += 1;
            continue;
        }
        let open = if src.get(i + 1) == Some(&'$') { 2 } else { 1 };
        let mut j = i + open;
        while j < src.len() && !(src[j] == '$' && (open == 1 || src.get(j + 1) == Some(&'$'))) {
            j += 1;
        }
        if j >= src.len() {
            break;
        }
        let end = j + open;
        if !caret.is_some_and(|c| i <= c && c <= end) {
            set_math(&src, i, end, open, &mut out, table);
        }
        i = end;
    }
    out.into_iter().collect()
}

/// The markup of a paragraph's decoded text: `\<^bold>‹…›` (decoded as a bold `‹`),
/// `\<^emph>‹…›` (a control glyph and a cartouche), `\<^verbatim>‹…›`, and antiquotations
/// (`@{name …}`, shown as what they hold, as code); none the caret is inside
pub fn prose_markup(decoded: &Decoded, caret: Option<usize>) -> Vec<Markup> {
    let cs: Vec<char> = decoded.text.chars().collect();
    let n = cs.len();
    let style_at: HashMap<usize, Style> = decoded.styles.iter().map(|(i, st)| (*i, *st)).collect();
    let inside = |s: usize, e: usize| caret.is_some_and(|c| s <= c && c <= e);
    // the cartouche opened at `i`: where it closes
    let close = |i: usize| -> Option<usize> {
        let mut depth = 0;
        for (k, c) in cs.iter().enumerate().skip(i) {
            match c {
                '‹' => depth += 1,
                '›' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                '\n' if cs.get(k + 1) == Some(&'\n') => return None,
                _ => {}
            }
        }
        None
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        // \<^bold>‹…›: the bold `‹` stands for both
        if cs[i] == '‹' && style_at.get(&i) == Some(&Style::Bold)
            && let Some(j) = close(i)
        {
            if !inside(i, j + 1) {
                out.push(Markup { hide: vec![i, j], style: (i + 1, j, Hl::Bold) });
            }
            i = j + 1;
            continue;
        }
        // \<^emph>‹…›: its control glyph, then the cartouche
        if cs[i] == '\u{2223}' && style_at.get(&i) == Some(&Style::Control) && cs.get(i + 1) == Some(&'‹')
            && let Some(j) = close(i + 1)
        {
            if !inside(i, j + 1) {
                out.push(Markup { hide: vec![i, i + 1, j], style: (i + 2, j, Hl::Emph) });
            }
            i = j + 1;
            continue;
        }
        // \<^verbatim>‹…› (and the like, written out)
        if cs[i] == '\\' && cs[i..].iter().collect::<String>().starts_with("\\<^verbatim>‹") {
            let open = i + "\\<^verbatim>".chars().count();
            if let Some(j) = close(open) {
                if !inside(i, j + 1) {
                    out.push(Markup { hide: (i..=open).chain([j]).collect(), style: (open + 1, j, Hl::Sem(Sem::Free)) });
                }
                i = j + 1;
                continue;
            }
        }
        // @{name arguments}: shown as the arguments, as code
        if cs[i] == '@' && cs.get(i + 1) == Some(&'{') {
            let mut j = i + 2;
            while j < n && (cs[j].is_alphanumeric() || cs[j] == '_') {
                j += 1;
            }
            while j < n && cs[j] == ' ' {
                j += 1;
            }
            let body = j;
            let (mut depth, mut quoted, mut k) = (0usize, false, j);
            let mut end = None;
            while k < n {
                let c = cs[k];
                if quoted {
                    quoted = c != '"';
                } else if c == '"' {
                    quoted = true;
                } else if c == '‹' {
                    depth += 1;
                } else if c == '›' {
                    depth = depth.saturating_sub(1);
                } else if c == '}' && depth == 0 {
                    end = Some(k);
                    break;
                } else if c == '\n' && cs.get(k + 1) == Some(&'\n') {
                    break;
                }
                k += 1;
            }
            if let Some(e) = end {
                if !inside(i, e + 1) && e > body {
                    // (its quotes or cartouche around the argument hidden too)
                    let (mut s0, mut e0) = (body, e);
                    let mut hide: Vec<usize> = (i..body).chain([e]).collect();
                    if (cs[s0] == '"' && cs[e0 - 1] == '"' && e0 - 1 > s0) || (cs[s0] == '‹' && cs[e0 - 1] == '›' && e0 - 1 > s0) {
                        hide.push(s0);
                        hide.push(e0 - 1);
                        s0 += 1;
                        e0 -= 1;
                    }
                    out.push(Markup { hide, style: (s0, e0, Hl::Sem(Sem::Free)) });
                }
                i = e + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A LaTeX command's Unicode character, through Isabelle's symbols (`le` is `\<le>`, `≤`)
fn latex_char(name: &str, table: &dyn Codec) -> Option<char> {
    let isabelle = super::symbols::LATEX.iter().find(|(_, names)| names.contains(&name)).map_or(name, |(n, _)| *n);
    let decoded = table.decode(&format!("\\<{isabelle}>")).text;
    let mut chars = decoded.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c != '\\' => Some(c),
        _ => None,
    }
}

/// The formula `s..e` of `src` (delimiters `open` wide) set in Unicode into `out`, where it can be
fn set_math(src: &[char], s: usize, e: usize, open: usize, out: &mut [char], table: &dyn Codec) {
    for k in (s..s + open).chain(e - open..e) {
        out[k] = ZERO_WIDTH;
    }
    let end = e - open;
    let mut k = s + open;
    while k < end {
        let c = src[k];
        if c == '\\' {
            let mut j = k + 1;
            while j < end && src[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j == k + 1 {
                // \, \; \! and the like: a space, or nothing
                if j < end {
                    out[k] = if matches!(src[j], ',' | ';' | ' ') { ' ' } else { ZERO_WIDTH };
                    out[j] = ZERO_WIDTH;
                }
                k = j + 1;
                continue;
            }
            let name: String = src[k + 1..j].iter().collect();
            // \frac{a}{b} as a⁄b, \sqrt{x} as √x, \text{…} and the like as what they hold
            let group = |at: usize| -> Option<usize> {
                (src.get(at) == Some(&'{')).then(|| {
                    let mut depth = 0;
                    (at..end).find(|&m| {
                        match src[m] {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        depth == 0
                    })
                })?
            };
            let blank = |out: &mut [char], from: usize, to: usize| {
                for slot in &mut out[from..to] {
                    *slot = ZERO_WIDTH;
                }
            };
            match name.as_str() {
                "frac" | "dfrac" | "tfrac" => {
                    if let Some(a) = group(j)
                        && let Some(b) = group(a + 1)
                    {
                        blank(out, k, j + 1);
                        out[a] = '⁄';
                        out[a + 1] = ZERO_WIDTH;
                        out[b] = ZERO_WIDTH;
                        k = j + 1;
                        continue;
                    }
                }
                "sqrt" => {
                    if group(j).is_some() {
                        out[k] = '√';
                        blank(out, k + 1, j + 1);
                        k = j + 1;
                        continue;
                    }
                }
                "text" | "mathrm" | "mathit" | "mathsf" | "mathtt" | "textrm" | "textit" | "operatorname" | "mathbf" => {
                    if group(j).is_some() {
                        blank(out, k, j + 1);
                        k = j + 1;
                        continue;
                    }
                }
                "left" | "right" | "big" | "Big" | "bigl" | "bigr" | "Bigl" | "Bigr" => {
                    blank(out, k, j);
                    k = j;
                    continue;
                }
                _ => {}
            }
            if let Some(ch) = latex_char(&name, table) {
                out[k] = ch;
                for slot in &mut out[k + 1..j] {
                    *slot = ZERO_WIDTH;
                }
            }
            k = j;
            continue;
        }
        if (c == '^' || c == '_') && k + 1 < end {
            let sup = c == '^';
            if src[k + 1] == '{' {
                if let Some(m) = (k + 2..end).find(|&m| src[m] == '}')
                    && src[k + 2..m].iter().all(|&ch| super::symbols::unicode_script(sup, ch).is_some())
                {
                    out[k] = ZERO_WIDTH;
                    out[k + 1] = ZERO_WIDTH;
                    for n in k + 2..m {
                        out[n] = super::symbols::unicode_script(sup, src[n]).unwrap();
                    }
                    out[m] = ZERO_WIDTH;
                    k = m + 1;
                    continue;
                }
            } else if let Some(sc) = super::symbols::unicode_script(sup, src[k + 1]) {
                out[k] = ZERO_WIDTH;
                out[k + 1] = sc;
                k += 2;
                continue;
            }
        }
        // (grouping braces; not one a \frac has already made its slash)
        if (c == '{' || c == '}') && out[k] == c {
            out[k] = ZERO_WIDTH;
        }
        k += 1;
    }
}

/// The code in a paragraph of prose, as it reads lexically: antiquotations (`@{name` and its
/// closing brace as keywords, quoted or cartouche arguments as terms, other arguments as names),
/// maths source (`$` and LaTeX commands, its braces and scripts) and raw LaTeX commands
pub fn prose_code_classes(text: &str) -> Vec<Option<Hl>> {
    let cs: Vec<char> = text.chars().collect();
    let n = cs.len();
    let mut out: Vec<Option<Hl>> = vec![None; n];
    let term = Some(Hl::Sem(Sem::Free));
    let name = Some(Hl::Sem(Sem::TFree));
    let punct = Some(Hl::Sem(Sem::Skolem));
    let mut i = 0;
    while i < n {
        if cs[i] == '@' && cs.get(i + 1) == Some(&'{') {
            let mut j = i + 2;
            while j < n && (cs[j].is_alphanumeric() || cs[j] == '_') {
                j += 1;
            }
            for c in out.iter_mut().take(j).skip(i) {
                *c = Some(Hl::Keyword);
            }
            // its arguments, to the closing brace (quotes and cartouches nest)
            let (mut depth, mut quoted, mut k) = (0usize, false, j);
            while k < n {
                let c = cs[k];
                if quoted {
                    out[k] = term;
                    quoted = c != '"';
                } else if c == '"' {
                    out[k] = term;
                    quoted = true;
                } else if c == '‹' {
                    depth += 1;
                    out[k] = term;
                } else if c == '›' {
                    depth = depth.saturating_sub(1);
                    out[k] = term;
                } else if depth > 0 {
                    out[k] = term;
                } else if c == '}' {
                    out[k] = Some(Hl::Keyword);
                    k += 1;
                    break;
                } else if !c.is_whitespace() {
                    out[k] = name;
                }
                k += 1;
            }
            i = k;
            continue;
        }
        if cs[i] == '$' {
            out[i] = Some(Hl::Keyword);
            let mut k = i + 1;
            while k < n && cs[k] != '$' {
                if cs[k] == '\\' {
                    let mut j = k + 1;
                    while j < n && cs[j].is_ascii_alphabetic() {
                        j += 1;
                    }
                    let j = j.max(k + 2).min(n);
                    for c in out.iter_mut().take(j).skip(k) {
                        *c = Some(Hl::Keyword);
                    }
                    k = j;
                    continue;
                }
                if matches!(cs[k], '{' | '}' | '^' | '_') {
                    out[k] = punct;
                }
                k += 1;
            }
            if k < n {
                out[k] = Some(Hl::Keyword);
            }
            i = k + 1;
            continue;
        }
        if cs[i] == '\\' && cs.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            let mut j = i + 1;
            while j < n && cs[j].is_ascii_alphabetic() {
                j += 1;
            }
            for c in out.iter_mut().take(j).skip(i) {
                *c = Some(Hl::Keyword);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::symbols::Table;
    use super::*;

    #[test]
    fn prose_maths_reads_as_unicode_until_the_caret_is_in_it() {
        let table = Table::fallback();
        let src = "so $x \\le y^2$ holds";
        let away = shown_text_at(src, Some(0), &table);
        assert_eq!(away.chars().count(), src.chars().count());
        assert_eq!(away.replace('\u{200B}', ""), "so x ≤ y² holds");
        let inside = shown_text_at(src, Some(6), &table);
        assert_eq!(inside, src);
        let frac = "half is $\\frac{1}{2}$ and root $\\sqrt{x}$, $\\text{if } a$";
        let shown = shown_text_at(frac, None, &table);
        assert_eq!(shown.chars().count(), frac.chars().count());
        assert_eq!(shown.replace('\u{200B}', ""), "half is 1⁄2 and root √x, if  a");
    }

    #[test]
    fn code_in_prose_reads_as_code() {
        let text = "so @{term \"x + y\"} by @{thm foo_def}, see \\cite{Nipkow} and $x \\le y^2$ here";
        let c = prose_code_classes(text);
        let at = |s: &str| text[..text.find(s).unwrap()].chars().count();
        assert_eq!(c[at("@{term")], Some(Hl::Keyword));
        assert_eq!(c[at("x + y")], Some(Hl::Sem(Sem::Free)), "the term");
        assert_eq!(c[at("foo_def")], Some(Hl::Sem(Sem::TFree)), "the theorem's name");
        assert_eq!(c[at("\\cite")], Some(Hl::Keyword));
        assert_eq!(c[at("\\le")], Some(Hl::Keyword), "a LaTeX command in maths");
        assert_eq!(c[at("^2")], Some(Hl::Sem(Sem::Skolem)), "a superscript in maths");
        assert_eq!(c[at("so")], None, "prose stays prose");
        assert_eq!(c[at("here")], None);
    }
}

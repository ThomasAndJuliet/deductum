// SPDX-License-Identifier: GPL-3.0-only

//! Code coloured by its language: syntect's grammars (Sublime Text's) find
//! each token's scope, and the scope a [`Class`], which the page's colours
//! give a colour. Coloured by class, not by a syntect theme, code takes the
//! desktop's light or dark look like the text around it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, LazyLock, Mutex};

use cosmic::iced::Color;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::rich::Typography;

/// What a token is, as far as its colour goes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Keyword,
    Type,
    Function,
    String,
    Number,
    Constant,
    Comment,
    Tag,
    Attribute,
    Heading,
    /// A diff's added and removed lines
    Inserted,
    Deleted,
}

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

type Tokens = Arc<Vec<(Range<usize>, Class)>>;

static CACHE: LazyLock<Mutex<HashMap<u64, Tokens>>> = LazyLock::new(Default::default);

/// The grammar for a code block's language, by its name or an extension
fn syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let lang = lang.trim().to_lowercase();
    if lang.is_empty() {
        return None;
    }
    let name = match lang.as_str() {
        "sh" | "shell" | "zsh" | "fish" | "ksh" | "console" | "shell-session" | "shellsession" => {
            "bash"
        }
        "py" | "python3" | "py3" | "pyw" => "python",
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "node" => "js",
        "ts" | "typescript" | "tsx" | "mts" | "cts" => "js",
        "json5" | "jsonc" => "json",
        "yml" => "yaml",
        "c++" | "cxx" | "hpp" | "cc" => "cpp",
        "cs" | "csharp" | "c#" => "cs",
        "rs" => "rust",
        "rb" => "ruby",
        "pl" | "pm" => "perl",
        "golang" => "go",
        "md" | "markdown" => "md",
        "tex" | "latex" => "tex",
        "hs" => "haskell",
        "kt" | "kotlin" => "java",
        "ml" | "ocaml" | "sml" => "ml",
        "objc" | "objective-c" => "m",
        "make" | "mk" | "mak" => "makefile",
        "patch" => "diff",
        "htm" | "xhtml" => "html",
        "svg" | "xsd" | "xsl" | "plist" => "xml",
        "bat" | "batch" | "cmd" => "bat",
        "elisp" | "emacs-lisp" | "el" => "lisp",
        "clj" | "cljs" => "clojure",
        other => other,
    };
    SYNTAXES
        .find_syntax_by_token(name)
        .or_else(|| SYNTAXES.find_syntax_by_extension(name))
}

/// Whether `lang` names a language that is coloured
pub fn knows(lang: &str) -> bool {
    syntax(lang).is_some()
}

/// The language a block's first line names, for a block without a label:
/// a `#!` line's interpreter, `<?xml`, `<!DOCTYPE html>`. Nothing else is
/// guessed from the content: a block without a label or a clue stays plain
pub fn clue(code: &str) -> Option<String> {
    let first = code.trim_start_matches('\u{feff}').lines().next()?.trim();
    let lower = first.to_lowercase();
    if lower.starts_with("<?xml") {
        return Some("xml".into());
    }
    if lower.starts_with("<!doctype html") {
        return Some("html".into());
    }
    let line = first.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        // `env -S python3 -u`: the first word that is not an option
        program = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
    }
    // python3.12 is python, and so on
    let name = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let lang = match name {
        "sh" | "bash" | "zsh" | "ksh" | "dash" | "ash" | "fish" => "bash",
        "python" | "pypy" => "python",
        "node" | "nodejs" | "deno" | "bun" => "js",
        "perl" => "perl",
        "ruby" => "ruby",
        "php" => "php",
        "lua" | "luajit" => "lua",
        "Rscript" => "r",
        "tclsh" | "wish" => "tcl",
        "make" => "makefile",
        other => other,
    };
    knows(lang).then(|| lang.to_string())
}

/// The language a block is coloured as: its label, or with none, the clue
/// on its first line; empty for plain
pub fn language(label: &str, code: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        clue(code).unwrap_or_default()
    } else {
        label.to_string()
    }
}

/// What "Colour as…" offers an unlabelled block: each language's name and
/// the label it is given
pub const COLOUR_AS: [(&str, &str); 18] = [
    ("Bash", "bash"),
    ("C", "c"),
    ("C++", "cpp"),
    ("CSS", "css"),
    ("Diff", "diff"),
    ("Go", "go"),
    ("HTML", "html"),
    ("Java", "java"),
    ("JavaScript", "js"),
    ("JSON", "json"),
    ("LaTeX", "latex"),
    ("Markdown", "markdown"),
    ("Python", "python"),
    ("Ruby", "ruby"),
    ("Rust", "rust"),
    ("SQL", "sql"),
    ("XML", "xml"),
    ("YAML", "yaml"),
];

/// The class of the innermost scope in `stack` that has one
fn class_of(stack: &ScopeStack) -> Option<Class> {
    static PREFIXES: LazyLock<Vec<(Scope, Class)>> = LazyLock::new(|| {
        [
            ("comment", Class::Comment),
            ("string", Class::String),
            ("constant.numeric", Class::Number),
            ("constant.character.escape", Class::Constant),
            ("constant.language", Class::Constant),
            ("constant", Class::Constant),
            ("keyword.operator", Class::Keyword),
            ("keyword", Class::Keyword),
            ("storage.type", Class::Keyword),
            ("storage.modifier", Class::Keyword),
            ("storage", Class::Keyword),
            ("entity.name.function", Class::Function),
            ("support.function", Class::Function),
            ("variable.function", Class::Function),
            ("meta.function-call", Class::Function),
            ("entity.name.type", Class::Type),
            ("entity.name.class", Class::Type),
            ("entity.name.struct", Class::Type),
            ("entity.name.enum", Class::Type),
            ("entity.name.trait", Class::Type),
            ("support.type", Class::Type),
            ("support.class", Class::Type),
            ("entity.name.tag", Class::Tag),
            ("entity.other.attribute-name", Class::Attribute),
            ("support.type.property-name", Class::Attribute),
            ("variable.other.member", Class::Attribute),
            ("markup.inserted", Class::Inserted),
            ("markup.deleted", Class::Deleted),
            ("meta.diff.header", Class::Comment),
            ("meta.diff.range", Class::Comment),
            ("markup.heading", Class::Heading),
            ("markup.bold", Class::Heading),
        ]
        .into_iter()
        .filter_map(|(s, c)| Some((Scope::new(s).ok()?, c)))
        .collect()
    });
    for scope in stack.as_slice().iter().rev() {
        for (prefix, class) in PREFIXES.iter() {
            if prefix.is_prefix_of(*scope) {
                // An operator is barely a keyword: left plain
                if scope.build_string().starts_with("keyword.operator") {
                    return None;
                }
                return Some(*class);
            }
        }
    }
    None
}

/// The coloured tokens of `code`, in `lang`: byte ranges and their classes,
/// in order. Without a label, as its first line's clue says ([`language`]);
/// empty for a language without a grammar
pub fn tokens(code: &str, lang: &str) -> Tokens {
    let key = {
        let mut hasher = std::hash::DefaultHasher::new();
        (code, lang).hash(&mut hasher);
        hasher.finish()
    };
    if let Some(found) = CACHE.lock().unwrap().get(&key) {
        return found.clone();
    }
    let lang = language(lang, code);
    let tokens = Arc::new(syntax(&lang).map_or_else(Vec::new, |syntax| parse(code, syntax)));
    let mut cache = CACHE.lock().unwrap();
    if cache.len() > 2048 {
        cache.clear();
    }
    cache.insert(key, tokens.clone());
    tokens
}

fn parse(code: &str, syntax: &SyntaxReference) -> Vec<(Range<usize>, Class)> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out: Vec<(Range<usize>, Class)> = Vec::new();
    let mut push = |range: Range<usize>, class: Option<Class>| {
        let Some(class) = class else { return };
        if range.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, c)) if *c == class && last.end == range.start => last.end = range.end,
            _ => out.push((range, class)),
        }
    };
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        let Ok(ops) = state.parse_line(line, &SYNTAXES) else {
            break;
        };
        let mut from = 0;
        for (offset, op) in ops {
            push(at + from..at + offset, class_of(&stack));
            let _ = stack.apply(&op);
            from = offset;
        }
        push(at + from..at + line.len(), class_of(&stack));
        at += line.len();
    }
    out
}

/// A class's colour on a page of typography `t`
pub fn color(class: Class, t: &Typography) -> Color {
    let dark = t.page.r * 0.299 + t.page.g * 0.587 + t.page.b * 0.114 < 0.5;
    let rgb = |hex: u32| Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    match (class, dark) {
        (Class::Comment, _) => t.muted,
        (Class::Keyword, true) => rgb(0xc792ea),
        (Class::Keyword, false) => rgb(0x7a2fd6),
        (Class::Type, true) => rgb(0xffcb6b),
        (Class::Type, false) => rgb(0x7f5500),
        (Class::Function, true) => rgb(0x82aaff),
        (Class::Function, false) => rgb(0x1a5aa8),
        (Class::String, true) => rgb(0xc3e88d),
        (Class::String, false) => rgb(0x2a6e2e),
        (Class::Number | Class::Constant, true) => rgb(0xf78c6c),
        (Class::Number | Class::Constant, false) => rgb(0xa64500),
        (Class::Tag, true) => rgb(0xf07178),
        (Class::Tag, false) => rgb(0xb01a25),
        (Class::Attribute, true) => rgb(0xffcb6b),
        (Class::Attribute, false) => rgb(0x7f5500),
        (Class::Heading, _) => t.link_strong,
        (Class::Inserted, _) => t.added,
        (Class::Deleted, _) => t.removed,
    }
}

/// Whether a class is set in italic
pub fn italic(class: Class) -> bool {
    class == Class::Comment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_is_coloured() {
        let code = "fn main() {\n    // hi\n    let x = \"s\" + 42;\n}\n";
        let tokens = tokens(code, "rust");
        let class_of = |word: &str| {
            let at = code.find(word).unwrap();
            tokens
                .iter()
                .find(|(r, _)| r.start <= at && at < r.end)
                .map(|(_, c)| *c)
        };
        assert_eq!(class_of("fn"), Some(Class::Keyword), "{tokens:?}");
        assert_eq!(class_of("main"), Some(Class::Function));
        assert_eq!(class_of("// hi"), Some(Class::Comment));
        assert_eq!(class_of("\"s\""), Some(Class::String));
        assert_eq!(class_of("42"), Some(Class::Number));
    }

    fn class_at(code: &str, lang: &str, word: &str) -> Option<Class> {
        let at = code.find(word).unwrap();
        tokens(code, lang)
            .iter()
            .find(|(r, _)| r.start <= at && at < r.end)
            .map(|(_, c)| *c)
    }

    #[test]
    fn the_usual_aliases_are_known() {
        for lang in [
            "py",
            "python",
            "python3",
            "sh",
            "bash",
            "zsh",
            "shell",
            "console",
            "rs",
            "rust",
            "ts",
            "tsx",
            "typescript",
            "js",
            "jsx",
            "javascript",
            "json",
            "yaml",
            "yml",
            "html",
            "xml",
            "css",
            "c",
            "h",
            "cpp",
            "c++",
            "cs",
            "csharp",
            "go",
            "golang",
            "java",
            "kt",
            "rb",
            "ruby",
            "pl",
            "perl",
            "php",
            "lua",
            "sql",
            "diff",
            "patch",
            "make",
            "makefile",
            "tex",
            "latex",
            "md",
            "markdown",
            "hs",
            "haskell",
            "ml",
            "ocaml",
            "r",
            "scala",
            "clojure",
            "lisp",
            "bat",
            "erlang",
            "d",
            "tcl",
            "svg",
        ] {
            assert!(knows(lang), "{lang}");
        }
        for (name, label) in COLOUR_AS {
            assert!(knows(label), "Colour as {name}");
        }
    }

    #[test]
    fn a_first_line_names_the_language() {
        let clue_of = |code: &str| clue(code);
        assert_eq!(clue_of("#!/bin/sh\necho hi\n").as_deref(), Some("bash"));
        assert_eq!(clue_of("#!/usr/bin/env bash\n").as_deref(), Some("bash"));
        assert_eq!(
            clue_of("#!/usr/bin/env -S python3 -u\nprint(1)\n").as_deref(),
            Some("python")
        );
        assert_eq!(
            clue_of("#!/usr/bin/python3.12\n").as_deref(),
            Some("python")
        );
        assert_eq!(clue_of("#!/usr/bin/env node\n").as_deref(), Some("js"));
        assert_eq!(clue_of("#!/usr/bin/perl -w\n").as_deref(), Some("perl"));
        assert_eq!(
            clue_of("<?xml version=\"1.0\"?>\n<a/>\n").as_deref(),
            Some("xml")
        );
        assert_eq!(
            clue_of("<!DOCTYPE html>\n<html></html>\n").as_deref(),
            Some("html")
        );
        assert_eq!(clue_of("<!doctype html>\n").as_deref(), Some("html"));
        // An interpreter without a grammar, and no clue at all
        assert_eq!(clue_of("#!/usr/bin/env no-such-shell\n"), None);
        assert_eq!(clue_of("fn main() {}\n"), None);
        assert_eq!(clue_of("<html>\n"), None);
        assert_eq!(clue_of(""), None);
    }

    #[test]
    fn unlabelled_code_is_coloured_by_its_clue_only() {
        let script = "#!/bin/bash\necho \"hi\"\n";
        assert_eq!(class_at(script, "", "\"hi\""), Some(Class::String));
        assert_eq!(class_at(script, "", "#!"), Some(Class::Comment));
        // Unmistakable Rust, but unlabelled: never guessed
        assert!(tokens("fn main() {\n    let x = \"s\";\n}\n", "").is_empty());
        // A label wins over a clue
        assert_eq!(language("text", "#!/bin/sh\n"), "text");
        assert!(tokens("#!/bin/sh\necho \"hi\"\n", "text").is_empty());
        let page = "<!DOCTYPE html>\n<p class=\"x\">hi</p>\n";
        assert_eq!(class_at(page, "", "p class"), Some(Class::Tag));
        assert_eq!(class_at(page, "", "class"), Some(Class::Attribute));
    }

    #[test]
    fn diffs_are_added_and_removed() {
        let diff = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n same\n";
        assert_eq!(class_at(diff, "diff", "old"), Some(Class::Deleted));
        assert_eq!(class_at(diff, "patch", "new"), Some(Class::Inserted));
        assert_eq!(class_at(diff, "diff", "@@"), Some(Class::Comment));
        assert_eq!(class_at(diff, "diff", "same"), None);
    }

    fn typography(page: Color, text: Color) -> Typography {
        // The theme's accent and success/destructive text, as it gives them
        // for a light page or a dark one
        let on_dark = page.r < 0.5;
        let pick = |light: u32, dark: u32| {
            let hex = if on_dark { dark } else { light };
            Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        };
        Typography {
            display_start: false,
            body: String::new(),
            mono: String::new(),
            size: 16.0,
            line_height: 1.5,
            math_scale: 1.0,
            justify: false,
            italic: false,
            text,
            strong: text,
            muted: Color { a: 0.55, ..text },
            link: pick(0x006080, 0x82c8e0),
            link_strong: pick(0x005070, 0x9ad4e8),
            code: text,
            code_background: page,
            page,
            rule: text,
            selection: text,
            error: Color::from_rgb8(0xc0, 0x1c, 0x28),
            added: pick(0x2e7d32, 0x8fd694),
            removed: pick(0xc01c28, 0xf28b82),
            image_width: 600.0,
        }
    }

    #[test]
    fn colours_come_from_the_page_by_class() {
        let light = typography(
            Color::from_rgb8(0xee, 0xee, 0xee),
            Color::from_rgb8(0x55, 0x55, 0x55),
        );
        let dark = typography(
            Color::from_rgb8(0x1b, 0x1b, 0x1b),
            Color::from_rgb8(0xdd, 0xdd, 0xdd),
        );
        let all = [
            Class::Keyword,
            Class::Type,
            Class::Function,
            Class::String,
            Class::Number,
            Class::Constant,
            Class::Comment,
            Class::Tag,
            Class::Attribute,
            Class::Heading,
            Class::Inserted,
            Class::Deleted,
        ];
        let lightness = |c: Color| c.r * 0.299 + c.g * 0.587 + c.b * 0.114;
        for class in all {
            // Readable on its page: dark on light, light on dark
            assert!(lightness(color(class, &light)) < 0.55, "{class:?} on light");
            assert!(lightness(color(class, &dark)) > 0.45, "{class:?} on dark");
        }
        // The theme's own colours where it has them
        assert_eq!(color(Class::Comment, &light), light.muted);
        assert_eq!(color(Class::Heading, &dark), dark.link_strong);
        assert_eq!(color(Class::Inserted, &light), light.added);
        assert_eq!(color(Class::Deleted, &dark), dark.removed);
        // The main classes tell apart
        let main = [
            Class::Keyword,
            Class::Type,
            Class::Function,
            Class::String,
            Class::Number,
            Class::Tag,
        ];
        for (i, a) in main.iter().enumerate() {
            for b in &main[i + 1..] {
                assert_ne!(color(*a, &light), color(*b, &light), "{a:?} {b:?}");
                assert_ne!(color(*a, &dark), color(*b, &dark), "{a:?} {b:?}");
            }
        }
        assert!(italic(Class::Comment) && !italic(Class::Keyword));
    }

    #[test]
    fn unknown_languages_are_plain() {
        assert!(tokens("whatever", "no-such-language").is_empty());
        assert!(tokens("x = 1", "").is_empty());
        assert!(knows("py") && knows("sh") && knows("rs") && !knows("isabelle-ish"));
    }
}

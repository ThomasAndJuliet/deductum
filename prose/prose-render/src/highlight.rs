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
        "sh" | "shell" | "zsh" | "fish" | "console" => "bash",
        "py" | "python3" => "python",
        "js" | "javascript" | "jsx" | "mjs" => "js",
        "ts" | "typescript" | "tsx" => "js",
        "yml" => "yaml",
        "c++" | "cxx" | "hpp" => "cpp",
        "rs" => "rust",
        "md" | "markdown" => "md",
        "tex" | "latex" => "tex",
        "hs" => "haskell",
        "kt" | "kotlin" => "java",
        "ml" | "ocaml" => "ml",
        "sml" => "ml",
        "objc" => "m",
        other => other,
    };
    SYNTAXES.find_syntax_by_token(name).or_else(|| SYNTAXES.find_syntax_by_extension(name))
}

/// Whether `lang` names a language that is coloured
pub fn knows(lang: &str) -> bool {
    syntax(lang).is_some()
}

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
/// in order. Empty for a language without a grammar
pub fn tokens(code: &str, lang: &str) -> Tokens {
    let key = {
        let mut hasher = std::hash::DefaultHasher::new();
        (code, lang).hash(&mut hasher);
        hasher.finish()
    };
    if let Some(found) = CACHE.lock().unwrap().get(&key) {
        return found.clone();
    }
    let tokens = Arc::new(syntax(lang).map_or_else(Vec::new, |syntax| parse(code, syntax)));
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
        let Ok(ops) = state.parse_line(line, &SYNTAXES) else { break };
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
        (Class::Keyword, false) => rgb(0x8839ef),
        (Class::Type, true) => rgb(0xffcb6b),
        (Class::Type, false) => rgb(0x9a6700),
        (Class::Function, true) => rgb(0x82aaff),
        (Class::Function, false) => rgb(0x1a5fb4),
        (Class::String, true) => rgb(0xc3e88d),
        (Class::String, false) => rgb(0x2e7d32),
        (Class::Number | Class::Constant, true) => rgb(0xf78c6c),
        (Class::Number | Class::Constant, false) => rgb(0xc45500),
        (Class::Tag, true) => rgb(0xf07178),
        (Class::Tag, false) => rgb(0xc01c28),
        (Class::Attribute, true) => rgb(0xffcb6b),
        (Class::Attribute, false) => rgb(0x9a6700),
        (Class::Heading, _) => t.link,
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
            tokens.iter().find(|(r, _)| r.start <= at && at < r.end).map(|(_, c)| *c)
        };
        assert_eq!(class_of("fn"), Some(Class::Keyword), "{tokens:?}");
        assert_eq!(class_of("main"), Some(Class::Function));
        assert_eq!(class_of("// hi"), Some(Class::Comment));
        assert_eq!(class_of("\"s\""), Some(Class::String));
        assert_eq!(class_of("42"), Some(Class::Number));
    }

    #[test]
    fn unknown_languages_are_plain() {
        assert!(tokens("whatever", "no-such-language").is_empty());
        assert!(tokens("x = 1", "").is_empty());
        assert!(knows("py") && knows("sh") && knows("rs") && !knows("isabelle-ish"));
    }
}

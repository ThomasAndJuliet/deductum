//! Spelling and grammar checking, backed by Harper, for chat messages and
//! for Markdown documents.
//!
//! Harper's linters are not cheap to build and a check can take a few
//! milliseconds on a long message, so all work runs off the caller's thread,
//! on one dedicated thread owned by [`SpellService`] (which spreads the first
//! check of a long document over a few helper threads). Callers send a check and await the reply; when
//! checks queue up faster than they finish, only the newest one is run and the
//! older callers see [`None`].
//!
//! Syntax is masked out before checking so it is never reported: Discord's
//! (code, links, mentions, custom emoji, `:shortcodes:`) by [`Checker::check`],
//! or Markdown's (everything but prose) by [`Checker::check_markdown`].

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::{FstDictionary, MergedDictionary, MutableDictionary};
use harper_core::{Dialect, DictWordMetadata, Document};
use tokio::sync::oneshot;

mod markdown;
mod mask;

pub use markdown::mask_markdown;
pub use mask::mask_discord_syntax;

/// The English variety the checker accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variety {
    #[default]
    British,
    American,
    Canadian,
    Australian,
    Indian,
}

impl Variety {
    pub const ALL: [Variety; 5] = [
        Self::British,
        Self::American,
        Self::Canadian,
        Self::Australian,
        Self::Indian,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::British => "British English",
            Self::American => "American English",
            Self::Canadian => "Canadian English",
            Self::Australian => "Australian English",
            Self::Indian => "Indian English",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::British => "en-GB",
            Self::American => "en-US",
            Self::Canadian => "en-CA",
            Self::Australian => "en-AU",
            Self::Indian => "en-IN",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.key() == key)
            .unwrap_or_default()
    }

    fn dialect(self) -> Dialect {
        match self {
            Self::British => Dialect::British,
            Self::American => Dialect::American,
            Self::Canadian => Dialect::Canadian,
            Self::Australian => Dialect::Australian,
            Self::Indian => Dialect::Indian,
        }
    }
}

/// What the checker reports beyond spelling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Strictness {
    /// Spelling only.
    Spelling,
    /// Spelling, typos and clear grammar mistakes; no style advice. Suits chat.
    #[default]
    Light,
    /// Spelling and grammar, as for a document: sentence capitals, spacing and
    /// quotes too; no style advice.
    Grammar,
    /// Everything Harper checks, including style and readability.
    Full,
}

impl Strictness {
    pub const ALL: [Strictness; 4] = [Self::Spelling, Self::Light, Self::Grammar, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spelling => "Spelling only",
            Self::Light => "Spelling and light grammar",
            Self::Grammar => "Spelling and grammar",
            Self::Full => "Spelling, grammar and style",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Spelling => "spelling",
            Self::Light => "light",
            Self::Grammar => "grammar",
            Self::Full => "full",
        }
    }

    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.key() == key)
            .unwrap_or_default()
    }
}

/// The checker's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub variety: Variety,
    pub strictness: Strictness,
    /// Words the user added to their dictionary.
    pub words: Vec<String>,
}

/// How serious an issue is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    /// A misspelling or typo.
    Spelling,
    /// Grammar, word choice or punctuation.
    Grammar,
    /// Style advice (only with [`Strictness::Full`]).
    Style,
}

/// One problem found in the text, with byte offsets into the checked text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
    /// The text the issue covers.
    pub text: String,
    /// Replacements for `start..end`, best first.
    pub replacements: Vec<String>,
}

impl Issue {
    /// `text` with replacement `index` applied, or `None` if there is no such
    /// replacement or the range no longer fits `text`.
    pub fn apply(&self, text: &str, index: usize) -> Option<String> {
        let replacement = self.replacements.get(index)?;
        if self.end > text.len()
            || !text.is_char_boundary(self.start)
            || !text.is_char_boundary(self.end)
        {
            return None;
        }
        let mut out = String::with_capacity(text.len() + replacement.len());
        out.push_str(&text[..self.start]);
        out.push_str(replacement);
        out.push_str(&text[self.end..]);
        Some(out)
    }

    /// Whether the issue is a single word Harper does not know, which the user
    /// can add to their dictionary.
    pub fn is_unknown_word(&self) -> bool {
        self.severity == Severity::Spelling
            && !self.text.is_empty()
            && self
                .text
                .chars()
                .all(|c| c.is_alphanumeric() || c == '\'' || c == '-')
    }
}

/// A synchronous checker. [`SpellService`] runs one on its own thread; tests
/// and tools can use it directly.
pub struct Checker {
    settings: Settings,
    dictionary: Arc<MergedDictionary>,
    linter: LintGroup,
    /// A Markdown document's paragraphs and their issues (offsets relative
    /// to the paragraph), by [`paragraph_key`]
    paragraphs: std::collections::HashMap<u64, Paragraph>,
    /// Checkers with the same settings for spreading a check over threads,
    /// made when first needed
    helpers: Vec<Checker>,
}

/// A checked paragraph of a Markdown document: its text and masked text
/// (compared on every hit, so a hash collision cannot return wrong issues)
/// and its issues, paragraph-relative
struct Paragraph {
    text: Box<str>,
    masked: Box<str>,
    issues: Vec<Issue>,
}

/// How far a check has got, and a way to stop it: shared with whoever
/// waits for it (a progress bar and its Cancel)
#[derive(Debug, Default)]
pub struct Watch {
    /// Paragraphs checked, of `total` to check
    pub done: std::sync::atomic::AtomicU64,
    pub total: std::sync::atomic::AtomicU64,
    /// Set to stop the check: it ends without a result
    pub cancel: std::sync::atomic::AtomicBool,
}

impl Watch {
    fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Paragraphs to check at once before the check is spread over threads: a
/// paragraph takes about 1-30 ms, so a few edited ones stay on one thread
const PARALLEL_MIN_PARAGRAPHS: usize = 16;

/// How many threads a large check uses: up to 4, leaving a core for the UI
fn check_threads() -> usize {
    thread::available_parallelism()
        .map_or(1, |n| n.get().saturating_sub(1))
        .clamp(1, 4)
}

fn paragraph_key(text: &str, masked: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, masked).hash(&mut h);
    h.finish()
}

// The checker moves to helper threads
const _: fn() = || {
    fn send<T: Send>() {}
    send::<Checker>();
};

impl Checker {
    pub fn new(settings: Settings) -> Self {
        let mut user = MutableDictionary::new();
        for word in &settings.words {
            let word = word.trim();
            if !word.is_empty() {
                user.append_word_str(word, DictWordMetadata::default());
                // Harper matches case-sensitively for proper nouns; accept a
                // lower-case word at the start of a sentence too.
                let capitalised = capitalise(word);
                if capitalised != word {
                    user.append_word_str(&capitalised, DictWordMetadata::default());
                }
            }
        }
        let mut merged = MergedDictionary::new();
        merged.add_dictionary(FstDictionary::curated());
        merged.add_dictionary(Arc::new(user));
        let dictionary = Arc::new(merged);
        let mut linter = LintGroup::new_curated(dictionary.clone(), settings.variety.dialect());
        match settings.strictness {
            Strictness::Spelling => {
                linter.set_all_rules_to(Some(false));
                linter.config.set_rule_enabled("SpellCheck", true);
            }
            Strictness::Light => {
                // Chat is informal: people start sentences in lower case and
                // write fragments. Keep the rules that catch real mistakes.
                for rule in [
                    "SentenceCapitalization",
                    "LongSentences",
                    "Spaces",
                    "UnclosedQuotes",
                    "EllipsisLength",
                ] {
                    linter.config.set_rule_enabled(rule, false);
                }
            }
            Strictness::Grammar | Strictness::Full => {}
        }
        Self {
            settings,
            dictionary,
            linter,
            paragraphs: Default::default(),
            helpers: Vec::new(),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The issues in `text`, a chat message, in order of position.
    pub fn check(&mut self, text: &str) -> Vec<Issue> {
        self.check_masked(text, &mask_discord_syntax(text))
    }

    /// The issues in `text`, a Markdown document, in order of position.
    ///
    /// The document is checked a paragraph at a time (cut at blank lines of
    /// the masked text) and each paragraph's issues are remembered by its
    /// text, so after an edit only the edited paragraphs are checked again.
    /// When many paragraphs are new (the first check of a document), they are
    /// spread over a few checker threads. Rules that would look across a blank
    /// line no longer see past it (about 1 % of issues differ on real text).
    pub fn check_markdown(&mut self, text: &str) -> Vec<Issue> {
        self.check_markdown_watched(text, &Watch::default())
            .unwrap_or_default()
    }

    /// [`Self::check_markdown`], telling `watch` how far it has got (the
    /// paragraphs not checked before, which a first check of a long
    /// document is), and stopping, with no result, when it is told to
    pub fn check_markdown_watched(&mut self, text: &str, watch: &Watch) -> Option<Vec<Issue>> {
        let masked = mask_markdown(text);
        let pieces: Vec<_> = paragraphs(text, &masked)
            .into_iter()
            .filter(|(_, m)| !masked[m.clone()].trim().is_empty())
            .map(|(t, m)| {
                let key = paragraph_key(&text[t.clone()], &masked[m.clone()]);
                (t, m, key)
            })
            .collect();

        // The paragraphs not in the cache (each distinct one once), checked
        let mut seen = std::collections::HashSet::new();
        let missing: Vec<(&str, &str, u64)> = pieces
            .iter()
            .filter(|(t, m, key)| {
                self.cached(*key, &text[t.clone()], &masked[m.clone()])
                    .is_none()
                    && seen.insert(*key)
            })
            .map(|(t, m, key)| (&text[t.clone()], &masked[m.clone()], *key))
            .collect();
        watch
            .total
            .store(missing.len() as u64, std::sync::atomic::Ordering::Relaxed);
        watch.done.store(0, std::sync::atomic::Ordering::Relaxed);
        let checked = self.check_paragraphs(&missing, check_threads(), watch)?;

        let mut keep = std::collections::HashMap::with_capacity(pieces.len());
        for (&(t, m, key), issues) in missing.iter().zip(checked) {
            keep.insert(
                key,
                Paragraph {
                    text: t.into(),
                    masked: m.into(),
                    issues,
                },
            );
        }
        let mut out = Vec::new();
        for (t, m, key) in pieces {
            let (tb, mb) = (&text[t.clone()], &masked[m]);
            if !keep.contains_key(&key)
                && let Some(p) = self.paragraphs.remove(&key)
            {
                keep.insert(key, p);
            }
            let issues = match keep.get(&key) {
                Some(p) if *p.text == *tb && *p.masked == *mb => p.issues.clone(),
                // Two paragraphs with one key: check this one directly
                _ => self.check_masked(tb, mb),
            };
            out.extend(issues.into_iter().map(|i| Issue {
                start: i.start + t.start,
                end: i.end + t.start,
                ..i
            }));
        }
        self.paragraphs = keep;
        Some(out)
    }

    /// The cached issues of a paragraph, if its text and masked text are
    /// exactly the ones cached under `key`
    fn cached(&self, key: u64, text: &str, masked: &str) -> Option<&[Issue]> {
        self.paragraphs
            .get(&key)
            .filter(|p| *p.text == *text && *p.masked == *masked)
            .map(|p| p.issues.as_slice())
    }

    /// The issues of each `(text, masked, _)` paragraph, paragraph-relative,
    /// in order; on several threads when there are many
    fn check_paragraphs(
        &mut self,
        paragraphs: &[(&str, &str, u64)],
        threads: usize,
        watch: &Watch,
    ) -> Option<Vec<Vec<Issue>>> {
        let tick = || {
            watch
                .done
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        };
        if paragraphs.len() < PARALLEL_MIN_PARAGRAPHS || threads < 2 {
            let mut out = Vec::with_capacity(paragraphs.len());
            for (t, m, _) in paragraphs {
                if watch.cancelled() {
                    return None;
                }
                out.push(self.check_masked(t, m));
                tick();
            }
            return Some(out);
        }
        while self.helpers.len() < threads - 1 {
            let settings = self.settings.clone();
            self.helpers.push(Checker::new(settings));
        }
        // Every checker takes the next unchecked paragraph until none are left
        let next = std::sync::atomic::AtomicUsize::new(0);
        let work = |checker: &mut Checker| {
            let mut done = Vec::new();
            loop {
                if watch.cancelled() {
                    break;
                }
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((t, m, _)) = paragraphs.get(i) else {
                    break;
                };
                done.push((i, checker.check_masked(t, m)));
                tick();
            }
            done
        };
        let mut helpers = std::mem::take(&mut self.helpers);
        let results: Vec<(usize, Vec<Issue>)> = thread::scope(|scope| {
            let handles: Vec<_> = helpers
                .iter_mut()
                .map(|helper| scope.spawn(|| work(helper)))
                .collect();
            let mut results = work(self);
            for handle in handles {
                results.extend(
                    handle
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                );
            }
            results
        });
        self.helpers = helpers;
        if watch.cancelled() {
            return None;
        }
        let mut out = vec![Vec::new(); paragraphs.len()];
        for (i, issues) in results {
            out[i] = issues;
        }
        Some(out)
    }
    /// The issues in `text`, checking `masked`: `text` with what is not to be
    /// checked replaced by spaces, char for char.
    pub fn check_masked(&mut self, text: &str, masked: &str) -> Vec<Issue> {
        if masked.trim().is_empty() {
            return Vec::new();
        }
        let chars: Vec<char> = masked.chars().collect();
        // Byte offset of each char index in the original text (masking keeps
        // char positions but may change byte lengths).
        let mut byte_at: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        byte_at.push(text.len());
        let original: Vec<char> = text.chars().collect();

        let document = Document::new_plain_english(&masked, self.dictionary.as_ref());
        let mut issues: Vec<Issue> = self
            .linter
            .lint(&document)
            .into_iter()
            .filter_map(|lint| {
                let severity = severity(lint.lint_kind, self.settings.strictness)?;
                let (s, e) = (
                    lint.span.start.min(chars.len()),
                    lint.span.end.min(chars.len()),
                );
                if s > e {
                    return None;
                }
                // About what was masked out (the space a formula or a mention
                // left), not about the prose
                if (s..e).any(|i| chars[i] != original[i]) {
                    return None;
                }
                let covered: String = original[s..e].iter().collect();
                let replacements = lint
                    .suggestions
                    .iter()
                    .map(|suggestion| match suggestion {
                        Suggestion::ReplaceWith(cs) => cs.iter().collect(),
                        Suggestion::InsertAfter(cs) => {
                            format!("{covered}{}", cs.iter().collect::<String>())
                        }
                        Suggestion::Remove => String::new(),
                    })
                    .filter(|r: &String| *r != covered)
                    .take(6)
                    .collect();
                Some(Issue {
                    start: byte_at[s],
                    end: byte_at[e],
                    severity,
                    message: plain_message(&lint.message),
                    text: covered,
                    replacements,
                })
            })
            .collect();
        issues.sort_by_key(|i| (i.start, i.end));
        issues.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.severity == b.severity);
        issues
    }
}

/// `text` and `masked` (the same chars) cut at blank lines of `masked`: the
/// byte ranges of each paragraph in each
fn paragraphs(text: &str, masked: &str) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let (mut ts, mut ms, mut prev_nl, mut blank) = (0, 0, false, true);
    for ((mb, c), (tb, _)) in masked.char_indices().zip(text.char_indices()) {
        if c == '\n' {
            if blank && prev_nl {
                out.push((ts..tb, ms..mb));
                ts = tb;
                ms = mb;
            }
            prev_nl = true;
            blank = true;
        } else if !c.is_whitespace() {
            blank = false;
        }
    }
    out.push((ts..text.len(), ms..masked.len()));
    out
}

fn severity(kind: LintKind, strictness: Strictness) -> Option<Severity> {
    let s = match kind {
        LintKind::Spelling | LintKind::Typo => Severity::Spelling,
        LintKind::Agreement
        | LintKind::BoundaryError
        | LintKind::Eggcorn
        | LintKind::Grammar
        | LintKind::Malapropism
        | LintKind::Punctuation
        | LintKind::Repetition
        | LintKind::Capitalization
        | LintKind::WordChoice
        | LintKind::Nonstandard
        | LintKind::Usage
        | LintKind::WordOrder
        | LintKind::Miscellaneous => Severity::Grammar,
        _ => Severity::Style,
    };
    match (strictness, s) {
        (Strictness::Spelling, Severity::Spelling) => Some(s),
        (Strictness::Spelling, _) => None,
        (Strictness::Light | Strictness::Grammar, Severity::Style) => None,
        _ => Some(s),
    }
}

/// Harper messages are Markdown; the UI shows plain text.
fn plain_message(message: &str) -> String {
    message.replace('`', "").replace("**", "").replace('*', "")
}

fn capitalise(word: &str) -> String {
    let mut cs = word.chars();
    match cs.next() {
        Some(first) => first.to_uppercase().chain(cs).collect(),
        None => String::new(),
    }
}

/// What a checked text is written in
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syntax {
    Discord,
    Markdown,
}

enum Request {
    Check {
        text: String,
        syntax: Syntax,
        reply: oneshot::Sender<Vec<Issue>>,
        watch: Arc<Watch>,
    },
    Configure(Settings),
}

/// A handle to the checker thread. Cloning shares the thread.
#[derive(Clone)]
pub struct SpellService {
    tx: mpsc::Sender<Request>,
}

impl std::fmt::Debug for SpellService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SpellService")
    }
}

impl SpellService {
    /// Starts the checker thread. The dictionaries load on that thread, so this
    /// returns at once.
    pub fn spawn(settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        thread::Builder::new()
            .name("spell-check".into())
            .spawn(move || run(settings, rx))
            .expect("spawn spell-check thread");
        Self { tx }
    }

    /// Checks `text`. Resolves to `None` if a newer check superseded this one
    /// or the thread is gone.
    pub async fn check(&self, text: String) -> Option<Vec<Issue>> {
        self.request(text, Syntax::Discord).await
    }

    /// Checks `text`, a Markdown document, as [`Self::check`] does a message.
    pub async fn check_markdown(&self, text: String) -> Option<Vec<Issue>> {
        self.request(text, Syntax::Markdown).await
    }

    /// [`Self::check_markdown`], telling `watch` how far it has got and
    /// stopping when it says so (then `None`)
    pub async fn check_markdown_watched(
        &self,
        text: String,
        watch: Arc<Watch>,
    ) -> Option<Vec<Issue>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Request::Check {
                text,
                syntax: Syntax::Markdown,
                reply,
                watch,
            })
            .ok()?;
        rx.await.ok()
    }

    async fn request(&self, text: String, syntax: Syntax) -> Option<Vec<Issue>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Request::Check {
                text,
                syntax,
                reply,
                watch: Arc::default(),
            })
            .ok()?;
        rx.await.ok()
    }

    /// Replaces the settings; the next check uses them.
    pub fn configure(&self, settings: Settings) {
        let _ = self.tx.send(Request::Configure(settings));
    }
}

fn run(settings: Settings, rx: mpsc::Receiver<Request>) {
    let started = std::time::Instant::now();
    let mut checker = Checker::new(settings);
    log::debug!("spell checker ready in {:?}", started.elapsed());
    while let Ok(first) = rx.recv() {
        // Coalesce: apply every queued configuration and run only the newest
        // check; dropping the older replies tells their callers.
        let mut pending_check = None;
        let mut pending_settings = None;
        for request in std::iter::once(first).chain(rx.try_iter()) {
            match request {
                Request::Check {
                    text,
                    syntax,
                    reply,
                    watch,
                } => pending_check = Some((text, syntax, reply, watch)),
                Request::Configure(s) => pending_settings = Some(s),
            }
        }
        if let Some(s) = pending_settings
            && &s != checker.settings()
        {
            checker = Checker::new(s);
        }
        if let Some((text, syntax, reply, watch)) = pending_check
            && !reply.is_closed()
        {
            let issues = match syntax {
                Syntax::Discord => Some(checker.check(&text)),
                Syntax::Markdown => checker.check_markdown_watched(&text, &watch),
            };
            // Stopped: no reply, which tells the caller
            if let Some(issues) = issues {
                let _ = reply.send(issues);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> Checker {
        Checker::new(Settings::default())
    }

    #[test]
    fn finds_misspelling_with_suggestion() {
        let mut c = checker();
        let text = "I recieved your mesage yesterday.";
        let issues = c.check(text);
        let words: Vec<&str> = issues.iter().map(|i| &text[i.start..i.end]).collect();
        assert!(words.contains(&"recieved"), "{issues:?}");
        assert!(words.contains(&"mesage"), "{issues:?}");
        let recieved = issues.iter().find(|i| i.text == "recieved").unwrap();
        assert_eq!(recieved.severity, Severity::Spelling);
        assert!(
            recieved.replacements.iter().any(|r| r == "received"),
            "{recieved:?}"
        );
        assert_eq!(
            recieved
                .apply(
                    text,
                    recieved
                        .replacements
                        .iter()
                        .position(|r| r == "received")
                        .unwrap()
                )
                .unwrap(),
            "I received your mesage yesterday."
        );
    }

    #[test]
    fn british_spelling_is_accepted() {
        let mut c = checker();
        assert!(c.check("My favourite colour is grey.").is_empty());
    }

    #[test]
    fn american_spelling_flagged_in_british() {
        let mut c = checker();
        let issues = c.check("My favorite color is gray.");
        assert!(!issues.is_empty());
    }

    #[test]
    fn light_grammar_catches_article() {
        let mut c = checker();
        let issues = c.check("It was a amazing day.");
        assert!(
            issues.iter().any(|i| i.severity == Severity::Grammar),
            "{issues:?}"
        );
    }

    #[test]
    fn chat_style_lowercase_is_fine() {
        let mut c = checker();
        assert!(
            c.check("sounds good, see you there").is_empty(),
            "{:?}",
            c.check("sounds good, see you there")
        );
    }

    #[test]
    fn discord_syntax_is_ignored() {
        let mut c = checker();
        let text = "hey <@123456789> look at `fooo_barr` and https://exmple.com/qwzx :blobcatt: <:pepge:12345>";
        assert!(c.check(text).is_empty(), "{:?}", c.check(text));
    }

    #[test]
    fn offsets_are_bytes_after_multibyte_text() {
        let mut c = checker();
        let text = "café ☕ is grate, I recieved it";
        let issues = c.check(text);
        let r = issues
            .iter()
            .find(|i| i.text == "recieved")
            .expect("recieved flagged");
        assert_eq!(&text[r.start..r.end], "recieved");
    }

    #[test]
    fn user_words_are_accepted() {
        let mut c = Checker::new(Settings {
            words: vec!["Zorblax".into()],
            ..Settings::default()
        });
        assert!(c.check("Zorblax is here.").is_empty());
        let mut plain = checker();
        assert!(!plain.check("Zorblax is here.").is_empty());
    }

    #[tokio::test]
    async fn service_checks_and_coalesces() {
        let svc = SpellService::spawn(Settings::default());
        let issues = svc.check("teh cat".into()).await.expect("reply");
        assert!(!issues.is_empty());
    }

    #[test]
    fn markdown_prose_is_checked_and_its_syntax_is_not() {
        let mut c = Checker::new(Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        });
        let text = "# Notes\n\nI **recieved** it; see `fooo_barr` and [the sitee](https://exmple.org).\n\n```\nqwzx\n```\n";
        let issues = c.check_markdown(text);
        let words: Vec<&str> = issues.iter().map(|i| &text[i.start..i.end]).collect();
        assert!(words.contains(&"recieved"), "{issues:?}");
        assert!(words.contains(&"sitee"), "{issues:?}");
        assert!(
            !words
                .iter()
                .any(|w| w.contains("fooo") || w.contains("exmple") || w.contains("qwzx")),
            "{issues:?}"
        );
    }

    #[test]
    fn nothing_is_reported_about_what_was_masked() {
        let mut c = Checker::new(Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        });
        let text = "It holds for every real \\(\\xi\\), and for every $x$, too.";
        assert!(
            c.check_markdown(text).is_empty(),
            "{:?}",
            c.check_markdown(text)
        );
    }

    #[test]
    fn paragraph_cache_matches_a_fresh_check_after_an_edit() {
        let mut c = Checker::new(Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        });
        let a = "I recieved it.\n\nThe sitee is fine.\n\n```\nqwzx\n```\n\nAnother mesage here.\n";
        let _ = c.check_markdown(a);
        let b = a.replace("sitee", "site and teh");
        let mut fresh = Checker::new(Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        });
        assert_eq!(c.check_markdown(&b), fresh.check_markdown(&b));
        let words: Vec<&str> = c
            .check_markdown(&b)
            .iter()
            .map(|i| &b[i.start..i.end])
            .collect::<Vec<_>>()
            .into_iter()
            .collect();
        assert!(
            words.contains(&"teh") && words.contains(&"mesage") && words.contains(&"recieved"),
            "{words:?}"
        );
    }

    #[test]
    fn a_check_spread_over_threads_matches_one_thread() {
        let settings = Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        };
        let text: String = (0..PARALLEL_MIN_PARAGRAPHS + 7)
            .map(|i| {
                format!(
                    "Paragraph {i} has a mesage in it, and teh end.\n\n`code {i}` and a sitee.\n\n"
                )
            })
            .collect();
        let masked = mask_markdown(&text);
        let pieces: Vec<_> = paragraphs(&text, &masked)
            .into_iter()
            .map(|(t, m)| {
                (
                    &text[t.clone()],
                    &masked[m.clone()],
                    paragraph_key(&text[t], &masked[m]),
                )
            })
            .collect();
        assert!(pieces.len() >= PARALLEL_MIN_PARAGRAPHS);
        let one = Checker::new(settings.clone())
            .check_paragraphs(&pieces, 1, &Watch::default())
            .unwrap();
        let mut c = Checker::new(settings);
        let three = c.check_paragraphs(&pieces, 3, &Watch::default()).unwrap();
        assert_eq!(c.helpers.len(), 2);
        assert_eq!(one, three);
        assert!(one.iter().flatten().any(|i| i.text == "mesage"));
    }

    #[test]
    fn repeated_paragraphs_are_each_reported() {
        let mut c = Checker::new(Settings::default());
        let text = "A mesage.\n\nA mesage.\n";
        let issues = c.check_markdown(text);
        assert_eq!(
            issues.iter().filter(|i| i.text == "mesage").count(),
            2,
            "{issues:?}"
        );
        assert_eq!(c.check_markdown(text), issues);
    }

    #[test]
    fn grammar_strictness_wants_sentence_capitals() {
        let mut c = Checker::new(Settings {
            strictness: Strictness::Grammar,
            ..Settings::default()
        });
        assert!(!c.check_markdown("the cat sat. it was happy.").is_empty());
    }

    /// A watched check says how far it has got, every new paragraph
    /// counted; told to stop, it stops with no result
    #[test]
    fn a_watched_check_counts_and_stops() {
        let text: String = (0..40)
            .map(|i| format!("Paragraph {i} has a mesage in it.\n\n"))
            .collect();
        let mut c = checker();
        let watch = Watch::default();
        let issues = c.check_markdown_watched(&text, &watch).unwrap();
        assert_eq!(issues.len(), 40);
        let counted = (
            watch.done.load(std::sync::atomic::Ordering::Relaxed),
            watch.total.load(std::sync::atomic::Ordering::Relaxed),
        );
        assert_eq!(counted, (40, 40));
        let mut fresh = checker();
        let stop = Watch::default();
        stop.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(fresh.check_markdown_watched(&text, &stop).is_none());
    }

    /// Benchmark: a whole document's first check, its progress counted,
    /// and how soon Cancel stops it (`cargo test --release -p prose-spell
    /// -- --ignored perf_first_check --nocapture`)
    #[test]
    #[ignore = "a benchmark: cargo test --release -p prose-spell -- --ignored perf_first_check --nocapture"]
    fn perf_first_check() {
        let paragraph = "The quick brown fox jumps over the lazy dog, and then it runs into the forest where the tall trees grow. \
            Their leaves turn in the autumn and fall when the winds come, and the fox finds a mesage written on the bark. ";
        let text: String = (0..2000)
            .map(|i| format!("{i}. {paragraph}{paragraph}\n\n"))
            .collect();
        let mut c = checker();
        let watch = Watch::default();
        let start = std::time::Instant::now();
        let issues = c.check_markdown_watched(&text, &watch).unwrap();
        let whole = start.elapsed();
        let mut again = checker();
        let stop = std::sync::Arc::new(Watch::default());
        let stopper = stop.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            stopper
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            std::time::Instant::now()
        });
        let stopped = again.check_markdown_watched(&text, &stop);
        let returned = std::time::Instant::now();
        let asked = canceller.join().unwrap();
        println!(
            "perf_first_check: {} KB, 2000 paragraphs on {} threads: {:.0} ms, {} issues; Cancel after 200 ms: stopped {:.1} ms after it was asked ({} of 2000 checked), {}",
            text.len() / 1024,
            check_threads(),
            whole.as_secs_f64() * 1000.0,
            issues.len(),
            returned.saturating_duration_since(asked).as_secs_f64() * 1000.0,
            stop.done.load(std::sync::atomic::Ordering::Relaxed),
            if stopped.is_none() {
                "nothing returned"
            } else {
                "finished first"
            },
        );
    }
}

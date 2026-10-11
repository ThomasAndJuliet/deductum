use harper_core::linting::{LintGroup, Linter};
use harper_core::prose_spell::FstDictionary;
use harper_core::{Dialect, Document};
fn main() {
    let dict = FstDictionary::curated();
    let mut lg = LintGroup::new_curated(dict.clone(), Dialect::British);
    for t in std::env::args().skip(1) {
        let doc = Document::new_plain_english(&t, dict.as_ref());
        for l in lg.lint(&doc) { println!("{t:?}: {:?} {:?} {:?} {:?}", l.lint_kind, l.span, l.message, l.suggestions.len()); }
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! A proof being edited, with Isabelle's token classes in it as the prover reports them (keywords,
//! free, bound and fixed variables, a comment), drawn headless in both themes: the frames for
//! reviewing the code colours. `CT_SNAPSHOTS=<dir>` keeps them (`code-colours-*`). It uses only
//! the demo and the harness, so it can be run against an earlier commit for a "before".

use cosmic::iced::Size;

use super::editor::{HighlightData, HighlightSettings, Hl, Mode, Sem, next_generation};
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;

const TEXT: &str = "lemma map_tock_length: \"length (map Tock Xs) = length Xs\"\nproof (induct Xs)\n  (* the empty list is simp's *)\n  case (Cons X Xs)\n  then show ?case by (simp add: map_def)\nqed simp";

/// Which text is which class
const CLASSES: &[(&str, Sem)] = &[
    ("lemma", Sem::Keyword1),
    ("proof", Sem::Keyword1),
    ("case", Sem::Keyword1),
    ("then", Sem::Keyword3),
    ("show", Sem::Keyword1),
    ("by", Sem::Keyword1),
    ("qed", Sem::Keyword1),
    ("add:", Sem::QuasiKeyword),
    ("map_tock_length", Sem::Free),
    ("Xs\"", Sem::Free),
    ("map Tock Xs", Sem::Bound),
    ("X ", Sem::Skolem),
    ("?case", Sem::Var),
    ("(* the empty list is simp's *)", Sem::Comment),
];

fn highlight() -> HighlightSettings {
    let mut lines: Vec<Vec<(std::ops::Range<usize>, Hl)>> = Vec::new();
    for line in TEXT.split('\n') {
        let mut spans: Vec<(std::ops::Range<usize>, Hl)> = Vec::new();
        for (needle, sem) in CLASSES {
            let mut from = 0;
            while let Some(at) = line[from..].find(needle) {
                let (s, e) = (from + at, from + at + needle.len());
                // (a keyword is a whole word, and the first class given to a stretch wins)
                let word = needle.chars().all(char::is_alphabetic);
                let before = line[..s].chars().next_back().is_none_or(|c| !c.is_alphanumeric() && c != '_');
                let after = line[e..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
                if (!word || (before && after)) && !spans.iter().any(|(r, _)| r.start < e && s < r.end) {
                    spans.push((s..e, Hl::Sem(*sem)));
                }
                from = e;
            }
        }
        spans.sort_by_key(|(r, _)| r.start);
        lines.push(spans);
    }
    HighlightSettings(std::sync::Arc::new(HighlightData { generation: next_generation(), lines }), false)
}

fn frame(dark: bool, name: &str) {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let i = t.blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let be = t.block_editor.as_mut().expect("an open block");
    be.content = cosmic::widget::text_editor::Content::with_text(TEXT);
    be.preview = None;
    t.block_highlight = highlight();
    let mut h = Harness::new(Size::new(640.0, 460.0));
    cosmic::theme::set_active(if dark { cosmic::theme::ThemeType::Dark } else { crate::tones::light_type_for_tests() });
    h.focus(&app, super::view::block_editor_id());
    let _ = h.draw(&app, &format!("{name}-warm"));
    h.draw(&app, name);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
}

/// The frames of the code colours, light and dark
#[test]
fn code_colours_frames() {
    frame(false, "code-colours-light");
    frame(true, "code-colours-dark");
}

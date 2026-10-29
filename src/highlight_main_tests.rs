// SPDX-License-Identifier: GPL-3.0-only

//! The selection tint changes how a selection is drawn and nothing else: with nothing selected,
//! an editor with the construct's band in it draws exactly the pixels it drew before (household-ui
//! TESTING.md section 5, step 3: the guard against a change nobody asked for). The band lies over
//! the construct's text, tinting it, as it did; only a selection lies over the band.
//!
//! Each frame is the window drawn headless with the caret in a proof and nothing selected, in
//! both themes: the source view, and the block being edited in the literate view, wide (the goal
//! beside it) and narrow (the goal in a gap opened in the editor, which splits the band). It is
//! compared pixel for pixel with the frame `origin/main` (6cacd9c, before the tint, at libcosmic
//! 2de9f343) drew from the same code, in `tests/fixtures/highlight-unselected-main/`, over the
//! editor's bounds less the buttons in them (the pop-out button, and in the narrow scene the
//! proof state's controls, whose chosen tab is meant to change: the muted accent).
//!
//! The reference frames were made by running this file's tests on `origin/main` (the file works
//! there unchanged: it asks nothing of the selection code, nor of household-tones) with
//! `CT_SNAPSHOTS` set to a folder, and keeping the `main-*.png` frames it left (the `-warm` ones
//! are not kept). When a later change to how the editor draws is
//! meant, make them again the same way from the commit before it, and say so in the commit.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use cosmic::iced::advanced::widget::Operation;
use cosmic::iced::advanced::widget::operation::Focusable;
use cosmic::iced::{Rectangle, Size};

use super::editor::Mode;
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;
use super::App;

/// The window's size for each scene
const WIDE: Size = Size::new(1400.0, 900.0);
const NARROW: Size = Size::new(760.0, 900.0);

/// A theory with a proof, the caret in its `have` step, so that the construct the caret is in
/// is tinted
const PROOF: &str = "theory Pop imports Main begin\n\n(* a line before the proof *)\n\nlemma pop: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  show ?thesis\n    using one\n    by simp\nqed\n\nend\n";

fn proof_app(mode: Mode) -> App {
    let commands: Vec<serde_json::Value> =
        super::prover::isabelle::blocks::lexical_commands(PROOF, &HashMap::new()).iter().map(|c| serde_json::json!([c.start, c.end, c.name, "finished"])).collect();
    let prover = serde_json::json!({
        "revision": 1,
        "status": {"finished": commands.len(), "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true},
        "commands": commands,
        "messages": [],
    });
    let doc = serde_json::json!({"text": PROOF, "revision": 1, "prover": prover});
    let mut app = demo_app(mode);
    let theory = super::editor::Theory::new("/tmp/Pop.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), Arc::new(HashMap::new()), mode, crate::prover::profile(None));
    app.workspace.as_mut().unwrap().theory = Some(theory);
    app.show_right = false;
    app.show_sidebar = false;
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = PROOF[..PROOF.find("have one").unwrap()].chars().count() + 3;
    app
}

/// One scene: which view, how wide the window is
#[derive(Clone, Copy)]
enum Scene {
    Source,
    Block,
    BlockNarrow,
}

impl Scene {
    const ALL: [Scene; 3] = [Scene::Source, Scene::Block, Scene::BlockNarrow];

    fn name(self) -> &'static str {
        match self {
            Scene::Source => "source",
            Scene::Block => "block",
            Scene::BlockNarrow => "block-narrow",
        }
    }
}

fn name_of(scene: Scene, dark: bool) -> String {
    format!("main-band-{}-{}", scene.name(), if dark { "dark" } else { "light" })
}

/// The focusable widgets of the window, with their ids, as laid out
#[derive(Default)]
struct Focusables(Vec<(Option<cosmic::widget::Id>, Rectangle)>);

impl Operation<()> for Focusables {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }
    fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle, _: &mut dyn Focusable) {
        self.0.push((id.cloned(), bounds));
    }
}

/// Which pixels are compared: those of the editor, less those of the other focusable widgets
/// over it
struct Compared {
    editor: Rectangle,
    others: Vec<Rectangle>,
}

impl Compared {
    fn contains(&self, x: u32, y: u32) -> bool {
        let p = cosmic::iced::Point::new(x as f32 + 0.5, y as f32 + 0.5);
        self.editor.contains(p) && !self.others.iter().any(|o| o.contains(p))
    }
}

/// The frame of `scene` in a theme: the caret in the proof's `have` step, its editor focused,
/// nothing selected (drawn twice, the first not kept, as the window's widgets and glyph cache
/// catch up with the layout)
fn frame(scene: Scene, dark: bool) -> (Vec<u8>, Compared) {
    let mode = if matches!(scene, Scene::Source) { Mode::Source } else { Mode::Literate };
    let mut app = proof_app(mode);
    let mut h = Harness::new(if matches!(scene, Scene::BlockNarrow) { NARROW } else { WIDE });
    cosmic::theme::set_active(if dark { cosmic::theme::ThemeType::Dark } else { cosmic::theme::ThemeType::Light });
    let id = if matches!(scene, Scene::Source) {
        super::view::source_editor_id()
    } else {
        let ws = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let cursor = ws.cursor;
        let i = ws.blocks.iter().position(|b| b.kind == Kind::Formal && b.start <= cursor && cursor < b.end).expect("the proof's block");
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        super::view::block_editor_id()
    };
    h.focus(&app, id.clone());
    let name = name_of(scene, dark);
    let _ = h.draw(&app, &format!("{name}-warm"));
    let rgba = h.draw(&app, &name);
    let mut found = Focusables::default();
    h.operate(&app, &mut found);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    let editor = found.0.iter().find(|(i, _)| i.as_ref() == Some(&id)).map(|(_, b)| *b).expect("the editor is on the page");
    // (a margin round each button for its focus ring and shadow)
    let others = found.0.iter().filter(|(i, b)| i.as_ref() != Some(&id) && b.intersects(&editor)).map(|(_, b)| b.expand(4.0)).collect();
    (rgba, Compared { editor, others })
}

/// What differs between two frames: how many pixels, the box round them, and the first of them
fn difference(now: &[u8], before: &[u8], width: u32, compared: &Compared) -> Option<String> {
    let (mut count, mut x0, mut y0, mut x1, mut y1) = (0usize, u32::MAX, u32::MAX, 0u32, 0u32);
    let mut first = None;
    for (i, (a, b)) in now.chunks(4).zip(before.chunks(4)).enumerate() {
        let (x, y) = (i as u32 % width, i as u32 / width);
        if a != b && compared.contains(x, y) {
            count += 1;
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            first.get_or_insert((x, y, a.to_vec(), b.to_vec()));
        }
    }
    let (x, y, a, b) = first?;
    Some(format!("{count} pixels differ, within x {x0}..{x1}, y {y0}..{y1}; the first at ({x}, {y}) is {a:02x?} now, {b:02x?} before"))
}

/// Nothing selected, the construct's band and the text under it are drawn as before: every pixel
/// of the source view and of the block being edited, wide and narrow, in both themes
#[test]
fn an_unselected_construct_is_drawn_as_before() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/highlight-unselected-main");
    let mut problems = Vec::new();
    let mut compared = 0;
    for scene in Scene::ALL {
        for dark in [false, true] {
            let name = name_of(scene, dark);
            let size = if matches!(scene, Scene::BlockNarrow) { NARROW } else { WIDE };
            let (now, area) = frame(scene, dark);
            // (the frame has something in it: not all one colour, and an editor of some size)
            assert!(now.chunks(4).any(|p| p != &now[..4]), "{name}: a blank frame");
            assert!(area.editor.width > 200.0 && area.editor.height > 100.0, "{name}: the editor is only {:?}", area.editor);
            let file = dir.join(format!("{name}.png"));
            let Ok(bytes) = std::fs::read(&file) else {
                problems.push(format!("{name}: no reference frame at {}", file.display()));
                continue;
            };
            let before = image::load_from_memory(&bytes).expect("a reference frame").to_rgba8();
            if (before.width(), before.height()) != (size.width as u32, size.height as u32) {
                problems.push(format!("{name}: the reference frame is the wrong size"));
                continue;
            }
            compared += 1;
            if let Some(what) = difference(&now, before.as_raw(), size.width as u32, &area) {
                problems.push(format!("{name}: {what}"));
            }
        }
    }
    assert!(problems.is_empty(), "\n{compared} frames compared with origin/main's\n{}", problems.join("\n"));
    assert!(compared == Scene::ALL.len() * 2, "only {compared} frames compared");
}

// SPDX-License-Identifier: GPL-3.0-only

//! Frames for judging the page's responsive layout: a real theory (named by `CT_SHEET_THEORY`) with
//! a cell open and a real-looking goal, drawn at a grid of window widths with the sidebar closed
//! and open, in the Literate and Source views, and the goal's cases (short, long with several
//! subgoals, no subgoals, the slide-up and a goal taller than its gap). `CT_SNAPSHOTS=<dir>` keeps
//! the frames and `CT_SHEET_TABLE=<file>` the layout's table. Without `CT_SHEET_THEORY` the test
//! does nothing. `CT_SHEET_COLUMNS` fixes the columns the goal's lines are broken at.

use cosmic::iced::Size;
use serde_json::json;

use super::editor::Mode;
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;
use super::App;

/// The goal's cases
#[derive(Clone, Copy)]
enum Case {
    Short,
    Long,
    None,
    /// The caret on a cell's last line, just above a wide paragraph
    Slide,
    /// As the slide, with a goal taller than the gap
    Tall,
    /// "No subgoals left", with the step's note
    NoneLeft,
}

const TERM: &str = "timed_assign_sequence (st\\<^sub>v := f (st\\<^sub>v a)) (a :: ('s, 'e) timed_vars) (b :: ('s, 'e) timed_vars) (cond_srea_apply (?P \\<triangleleft> ?c \\<triangleright>\\<^sub>R ?Q) (?a, ?b)) = (if ?c (st\\<^sub>v ?a) then ?P (?a, ?b) else ?Q (?a, ?b))";

/// Isabelle's breaking of a term at `columns`: at spaces, the continuation indented two
fn broken(term: &str, columns: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for word in term.split(' ') {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= columns => {
                l.push(' ');
                l.push_str(word);
            }
            Some(_) => lines.push(format!("  {word}")),
            None => lines.push(word.to_string()),
        }
    }
    lines.join("\n")
}

fn parts(raw: &[String]) -> Vec<serde_json::Value> {
    raw.iter().map(|r| json!({"kind": "writeln", "raw": r, "text": r, "spans": []})).collect()
}

fn app_for(text: &str, mode: Mode, width: f32, sidebar: bool, case: Case, columns: usize) -> App {
    // (for the slide, the cell has a wide paragraph above it as well as below)
    let para = "We now relate the raw forward equations to the refusal traces of patient deadlock, one lemma at a time, and then state what they do not show. The first of them is about the tick refusal before a tock, which deterministic tockify leaves out, and it is stated for any action, with its alternatives named in the discussion that follows the proof.";
    let text = &if matches!(case, Case::Slide | Case::Tall) {
        // (a short theory: the header, a paragraph, the cell, and the paragraph after it)
        let head = &text[..text.find("lemma raw_execution_stop_iff").expect("the first lemma")];
        let cell = &text[text.find("lemma raw_FE_stop:").expect("a long lemma")..text.find("lemma raw_FR_stop").expect("the next lemma")];
        let after = &text[text.find("text \\<open>These").expect("the closing paragraph")..];
        format!("{head}text \\<open>{para}\\<close>\n\n{cell}{after}")
    } else {
        text.to_string()
    };
    let mut app = demo_app(mode);
    app.show_right = false;
    app.show_sidebar = sidebar;
    app.window_width = width;
    let mut prover = super::snapshot::prover_json(&format!("{text}\n(* by simp *)"));
    prover["messages"] = json!([]);
    let doc = json!({"text": text, "revision": 1, "prover": prover});
    let needle = match case {
        Case::Slide | Case::Tall => "[Ref {Tick}, ObsEvent Tock] \\<in> raw_FE",
        _ => "raw_FE_stop:",
    };
    let at = match case {
        Case::Slide | Case::Tall => text.find("qed\n\ntext \\<open>These").expect("the cell's last qed") + 1,
        _ => text.find("lemma raw_execution_stop_iff").expect("a lemma") + 20,
    };
    let _ = needle;
    let ws = app.workspace.as_mut().unwrap();
    ws.notes.clear();
    let mut theory = super::editor::Theory::new("/tmp/Timed_Circus_Stop_Traces.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), mode, super::prover::profile(None));
    theory.cursor = text[..at].chars().count();
    ws.theory = Some(theory);
    let goal = |n: usize, term: &str| -> Vec<String> {
        let mut raw = vec![format!("#deductum:count {n}")];
        for i in 1..=n {
            raw.push(format!("#deductum:param {i}\nst :: 's"));
            raw.push(format!("#deductum:prem {i}\n{}", broken("ok\\<^sub>v a \\<and> \\<not> wait\\<^sub>v a \\<and> tr\\<^sub>v b = tr\\<^sub>v a @ xs", columns)));
            raw.push(format!("#deductum:concl {i}\n{}", broken(term, columns)));
        }
        raw
    };
    let mut output = json!({"command": {"name": "proof", "status": "finished"}, "messages": [{"kind": "state", "text": "proof (state)"}]});
    ws.goals.messages = match case {
        Case::Short => parts(&["#deductum:count 1".to_string(), "#deductum:concl 1\n?a = ?b".to_string()]),
        Case::Long => parts(&goal(3, TERM)),
        Case::Slide => parts(&goal(1, "ttWF t")),
        Case::Tall => parts(&goal(6, TERM)),
        Case::None => {
            output = json!({"command": {"name": "by", "status": "finished"}, "messages": [{"kind": "writeln", "raw": broken("show timed_assign f is TC1", columns), "text": broken("show timed_assign f is TC1", columns), "spans": []}]});
            Vec::new()
        }
        Case::NoneLeft => parts(&["#deductum:count 0".to_string()]),
    };
    if matches!(case, Case::NoneLeft) {
        ws.goals_prev.messages = parts(&goal(2, "?a = ?b"));
        ws.goals_prev.status = "finished".into();
    }
    ws.output = Some(output);
    // (a cell open at the caret in the Literate view)
    if mode == Mode::Literate {
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let cursor = t.cursor;
        if let Some(i) = t.blocks.iter().position(|b| b.kind == Kind::Formal && b.start <= cursor && cursor <= b.end) {
            let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        }
    }
    app
}

#[test]
fn goal_layout_sheet_frames() {
    let Some(path) = std::env::var_os("CT_SHEET_THEORY") else { return };
    let text = std::fs::read_to_string(path).expect("the theory");
    let fixed = std::env::var("CT_SHEET_COLUMNS").ok().and_then(|c| c.parse::<usize>().ok());
    let mut table = String::from("window sidebar view cell goal column margin\n");
    let mut frames: Vec<(String, f32, bool, Mode, Case)> = Vec::new();
    for w in [1000.0, 1100.0, 1190.0, 1280.0, 1344.0, 1400.0, 1700.0, 1920.0, 2100.0, 2600.0] {
        for sidebar in [false, true] {
            for mode in [Mode::Literate, Mode::Source] {
                frames.push((format!("grid-{w}-{}-{}", if sidebar { "open" } else { "closed" }, if mode == Mode::Literate { "literate" } else { "source" }), w, sidebar, mode, Case::Long));
            }
        }
    }
    for w in [1344.0, 1920.0] {
        for (name, case) in [("short", Case::Short), ("long", Case::Long), ("none", Case::None), ("slide", Case::Slide), ("tall", Case::Tall), ("noneleft", Case::NoneLeft)] {
            frames.push((format!("case-{name}-{w}"), w, false, Mode::Literate, case));
        }
    }
    // the cases with the goal under the cell too: a window with no room for it beside
    frames.push(("case-noneleft-under-880".into(), 880.0, false, Mode::Literate, Case::NoneLeft));
    frames.push(("case-noneleft-under-700".into(), 700.0, false, Mode::Literate, Case::NoneLeft));
    // single column: the goal under the cell, whether for lack of room or Goal beside off
    for (mode, view) in [(Mode::Literate, "literate"), (Mode::Source, "source")] {
        frames.push((format!("single-off-1920-{view}"), 1920.0, false, mode, Case::Long));
        frames.push((format!("single-965-{view}"), 965.0, false, mode, Case::Long));
        frames.push((format!("single-off-1344-open-{view}"), 1344.0, true, mode, Case::Long));
    }
    frames.push(("single-none-1920-literate".into(), 1920.0, false, Mode::Literate, Case::None));
    for light in [true, false] {
        for (name, width, sidebar, mode, case) in &frames {
            super::view::set_goal_beside(!name.starts_with("single-off"));
            let columns = fixed.unwrap_or_else(|| {
                let app = app_for(&text, *mode, *width, *sidebar, *case, 40);
                super::view::goal_columns(&app, true)
            });
            let app = app_for(&text, *mode, *width, *sidebar, *case, columns);
            if light && name.starts_with("grid") {
                let main = width - if *sidebar && *width >= 640.0 { app.left_width + super::sidebar::INSET_SPAN + 6.0 } else { 0.0 };
                let display = super::settings::display();
                let l = super::view::page_layout(main, super::settings::cell_width(), (display.measure > 0.0).then(|| super::view::z(display.measure * display.text_size)), true);
                table.push_str(&format!("{width} {sidebar} {mode:?} {:.1} {} {:.1} {:.1}\n", l.cell, l.goal.map_or("under".to_string(), |g| format!("{g:.1}")), l.column, l.gutter));
            }
            let height = 900.0;
            let mut h = Harness::new(Size::new(*width, height));
            cosmic::theme::set_active(if light { crate::tones::light_type_for_tests() } else { cosmic::theme::ThemeType::Dark });
            super::view::set_goal_beside(!name.starts_with("single-off"));
            let _ = h.draw(&app, "sheet-warm");
            let _ = h.draw(&app, &format!("sheet-{}-{name}", if light { "light" } else { "dark" }));
            cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
        }
    }
    if let Some(file) = std::env::var_os("CT_SHEET_TABLE") {
        std::fs::write(file, table).expect("the table");
    }
}

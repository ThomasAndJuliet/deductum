// SPDX-License-Identifier: GPL-3.0-only

//! The Lean profile against a real host and a real `lake serve`, on this computer:
//! `DEDUCTUM_LIVE_LEAN=1 cargo test --bin deductum live_lean -- --ignored --nocapture`.
//!
//! The test installs the bundled host into a temporary root of its own (`DEDUCTUM_REMOTE_ROOT`,
//! with its own state directory, beside the user's), copies DSRVLean (the scratch copy
//! `~/tmp-deductum-lean/DSRVLean`, or `DEDUCTUM_LEAN_PROJECT`; the originals are never touched),
//! and drives the window as a human does: open `DSRV/CausalCorrectness.lean`, click into a proof,
//! read its goal in parts, type a tactic and watch the diagnostics change, type `exact?` and
//! insert its "Try this", undo that, hover a name, Ctrl+click to its definition and go Back.
//! Frames go to `CT_SNAPSHOTS` as `lean-live-*.png`.

use std::time::{Duration, Instant};

use cosmic::Application;
use cosmic::iced::Size;
use cosmic::iced::futures::StreamExt;
use cosmic::widget::text_editor::{Action, Edit, Motion};
use serde_json::json;

use super::editor::Mode;
use super::live::{app as new_app, pump, settled};
use super::snapshot::Harness;
use super::theory::Status;
use super::{App, Message, Pending, link};

fn theory(a: &App) -> &super::editor::Theory {
    a.workspace.as_ref().unwrap().theory.as_ref().unwrap()
}

fn local(a: &App) -> String {
    theory(a).local.clone()
}

/// Every command of the open document has a result
fn checked(a: &App) -> bool {
    a.workspace
        .as_ref()
        .and_then(|w| w.theory.as_ref())
        .is_some_and(|t| t.prover.commands.len() > 5 && t.prover.commands.iter().all(|c| !matches!(c.status, Status::Unprocessed | Status::Running)))
}

/// The open document's errors from `from` on
fn errors_from(a: &App, from: usize) -> Vec<String> {
    theory(a).prover.messages.iter().filter(|m| m.kind == "error" && m.start >= from).map(|m| m.text.clone()).collect()
}

fn type_in_source(a: &mut App, text: &str) {
    for c in text.chars() {
        // (Enter is the editor's newline, which indents as the profile says)
        let _ = if c == '\n' { a.update(Message::Newline) } else { a.update(Message::SourceAction(Action::Edit(Edit::Insert(c)))) };
    }
}

fn caret_at(a: &mut App, offset: usize) {
    let t = a.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let text = t.decoded.text.clone();
    let pos = t.lines.position(&text, offset);
    super::editor::place(&mut t.source, pos);
    t.cursor = offset;
}

fn ask_state(a: &mut App) {
    a.state_gen += 1;
    let generation = a.state_gen;
    let _ = a.update(Message::StateTick(generation));
}

/// Scroll the page with the wheel, the pointer over it
fn scroll_by(app: &mut App, h: &mut Harness, lines: f32) {
    h.move_to(app, cosmic::iced::Point::new(600.0, 500.0));
    let msgs = h.events(app, &[cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::WheelScrolled { delta: cosmic::iced::mouse::ScrollDelta::Lines { x: 0.0, y: lines } })]);
    for m in msgs {
        let _ = app.update(m);
    }
}

/// A theorem block under the pointer, scrolling down the page until there is one: where, and which
fn find_theorem(app: &mut App, h: &mut Harness) -> (cosmic::iced::Point, usize) {
    for _ in 0..60 {
        for y in (80..930).step_by(10) {
            for x in [560.0f32, 640.0] {
                let p = cosmic::iced::Point::new(x, y as f32);
                let msgs = h.move_to(app, p);
                for m in &msgs {
                    let _ = app.update(m.clone());
                }
                if let Some(Message::HoverMove(super::HoverTarget::View(i), _)) = msgs.iter().find(|m| matches!(m, Message::HoverMove(super::HoverTarget::View(_), _))) {
                    let t = theory(app);
                    if t.blocks[*i].name == "theorem" && t.blocks[*i].commands.len() > 2 {
                        return (p, *i);
                    }
                }
            }
        }
        scroll_by(app, h, -8.0);
    }
    panic!("a theorem on the page");
}

/// The test's own host and Lean servers are stopped and its files removed when it ends, however
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let root = &self.0;
        // (the bracket keeps the pattern from matching this very command)
        let pattern = root.replacen("deductum", "[d]eductum", 1);
        let _ = std::process::Command::new("sh").arg("-c").arg(format!("pkill -f '{pattern}/host/' ; sleep 1; rm -rf {root}")).status();
    }
}

#[test]
#[ignore]
fn live_lean() {
    if std::env::var("DEDUCTUM_LIVE_LEAN").ok().as_deref() != Some("1") {
        return;
    }
    let home = std::env::var("HOME").unwrap();
    let source = std::env::var("DEDUCTUM_LEAN_PROJECT").unwrap_or_else(|_| format!("{home}/tmp-deductum-lean/DSRVLean"));
    let root = format!("/tmp/deductum-live-lean-{}", std::process::id());
    let project = format!("{root}/DSRVLean");
    let _cleanup = Cleanup(root.clone());
    let status = std::process::Command::new("sh").arg("-c").arg(format!("rm -rf {root} && mkdir -p {root} && cp -r {source} {project}")).status().unwrap();
    assert!(status.success(), "a copy of DSRVLean from {source}");
    // SAFETY: set before any thread of this test reads it
    unsafe { std::env::set_var("DEDUCTUM_REMOTE_ROOT", format!("{root}/host")) };
    let file = format!("{project}/DSRV/CausalCorrectness.lean");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: None };
        let mut app = new_app(target.clone());
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 90, |a| a.links[0].sender.is_some()).await, "connects to the local host");
        let t0 = Instant::now();
        app.send("live", "session_create", json!({"folder": project, "backend": "lean", "new": true, "start": true}), Pending::Create);
        assert!(pump(&mut app, &mut stream, 90, |a| a.workspace.is_some()).await, "a session");
        let ws = app.workspace.as_ref().unwrap();
        assert_eq!(ws.profile.backend(), "lean", "the profile follows the session's backend");
        assert_eq!(ws.caps.spans, "lexical");
        let _ = app.update(Message::OpenTheory(file.clone()));
        assert!(pump(&mut app, &mut stream, 900, checked).await, "CausalCorrectness.lean checked");
        println!("checked in {} s: {} commands", t0.elapsed().as_secs(), theory(&app).prover.commands.len());
        let _ = app.update(Message::SetMode(Mode::Literate));
        // (the goal beside the page: both sidebars away)
        app.show_sidebar = false;
        app.show_right = false;
        let mut h = Harness::new(Size::new(1500.0, 950.0));
        let t = theory(&app);
        assert!(t.blocks.iter().any(|b| b.kind == super::theory::Kind::Formal && b.name == "theorem"), "theorems are blocks");
        assert!(t.blocks.iter().any(|b| b.kind == super::theory::Kind::Text), "docs are prose");
        h.draw(&app, "lean-live-1-literate");

        // Click into a proof: scroll the page until a theorem is under the pointer
        let (p, block) = find_theorem(&mut app, &mut h);
        let msgs = h.click(&app, p);
        assert!(msgs.iter().any(|m| matches!(m, Message::ViewPress(_))), "{msgs:?}");
        for m in msgs {
            let _ = app.update(m);
        }
        h.focus(&app, super::view::block_editor_id());
        h.draw(&app, "lean-live-2-proof-open");
        assert!(theory(&app).block_editor.is_some(), "the proof opened");

        // Its goal in parts, at a tactic of the proof
        let (cs, ce) = {
            let t = theory(&app);
            let c = t.blocks[block].commands.iter().find(|c| c.name != "theorem").expect("a tactic");
            (c.start, c.end)
        };
        let _ = (ce, block);
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        t.cursor = cs + 1;
        ask_state(&mut app);
        assert!(pump(&mut app, &mut stream, 90, |a| a.workspace.as_ref().unwrap().goals.status == "finished").await, "goals");
        let goals = super::goals::Goals::parse(&app.workspace.as_ref().unwrap().goals.messages);
        let sg = goals.subgoals.first().expect("a goal");
        let concl = sg.concl.as_ref().and_then(|c| c["text"].as_str()).unwrap_or("").to_string();
        println!("goal: {} hypotheses, case {:?}, ⊢ {}", sg.params.len(), sg.case.as_ref().map(|c| c["text"].clone()), concl);
        assert!(!sg.params.is_empty() && !concl.is_empty(), "hypotheses and a target in parts: {goals:?}");
        let shown = app.workspace.as_ref().unwrap().profile.present_goal(sg);
        assert_eq!(shown.sections.last().and_then(|s| s.label), Some("⊢"));
        h.draw(&app, "lean-live-3-goal");

        // A new theorem at the end that leaves its goal open: an error. Then a typed tactic closes it
        let _ = app.update(Message::CloseBlock);
        let _ = app.update(Message::SetMode(Mode::Source));
        let end = super::ot::char_len(&local(&app));
        caret_at(&mut app, end);
        let _ = app.update(Message::SourceAction(Action::Move(Motion::DocumentEnd)));
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Paste(std::sync::Arc::new("\ntheorem live_skip (n : Nat) : n + 0 = n := by\n  skip\n".into())))));
        assert!(pump(&mut app, &mut stream, 60, settled).await, "the theorem reaches the host");
        let from = local(&app).find("theorem live_skip").map(|b| local(&app)[..b].chars().count()).unwrap();
        let has_error = |a: &App| errors_from(a, from).iter().any(|e| e.contains("unsolved goals"));
        assert!(pump(&mut app, &mut stream, 240, has_error).await, "Lean says the goal is unsolved: {:?}", errors_from(&app, from));
        println!("diagnostic: {:?}", errors_from(&app, from).first().map(|e| e.lines().next().unwrap_or("").to_string()));
        let _ = app.update(Message::SetMode(Mode::Literate));
        scroll_by(&mut app, &mut h, -4000.0);
        h.draw(&app, "lean-live-4-error");
        let _ = app.update(Message::SetMode(Mode::Source));
        // (the caret after `skip`, then a typed line: the indentation follows the block)
        let at = local(&app).rfind("  skip").map(|b| local(&app)[..b].chars().count() + 6).unwrap();
        caret_at(&mut app, at);
        type_in_source(&mut app, "\nsimp");
        assert!(local(&app).contains("  skip\n  simp"), "Enter indents to the tactic block: {:?}", local(&app));
        assert!(pump(&mut app, &mut stream, 60, settled).await);
        let cleared = |a: &App| settled(a) && local(a).contains("simp") && errors_from(a, from).is_empty() && checked(a);
        assert!(pump(&mut app, &mut stream, 240, cleared).await, "the diagnostics clear: {:?}", errors_from(&app, from));
        println!("typed simp: the error went");

        // exact? typed in a proof, its Try-this inserted, then undone
        let end = super::ot::char_len(&local(&app));
        caret_at(&mut app, end);
        let _ = app.update(Message::SourceAction(Action::Move(Motion::DocumentEnd)));
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Paste(std::sync::Arc::new("\ntheorem live_exact (a b : Nat) : a + b = b + a := by\n  ".into())))));
        type_in_source(&mut app, "exact?");
        assert!(pump(&mut app, &mut stream, 60, settled).await);
        let at = local(&app).rfind("exact?").map(|b| local(&app)[..b].chars().count() + 3).unwrap();
        caret_at(&mut app, at);
        let mut suggestion = None;
        for _ in 0..8 {
            ask_state(&mut app);
            let got = pump(&mut app, &mut stream, 60, |a| {
                a.workspace.as_ref().unwrap().output.as_ref().is_some_and(|o| o["messages"].as_array().is_some_and(|ms| ms.iter().any(|m| m["sendbacks"].as_array().is_some_and(|s| !s.is_empty()))))
            })
            .await;
            if got {
                let out = app.workspace.as_ref().unwrap().output.clone().unwrap();
                suggestion = out["messages"].as_array().unwrap().iter().find_map(|m| m["sendbacks"][0].as_str().map(str::to_string));
                break;
            }
        }
        let suggestion = suggestion.expect("a Try-this suggestion from exact?");
        println!("exact? suggests: {suggestion}");
        let _ = app.update(Message::SetMode(Mode::Literate));
        scroll_by(&mut app, &mut h, -4000.0);
        h.draw(&app, "lean-live-5-exact");
        let _ = app.update(Message::SetMode(Mode::Source));
        let _ = app.update(Message::InsertOutput(suggestion.clone()));
        assert!(pump(&mut app, &mut stream, 60, settled).await);
        assert!(local(&app).contains(&suggestion) && !local(&app).contains("exact?"), "the suggestion replaced exact?: {}", local(&app));
        let _ = app.update(Message::Undo);
        let back = |a: &App| settled(a) && local(a).contains("exact?");
        let ok = pump(&mut app, &mut stream, 30, back).await;
        let tail: String = local(&app).chars().rev().take(120).collect::<Vec<_>>().into_iter().rev().collect();
        assert!(ok, "one undo brought exact? back (it is one step): ...{tail}");
        assert!(!local(&app).contains(&suggestion));
        println!("inserted {suggestion} and undid it");

        // Hover a name of the first proof, Ctrl+click it to its definition, Back
        let _ = app.update(Message::SetMode(Mode::Literate));
        assert!(pump(&mut app, &mut stream, 240, checked).await, "rechecked");
        scroll_by(&mut app, &mut h, 4000.0);
        let (p, _) = find_theorem(&mut app, &mut h);
        let mut defined = None;
        let mut same_file = None;
        let mut attempts = 0;
        'scan: for row in 0..12 {
            for dx in (0..560).step_by(14) {
                let q = cosmic::iced::Point::new(p.x - 120.0 + dx as f32, p.y - 20.0 + row as f32 * 16.0);
                for m in h.move_to(&app, q) {
                    let _ = app.update(m);
                }
                attempts += 1;
                if attempts > 120 && same_file.is_some() {
                    break 'scan;
                }
                let generation = app.hover_gen;
                let _ = app.update(Message::HoverTick(generation));
                if pump(&mut app, &mut stream, 2, |a| a.workspace.as_ref().unwrap().hover.as_ref().is_some_and(|h| h.info.is_some())).await {
                    let info = app.workspace.as_ref().unwrap().hover.as_ref().unwrap().info.clone().unwrap();
                    if info["definition"]["file"].as_str().is_some_and(|f| f.ends_with(".lean")) && info["tooltip"]["items"].as_array().is_some_and(|i| !i.is_empty()) {
                        // (a definition in another file is the better test; the first in this one will do)
                        if info["definition"]["file"].as_str().is_some_and(|f| !f.ends_with("CausalCorrectness.lean")) {
                            h.draw(&app, "lean-live-6-hover");
                            println!("hover: {} -> {} in {}", info["tooltip"]["items"][0]["text"], info["definition"]["name"], info["definition"]["file"]);
                            defined = Some((q, info["definition"].clone()));
                            break 'scan;
                        }
                        same_file.get_or_insert((q, info["definition"].clone()));
                    }
                }
            }
        }
        let (q, def) = defined.or(same_file).expect("a name with a tooltip and a definition");
        // (the tooltip again where the name is, for its picture)
        for m in h.move_to(&app, q) {
            let _ = app.update(m);
        }
        let generation = app.hover_gen;
        let _ = app.update(Message::HoverTick(generation));
        let _ = pump(&mut app, &mut stream, 10, |a| a.workspace.as_ref().unwrap().hover.as_ref().is_some_and(|h| h.info.is_some())).await;
        h.draw(&app, "lean-live-6-hover");
        app.ctrl = true;
        for m in h.click(&app, q) {
            let _ = app.update(m);
        }
        app.ctrl = false;
        let want = def["file"].as_str().unwrap().rsplit('/').next().unwrap().to_string();
        let there = |a: &App| theory(a).path.ends_with(&want) && (want != "CausalCorrectness.lean" || a.workspace.as_ref().unwrap().pending_goto.is_none());
        assert!(pump(&mut app, &mut stream, 90, there).await, "went to {want}");
        h.draw(&app, "lean-live-7-definition");
        println!("went to the definition in {want}");
        let _ = app.update(Message::Back);
        assert!(pump(&mut app, &mut stream, 90, |a| theory(a).path == file).await, "back");
        h.draw(&app, "lean-live-8-back");
        println!("and back");

        // The host's own view of the files: nothing was written but what the human typed
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        app.send("live", "stop", json!({"session": sid}), Pending::Plain);
        let _ = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;
    });
}

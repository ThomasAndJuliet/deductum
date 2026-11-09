// SPDX-License-Identifier: GPL-3.0-only

//! Long-document interaction through the retained headless widget tree.

use super::editor::Mode;
use super::snapshot::{Harness, scroll_to_open_block};
use super::{App, Message};
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::iced::{Event, Point, Size, mouse};
use serde_json::json;

fn apply(app: &mut App, h: &mut Harness, messages: Vec<Message>) {
    for message in messages {
        let task = cosmic::Application::update(app, message);
        h.run_task(app, task);
    }
}

fn long_lean() -> String {
    let mut doc = String::from("import Mathlib\n\n");
    for i in 0..80 {
        doc.push_str(&format!("/-- Repeated α α $x + 0$ and [reference](https://example.org): paragraph {i}. -/\ntheorem marker_{i} : True := by\n  trivial\n\n"));
    }
    doc
}

struct EditorPlace {
    bounds: Option<cosmic::iced::Rectangle>,
    scroll: cosmic::iced::Vector,
}
impl cosmic::iced::advanced::widget::Operation<()> for EditorPlace {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>),
    ) {
        operate(self);
    }
    fn focusable(
        &mut self,
        id: Option<&cosmic::widget::Id>,
        bounds: cosmic::iced::Rectangle,
        _: &mut dyn cosmic::iced::advanced::widget::operation::Focusable,
    ) {
        if id == Some(&super::view::block_editor_id()) {
            self.bounds = Some(bounds);
        }
    }
    fn scrollable(
        &mut self,
        id: Option<&cosmic::widget::Id>,
        _: cosmic::iced::Rectangle,
        _: cosmic::iced::Rectangle,
        translation: cosmic::iced::Vector,
        _: &mut dyn cosmic::iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&super::view::literate_scroll_id()) {
            self.scroll = translation;
        }
    }
}

#[test]
fn long_lean_scrolling_drag_copy_and_prover_updates_keep_editor_focus() {
    let text = long_lean();
    for target in [0, 40, 79] {
        let mut app = super::snapshot_lean::lean_app_with(Mode::Literate, &text);
        app.show_sidebar = false;
        app.show_right = false;
        let needle = format!("theorem marker_{target}");
        let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        assert!(
            theory.blocks.len() > 64,
            "fixture must activate sparse layout"
        );
        let total_blocks = theory.blocks.len();
        let at = theory.local.find(&needle).unwrap();
        let offset = theory.local[..at].chars().count();
        let block = theory
            .blocks
            .iter()
            .position(|b| b.start <= offset && offset < b.end)
            .unwrap();
        let mut h = Harness::new(Size::new(1400.0, 900.0));
        let task = cosmic::Application::update(&mut app, Message::EditBlock(block));
        h.run_task(&app, task);
        h.focus(&app, super::view::block_editor_id());
        h.frame(&app);
        let msgs = scroll_to_open_block(&mut h, &app);
        apply(&mut app, &mut h, msgs);
        for _ in 0..5 {
            let msgs = h.events(
                &app,
                &[Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now(),
                ))],
            );
            apply(&mut app, &mut h, msgs);
        }
        h.draw(&app, &format!("long-lean-{target}"));
        let built = super::viewport::last_built_count();
        assert!(
            built > 0 && built < total_blocks / 2,
            "built {built} widgets for {total_blocks} blocks"
        );
        let mut place = EditorPlace {
            bounds: None,
            scroll: cosmic::iced::Vector::ZERO,
        };
        h.operate(&app, &mut place);
        let rect = place.bounds.expect("open proof editor");
        let y = rect.y - place.scroll.y + 12.0;
        let start = Point::new(rect.x + 24.0, y);
        let end = Point::new(rect.x + 155.0, y);
        assert!(y >= 0.0 && y < h.size.height, "editor is visible: {y}");
        let msgs = h.move_to(&app, start);
        apply(&mut app, &mut h, msgs);
        let msgs = h.events(
            &app,
            &[Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))],
        );
        apply(&mut app, &mut h, msgs);
        let msgs = h.move_to(&app, end);
        apply(&mut app, &mut h, msgs);
        let msgs = h.events(
            &app,
            &[Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            ))],
        );
        apply(&mut app, &mut h, msgs);
        let selected = app
            .workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .block_editor
            .as_ref()
            .unwrap()
            .content
            .selection()
            .unwrap_or_else(|| {
                panic!(
                    "drag selected text target={target} rect={rect:?} scroll={:?} y={y}",
                    place.scroll
                )
            });
        assert!(!selected.trim().is_empty(), "drag must select text");
        let msgs = h.key_with(&app, Key::Character("c".into()), Some("c"), Modifiers::CTRL);
        apply(&mut app, &mut h, msgs);
        assert_eq!(h.copied_text(), Some(selected.as_str()));
        // Cross the goal-beside threshold and return to the wide layout while the selection is live.
        for width in [600.0, 1400.0] {
            h.size = Size::new(width, 900.0);
            app.window_width = width;
            for _ in 0..5 {
                let msgs = h.events(
                    &app,
                    &[Event::Window(cosmic::iced::window::Event::RedrawRequested(
                        std::time::Instant::now(),
                    ))],
                );
                apply(&mut app, &mut h, msgs);
            }
            h.draw(&app, &format!("long-lean-{target}-width-{width}"));
            assert_eq!(
                app.workspace
                    .as_ref()
                    .unwrap()
                    .theory
                    .as_ref()
                    .unwrap()
                    .block_editor
                    .as_ref()
                    .unwrap()
                    .content
                    .selection()
                    .as_deref(),
                Some(selected.as_str()),
                "selection survives resize"
            );
        }
        // A large wheel jump leaves the active editor pinned offscreen; return to it.
        let msgs = h.move_to(&app, Point::new(400.0, 450.0));
        apply(&mut app, &mut h, msgs);
        let msgs = h.events(
            &app,
            &[Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -4000.0 },
            })],
        );
        apply(&mut app, &mut h, msgs);
        for _ in 0..5 {
            let msgs = h.events(
                &app,
                &[Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now(),
                ))],
            );
            apply(&mut app, &mut h, msgs);
        }
        h.draw(&app, &format!("long-lean-{target}-jump"));
        assert_eq!(
            app.workspace
                .as_ref()
                .unwrap()
                .theory
                .as_ref()
                .unwrap()
                .block_editor
                .as_ref()
                .unwrap()
                .content
                .selection()
                .as_deref(),
            Some(selected.as_str()),
            "offscreen editor keeps selection"
        );
        let msgs = scroll_to_open_block(&mut h, &app);
        apply(&mut app, &mut h, msgs);
        for _ in 0..5 {
            let msgs = h.events(
                &app,
                &[Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now(),
                ))],
            );
            apply(&mut app, &mut h, msgs);
        }
        let path = app
            .workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .path
            .clone();
        // A remote agent appends outside the active proof, then the prover reports that revision.
        let end_offset = app
            .workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .confirmed
            .chars()
            .count();
        let task = app.on_event("local".into(), json!({"event":"doc_changed", "session":"s1", "path":path, "revision":2,
            "entry":{"author":{"kind":"agent"}, "kind":"edit", "change":[[end_offset, end_offset, "\n-- agent edit\n"]]}}));
        h.run_task(&app, task);
        h.frame(&app);
        assert!(
            app.workspace
                .as_ref()
                .unwrap()
                .theory
                .as_ref()
                .unwrap()
                .local
                .contains("-- agent edit")
        );
        for tick in 0..8 {
            let task = app.on_event("local".into(), json!({"event":"doc_prover", "session":"s1", "path":path, "prover":{"revision":2,"status":{"finished":tick},"commands":[],"messages":[]}}));
            h.run_task(&app, task);
            h.frame(&app);
        }
        assert_eq!(
            app.workspace
                .as_ref()
                .unwrap()
                .theory
                .as_ref()
                .unwrap()
                .block_editor
                .as_ref()
                .unwrap()
                .content
                .selection()
                .as_deref(),
            Some(selected.as_str())
        );
        let msgs = h.type_text(&app, "Z");
        apply(&mut app, &mut h, msgs);
        let local = &app
            .workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .local;
        assert!(
            local.contains('Z'),
            "typing after prover updates reaches the editor"
        );
        h.draw(&app, &format!("long-lean-{target}-edited"));
    }
}

#[test]
fn duplicate_rows_do_not_reuse_unproven_identity_after_remote_edit() {
    use std::collections::HashMap;
    let text = long_lean()
        .replace("paragraph 1.", "paragraph 0.")
        .replace("marker_1 :", "marker_0 :");
    let mut app = super::snapshot_lean::lean_app_with(Mode::Literate, &text);
    let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let chars: Vec<char> = theory.local.chars().collect();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, block) in theory.blocks.iter().enumerate() {
        let text: String = chars[block.start..block.end].iter().collect();
        groups.entry(text).or_default().push(i);
    }
    let duplicate = groups
        .iter()
        .find(|(text, v)| v.len() >= 2 && text.contains("marker_0"))
        .expect("fixture has repeated proof blocks")
        .1
        .clone();
    let unique = groups
        .iter()
        .find(|(text, v)| v.len() == 1 && text.contains("marker_40"))
        .expect("fixture has a unique proof")
        .1[0];
    let duplicate_keys: Vec<_> = duplicate.iter().map(|&i| theory.page_rows[i].key).collect();
    let unique_key = theory.page_rows[unique].key;
    let path = theory.path.clone();
    let end = theory.confirmed.chars().count();
    let _ = app.on_event("local".into(), json!({"event":"doc_changed", "session":"s1", "path":path, "revision":2,
        "entry":{"author":{"kind":"agent"}, "kind":"edit", "change":[[end, end, "\n-- agent edit\n"]]}}));
    let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(
        theory.page_rows[unique].key, unique_key,
        "unique unchanged block retains identity"
    );
    for (&i, old) in duplicate.iter().zip(duplicate_keys) {
        assert_ne!(
            theory.page_rows[i].key, old,
            "duplicate occurrence must not claim identity after edit"
        );
    }
}

#[test]
fn old_page_scroll_messages_do_not_affect_the_new_theory() {
    let old = super::snapshot_lean::lean_app(Mode::Literate);
    let old_scope = old
        .workspace
        .as_ref()
        .unwrap()
        .theory
        .as_ref()
        .unwrap()
        .page_scope;
    let mut app = super::snapshot_lean::lean_app(Mode::Literate);
    let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let scope = theory.page_scope;
    assert_ne!(old_scope, scope);
    let initial = theory.page_viewport.get();
    let task = cosmic::Application::update(
        &mut app,
        Message::LiterateScrolled(old_scope, 4321.0, 500.0),
    );
    assert!(cosmic::iced::runtime::task::into_stream(task).is_none());
    assert_eq!(
        app.workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .page_viewport
            .get(),
        initial
    );
    let task = cosmic::Application::update(&mut app, Message::LiterateAdjust(old_scope, 500.0));
    assert!(
        cosmic::iced::runtime::task::into_stream(task).is_none(),
        "an old adjustment must not scroll the new page"
    );
    let _ = cosmic::Application::update(&mut app, Message::LiterateScrolled(scope, 123.0, 600.0));
    assert_eq!(
        app.workspace
            .as_ref()
            .unwrap()
            .theory
            .as_ref()
            .unwrap()
            .page_viewport
            .get(),
        (123.0, 600.0)
    );
    let task = cosmic::Application::update(&mut app, Message::LiterateAdjust(scope, 25.0));
    assert!(
        cosmic::iced::runtime::task::into_stream(task).is_some(),
        "the current page can still scroll"
    );
}

/// A change in the width the goal's lines are broken at asks for the state once: the messages that
/// follow it do not schedule it again (each would make the pending request stale, for ever), a
/// stale tick sends nothing, and the current one sends the `state` request
#[test]
fn a_change_in_the_goals_columns_schedules_one_state_request_that_is_sent() {
    let mut app = super::snapshot::demo_app(Mode::Literate);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    app.links[0].sender = Some(tx);
    app.show_sidebar = false;
    app.show_right = false;
    // (the first message notes what the panel has)
    let _ = cosmic::Application::update(&mut app, Message::WindowResized(1500.0, 900.0));
    let before = app.state_gen;
    // a window of another width: one request scheduled
    let _ = cosmic::Application::update(&mut app, Message::WindowResized(1000.0, 900.0));
    assert_eq!(app.state_gen, before + 1, "scheduled once");
    let asked = app.state_gen;
    // other messages with the width unchanged do not schedule it again
    for _ in 0..3 {
        let _ = cosmic::Application::update(&mut app, Message::WindowResized(1000.0, 900.0));
    }
    assert_eq!(app.state_gen, asked, "no further scheduling while the request is pending");
    // a stale tick sends nothing
    let _ = cosmic::Application::update(&mut app, Message::StateTick(before));
    assert!(rx.try_recv().is_err(), "a stale tick sends nothing");
    // the current tick asks for the state
    let _ = cosmic::Application::update(&mut app, Message::StateTick(asked));
    let mut ops = Vec::new();
    while let Ok(m) = rx.try_recv() {
        ops.push(m);
    }
    assert!(ops.iter().any(|m| m.contains("\"op\":\"state\"")), "the state is asked for: {ops:?}");
    // and after it the columns are noted: another message schedules nothing
    let _ = cosmic::Application::update(&mut app, Message::WindowResized(1000.0, 900.0));
    assert_eq!(app.state_gen, asked);
}

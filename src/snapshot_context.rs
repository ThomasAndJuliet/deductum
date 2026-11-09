// SPDX-License-Identifier: GPL-3.0-only

//! The right-click menus (Tom, 8 Nov, mockups/t30), drawn headless and driven through the widgets:
//! a right press on a cell, a goal (in the goal panel beside its cell, where libcosmic's own menus
//! do not open), selected code and a session's row opens its menu where it is used; each item
//! sends what it should; what a backend cannot do is disabled with its reason. `CT_SNAPSHOTS=<dir>`
//! keeps the frames (`menu-*`).

use cosmic::iced::advanced::widget::Operation;
use cosmic::iced::{Event, Point, Rectangle, Size, mouse};
use cosmic::widget::text_editor::{Action, Motion};

use super::context_menu::{self, Target};
use super::editor::Mode;
use super::menubar::{Row, find, labels};
use super::shell::Msg;
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;
use super::{App, EditorTarget, Message};

const WINDOW: Size = Size::new(1400.0, 900.0);

/// The containers of a frame with ids, with their bounds
struct Bounds(Vec<(cosmic::widget::Id, Rectangle)>);

impl Operation<()> for Bounds {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation<()>)) {
        f(self);
    }
    fn container(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle) {
        if let Some(id) = id {
            self.0.push((id.clone(), bounds));
        }
    }
}

/// A harness in the theme asked for (it holds the UI's turn, so that no other test changes the
/// theme under it); the dark one, the tests' default, is set again after
fn themed<T>(light: bool, f: impl FnOnce(&mut Harness) -> T) -> T {
    let mut h = Harness::new(WINDOW);
    cosmic::theme::set_active(if light { crate::tones::light_type_for_tests() } else { cosmic::theme::ThemeType::Dark });
    let out = f(&mut h);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    out
}

fn update(app: &mut App, m: Message) {
    let _ = cosmic::Application::update(app, m);
}

fn apply(app: &mut App, messages: Vec<Message>) {
    for m in messages {
        update(app, m);
    }
}

fn theory(app: &App) -> &super::editor::Theory {
    app.workspace.as_ref().unwrap().theory.as_ref().unwrap()
}

fn ws(app: &App) -> &super::Workspace {
    app.workspace.as_ref().unwrap()
}

fn rect_at(h: &mut Harness, app: &App, id: cosmic::widget::Id) -> Rectangle {
    let mut all = Bounds(Vec::new());
    h.operate(app, &mut all);
    all.0.iter().find(|(x, _)| *x == id).unwrap_or_else(|| panic!("{id:?} is not on the page")).1
}

fn centre(r: Rectangle) -> Point {
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// A right press and release at `at`
fn right_click(h: &mut Harness, app: &App, at: Point) -> Vec<Message> {
    let mut out = h.move_to(app, at);
    out.extend(h.events(app, &[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]));
    out.extend(h.events(app, &[Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right))]));
    out
}

/// What a right press at `at` opened: the target of the menu, if one opened (the messages are
/// applied to the app, as the runtime would)
fn open_menu(h: &mut Harness, app: &mut App, at: Point) -> Option<Target> {
    let messages = right_click(h, app, at);
    apply(app, messages);
    app.shell.context.as_ref().map(|o| o.target.clone())
}

/// The demo's cells: the index of the `n`th formal block
fn formal(app: &App, n: usize) -> usize {
    theory(app).blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal).map(|(i, _)| i).nth(n).unwrap()
}

/// The demo page with no cell open and no sidebars
fn page_app() -> App {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    app.window_width = WINDOW.width;
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.cursor = 0;
    t.selected = None;
    app
}

/// The demo's first proof being edited with the caret in `apply`, goals beside it
fn goal_app() -> App {
    let mut app = page_app();
    let i = formal(&app, 0);
    update(&mut app, Message::EditBlock(i));
    super::snapshot::caret_in_block(&mut app, "apply", 0);
    app
}

fn rows_of(rows: &[Row]) -> Vec<String> {
    labels(rows)
}

// ----- the cell

#[test]
fn a_right_press_on_a_cell_opens_its_menu_under_the_pointer() {
    for light in [true, false] {
        let mut app = page_app();
        let i = formal(&app, 0);
        themed(light, |h| {
            let theme = if light { "light" } else { "dark" };
            let _ = h.draw(&app, "menu-warm");
            let closed = h.draw(&app, "menu-closed");
            let cell = rect_at(h, &app, super::view::test_block_id(i));
            let at = Point::new(cell.x + 60.0, cell.y + 12.0);
            assert_eq!(open_menu(h, &mut app, at), Some(Target::Cell(i)), "the cell's menu opens ({theme})");
            let open = h.draw(&app, &format!("menu-cell-{theme}"));
            assert_ne!(open, closed, "the menu is drawn ({theme})");
            // (its card begins at the pointer: ink below and to the right of it, none above-left)
            let w = WINDOW.width as usize;
            let differs = |x: f32, y: f32| { let k = (y as usize * w + x as usize) * 4; open[k..k + 4] != closed[k..k + 4] };
            assert!(differs(at.x + 40.0, at.y + 20.0), "the menu is under the pointer ({theme})");
            assert!(!differs(at.x - 30.0, at.y - 6.0), "nothing above-left of the pointer changed ({theme})");
            // a press outside closes it, and goes no further
            let outside = Point::new(WINDOW.width - 20.0, WINDOW.height - 20.0);
            let messages = h.click(&app, outside);
            assert!(messages.iter().any(|m| matches!(m, Message::Shell(Msg::ContextClose))), "a press outside closes the menu ({theme}): {messages:?}");
            apply(&mut app, messages);
            assert!(app.shell.context.is_none());
        });
    }
}

/// A menu's rows as the design has them: Windows has no Ask an agent row, nor a separator left
/// at the end by its going
fn designed<'a>(rows: &[&'a str]) -> Vec<&'a str> {
    let mut rows = rows.to_vec();
    if cfg!(windows) {
        rows.retain(|r| *r != "Ask an agent about it\u{2026}");
        if rows.last() == Some(&"\u{2014}") {
            rows.pop();
        }
    }
    rows
}

#[test]
fn the_cell_menu_is_the_designs_in_its_order_and_each_item_sends_what_it_should() {
    let app = page_app();
    let i = formal(&app, 1);
    let table = app.table("local");
    let rows = context_menu::cell_rows(ws(&app), &*table, i);
    assert_eq!(
        rows_of(&rows),
        designed(&[
            "Edit", "Show its goals", "Open in its own window", "\u{2014}", "Run sledgehammer", "Proof tools \u{25b8}", "Ask an agent about it\u{2026}", "\u{2014}", "Copy", "Copy as Markdown", "\u{2014}",
            "Insert text above", "Insert lemma below", "Insert definition below", "Move up", "Move down", "\u{2014}", "!Delete",
        ])
    );
    let msg = |label: &str| match find(&rows, label) {
        Some(Row::Item { message, .. } | Row::Danger { message, .. }) => message.clone(),
        other => panic!("{label}: {other:?}"),
    };
    assert!(matches!(msg("Edit"), Message::EditBlock(n) if n == i));
    assert!(matches!(msg("Show its goals"), Message::SelectBlock(n) if n == i));
    assert!(matches!(msg("Insert text above"), Message::InsertBlock(Some(n), "text") if n == i - 1));
    assert!(matches!(msg("Insert lemma below"), Message::InsertBlock(Some(n), "lemma") if n == i));
    assert!(matches!(msg("Insert definition below"), Message::InsertBlock(Some(n), "definition") if n == i));
    assert!(matches!(msg("Move up"), Message::MoveBlock(n, false) if n == i));
    assert!(matches!(msg("Move down"), Message::MoveBlock(n, true) if n == i));
    assert!(matches!(msg("Delete"), Message::DeleteBlock(n) if n == i));
    assert!(matches!(msg("Run sledgehammer"), Message::RunToolAt(ref tool, _) if tool == "sledgehammer"));
    // the cell's source, and the same as Markdown in a fenced block of the backend's language
    let t = theory(&app);
    let source = t.source_text(t.blocks[i].start, t.blocks[i].end);
    assert!(matches!(msg("Copy"), Message::CopyText(s) if s == source));
    let Message::CopyText(md) = msg("Copy as Markdown") else { panic!() };
    assert!(md.starts_with("```isabelle\nlemma add_comm") && md.ends_with("\n```"), "{md}");
    // Delete is red, last; the keys are the ones that exist
    assert!(matches!(rows.last(), Some(Row::Danger { .. })));
    let keys = |label: &str| match find(&rows, label) {
        Some(Row::Item { keys, .. } | Row::Off { keys, .. }) => *keys,
        other => panic!("{other:?}"),
    };
    assert_eq!([keys("Edit"), keys("Copy"), keys("Move up"), keys("Move down")], ["Enter", "Ctrl+C", "Alt+\u{2191}", "Alt+\u{2193}"]);
}

#[test]
fn a_text_block_has_the_cell_menu_without_the_proof_items_and_copies_as_its_markdown() {
    let app = page_app();
    let i = theory(&app).blocks.iter().position(|b| b.kind == Kind::Text).unwrap();
    let table = app.table("local");
    let rows = context_menu::cell_rows(ws(&app), &*table, i);
    assert_eq!(rows_of(&rows), ["Edit", "\u{2014}", "Copy", "Copy as Markdown", "\u{2014}", "Insert text above", "Insert lemma below", "Insert definition below", "Move up", "Move down", "\u{2014}", "!Delete"]);
    let Some(Row::Item { message: Message::CopyText(md), .. }) = find(&rows, "Copy as Markdown") else { panic!() };
    assert!(!md.contains("```") && md.contains("addition with zero"), "prose as Markdown, not as code: {md}");
}

#[test]
fn a_cell_that_cannot_move_says_why() {
    let app = page_app();
    let table = app.table("local");
    let first = theory(&app).blocks.iter().position(|b| b.kind != Kind::Header).unwrap();
    let rows = context_menu::cell_rows(ws(&app), &*table, first);
    assert!(matches!(find(&rows, "Move up"), Some(Row::Off { why, .. }) if why == "It is the first cell"), "{:?}", find(&rows, "Move up"));
    let last = theory(&app).blocks.iter().rposition(|b| b.kind != Kind::End).unwrap();
    let rows = context_menu::cell_rows(ws(&app), &*table, last);
    assert!(matches!(find(&rows, "Move down"), Some(Row::Off { why, .. }) if why == "It is the last cell"));
}

#[test]
fn move_up_and_down_swap_a_cell_with_its_neighbour_keeping_the_blank_lines() {
    let first_line = |app: &App, i: usize| { let t = theory(app); t.source_text(t.blocks[i].start, t.blocks[i].end).lines().next().unwrap().to_string() };
    let (add_comm, broken) = (formal_named("lemma add_comm"), formal_named("lemma broken"));
    // the two proofs are neighbours: the second up, or the first down, is the same document, the
    // blank line between them where it was
    let mut up = page_app();
    let before = theory(&up).local.clone();
    update(&mut up, Message::MoveBlock(broken, false));
    let mut down = page_app();
    update(&mut down, Message::MoveBlock(add_comm, true));
    assert_eq!(theory(&up).local, theory(&down).local, "moving a cell up is moving its neighbour down");
    let moved = theory(&up).local.clone();
    assert_eq!(moved.chars().count(), before.chars().count(), "nothing lost or added");
    let comm_at = moved.find("lemma add_comm: \"(a::nat) + b = b + a\"\n  sorry").expect("its proof went with it");
    assert!(moved.find("lemma broken: \"(x::nat) < x\"\n  by simp\n\n").unwrap() < comm_at, "broken now comes first, one blank line before the next");
    assert!(moved.ends_with("sorry\n\nend\n"), "{moved:?}");
    assert_eq!(theory(&up).selected, Some(add_comm), "the moved cell stays selected, at its new place");
    assert_eq!(first_line(&up, add_comm), "lemma broken: \"(x::nat) < x\"");
    // the header and the `end` stay where they are
    let mut fixed = page_app();
    let header = theory(&fixed).blocks.iter().position(|b| b.kind == Kind::Header).unwrap();
    let end = theory(&fixed).blocks.iter().position(|b| b.kind == Kind::End).unwrap();
    update(&mut fixed, Message::MoveBlock(header, true));
    update(&mut fixed, Message::MoveBlock(end, false));
    update(&mut fixed, Message::MoveBlock(end - 1, true));
    assert_eq!(theory(&fixed).local, before);
}

/// The index of the cell whose source begins with `prefix`
fn formal_named(prefix: &str) -> usize {
    let app = page_app();
    let t = theory(&app);
    t.blocks.iter().position(|b| t.source_text(b.start, b.end).starts_with(prefix)).unwrap_or_else(|| panic!("no cell {prefix:?}"))
}

// ----- the goal

/// Where the first subgoal is drawn, with the goal panel beside the open cell (the panel is moved
/// from where its layout puts it, so its widgets are drawn and take the pointer that far off)
fn goal_point(h: &mut Harness, app: &App) -> Point {
    let _ = h.draw(app, "menu-goal-warm");
    let _ = h.draw(app, "menu-goal-warm");
    assert!(super::view::last_wide(), "the page has room for the goal beside");
    let moved = super::gap::goal_moved();
    let card = rect_at(h, app, super::view::test_panel_id("subgoal-1"));
    Point::new(card.x + moved.x + 40.0, card.y + moved.y + 8.0)
}

#[test]
fn a_right_press_on_a_goal_beside_its_cell_opens_its_menu_there() {
    // (libcosmic's menu bar and context menu do not open in the translated goal panel: this one does)
    for light in [true, false] {
        let mut app = goal_app();
        let theme = if light { "light" } else { "dark" };
        themed(light, |h| {
            super::view::set_goal_beside(true);
            let at = goal_point(h, &app);
            let closed = h.draw(&app, "menu-goal-closed");
            assert_eq!(open_menu(h, &mut app, at), Some(Target::Goal(1)), "the goal's menu opens ({theme})");
            let open = h.draw(&app, &format!("menu-goal-{theme}"));
            assert_ne!(open, closed, "the menu is drawn ({theme})");
            // (under the pointer, as wide as its card)
            let w = WINDOW.width as usize;
            let differs = |x: f32, y: f32| { let k = (y as usize * w + x as usize) * 4; open[k..k + 4] != closed[k..k + 4] };
            assert!(differs(at.x + 60.0, at.y + 30.0), "the menu is under the pointer ({theme})");
            assert!(!differs(at.x - 30.0, at.y - 12.0), "nothing above-left of the pointer changed ({theme})");
            // its first row is what a click does: the item at the top of the card is pressed
            let messages = h.click(&app, Point::new(at.x + 60.0, at.y + 22.0));
            assert!(
                messages.iter().any(|m| matches!(m, Message::Shell(Msg::ContextDo(inner)) if matches!(inner.as_slice(), [Message::UseStep(c)] if c.contains("noteq")))),
                "pressing the first row sends Use as the next step ({theme}): {messages:?}"
            );
            super::view::set_goal_beside(false);
        });
    }
}

#[test]
fn the_goal_menu_is_the_designs_and_each_item_sends_what_it_should() {
    let app = goal_app();
    let rows = context_menu::goal_rows(ws(&app), 1);
    assert_eq!(
        rows_of(&rows),
        ["Use as the next step", "Show the prover's text", "\u{2014}", "Copy", "Copy as Markdown", "Copy as a have", "\u{2014}", "Search theorems for it\u{2026}"]
    );
    let msg = |label: &str| match find(&rows, label) {
        Some(Row::Item { message, .. }) => message.clone(),
        other => panic!("{label}: {other:?}"),
    };
    // the statement as the editor shows it goes in; the source (symbols as escapes) is copied
    assert!(matches!(msg("Use as the next step"), Message::UseStep(c) if c == "xs \\<noteq> []"));
    assert!(matches!(msg("Show the prover's text"), Message::StateText(true)));
    assert!(matches!(msg("Copy as a have"), Message::CopyText(s) if s == "have \"xs \\<noteq> []\""));
    assert!(matches!(msg("Copy"), Message::CopyText(s) if s == "fix xs :: 'a list\nassume 0 < n\nshow xs \\<noteq> []"));
    assert!(matches!(msg("Copy as Markdown"), Message::CopyText(s) if s == "```isabelle\nfix xs :: 'a list\nassume 0 < n\nshow xs \\<noteq> []\n```"));
    assert!(matches!(msg("Search theorems for it\u{2026}"), Message::FindFor(q, false) if q == "\"xs \\<noteq> []\""));
    // the second goal is its own
    let second = context_menu::goal_rows(ws(&app), 2);
    assert!(matches!(find(&second, "Copy as a have"), Some(Row::Item { message: Message::CopyText(s), .. }) if s == "have \"P n\""));
    // no cell open (and not the source view): there is no caret to put a step at, and it says so
    let closed = page_app();
    let rows = context_menu::goal_rows(ws(&closed), 1);
    assert!(matches!(find(&rows, "Use as the next step"), Some(Row::Off { why, .. }) if why.contains("Open the cell")), "{:?}", find(&rows, "Use as the next step"));
}

#[test]
fn use_as_the_next_step_puts_the_goal_as_a_have_on_a_new_line_after_the_caret_s() {
    let mut app = goal_app();
    let before = match &theory(&app).block_editor {
        Some(be) => be.content.text(),
        None => panic!("the proof is open"),
    };
    update(&mut app, Message::UseStep("xs \\<noteq> []".into()));
    let be = theory(&app).block_editor.as_ref().unwrap();
    let after = be.content.text();
    assert!(after.starts_with(before.lines().next().unwrap()), "{after}");
    // after the caret's line (apply simp), indented as the proof is, its proof a placeholder
    assert!(after.contains("  apply simp\n  have \"xs \u{2260} []\"\n    sorry\n  done"), "{after:?}");
}

#[test]
fn the_have_is_each_backends_own() {
    use super::prover::{Profile, ISABELLE, LEAN};
    assert_eq!(ISABELLE.have_line("P n"), "have \"P n\"");
    assert_eq!(ISABELLE.have_step("P n", "  "), "have \"P n\"\n    sorry");
    assert_eq!(LEAN.have_line("P n"), "have : P n");
    assert_eq!(LEAN.have_step("P n", "  "), "have : P n := by\n    sorry");
}

// ----- selected code

#[test]
fn a_right_press_in_the_open_editor_opens_the_selection_menu() {
    for light in [true, false] {
        let mut app = goal_app();
        let theme = if light { "light" } else { "dark" };
        update(&mut app, Message::BlockAction(Action::Move(Motion::DocumentStart)));
        update(&mut app, Message::BlockAction(Action::SelectWord));
        assert_eq!(context_menu::editor_selection(ws(&app), EditorTarget::Block).as_deref(), Some("lemma"));
        themed(light, |h| {
            super::view::set_goal_beside(false);
            let _ = h.draw(&app, "menu-code-warm");
            let closed = h.draw(&app, "menu-code-closed");
            let cell = rect_at(h, &app, super::view::selected_block_id());
            let at = Point::new(cell.x + 120.0, cell.y + 14.0);
            assert_eq!(open_menu(h, &mut app, at), Some(Target::Editor(EditorTarget::Block)), "the editor's menu opens ({theme})");
            let open = h.draw(&app, &format!("menu-code-{theme}"));
            assert_ne!(open, closed, "the menu is drawn ({theme})");
        });
    }
}

#[test]
fn the_selection_menu_is_the_designs_and_each_item_sends_what_it_should() {
    let mut app = goal_app();
    update(&mut app, Message::BlockAction(Action::Move(Motion::DocumentStart)));
    update(&mut app, Message::BlockAction(Action::SelectWord));
    let rows = context_menu::editor_rows(ws(&app), EditorTarget::Block);
    assert_eq!(
        rows_of(&rows),
        designed(&["Copy", "Copy as Markdown", "Cut", "Paste", "\u{2014}", "Go to definition", "Find uses", "Search theorems for it\u{2026}", "\u{2014}", "Ask an agent about it\u{2026}"])
    );
    let msg = |label: &str| match find(&rows, label) {
        Some(Row::Item { message, .. }) => message.clone(),
        other => panic!("{label}: {other:?}"),
    };
    use cosmic::widget::text_editor::Binding;
    assert!(matches!(msg("Copy"), Message::NavBinding(EditorTarget::Block, b) if matches!(*b, Binding::Copy)));
    assert!(matches!(msg("Cut"), Message::NavBinding(EditorTarget::Block, b) if matches!(*b, Binding::Cut)));
    assert!(matches!(msg("Paste"), Message::NavBinding(EditorTarget::Block, b) if matches!(*b, Binding::Paste)));
    assert!(matches!(msg("Copy as Markdown"), Message::CopyText(s) if s == "```isabelle\nlemma\n```"));
    assert!(matches!(msg("Go to definition"), Message::GotoDefinition));
    // one name selected: its uses are searched for at once; the same text opens the panel unrun
    assert!(matches!(msg("Find uses"), Message::FindFor(q, true) if q == "\"lemma\""));
    assert!(matches!(msg("Search theorems for it\u{2026}"), Message::FindFor(q, false) if q == "\"lemma\""));
    // Windows has no agent-session popover, so this message doesn't exist there
    #[cfg(not(windows))]
    assert!(matches!(msg("Ask an agent about it\u{2026}"), Message::Shell(Msg::AgentOpen(true))));
    // two words: no single name to find the uses of, and it says so; with nothing selected, Copy
    // and Cut are disabled
    update(&mut app, Message::BlockAction(Action::Move(Motion::DocumentStart)));
    update(&mut app, Message::BlockAction(Action::Select(Motion::WordRight)));
    update(&mut app, Message::BlockAction(Action::Select(Motion::WordRight)));
    let rows = context_menu::editor_rows(ws(&app), EditorTarget::Block);
    assert!(matches!(find(&rows, "Find uses"), Some(Row::Off { why, .. }) if why.contains("one name")), "{:?}", find(&rows, "Find uses"));
    update(&mut app, Message::BlockAction(Action::Move(Motion::DocumentStart)));
    let rows = context_menu::editor_rows(ws(&app), EditorTarget::Block);
    for label in ["Copy", "Copy as Markdown", "Cut"] {
        assert!(matches!(find(&rows, label), Some(Row::Off { why, .. }) if why.contains("Select")), "{label}");
    }
    assert!(matches!(find(&rows, "Paste"), Some(Row::Item { .. })));
}

#[test]
fn a_selection_in_a_shown_cell_makes_its_menu_the_selection_s_read_only() {
    let mut app = page_app();
    let i = formal(&app, 0);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let view = t.block_views[i].as_mut().expect("the cell is shown");
    view.content.perform(Action::Move(Motion::DocumentStart));
    view.content.perform(Action::SelectWord);
    let table = app.table("local");
    let rows = context_menu::cell_rows(ws(&app), &*table, i);
    assert_eq!(rows_of(&rows)[..4], ["Copy", "Copy as Markdown", "(Cut)", "(Paste)"]);
    assert!(matches!(find(&rows, "Paste"), Some(Row::Off { why, .. }) if why.contains("Enter")));
    assert!(matches!(find(&rows, "Copy"), Some(Row::Item { message: Message::CopyText(s), .. }) if s == "lemma"));
}

// ----- a session's row

#[test]
fn a_right_press_on_a_session_row_opens_its_menu_with_forget_last_in_red() {
    for light in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.show_sidebar = true;
        app.left = super::LeftMode::Sessions;
        app.window_width = WINDOW.width;
        let theme = if light { "light" } else { "dark" };
        themed(light, |h| {
            let _ = h.draw(&app, "menu-session-warm");
            let closed = h.draw(&app, "menu-session-closed");
            // (the other session's row, which is not the open one: its name is in the sidebar)
            let at = find_text_row(h, &app, "UTP");
            assert_eq!(open_menu(h, &mut app, at), Some(Target::Session("local".into(), "s2".into())), "the row's menu opens ({theme})");
            let open = h.draw(&app, &format!("menu-session-{theme}"));
            assert_ne!(open, closed, "the menu is drawn ({theme})");
        });
        let rows = context_menu::session_rows("local", "s2", Some("/home/me/UTP"));
        assert_eq!(rows_of(&rows), ["Open", "Copy the folder's path", "\u{2014}", "!Forget this session"]);
        assert!(matches!(rows.last(), Some(Row::Danger { message: Message::ForgetSession(m, s), .. }) if m == "local" && s == "s2"));
        assert!(matches!(find(&rows, "Open"), Some(Row::Item { message: Message::OpenSession(m, s), bold: true, .. }) if m == "local" && s == "s2"));
    }
}

/// Where a session's row is drawn: the centre of the row that holds its name
fn find_text_row(h: &mut Harness, app: &App, name: &str) -> Point {
    let _ = h.draw(app, "menu-find");
    // the sidebar's rows are buttons: press at the left and read which session the click opens
    for y in (60..800).step_by(6) {
        let messages = h.click(app, Point::new(120.0, y as f32));
        if messages.iter().any(|m| matches!(m, Message::OpenSession(_, s) if app.sessions.iter().any(|((_, id), v)| id == s && v["name"] == name))) {
            return Point::new(120.0, y as f32);
        }
    }
    panic!("no row for {name}");
}

// ----- the submenu, the keys, Esc

#[test]
fn proof_tools_opens_its_submenu_beside_the_cell_menu_and_an_item_there_acts() {
    for light in [true, false] {
        let mut app = page_app();
        let i = formal(&app, 0);
        let theme = if light { "light" } else { "dark" };
        themed(light, |h| {
            let _ = h.draw(&app, "menu-warm");
            let cell = rect_at(h, &app, super::view::test_block_id(i));
            let at = Point::new(cell.x + 60.0, cell.y + 12.0);
            assert_eq!(open_menu(h, &mut app, at), Some(Target::Cell(i)));
            let _ = h.draw(&app, "menu-warm");
            let plain = h.draw(&app, "menu-cell-plain");
            // the pointer over the Proof tools row brings the submenu out; over another row, back in
            let mut found = None;
            for dy in (10..300).step_by(6) {
                let p = Point::new(at.x + 60.0, at.y + dy as f32);
                let messages = h.move_to(&app, p);
                apply(&mut app, messages);
                if app.shell.context.as_ref().is_some_and(|o| o.sub.is_some()) {
                    found = Some(p);
                    break;
                }
            }
            let over = found.unwrap_or_else(|| panic!("a row opens a submenu ({theme})"));
            let out = h.draw(&app, &format!("menu-submenu-{theme}"));
            assert_ne!(out, plain, "the submenu is drawn ({theme})");
            // an item of it, to the right of the cell menu's own width
            let menu_width = f32::from(super::menubar::width(&context_menu::cell_rows(ws(&app), &*app.table("local"), i)));
            let pressed = h.click(&app, Point::new(at.x + menu_width + 40.0, over.y + 14.0));
            assert!(
                pressed.iter().any(|m| matches!(m, Message::Shell(Msg::ContextDo(inner)) if matches!(inner.as_slice(), [Message::Panel(super::Panel::Tool)]))),
                "its first item opens the tools panel ({theme}): {pressed:?}"
            );
            apply(&mut app, pressed);
            assert!(app.shell.context.is_none(), "choosing an item closes the menu");
        });
    }
}

#[test]
fn esc_closes_a_menu_and_a_chosen_item_does_its_message() {
    let mut app = page_app();
    update(&mut app, Message::Shell(Msg::ContextOpen(Target::Cell(3), Point::new(10.0, 10.0))));
    assert!(app.shell.context.is_some());
    update(&mut app, Message::Escape);
    assert!(app.shell.context.is_none(), "Esc closes it");
    // an item closes the menu and then does what it says
    update(&mut app, Message::Shell(Msg::ContextOpen(Target::Cell(3), Point::new(10.0, 10.0))));
    update(&mut app, Message::Shell(Msg::ContextDo(vec![Message::SelectBlock(5)])));
    assert!(app.shell.context.is_none());
    assert_eq!(theory(&app).selected, Some(5), "Show its goals selects the cell, which is then the goal's");
}

#[test]
fn alt_up_and_alt_down_move_the_selected_cell_and_ctrl_c_copies_it() {
    let mut app = page_app();
    let (add_comm, broken) = (formal_named("lemma add_comm"), formal_named("lemma broken"));
    update(&mut app, Message::SelectBlock(broken));
    update(&mut app, Message::MoveSelected(false));
    assert_eq!(theory(&app).selected, Some(add_comm));
    assert!(theory(&app).local.find("lemma broken").unwrap() < theory(&app).local.find("lemma add_comm").unwrap());
    // (a cell being edited is not moved from outside)
    update(&mut app, Message::EditBlock(add_comm));
    let before = theory(&app).local.clone();
    update(&mut app, Message::MoveSelected(true));
    assert_eq!(theory(&app).local, before);
    let keys = include_str!("main.rs");
    for key in ["ArrowUp) if modifiers.alt()", "ArrowDown) if modifiers.alt()", "K::Character(\"c\") if modifiers.command()"] {
        assert_eq!(keys.matches(key).count(), 1, "{key}");
    }
}

#[test]
fn forget_this_session_asks_the_host_to_discard_it() {
    let mut app = page_app();
    let before = app.sessions.len();
    update(&mut app, Message::ForgetSession("local".into(), "s2".into()));
    // (the host's session_removed event is what takes it from the list: nothing is dropped before)
    assert_eq!(app.sessions.len(), before);
    let removed = app.on_event("local".into(), serde_json::json!({"event": "session_removed", "session": "s2"}));
    let _ = removed;
    assert_eq!(app.sessions.len(), before - 1);
}

#[test]
fn edits_copy_as_markdown_takes_the_selection_else_the_selected_cell() {
    let mut app = page_app();
    let menu = |app: &App| super::menubar::menus(app).into_iter().find(|(t, _)| *t == "Edit").unwrap().1;
    // a selected cell
    let i = formal(&app, 0);
    update(&mut app, Message::SelectBlock(i));
    assert!(matches!(find(&menu(&app), "Copy as Markdown"), Some(Row::Item { message: Message::CopyText(s), .. }) if s.starts_with("```isabelle\nlemma add_zero")));
    // a selection in the open editor
    update(&mut app, Message::EditBlock(i));
    update(&mut app, Message::BlockAction(Action::Move(Motion::DocumentStart)));
    update(&mut app, Message::BlockAction(Action::SelectWord));
    assert!(matches!(find(&menu(&app), "Copy as Markdown"), Some(Row::Item { message: Message::CopyText(s), .. }) if s == "```isabelle\nlemma\n```"));
    // nothing to copy
    update(&mut app, Message::CloseBlock);
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().selected = None;
    assert!(matches!(find(&menu(&app), "Copy as Markdown"), Some(Row::Off { why, .. }) if why.contains("Select")));
}

// ----- the two backends

#[test]
fn lean_says_what_it_cannot_do_and_has_its_own_have() {
    let app = super::snapshot_lean::lean_app(Mode::Literate);
    let goals = context_menu::goal_rows(ws(&app), 2);
    let msg = |rows: &[Row], label: &str| match find(rows, label) {
        Some(Row::Item { message, .. }) => message.clone(),
        other => panic!("{label}: {other:?}"),
    };
    // Lean's goal: `case`, the hypotheses, `⊢`; its have is `have : P`; Loogle takes `⊢ P`
    assert!(matches!(msg(&goals, "Copy as a have"), Message::CopyText(s) if s == "have : x :: xs ++ [] = x :: xs"));
    assert!(matches!(msg(&goals, "Copy"), Message::CopyText(s) if s == "case cons\nx : Nat\nxs : List Nat\nih : xs ++ [] = xs\n\u{22a2} x :: xs ++ [] = x :: xs"), "{:?}", find(&goals, "Copy"));
    assert!(matches!(msg(&goals, "Copy as Markdown"), Message::CopyText(s) if s.starts_with("```lean\ncase cons\n")));
    assert!(matches!(msg(&goals, "Search theorems for it\u{2026}"), Message::FindFor(q, false) if q == "\u{22a2} x :: xs ++ [] = x :: xs"));
    // there is no Sledgehammer: the row stays, disabled, and says where Lean's searches are
    let table = app.table("local");
    let t = theory(&app);
    let i = t.blocks.iter().position(|b| b.kind == Kind::Formal && super::view::goal_block(t, b)).unwrap();
    let rows = context_menu::cell_rows(ws(&app), &*table, i);
    assert!(matches!(find(&rows, "Run a proof search"), Some(Row::Off { why, .. }) if why.contains("exact?")), "{:?}", rows_of(&rows));
    assert!(find(&rows, "Run sledgehammer").is_none());
    // selected names are searched for by Loogle as they are, not as a quoted term
    let rows = context_menu::selection_rows(ws(&app), Some("Nat.add_comm".into()), None);
    assert!(matches!(msg(&rows, "Find uses"), Message::FindFor(q, true) if q == "Nat.add_comm"));
    // Markdown from a Lean cell is a lean block
    assert!(context_menu::cell_markdown(ws(&app), &*table, i).unwrap().starts_with("```lean\n"));
}

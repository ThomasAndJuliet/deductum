// SPDX-License-Identifier: GPL-3.0-only

//! The one-line fields as the household's short text boxes, through the real window, headless
//! (see `snapshot.rs`): Ctrl+Z in a field undoes its typing and goes no further, the app's keys
//! outside fields and the proof editor's undo are as they were, and each kind of field keeps
//! what it did (a search's Clear, a number's digits, a setting's value, a reply in a cell)

use cosmic::Application;
use cosmic::iced::advanced::widget::{Id, Operation, operation};
use cosmic::iced::event::Status;
use cosmic::iced::keyboard::key::{Code, Physical};
use cosmic::iced::keyboard::{self, Key, Location, Modifiers};
use cosmic::iced::{Event, Point, Rectangle, Size, mouse};
use household_widgets::short_text_box::Action;

use super::editor::Mode;
use super::fields::Field;
use super::shell::Msg;
use super::snapshot::{Harness, demo_app};
use super::{App, Message, Panel};

const WINDOW: Size = Size::new(1400.0, 900.0);

/// The text fields on the page, where each is
struct Fields(Vec<Rectangle>);

impl Operation for Fields {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text_input(&mut self, _id: Option<&Id>, bounds: Rectangle, _state: &mut dyn operation::TextInput) {
        self.0.push(bounds);
    }
}

fn fields(h: &mut Harness, app: &App) -> Vec<Rectangle> {
    let mut fields = Fields(Vec::new());
    h.operate(app, &mut fields);
    fields.0
}

/// The field in the right-hand panel (the Symbols panel's search, the tool's settings) that
/// `pick` chooses among them
fn panel_field(h: &mut Harness, app: &App, pick: impl Fn(&Rectangle) -> bool) -> Point {
    let edge = WINDOW.width - app.right_width;
    let r = fields(h, app).into_iter().filter(|r| r.x >= edge).find(pick).expect("a field in the right-hand panel");
    Point::new(r.x + r.width - 8.0, r.y + r.height / 2.0)
}

/// Messages done to the app, as the runtime would
fn apply(h: &mut Harness, app: &mut App, messages: Vec<Message>) {
    for m in messages {
        let task = app.update(m);
        h.run_task_with_updates(app, task);
    }
}

/// Text typed a key at a time, each key's messages done before the next key, as the runtime does
fn type_in(h: &mut Harness, app: &mut App, text: &str) {
    for c in text.chars() {
        let typed = h.type_text(app, &c.to_string());
        apply(h, app, typed);
    }
}

/// A key pressed with `modifiers` (Ctrl+Z: no text): the event, what the widgets published, and
/// whether one of them took it
fn press(h: &mut Harness, app: &App, letter: &str, code: Code, modifiers: Modifiers) -> (Event, Vec<Message>, Status) {
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character(letter.into()),
        modified_key: Key::Character(letter.into()),
        physical_key: Physical::Code(code),
        location: Location::Standard,
        modifiers,
        text: (!modifiers.control()).then(|| letter.into()),
        repeat: false,
    });
    let (messages, taken) = h.events_taken(app, std::slice::from_ref(&event));
    (event, messages, taken[0])
}

fn symbols(app: &App) -> &str {
    app.workspace.as_ref().unwrap().symbol_search.value()
}

/// Ctrl+Z in a field undoes the field's typing, and is the field's: no widget passes it on, so
/// the app's own keys (the proof's Undo) never see it; Ctrl+Shift+Z and Ctrl+Y redo
#[test]
fn ctrl_z_in_a_field_undoes_its_typing_and_goes_no_further() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let proof = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    let at = panel_field(&mut h, &app, |_| true);
    let clicked = h.click(&app, at);
    apply(&mut h, &mut app, clicked);
    type_in(&mut h, &mut app, "leq");
    assert_eq!(symbols(&app), "leq");
    for (letter, code, modifiers, action, after) in [
        ("z", Code::KeyZ, Modifiers::CTRL, Action::Undo, ""),
        ("z", Code::KeyZ, Modifiers::CTRL | Modifiers::SHIFT, Action::Redo, "leq"),
        ("z", Code::KeyZ, Modifiers::CTRL, Action::Undo, ""),
        ("y", Code::KeyY, Modifiers::CTRL, Action::Redo, "leq"),
    ] {
        let (event, messages, taken) = press(&mut h, &app, letter, code, modifiers);
        assert_eq!(taken, Status::Captured, "{letter} with {modifiers:?} is the field's");
        assert!(super::app_key(event, taken, cosmic::iced::window::Id::NONE).is_none(), "the app's keys do not see it");
        assert!(matches!(messages.as_slice(), [Message::Field(Field::Symbols, a)] if *a == action), "{messages:?}");
        apply(&mut h, &mut app, messages);
        assert_eq!(symbols(&app), after);
    }
    assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local, proof, "the proof is as it was");
}

/// With no field focused the keys go where they went: Ctrl+Z is the app's (its window key, the
/// proof's undo in an open cell), and a shortcut such as Ctrl+= zooms
#[test]
fn outside_fields_the_apps_keys_go_where_they_went() {
    let mut h = Harness::new(WINDOW);
    let app = demo_app(Mode::Literate);
    let (event, messages, taken) = press(&mut h, &app, "z", Code::KeyZ, Modifiers::CTRL);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(taken, Status::Ignored);
    assert!(matches!(super::app_key(event, taken, cosmic::iced::window::Id::NONE), Some(Message::WindowKey(p)) if p.key == Key::Character("z".into())));
    let (event, _, taken) = press(&mut h, &app, "=", Code::Equal, Modifiers::CTRL);
    assert!(matches!(super::app_key(event, taken, cosmic::iced::window::Id::NONE), Some(Message::Zoom(1))));
}

/// The proof editor keeps its own undo, the prover host's: Ctrl+Z in an open cell is the app's
/// Undo, as before, and the fields' boxes are not touched
#[test]
fn the_proof_editors_undo_is_unchanged() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
    let task = app.update(Message::EditBlock(i));
    h.run_task_with_updates(&mut app, task);
    h.focus(&app, super::view::block_editor_id());
    for (modifiers, want) in [(Modifiers::CTRL, Message::Undo), (Modifiers::CTRL | Modifiers::SHIFT, Message::Redo)] {
        let (_, messages, taken) = press(&mut h, &app, "z", Code::KeyZ, modifiers);
        assert_eq!(taken, Status::Captured);
        assert!(matches!((messages.as_slice(), &want), ([Message::Undo], Message::Undo) | ([Message::Redo], Message::Redo)), "{messages:?}");
    }
    let (_, messages, _) = press(&mut h, &app, "y", Code::KeyY, Modifiers::CTRL);
    assert!(matches!(messages.as_slice(), [Message::Redo]), "{messages:?}");
}

/// A search field: its clear button only while there is something to clear, and Clear is a step
/// the field's Undo takes back
#[test]
fn a_search_fields_clear_is_undone_by_its_undo() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let empty = h.draw(&app, "fields-search-empty");
    let _ = app.set_field(Field::Symbols, "leq".into());
    assert_ne!(h.draw(&app, "fields-search-leq"), empty);
    // (the clear button is at the field's right end: a press there clears)
    let at = panel_field(&mut h, &app, |_| true) - cosmic::iced::Vector::new(8.0, 0.0);
    let pressed = h.click(&app, at);
    assert!(pressed.iter().any(|m| matches!(m, Message::SetField(Field::Symbols, t) if t.is_empty())), "{pressed:?}");
    apply(&mut h, &mut app, pressed);
    assert_eq!(symbols(&app), "");
    let task = app.update(Message::Field(Field::Symbols, Action::Undo));
    h.run_task_with_updates(&mut app, task);
    assert_eq!(symbols(&app), "leq");
    // (empty, it is drawn as it was: no clear button)
    drop(h);
    let mut fresh = Harness::new(WINDOW);
    let _ = app.set_field(Field::Symbols, String::new());
    assert_eq!(fresh.draw(&app, "fields-search-empty-again"), empty);
}

/// A number setting of a proof tool takes digits only, and what is typed is the tool's setting
#[test]
fn a_tool_number_field_takes_digits_and_sets_the_setting() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    app.workspace.as_mut().unwrap().panel = Panel::Tool;
    // (the Timeout field is the narrow one; Provers is as wide as the panel)
    let at = panel_field(&mut h, &app, |r| r.width < 100.0);
    let clicked = h.click(&app, at);
    apply(&mut h, &mut app, clicked);
    type_in(&mut h, &mut app, "4x5");
    let ws = app.workspace.as_ref().unwrap();
    assert_eq!(super::fields::tool(ws, "sledgehammer", "timeout").value(), "45");
    assert_eq!(ws.tool_settings.get("sledgehammer.timeout").map(String::as_str), Some("45"));
    let (_, messages, _) = press(&mut h, &app, "z", Code::KeyZ, Modifiers::CTRL);
    apply(&mut h, &mut app, messages);
    assert_eq!(app.workspace.as_ref().unwrap().tool_settings.get("sledgehammer.timeout").map(String::as_str), Some(""), "Undo puts the setting back too");
    // Typed fast, the keys given together before the app answers any: the letter is still refused
    let keys: Vec<Event> = "4x5"
        .chars()
        .map(|c| {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Character(c.to_string().into()),
                modified_key: Key::Character(c.to_string().into()),
                physical_key: Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: Some(c.to_string().into()),
                repeat: false,
            })
        })
        .collect();
    let (messages, _) = h.events_taken(&app, &keys);
    apply(&mut h, &mut app, messages);
    let ws = app.workspace.as_ref().unwrap();
    assert_eq!(super::fields::tool(ws, "sledgehammer", "timeout").value(), "45", "however fast it is typed");
    assert_eq!(ws.tool_settings.get("sledgehammer.timeout").map(String::as_str), Some("45"));
}

/// A setting typed in (Claude's model): what is typed is the setting; putting the row back to its
/// default empties the field, a step the field's Undo takes back, setting and all
#[test]
fn a_settings_field_is_the_setting_and_its_reset_is_undone() {
    let _turn = super::snapshot::ui_turn();
    let mut app = demo_app(Mode::Literate);
    let _ = app.shell_update(Msg::Settings(true));
    let _ = app.set_field(Field::ClaudeModel, "opus".into());
    assert_eq!(app.shell.settings.llm.claude_model, "opus");
    let _ = app.shell_update(Msg::ClaudeModel(String::new()));
    assert_eq!(app.shell.page.claude_model.value(), "");
    let _ = app.update(Message::Field(Field::ClaudeModel, Action::Undo));
    assert_eq!(app.shell.page.claude_model.value(), "opus");
    assert_eq!(app.shell.settings.llm.claude_model, "opus");
    // (opened again, the field starts as the setting is, with nothing to undo)
    let _ = app.shell_update(Msg::Settings(false));
    let _ = app.shell_update(Msg::Settings(true));
    assert_eq!(app.shell.page.claude_model.value(), "opus");
    assert!(!household_widgets::Undoable::can_undo(&app.shell.page.claude_model));
}

/// A reply to an agent's note, in its cell: a right press on the field opens the field's own
/// text menu, not the cell's; Send takes the reply and the next one starts empty
#[test]
fn a_reply_field_in_a_cell_has_its_own_text_menu() {
    let mut h = Harness::new(Size::new(WINDOW.width, 1600.0));
    let mut app = demo_app(Mode::Literate);
    let edge = WINDOW.width - app.right_width;
    let r = fields(&mut h, &app).into_iter().find(|r| r.x < edge && r.x > app.left_width).expect("the reply field on the page");
    let at = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
    let _ = h.move_to(&app, at);
    let (messages, taken) = h.events_taken(&app, &[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
    assert!(!messages.iter().any(|m| matches!(m, Message::Shell(Msg::ContextOpen(..)))), "{messages:?}");
    assert_eq!(taken, [Status::Captured], "the field's menu took it");
    let _ = h.events(&app, &[Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right))]);
    // (beside the field, in the same cell, the cell's own menu opens as before)
    let _ = h.events(&app, &[Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(keyboard::key::Named::Escape),
        modified_key: Key::Named(keyboard::key::Named::Escape),
        physical_key: Physical::Code(Code::Escape),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    })]);
    let _ = h.move_to(&app, Point::new(r.x + 4.0, r.y - 12.0));
    let (messages, _) = h.events_taken(&app, &[Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
    assert!(messages.iter().any(|m| matches!(m, Message::Shell(Msg::ContextOpen(..)))), "{messages:?}");
    // The reply is sent and the field starts again
    let _ = app.set_field(Field::Reply("n1".into()), "thanks".into());
    assert_eq!(super::fields::reply(&app.workspace.as_ref().unwrap().note_reply, "n1").value(), "thanks");
    let _ = app.update(Message::NoteReply("n1".into(), false));
    assert_eq!(super::fields::reply(&app.workspace.as_ref().unwrap().note_reply, "n1").value(), "");
}

/// A field with the keys has the muted accent border in the window in front, the rest colour
/// while its window is behind others (rule 25a)
#[test]
fn a_focused_fields_border_follows_its_window() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let at = panel_field(&mut h, &app, |_| true);
    let clicked = h.click(&app, at);
    apply(&mut h, &mut app, clicked);
    let _ = h.move_to(&app, Point::new(5.0, 5.0));
    super::fields::set_window_behind(false);
    let front = h.draw(&app, "fields-focused-front");
    super::fields::set_window_behind(true);
    let behind = h.draw(&app, "fields-focused-behind");
    super::fields::set_window_behind(false);
    let theme = cosmic::theme::active();
    let muted: [u8; 3] = {
        let [r, g, b, _] = super::tones::muted(&theme).0.into_rgba8();
        [r, g, b]
    };
    let count = |frame: &[u8]| frame.chunks(4).filter(|p| p[0].abs_diff(muted[0]) < 3 && p[1].abs_diff(muted[1]) < 3 && p[2].abs_diff(muted[2]) < 3).count();
    assert!(count(&front) > count(&behind) + 100, "the muted accent ring in front ({}), not behind ({})", count(&front), count(&behind));
}

/// The text field `element` holds, as laid out alone in a window
fn laid_out(element: cosmic::Element<'_, Message>) -> Rectangle {
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
    super::load_fonts();
    let mut renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(cosmic::iced::Font::default(), cosmic::iced::Pixels(14.0), Some("tiny-skia"))).expect("a headless renderer");
    let mut ui = UserInterface::build(element, Size::new(800.0, 200.0), Cache::default(), &mut renderer);
    let mut fields = Fields(Vec::new());
    ui.operate(&renderer, &mut fields);
    fields.0.first().copied().expect("a text field")
}

/// The adoption changes no field's size: each field is laid out as main's libcosmic input with
/// the same settings, empty and with text (the widths the app's fields set)
#[test]
fn each_field_is_the_size_mains_input_was() {
    use cosmic::iced::Length;
    use cosmic::widget;
    let _turn = super::snapshot::ui_turn();
    let set = |_: String| Message::SetField(Field::Find, String::new());
    for value in ["", "abc"] {
        let text_box = household_widgets::ShortTextBox::starting_with(value);
        for width in [None, Some(Length::Fill), Some(Length::Fixed(340.0)), Some(Length::Fixed(200.0)), Some(Length::Fixed(64.0))] {
            let adopted = laid_out(super::fields::text(&text_box, Field::Find, "", |f| match width {
                Some(width) => f.width(width),
                None => f,
            }));
            let main = widget::text_input("", value.to_string()).on_input(set);
            let main = laid_out(match width {
                Some(width) => main.width(width).into(),
                None => main.into(),
            });
            assert_eq!(adopted.size(), main.size(), "a text field {width:?} ({value:?})");
        }
        // The search fields: their clear button only with something to clear (Juliet's approved
        // choice); on main the palette's was so, the Symbols' and Settings' always there: a
        // search field is the same size either way
        let adopted = laid_out(super::fields::search(&text_box, Field::Symbols, "", |f| f));
        let while_typed = widget::search_input("", value.to_string()).on_input(set);
        let while_typed = laid_out(if value.is_empty() { while_typed.into() } else { while_typed.on_clear(set(String::new())).into() });
        let always = laid_out(widget::search_input("", value.to_string()).on_input(set).on_clear(set(String::new())).into());
        assert_eq!(adopted.size(), while_typed.size(), "the palette's search as on main ({value:?})");
        assert_eq!(adopted.size(), always.size(), "the Symbols and Settings searches as on main ({value:?})");
    }
}

/// In the window, the Symbols panel's search is the size main's was, typed in or not, though its
/// clear button shows only with text
#[test]
fn the_symbols_search_is_the_size_it_was() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let field = |h: &mut Harness, app: &App| {
        let edge = WINDOW.width - app.right_width;
        fields(h, app).into_iter().find(|r| r.x >= edge).expect("the Symbols search")
    };
    let empty = field(&mut h, &app);
    let _ = app.set_field(Field::Symbols, "leq".into());
    let typed = field(&mut h, &app);
    assert_eq!(empty.size(), typed.size(), "typing does not change its size");
}

/// The Settings page's search is the size main's was, typed in or not
#[test]
fn the_settings_search_is_the_size_it_was() {
    let mut h = Harness::new(WINDOW);
    let mut app = demo_app(Mode::Literate);
    let _ = app.shell_update(Msg::Settings(true));
    let field = |h: &mut Harness, app: &App| fields(h, app).into_iter().min_by(|a, b| a.y.total_cmp(&b.y)).expect("the Settings search");
    let empty = field(&mut h, &app);
    let _ = app.set_field(Field::SettingsSearch, "size".into());
    let typed = field(&mut h, &app);
    assert_eq!(empty.size(), typed.size(), "typing does not change its size");
}

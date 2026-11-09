// SPDX-License-Identifier: GPL-3.0-only

//! Deductum's one-line fields are the household's short text boxes (household-widgets
//! `short_text_box`): each keeps its value with an undo history, takes the standard keys, is drawn
//! in the household's normal look (household-ui rule 25a: 1 px at rest, 2 px in the muted accent
//! when it has the keys, the rest colour while its window is behind others) and has the household
//! text menu, Cut · Copy · Paste · Select all, then Undo · Redo. Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y
//! are a field's only while it has the keys; otherwise they go where they went before (the proof's
//! undo). Juliet, 6 Nov 2026: "You were asked to implement a input text box. This obviously
//! includes the usual functionality of such a box which includes ctrl + z."
//!
//! Here, once: which fields there are ([`Field`]), where each one's box is kept, what an edit of
//! one changes beyond its value, and how a field is built. Deductum has no multi-line text box:
//! its editors are the document itself, drawn on the page (no box around them, their selection
//! the tint, rule 6: "use the tint for deductum code"), with the prover host's undo.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use cosmic::app::Task;
use cosmic::widget::text_context_menu::TextMenuCommand;
use cosmic::{Element, widget};
use household_widgets::menu::MenuStyle;
use household_widgets::short_text_box::{self, Action, Caret};
use household_widgets::{ShortTextBox, TextMenuHistory, TextMenuWords, Undoable};

use super::shell::Msg;
use super::{App, Message, Tool, Workspace};

/// A one-line field of Deductum's, by what it is for
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// The insert palette's search, beside the text, and the insert bar's above the goal
    Palette,
    /// The Symbols panel's search
    Symbols,
    /// The Search panel's query (Find Theorems, Loogle)
    Find,
    /// The sidebar's filter of the session's documents
    Documents,
    /// The counterexample panel's parameters (Nitpick, Quickcheck)
    Counterexample,
    /// The reply being written to an agent's note, by the note's id
    Reply(String),
    /// A proof tool's setting typed in (a number, a name), by tool and setting
    Tool(String, String),
    /// The new-session page's folder, Isabelle's base logic and its session directories
    NewFolder,
    NewLogic,
    NewDirs,
    /// The Settings page's search
    SettingsSearch,
    /// The Settings page's model names for the agents' keyword lines
    ClaudeModel,
    CodexModel,
}

/// A field's box before anything is typed in it: a reply to a note, until it is written to
static EMPTY: LazyLock<ShortTextBox> = LazyLock::new(ShortTextBox::new);

/// Whether the window whose view is being built is behind others (published as the view is
/// built, as the vim mode is: the fields are built far from the window). A window's fields then
/// have the rest colour when they have the keys (rule 25a)
static BEHIND: AtomicBool = AtomicBool::new(false);

pub fn set_window_behind(behind: bool) {
    BEHIND.store(behind, Ordering::Relaxed);
}

fn window_behind() -> bool {
    BEHIND.load(Ordering::Relaxed)
}

/// A field over `text_box` (as libcosmic's `text_input`): its options set by `finish` (its width,
/// what Enter does), in its window's look, with its menu
pub fn text<'a>(
    text_box: &'a ShortTextBox,
    field: Field,
    placeholder: impl Into<Cow<'a, str>>,
    finish: impl FnOnce(short_text_box::Field<'a, Message>) -> short_text_box::Field<'a, Message>,
) -> Element<'a, Message> {
    let on_action = on_action(field);
    with_menu(finish(short_text_box::text(text_box, placeholder, on_action.clone())), text_box, on_action)
}

/// A search field over `text_box` (as libcosmic's `search_input`), with its clear button while
/// there is something to clear; otherwise as [`text`]
pub fn search<'a>(
    text_box: &'a ShortTextBox,
    field: Field,
    placeholder: impl Into<Cow<'a, str>>,
    finish: impl FnOnce(short_text_box::Field<'a, Message>) -> short_text_box::Field<'a, Message>,
) -> Element<'a, Message> {
    let on_action = on_action(field.clone());
    let mut search = short_text_box::search(text_box, placeholder, on_action.clone());
    if !text_box.is_empty() {
        search = search.on_clear(Message::SetField(field, String::new()));
    }
    with_menu(finish(search), text_box, on_action)
}

fn on_action(field: Field) -> impl Fn(Action) -> Message + Clone + 'static {
    move |action| Message::Field(field.clone(), action)
}

/// `field` in its window's look, in the household text menu with its box's Undo and Redo
fn with_menu<'a>(field: short_text_box::Field<'a, Message>, text_box: &ShortTextBox, on_action: impl Fn(Action) -> Message) -> Element<'a, Message> {
    household_widgets::text_menu(field.behind(window_behind()), &words(), style()).history(TextMenuHistory::of(text_box, on_action)).into()
}

/// The text menu's words
fn words() -> TextMenuWords {
    TextMenuWords {
        cut: "Cut".into(),
        copy: "Copy".into(),
        paste: "Paste".into(),
        select_all: "Select all".into(),
        no_selection: "Select some text first".into(),
        hidden: "A hidden password is not copied".into(),
        read_only: "This text cannot be changed here".into(),
        clipboard_empty: "There is no text to paste".into(),
        empty_content: "There is no text yet".into(),
        undo: "Undo".into(),
        redo: "Redo".into(),
        nothing_to_undo: "There is nothing to undo".into(),
        nothing_to_redo: "There is nothing to redo".into(),
    }
}

/// The text menu's rows, as Deductum's menus draw them: the desktop's icons, body text
fn style() -> MenuStyle<TextMenuCommand> {
    fn icon(name: &'static str) -> widget::icon::Handle {
        widget::icon::from_name(name).handle()
    }
    fn body(text: String) -> Element<'static, TextMenuCommand> {
        widget::text::body(text).into()
    }
    MenuStyle { icon, body }
}

/// The box of a note's reply: the one being written, else an empty one
pub fn reply<'a>(replies: &'a HashMap<String, ShortTextBox>, note: &str) -> &'a ShortTextBox {
    replies.get(note).unwrap_or(&EMPTY)
}

/// The box of a proof tool's setting typed in (an empty one until the tool's settings are known)
pub fn tool<'a>(ws: &'a Workspace, tool: &str, key: &str) -> &'a ShortTextBox {
    ws.tool_fields.get(&Tool::setting_key(tool, key)).unwrap_or(&EMPTY)
}

/// The boxes of the proof tools' settings that are typed in, each starting as the setting is
/// (its default until it is set); the boxes `kept` already has stay as they are, with their history
pub fn tool_fields<'a>(tools: impl IntoIterator<Item = &'a Tool>, values: &HashMap<String, String>, mut kept: HashMap<String, ShortTextBox>) -> HashMap<String, ShortTextBox> {
    let mut boxes = HashMap::new();
    for tool in tools {
        for s in tool.settings.iter().filter(|s| s.kind != "bool") {
            let key = Tool::setting_key(&tool.name, &s.key);
            let text_box = kept.remove(&key).unwrap_or_else(|| ShortTextBox::starting_with(tool.value(values, &s.key)));
            boxes.insert(key, text_box);
        }
    }
    boxes
}

/// What `edit` makes a field's value, whatever the box holds now
fn edited_value(edit: &short_text_box::Edit) -> String {
    let mut probe: ShortTextBox = ShortTextBox::new();
    probe.perform(Action::Edit(edit.clone()));
    probe.value().to_string()
}

impl App {
    /// The box of `field`, where it is kept; none while what it belongs to is not open
    fn field_box(&mut self, field: &Field) -> Option<&mut ShortTextBox> {
        match field {
            Field::SettingsSearch | Field::ClaudeModel | Field::CodexModel => self.shell.page.field_mut(field),
            Field::NewFolder => self.new_session.as_mut().map(|ns| &mut ns.folder),
            Field::NewLogic => self.new_session.as_mut().map(|ns| &mut ns.logic),
            Field::NewDirs => self.new_session.as_mut().map(|ns| &mut ns.dirs),
            _ => {
                let ws = self.workspace.as_mut()?;
                match field {
                    Field::Palette => Some(&mut ws.palette_query),
                    Field::Symbols => Some(&mut ws.symbol_search),
                    Field::Find => Some(&mut ws.find_input),
                    Field::Documents => Some(&mut ws.theory_filter),
                    Field::Counterexample => Some(&mut ws.cex_params),
                    Field::Reply(note) => Some(ws.note_reply.entry(note.clone()).or_default()),
                    Field::Tool(tool, key) => ws.tool_fields.get_mut(&Tool::setting_key(tool, key)),
                    _ => None,
                }
            }
        }
    }

    /// The field's own message: what it was asked to do (typing, Undo and Redo, from its keys or
    /// its menu)
    pub fn field_action(&mut self, field: Field, action: Action) -> Task<Message> {
        // (a number's field takes digits only: an edit that would put anything else in it is not
        // made)
        if let (Field::Tool(tool, key), Action::Edit(edit)) = (&field, &action)
            && self.is_number_setting(tool, key)
            && !edited_value(edit).chars().all(|c| c.is_ascii_digit())
        {
            return Task::none();
        }
        let Some(text_box) = self.field_box(&field) else { return Task::none() };
        let before = text_box.value().to_string();
        text_box.perform(action);
        self.field_changed(field, before)
    }

    /// The app's own change of a field's value (Clear, a value the app worked out): one step
    /// that the field's Undo takes back
    pub fn set_field(&mut self, field: Field, text: String) -> Task<Message> {
        let Some(text_box) = self.field_box(&field) else { return Task::none() };
        let before = text_box.value().to_string();
        text_box.replace(&text, Caret::End);
        self.field_changed(field, before)
    }

    fn is_number_setting(&self, tool: &str, key: &str) -> bool {
        let ws = self.workspace.as_ref();
        ws.and_then(|ws| ws.caps.tool(tool)).and_then(|t| t.settings.iter().find(|s| s.key == key)).is_some_and(|s| s.kind == "number")
    }

    /// What a field's new value changes beyond the field, once it differs from `before`
    fn field_changed(&mut self, field: Field, before: String) -> Task<Message> {
        let Some(value) = self.field_box(&field).map(|b| b.value().to_string()) else { return Task::none() };
        if value == before {
            return Task::none();
        }
        match field {
            // A folder typed is another folder: what was found in the last one no longer holds
            Field::NewFolder => {
                if let Some(ns) = &mut self.new_session {
                    ns.invalidate_folder();
                }
            }
            Field::Tool(tool, key) => {
                if let Some(ws) = &mut self.workspace {
                    ws.tool_settings.insert(Tool::setting_key(&tool, &key), value);
                }
            }
            Field::ClaudeModel => return self.shell_update(Msg::ClaudeModel(value)),
            Field::CodexModel => return self.shell_update(Msg::CodexModel(value)),
            // (the others' value is the box's, read where it is used)
            _ => {}
        }
        Task::none()
    }

    /// A model setting changed (typed, or put back to its default): its field follows, a step
    /// Undo takes back
    pub fn model_field_follows(&mut self, field: Field, value: &str) {
        if let Some(text_box) = self.field_box(&field)
            && text_box.value() != value
        {
            text_box.replace(value, Caret::End);
        }
    }
}

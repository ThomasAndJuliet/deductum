// SPDX-License-Identifier: GPL-3.0-only

//! The menu bar (household-ui rule 2; Tom, 8 Nov, t29): File, Edit, View, Prover and Help, folding
//! into one menu button when the window is narrow. What the header no longer holds is here: undo
//! and redo, Back, the folder, Zed, terminal and Settings buttons. An item for something not built
//! yet is there, disabled, and its tooltip says why (rule 17). The rows are plain facts
//! ([`Row`]) built from the app, so that tests can read them; [`bar`] draws them with libcosmic's
//! menu bar, as Cerea and the terminal do.

use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget::{self, menu};
use cosmic::theme;

use super::chip::{self, State};
use super::editor::Mode;
use super::settings_page::Sub;
use super::shell::{Msg, sp};
use super::{App, Message, Panel, link};

/// Below this window width the menus fold into one button
pub const FOLD_WIDTH: f32 = 1050.0;

/// One row of a menu
#[derive(Clone, Debug)]
pub enum Row {
    /// What it does when chosen, with its shortcut ("" for none)
    Item { label: String, keys: &'static str, message: Message, bold: bool },
    /// An option, ticked while it is on
    Check { label: String, keys: &'static str, on: bool, message: Message },
    /// What cannot be done now or is not built yet: shown disabled, its tooltip saying why
    Off { label: String, keys: &'static str, why: String },
    /// A submenu
    Sub { label: String, rows: Vec<Row> },
    Divider,
}

fn item(label: &str, keys: &'static str, message: Message) -> Row {
    Row::Item { label: label.into(), keys, message, bold: false }
}

fn check(label: &str, keys: &'static str, on: bool, message: Message) -> Row {
    Row::Check { label: label.into(), keys, on, message }
}

fn off(label: &str, keys: &'static str, why: &str) -> Row {
    Row::Off { label: label.into(), keys, why: why.into() }
}

/// A row that acts, or else is disabled saying why
fn act_or(label: &str, keys: &'static str, message: Result<Message, &str>) -> Row {
    match message {
        Ok(message) => item(label, keys, message),
        Err(why) => off(label, keys, why),
    }
}

/// The labels of rows, "(label)" for a disabled one, "label ▸" for a submenu and "—" for a divider
pub fn labels(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            Row::Item { label, .. } => label.clone(),
            Row::Check { label, on, .. } => format!("{}{label}", if *on { "\u{2713} " } else { "" }),
            Row::Off { label, .. } => format!("({label})"),
            Row::Sub { label, .. } => format!("{label} \u{25b8}"),
            Row::Divider => "\u{2014}".to_string(),
        })
        .collect()
}

/// The row labelled `label`, in a menu or its submenus
pub fn find<'a>(rows: &'a [Row], label: &str) -> Option<&'a Row> {
    rows.iter().find_map(|r| match r {
        Row::Item { label: l, .. } | Row::Check { label: l, .. } | Row::Off { label: l, .. } if l == label => Some(r),
        Row::Sub { label: l, rows } => if l == label { Some(r) } else { find(rows, label) },
        _ => None,
    })
}

/// File's items for agent sessions: not on Windows (Tom, 8 Nov), where there are none
pub fn agent_file_rows(windows: bool) -> Vec<Row> {
    if windows {
        return Vec::new();
    }
    Vec::new()
}

/// Prover's item for the agents' rights: not on Windows
pub fn agent_prover_rows(windows: bool) -> Vec<Row> {
    if windows {
        return Vec::new();
    }
    Vec::new()
}

/// The menus, as the open session lets them be used
pub fn menus(app: &App) -> Vec<(&'static str, Vec<Row>)> {
    let ws = app.workspace.as_ref();
    let t = ws.and_then(|w| w.theory.as_ref()).filter(|_| !app.shell.settings_open);
    let state = chip::state_of(app);
    let folder = ws.and_then(|w| w.info.as_ref()).and_then(|i| i["folder"].as_str()).is_some();
    let no_doc = "Open a document first";

    // ----- File
    let mut recent: Vec<(String, String, String)> = app
        .sessions
        .iter()
        .map(|((m, id), v)| (v["name"].as_str().unwrap_or(id).to_string(), m.clone(), id.clone()))
        .collect();
    recent.sort();
    let recent_row = if recent.is_empty() {
        off("Open recent", "", "No session has been opened yet")
    } else {
        Row::Sub {
            label: "Open recent".into(),
            rows: recent.into_iter().map(|(name, m, id)| item(&name, "", Message::OpenSession(m, id))).collect(),
        }
    };
    let open_in = |label: &str, opener: link::Opener| act_or(label, "", if folder { Ok(Message::OpenFolder(opener)) } else { Err("Open a session first") });
    let mut file = vec![
        item("New session\u{2026}", "", Message::NewSession(true)),
        // (the new-session form is where a folder is chosen)
        item("Open a folder\u{2026}", "", Message::NewSession(true)),
        recent_row,
        Row::Divider,
        act_or("Save", "Ctrl+S", if t.is_some() { Ok(Message::Save) } else { Err(no_doc) }),
        off("Save all", "", "Not built yet: Save saves the open document, and the host autosaves the others"),
        Row::Divider,
        Row::Sub {
            label: "Open in".into(),
            rows: vec![open_in("Files", link::Opener::Files), open_in("Zed", link::Opener::Zed), open_in("A terminal", link::Opener::Terminal)],
        },
    ];
    file.extend(agent_file_rows(cfg!(windows)));
    file.extend([
        Row::Divider,
        act_or("Close session", "", if ws.is_some() { Ok(Message::CloseWorkspace) } else { Err("No session is open") }),
        item("Quit", "", Message::Shell(Msg::Quit)),
    ]);

    // ----- Edit
    let selection = if t.is_some() { Ok(()) } else { Err(no_doc) };
    let edit = vec![
        act_or("Undo", "Ctrl+Z", match t {
            Some(t) if t.can_undo => Ok(Message::Undo),
            Some(_) => Err("Nothing to undo"),
            None => Err(no_doc),
        }),
        act_or("Redo", "Ctrl+Shift+Z", match t {
            Some(t) if t.can_redo => Ok(Message::Redo),
            Some(_) => Err("Nothing to redo"),
            None => Err(no_doc),
        }),
        Row::Divider,
        act_or("Cut", "Ctrl+X", selection.map(|_| Message::CopySelection(true))),
        act_or("Copy", "Ctrl+C", selection.map(|_| Message::CopySelection(false))),
        off("Copy as Markdown", "", "Not built yet: copying a cell or a selection as Markdown is a new feature"),
        off("Paste", "Ctrl+V", "Press Ctrl+V with the caret in a cell: the menu cannot yet tell which editor to paste into"),
        off("Select all", "Ctrl+A", "Press Ctrl+A with the caret in a cell: the menu cannot yet tell which editor to select in"),
        Row::Divider,
        off("Find", "", "Not built yet: searching inside the document (theorems are found under Prover \u{25b8} Search theorems)"),
        off("Find and replace", "", "Not built yet"),
        Row::Divider,
        item("Settings", "Ctrl+,", Message::Shell(Msg::ToggleSettings)),
    ];

    // ----- View
    let mode = |m: Mode| act_or(if m == Mode::Literate { "Literate" } else { "Source" }, if m == Mode::Literate { "Ctrl+1" } else { "Ctrl+2" }, if t.is_some() { Ok(Message::SetMode(m)) } else { Err(no_doc) });
    let on = |m: Mode| t.is_some_and(|t| t.mode == m);
    let view = vec![
        check("Sidebar", "F9", app.show_sidebar, Message::ToggleSidebar),
        check("Proof panel", "", app.show_right, Message::ToggleRight),
        check("Goals beside the proof", "", super::view::goal_beside_on(), Message::ToggleGoalBeside),
        Row::Divider,
        match mode(Mode::Literate) {
            Row::Item { label, keys, message, .. } => Row::Check { label, keys, on: on(Mode::Literate), message },
            other => other,
        },
        match mode(Mode::Source) {
            Row::Item { label, keys, message, .. } => Row::Check { label, keys, on: on(Mode::Source), message },
            other => other,
        },
        Row::Divider,
        item("Larger text", "Ctrl+=", Message::Zoom(1)),
        item("Smaller text", "Ctrl+\u{2212}", Message::Zoom(-1)),
        item("Default text size", "Ctrl+0", Message::Zoom(0)),
        Row::Divider,
        act_or("Back", "Alt+Left", if app.back.is_empty() { Err("No Ctrl+click jump to go back from") } else { Ok(Message::Back) }),
        off("Full screen", "F11", "Not built yet"),
    ];

    // ----- Prover
    let (failed, unfinished) = t.map_or((0, 0), super::attention::counts);
    let first = chip::controls_for(&state);
    let first_label = match state {
        State::Paused => "Resume",
        State::Stopped => "Start",
        State::Crashed | State::NoSession => "Pause",
        _ => "Pause",
    };
    let restart = match state {
        State::Stopped | State::Crashed => Ok(Message::Lifecycle("resume")),
        State::NoSession => Err("No session is open"),
        _ => Err("The prover is running: Restart is for a stopped or crashed prover"),
    };
    let running_tool = ws.is_some_and(|w| {
        w.activity.as_ref().and_then(|a| a["running"].as_array()).is_some_and(|r| r.iter().any(|x| x["name"].as_str().is_some_and(|n| w.caps.typed_tool(n).is_some())))
    });
    let busy = ws.is_some_and(|w| w.activity.as_ref().and_then(|a| a["running"].as_array()).is_some_and(|r| !r.is_empty()));
    let problem = |label: &str, keys: &'static str, n: usize, kind: super::attention::Problem, none: &str| {
        act_or(label, keys, if n > 0 { Ok(Message::Shell(Msg::NextProblem(kind))) } else { Err(none) })
    };
    let mut prover = vec![
        act_or(first_label, "", match first.first.2 {
            Some(op) => Ok(Message::Lifecycle(op)),
            None => Err("No session is open"),
        }),
        act_or("Stop", "", if first.stop.1 { Ok(Message::Lifecycle("stop")) } else { Err(first.stop.0) }),
        act_or("Restart", "", restart),
        Row::Divider,
        problem("Next failure", "F8", failed, super::attention::Problem::Failed, "No cell has failed"),
        problem("Next unfinished", "Shift+F8", unfinished, super::attention::Problem::Unfinished, "No cell is unfinished"),
        Row::Divider,
        Row::Sub {
            label: "Proof tools".into(),
            rows: vec![
                act_or("Open the tools panel", "", if ws.is_some() { Ok(Message::Panel(Panel::Tool)) } else { Err("No session is open") }),
                act_or(
                    "Interrupt running proofs",
                    "",
                    if ws.is_some_and(|w| w.caps.interrupt) && busy { Ok(Message::Lifecycle("cancel")) } else { Err("No proof is running, or this prover cannot interrupt") },
                ),
                act_or("Stop the running tool", "", if running_tool { Ok(Message::StopTools) } else { Err("No tool is running") }),
            ],
        },
        act_or("Search theorems\u{2026}", "", if ws.is_some() { Ok(Message::Panel(Panel::Search)) } else { Err("No session is open") }),
    ];
    prover.extend(agent_prover_rows(cfg!(windows)));

    // ----- Help
    let help = vec![
        item("Keyboard shortcuts", "", Message::Shell(Msg::SettingsAt(Sub::Shortcuts))),
        item("About Deductum", "", Message::Shell(Msg::SettingsAt(Sub::About))),
    ];

    vec![("File", file), ("Edit", edit), ("View", view), ("Prover", prover), ("Help", help)]
}

// ----- drawing

const TICK: f32 = 16.0;

/// One row of a menu as libcosmic draws its own: the label, the shortcut dimmed at the right, and
/// a tick column when the menu has options
fn entry(label: &str, keys: &str, tick: Option<bool>, bold: bool, message: Option<Message>) -> widget::Button<'static, Message> {
    let mut parts: Vec<Element<'static, Message>> = Vec::new();
    if let Some(on) = tick {
        parts.push(if on {
            widget::icon::from_name("object-select-symbolic").size(16).icon().into()
        } else {
            widget::Space::new().width(Length::Fixed(TICK)).into()
        });
        parts.push(widget::Space::new().width(Length::Fixed(sp().space_xxs as f32)).into());
    }
    let text = widget::text(label.to_string());
    parts.push(if bold { text.font(cosmic::font::bold()).into() } else { text.into() });
    parts.push(widget::Space::new().width(Length::Fill).into());
    if !keys.is_empty() {
        parts.push(widget::text(keys.to_string()).class(super::view::muted()).into());
    }
    menu::menu_button(parts).on_press_maybe(message)
}

/// How wide the menus must be for every label and shortcut to show whole
pub fn width(rows: &[Row]) -> u16 {
    let chars = |label: &str, keys: &str| (label.chars().count() + keys.chars().count()) as f32 * 8.0 + 90.0;
    rows.iter()
        .map(|r| match r {
            Row::Item { label, keys, .. } | Row::Check { label, keys, .. } | Row::Off { label, keys, .. } => chars(label, keys),
            Row::Sub { label, rows } => chars(label, "").max(f32::from(width(rows))),
            Row::Divider => 0.0,
        })
        .fold(220.0, f32::max)
        .ceil() as u16
}

/// The rows as libcosmic menu trees
pub fn tree(rows: Vec<Row>) -> Vec<menu::Tree<Message>> {
    let n = rows.len();
    let ticks = rows.iter().any(|r| matches!(r, Row::Check { .. }));
    rows.into_iter()
        .enumerate()
        .filter_map(|(i, row)| match row {
            Row::Item { label, keys, message, bold } => Some(menu::Tree::from(Element::from(entry(&label, keys, ticks.then_some(false), bold, Some(message))))),
            Row::Check { label, keys, on, message } => Some(menu::Tree::from(Element::from(entry(&label, keys, Some(on), false, Some(message))))),
            Row::Off { label, keys, why } => Some(menu::Tree::from(Element::from(widget::tooltip(
                entry(&label, keys, ticks.then_some(false), false, None),
                widget::text(why),
                widget::tooltip::Position::Right,
            )))),
            Row::Sub { label, rows } => {
                let button = menu::menu_button::<Message>(vec![
                    widget::text(label).into(),
                    widget::Space::new().width(Length::Fill).into(),
                    widget::icon::from_name("go-next-symbolic").size(16).icon().into(),
                ])
                .class(theme::Button::MenuFolder);
                Some(menu::Tree::with_children(widget::RcElementWrapper::new(Element::from(button)), tree(rows)))
            }
            // A divider at either end separates nothing
            Row::Divider if i != 0 && i + 1 != n => Some(menu::Tree::from(Element::from(widget::divider::horizontal::light()))),
            Row::Divider => None,
        })
        .collect()
}

/// A button that opens a menu of rows (the folded menu bar, the Open in button's arrow)
pub fn dropdown(root: Element<'static, Message>, rows: Vec<Row>) -> Element<'static, Message> {
    let w = width(&rows);
    // In a row (a container passes on its content's tag), so that a window crossing the width where
    // the menus fold makes the bar anew: libcosmic's menu bar lays out its roots from children it
    // never brings up to date, and a button root laid out with a text root's state panicked
    widget::row::with_children(vec![
        menu::bar(vec![menu::Tree::with_children(widget::RcElementWrapper::new(root), tree(rows))])
            .item_height(menu::ItemHeight::Dynamic(36))
            .item_width(menu::ItemWidth::Uniform(w))
            .into(),
    ])
    .into()
}

/// The menu bar: File, Edit, View, Prover and Help, or one menu button holding them when the
/// window is narrow
pub fn bar(app: &App) -> Element<'static, Message> {
    let menus = menus(app);
    let w = menus.iter().map(|(_, rows)| width(rows)).max().unwrap_or(220);
    if app.window_width < FOLD_WIDTH {
        let rows = menus.into_iter().map(|(title, rows)| Row::Sub { label: title.to_string(), rows }).collect();
        let button = widget::button::icon(widget::icon::from_name("open-menu-symbolic")).padding([4, 12]).class(theme::Button::MenuRoot);
        let root = widget::tooltip(button, widget::text("Menu: File, Edit, View, Prover, Help"), widget::tooltip::Position::Bottom);
        return dropdown(root.into(), rows);
    }
    let roots = menus
        .into_iter()
        .map(|(title, rows)| menu::Tree::with_children(widget::RcElementWrapper::new(Element::from(menu::root(title))), tree(rows)))
        .collect();
    menu::bar(roots).item_height(menu::ItemHeight::Dynamic(36)).item_width(menu::ItemWidth::Uniform(w)).spacing(2.0).into()
}

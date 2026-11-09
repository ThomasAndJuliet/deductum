// SPDX-License-Identifier: GPL-3.0-only

//! Right-click menus (household-ui rules 27-32; Tom, 8 Nov, mockups/t30): on a cell, on a goal,
//! on selected code and on a session's row. The first item is what a click does, then the other
//! actions on the thing, copying, moving and arranging, and the destructive ones last, in red.
//!
//! libcosmic's `menu::bar` and `context_menu` open from the overlay layer with coordinates the
//! page's translated containers (the goal panel, the scrolled page) do not share, so they do not
//! open there. [`area`] is a small widget of its own: a right press over its content opens the
//! menu at the pointer, as an overlay placed from the content's own bounds and the translation
//! the overlay was given, which is how the popover places itself, so it opens wherever the thing
//! under the pointer is drawn.
//!
//! The rows are plain facts ([`Row`], as the menu bar's are) built from the open session, so
//! that tests read them; an action a backend cannot do is a disabled row whose tooltip says why.

use std::sync::Mutex;

use cosmic::iced::advanced::overlay;
use cosmic::iced::advanced::widget::{Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use cosmic::iced::{Event, Length, Point, Rectangle, Size, Vector};
use cosmic::prelude::*;
use cosmic::{theme, widget};

use super::codec::Symbols;
use super::editor::Mode;
use super::menubar::{self, Row, act_or, item, off};
use super::shell::Msg;
use super::theory::Kind;
use super::{EditorTarget, Message, Panel, Workspace};

/// What a menu is for
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A cell shown on the literate page, by block index (a selection in it makes the menu the
    /// selection's)
    Cell(usize),
    /// The editor of the open cell, or the source view
    Editor(EditorTarget),
    /// A subgoal in the proof state, by its number
    Goal(usize),
    /// A session's row, by machine and id
    Session(String, String),
}

/// The menu that is open: what it is for, where the pointer was (in the coordinates of the thing
/// pressed) and which submenu is out
#[derive(Clone, Debug)]
pub struct Open {
    pub target: Target,
    pub at: Point,
    pub sub: Option<usize>,
}

/// The open menu as the view last saw it (published when the view is built, like the vim mode and
/// the zoom: the widgets that carry menus are built far from the app)
static CURRENT: Mutex<Option<Open>> = Mutex::new(None);

pub fn publish(open: Option<&Open>) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = open.cloned();
}

fn current() -> Option<Open> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ----- the widget

/// `content`, which opens a menu of `rows` at the pointer on a right press. `rows` is called only
/// while this thing's menu is the open one
pub fn area<'a>(content: impl Into<Element<'a, Message>>, target: Target, rows: impl FnOnce() -> Vec<Row>) -> Element<'a, Message> {
    let open = current().filter(|o| o.target == target);
    let popup = open.as_ref().map(|o| card(rows(), o.sub));
    Element::new(Area { content: content.into(), popup, at: open.map(|o| o.at), target })
}

struct Area<'a> {
    content: Element<'a, Message>,
    popup: Option<Element<'a, Message>>,
    at: Option<Point>,
    target: Target,
}

/// Whether `at` is over one of `content`'s text fields (each has its text menu)
fn over_text_field(content: &mut Element<'_, Message>, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, at: Point) -> bool {
    struct Fields {
        at: Point,
        over: bool,
    }
    impl Operation for Fields {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn text_input(&mut self, _id: Option<&cosmic::widget::Id>, bounds: Rectangle, _state: &mut dyn cosmic::iced::advanced::widget::operation::TextInput) {
            self.over |= bounds.contains(self.at);
        }
    }
    let mut fields = Fields { at, over: false };
    content.as_widget_mut().operate(tree, layout, renderer, &mut fields);
    fields.over
}

impl Widget<Message, cosmic::Theme, cosmic::Renderer> for Area<'_> {
    fn children(&self) -> Vec<Tree> {
        match &self.popup {
            Some(popup) => vec![Tree::new(&self.content), Tree::new(popup)],
            None => vec![Tree::new(&self.content)],
        }
    }

    fn diff(&mut self, tree: &mut Tree) {
        match &mut self.popup {
            Some(popup) => tree.diff_children(&mut [&mut self.content, popup]),
            None => tree.diff_children(&mut [&mut self.content]),
        }
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Before the content sees it: an editor takes a right press for itself. A text field's
        // own menu (Cut, Copy, Paste, Undo) is the field's, though: the content has it
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event
            && !shell.is_event_captured()
            && let Some(p) = cursor.position_over(layout.bounds())
            && !over_text_field(&mut self.content, &mut tree.children[0], layout, renderer, p)
        {
            let b = layout.bounds();
            shell.publish(Message::Shell(Msg::ContextOpen(self.target.clone(), Point::new(p.x - b.x, p.y - b.y))));
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &cosmic::Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(&self, tree: &Tree, renderer: &mut cosmic::Renderer, theme: &cosmic::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        mut translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        let (Some(popup), Some(at)) = (&mut self.popup, self.at) else {
            return self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation);
        };
        // (from the content's own bounds and the translation it was given: where it is drawn)
        let b = layout.bounds();
        translation.x += (b.x + at.x).round();
        translation.y += (b.y + at.y).round();
        Some(overlay::Element::new(Box::new(MenuOverlay { tree: &mut tree.children[1], content: popup, pos: Point::new(translation.x, translation.y) })))
    }
}

impl<'a> From<Area<'a>> for Element<'a, Message> {
    fn from(area: Area<'a>) -> Self {
        Element::new(area)
    }
}

/// The menu, laid out where it was asked and kept in the window, above the pointer when it would
/// run off the bottom; a press outside it closes it
struct MenuOverlay<'a, 'b> {
    tree: &'a mut Tree,
    content: &'a mut Element<'b, Message>,
    pos: Point,
}

impl overlay::Overlay<Message, cosmic::Theme, cosmic::Renderer> for MenuOverlay<'_, '_> {
    fn layout(&mut self, renderer: &cosmic::Renderer, bounds: Size) -> layout::Node {
        let node = self.content.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::UNIT, bounds));
        let size = node.size();
        let mut at = self.pos;
        at.x = at.x.clamp(0.0, (bounds.width - size.width).max(0.0));
        if at.y + size.height > bounds.height {
            at.y = (at.y - size.height).clamp(0.0, (bounds.height - size.height).max(0.0));
        }
        node.move_to(Point::new(at.x.round(), at.y.round()))
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(self.tree, layout, renderer, operation);
    }

    fn update(&mut self, event: &Event, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &cosmic::Renderer, clipboard: &mut dyn Clipboard, shell: &mut Shell<'_, Message>) {
        self.content.as_widget_mut().update(self.tree, event, layout, cursor, renderer, clipboard, shell, &layout.bounds());
        // A press anywhere else closes the menu, and goes no further (a submenu's own overlay
        // has taken its own presses by now)
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(cosmic::iced::touch::Event::FingerPressed { .. }))
            && !shell.is_event_captured()
            && !cursor.is_over(layout.bounds())
        {
            shell.publish(Message::Shell(Msg::ContextClose));
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &cosmic::Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(self.tree, layout, cursor, &layout.bounds(), renderer)
    }

    fn draw(&self, renderer: &mut cosmic::Renderer, theme: &cosmic::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        self.content.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
    }

    fn overlay<'c>(&'c mut self, layout: Layout<'c>, renderer: &cosmic::Renderer) -> Option<overlay::Element<'c, Message, cosmic::Theme, cosmic::Renderer>> {
        self.content.as_widget_mut().overlay(self.tree, layout, renderer, &layout.bounds(), Vector::ZERO)
    }
}

// ----- the card

/// A menu's rows on a card: the dropdown surface, a row to each item (36 px, as every menu's),
/// hairlines between the groups. A press on an item closes the menu and does it
fn card(rows: Vec<Row>, sub: Option<usize>) -> Element<'static, Message> {
    let width = f32::from(menubar::width(&rows));
    let n = rows.len();
    let mut col = widget::column::with_capacity(n).width(Length::Fixed(width));
    for (i, row) in rows.into_iter().enumerate() {
        // (hovering any row but a submenu's draws that one in)
        let leave = |el: Element<'static, Message>| -> Element<'static, Message> {
            widget::mouse_area(el).on_enter(Message::Shell(Msg::ContextSub(None))).into()
        };
        col = col.push(match row {
            Row::Item { label, keys, message, bold } => leave(entry(&label, keys, bold, false, Some(do_it(message))).into()),
            Row::Check { label, keys, message, .. } => leave(entry(&label, keys, false, false, Some(do_it(message))).into()),
            Row::Danger { label, keys, message } => leave(entry(&label, keys, false, true, Some(do_it(message))).into()),
            Row::Off { label, keys, why } => leave(household_widgets::tooltip_with(entry(&label, keys, false, false, None), widget::text(why), widget::tooltip::Position::Right).into()),
            Row::Sub { label, rows } => {
                let button = entry_sub(&label);
                let hover = widget::mouse_area(button).on_enter(Message::Shell(Msg::ContextSub(Some(i))));
                let out = widget::popover(hover).position(widget::popover::Position::Point(Point::new(width - 4.0, 0.0)));
                if sub == Some(i) { out.popup(card(rows, None)).into() } else { out.into() }
            }
            // A divider at either end separates nothing
            Row::Divider if i != 0 && i + 1 != n => widget::divider::horizontal::light().into(),
            Row::Divider => widget::Space::new().height(Length::Fixed(0.0)).into(),
        });
    }
    widget::container(col).padding(4).class(theme::Container::Dropdown).into()
}

/// What a press on an item sends: the menu closes, then the message is handled
fn do_it(message: Message) -> Message {
    Message::Shell(Msg::ContextDo(vec![message]))
}

fn entry(label: &str, keys: &str, bold: bool, red: bool, on_press: Option<Message>) -> cosmic::widget::Button<'static, Message> {
    let mut text = widget::text(label.to_string());
    if bold {
        text = text.font(cosmic::font::bold());
    }
    if red {
        text = text.class(theme::Text::Color(super::view::error_color(&theme::active())));
    }
    let mut parts: Vec<Element<'static, Message>> = vec![text.into(), widget::Space::new().width(Length::Fill).into()];
    if !keys.is_empty() {
        parts.push(widget::text(keys.to_string()).class(super::view::muted()).into());
    }
    widget::menu::menu_button(parts).on_press_maybe(on_press)
}

fn entry_sub(label: &str) -> cosmic::widget::Button<'static, Message> {
    widget::menu::menu_button::<Message>(vec![
        widget::text(label.to_string()).into(),
        widget::Space::new().width(Length::Fill).into(),
        widget::icon::from_name("go-next-symbolic").size(16).icon().into(),
    ])
    .class(theme::Button::MenuFolder)
}

// ----- what the actions copy and insert

/// A fenced block of `text`, in the backend's language, as Markdown
pub fn fenced(backend: &str, text: &str) -> String {
    format!("```{backend}\n{}\n```", text.trim_end_matches('\n'))
}

/// A cell as Markdown: prose as the profile renders it, code as a fenced block of the backend's
/// language (its symbols as they read, not as escapes)
pub fn cell_markdown(ws: &Workspace, table: &dyn Symbols, i: usize) -> Option<String> {
    let t = ws.theory.as_ref()?;
    let b = t.blocks.get(i)?;
    let source = t.source_text(b.start, b.end);
    Some(match (b.kind, b.body) {
        (Kind::Heading(_) | Kind::Text, Some((s, e))) => {
            let md = t.profile.prose(&t.source_text(s, e), table);
            match b.kind {
                Kind::Heading(level) => format!("{} {}", "#".repeat(usize::from(level).clamp(1, 6)), md.replace('\n', " ").trim()),
                _ => md.trim().to_string(),
            }
        }
        _ => fenced(t.profile.backend(), &table.decode(&source).text),
    })
}

/// The selection of the editor `target` (an open cell, the source view), as the editor shows it
pub fn editor_selection(ws: &Workspace, target: EditorTarget) -> Option<String> {
    let t = ws.theory.as_ref()?;
    match target {
        EditorTarget::Block => t.block_editor.as_ref().and_then(|be| be.content.selection()),
        EditorTarget::Source => t.source.selection(),
    }
    .map(|s| s.replace('\u{200B}', ""))
    .filter(|s| !s.is_empty())
}

/// The selection inside a shown cell
fn cell_selection(ws: &Workspace, i: usize) -> Option<String> {
    let t = ws.theory.as_ref()?;
    t.block_views.get(i)?.as_ref()?.content.selection().map(|s| s.replace('\u{200B}', "")).filter(|s| !s.is_empty())
}

/// A subgoal as the prover's profile lays it out, a part to a line: Isabelle's `fix`, `assume`
/// and `show`, Lean's hypotheses and `⊢`. `raw` is the source (symbols as escapes), else as shown
pub fn goal_text(ws: &Workspace, sg: &super::goals::Subgoal, raw: bool) -> String {
    use super::prover::GoalBody;
    let part = |p: &serde_json::Value| -> String {
        let key = if raw { "raw" } else { "text" };
        p[key].as_str().or_else(|| p["text"].as_str()).unwrap_or("").trim().to_string()
    };
    let shown = ws.profile.present_goal(sg);
    let mut lines: Vec<String> = shown.heading.into_iter().collect();
    for section in shown.sections {
        let parts: Vec<String> = match section.body {
            GoalBody::One(p) => vec![part(p)],
            GoalBody::Many(ps) => ps.iter().map(part).collect(),
        };
        for p in parts {
            lines.push(match section.label {
                Some(label) => format!("{label} {p}"),
                None => p,
            });
        }
    }
    lines.join("\n")
}

/// A subgoal's conclusion on one line (source when `raw`)
pub fn conclusion(sg: &super::goals::Subgoal, raw: bool) -> Option<String> {
    let c = sg.concl.as_ref()?;
    let text = c[if raw { "raw" } else { "text" }].as_str().or_else(|| c["text"].as_str())?;
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one.is_empty()).then_some(one)
}

/// The query the search panel is given for a statement or a name: Isabelle's `find_theorems` takes
/// a term pattern in quotes, Lean's Loogle takes the text (a conclusion after `⊢`)
fn search_query(backend: &str, text: &str, conclusion: bool) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match (backend, conclusion) {
        ("lean", true) => format!("\u{22a2} {one}"),
        ("lean", false) => one,
        _ => format!("\"{}\"", one.replace('"', "\\\"")),
    }
}

// ----- the rows

fn goal_of(ws: &Workspace, n: usize) -> Option<super::goals::Subgoal> {
    super::goals::Goals::parse(&ws.goals.messages).subgoals.into_iter().find(|s| s.n == n)
}

/// The rows every menu has for Ask an agent: not on Windows
#[cfg(not(windows))]
fn ask_agent(select: Option<Message>) -> Vec<Row> {
    let open = Message::Shell(Msg::AgentOpen(true));
    vec![item("Ask an agent about it\u{2026}", "", open).into_do(select)]
}

#[cfg(windows)]
fn ask_agent(_select: Option<Message>) -> Vec<Row> {
    Vec::new()
}

impl Row {
    /// An item that first does `before`, if there is one (the cell is chosen, then the action)
    fn into_do(self, before: Option<Message>) -> Row {
        match (self, before) {
            (Row::Item { label, keys, message, bold }, Some(before)) => Row::Item { label, keys, message: chain(vec![before, message]), bold },
            (row, _) => row,
        }
    }
}

/// Several messages as one, for a row that has to choose its cell before acting on it
fn chain(messages: Vec<Message>) -> Message {
    Message::Shell(Msg::ContextDo(messages))
}

/// The rows of the proof tools submenu
fn proof_tools(ws: &Workspace) -> Row {
    let running = ws.activity.as_ref().and_then(|a| a["running"].as_array()).is_some_and(|r| r.iter().any(|x| x["name"].as_str().is_some_and(|n| ws.caps.typed_tool(n).is_some())));
    Row::Sub {
        label: "Proof tools".into(),
        rows: vec![
            item("Open the tools panel", "", Message::Panel(Panel::Tool)),
            act_or("Stop the running tool", "", if running { Ok(Message::StopTools) } else { Err("No tool is running") }),
        ],
    }
}

/// The menu of a cell shown on the page: its first item is what a click on it does (edit it)
pub fn cell_rows(ws: &Workspace, table: &dyn Symbols, i: usize) -> Vec<Row> {
    let Some(t) = ws.theory.as_ref() else { return Vec::new() };
    let Some(b) = t.blocks.get(i) else { return Vec::new() };
    // A selection in the cell: the menu is the selection's
    if let Some(sel) = cell_selection(ws, i) {
        return selection_rows(ws, Some(sel), None);
    }
    let formal = b.kind == Kind::Formal;
    let source = t.source_text(b.start, b.end);
    let pick = Message::SelectBlock(i);
    let mut rows = vec![item("Edit", "Enter", Message::EditBlock(i))];
    if formal {
        let goals = super::view::goal_block(t, b);
        rows.push(act_or("Show its goals", "", if goals { Ok(pick.clone()) } else { Err("This cell has no proof state") }));
        rows.push(item("Open in its own window", "", chain(vec![Message::EditBlock(i), Message::PopOut])));
        rows.push(Row::Divider);
        // Sledgehammer works on the goal at the cell's unfinished step, or its last; Lean has no
        // such tool (its search tactics are typed into the proof, from the insert bar)
        rows.push(match ws.caps.panel_tool() {
            Some(tool) if goals => {
                let at = b.commands.iter().find(|c| t.profile.unfinished().contains(&c.name.as_str())).or(b.commands.last()).map_or(b.end.saturating_sub(1), |c| c.start);
                item(&format!("Run {}", tool.name), "", Message::RunToolAt(tool.name.clone(), at))
            }
            Some(tool) => off(&format!("Run {}", tool.name), "", "This cell has no goal to run it on"),
            None => off("Run a proof search", "", "This prover has no tool that runs on a cell: its search tactics (exact?, apply?) are typed into the proof from the insert bar"),
        });
        rows.push(proof_tools(ws));
        rows.extend(ask_agent(Some(pick)));
    }
    rows.push(Row::Divider);
    // (the cell's source, as Ctrl+C copies a selected cell)
    rows.push(item("Copy", "Ctrl+C", Message::CopyText(source)));
    rows.push(match cell_markdown(ws, table, i) {
        Some(md) => item("Copy as Markdown", "", Message::CopyText(md)),
        None => off("Copy as Markdown", "", "Nothing to copy"),
    });
    rows.push(Row::Divider);
    rows.push(act_or("Insert text above", "", if i == 0 { Err("Text goes after the theory's header") } else { Ok(Message::InsertBlock(Some(i - 1), "text")) }));
    rows.push(item("Insert lemma below", "", Message::InsertBlock(Some(i), "lemma")));
    rows.push(item("Insert definition below", "", Message::InsertBlock(Some(i), "definition")));
    rows.push(item("Insert cell below", "", Message::InsertBlock(Some(i), "cell")));
    let movable = |k: Kind| !matches!(k, Kind::Header | Kind::End);
    let up = i.checked_sub(1).and_then(|j| t.blocks.get(j));
    let down = t.blocks.get(i + 1);
    rows.push(act_or("Move up", "Alt+\u{2191}", match up {
        _ if !movable(b.kind) => Err("This cell is fixed"),
        Some(p) if movable(p.kind) => Ok(Message::MoveBlock(i, false)),
        _ => Err("It is the first cell"),
    }));
    rows.push(act_or("Move down", "Alt+\u{2193}", match down {
        _ if !movable(b.kind) => Err("This cell is fixed"),
        Some(n) if movable(n.kind) => Ok(Message::MoveBlock(i, true)),
        _ => Err("It is the last cell"),
    }));
    if b.kind != Kind::Header {
        rows.push(Row::Divider);
        rows.push(Row::Danger { label: "Delete".into(), keys: "", message: Message::DeleteBlock(i) });
    }
    rows
}

/// The menu of selected code, in an editor: `selection` is what is selected; `target` the editor
/// when it can be typed in (Cut and Paste), none for a cell that is only shown
pub fn selection_rows(ws: &Workspace, selection: Option<String>, target: Option<EditorTarget>) -> Vec<Row> {
    use cosmic::widget::text_editor::Binding;
    let backend = ws.profile.backend();
    let sel = selection.as_deref();
    let none = "Select some code first";
    let encode = |s: &str| -> String { ws.profile.symbols(None).encode(s) };
    let mut rows = Vec::new();
    rows.push(match (sel, target) {
        (Some(_), Some(t)) => item("Copy", "Ctrl+C", Message::NavBinding(t, Box::new(Binding::Copy))),
        (Some(s), None) => item("Copy", "Ctrl+C", Message::CopyText(encode(s))),
        (None, _) => off("Copy", "Ctrl+C", none),
    });
    rows.push(match sel {
        Some(s) => item("Copy as Markdown", "", Message::CopyText(fenced(backend, s))),
        None => off("Copy as Markdown", "", none),
    });
    match target {
        Some(t) => {
            rows.push(if sel.is_some() { item("Cut", "Ctrl+X", Message::NavBinding(t, Box::new(Binding::Cut))) } else { off("Cut", "Ctrl+X", none) });
            rows.push(item("Paste", "Ctrl+V", Message::NavBinding(t, Box::new(Binding::Paste))));
        }
        None => {
            let why = "This cell is shown, not open: press Enter to edit it";
            rows.push(off("Cut", "Ctrl+X", why));
            rows.push(off("Paste", "Ctrl+V", why));
        }
    }
    rows.push(Row::Divider);
    rows.push(match target {
        Some(_) if ws.caps.definition => item("Go to definition", "Ctrl+click", Message::GotoDefinition),
        Some(_) => off("Go to definition", "Ctrl+click", "This prover cannot say where a name is defined"),
        None => off("Go to definition", "Ctrl+click", "Press Enter to edit the cell, then put the caret in the name"),
    });
    // Find uses: the facts and lemmas of the context that mention the selected name, searched at
    // once (Isabelle: find_theorems for the name as a term; Lean: Loogle's search by constant)
    let name = sel.map(str::trim).filter(|s| !s.is_empty() && !s.contains(char::is_whitespace));
    rows.push(match (&ws.caps.search, name) {
        (None, _) => off("Find uses", "", "This prover has no search"),
        (Some(_), Some(n)) => item("Find uses", "", Message::FindFor(search_query(backend, n, false), true)),
        (Some(_), None) => off("Find uses", "", "Select one name to find where it is used"),
    });
    rows.push(match (&ws.caps.search, sel) {
        (None, _) => off("Search theorems for it\u{2026}", "", "This prover has no search"),
        (Some(_), Some(s)) => item("Search theorems for it\u{2026}", "", Message::FindFor(search_query(backend, s, false), false)),
        (Some(_), None) => off("Search theorems for it\u{2026}", "", none),
    });
    let agent = ask_agent(None);
    if !agent.is_empty() {
        rows.push(Row::Divider);
        rows.extend(agent);
    }
    rows
}

/// The menu of the open cell's editor or the source view
pub fn editor_rows(ws: &Workspace, target: EditorTarget) -> Vec<Row> {
    selection_rows(ws, editor_selection(ws, target), Some(target))
}

/// The menu of a subgoal in the proof state: its first item is what a click on a goal means, to
/// make it the next step
pub fn goal_rows(ws: &Workspace, n: usize) -> Vec<Row> {
    let Some(sg) = goal_of(ws, n) else { return Vec::new() };
    let backend = ws.profile.backend();
    let have = conclusion(&sg, true).map(|c| ws.profile.have_line(&c));
    let t = ws.theory.as_ref();
    // The step goes at the caret, so a cell has to be open (or the source view shown)
    let editing = t.is_some_and(|t| super::insert::block_in_use(t, false) || t.mode == Mode::Source);
    let mut rows = vec![match (conclusion(&sg, false), editing) {
        (None, _) => off("Use as the next step", "", "This goal has no conclusion to state"),
        (Some(_), false) => off("Use as the next step", "", "Open the cell (Enter) and put the caret in its proof: the step goes on a new line after the caret's"),
        (Some(c), true) => item("Use as the next step", "", Message::UseStep(c)),
    }];
    rows.push(item("Show the prover's text", "", Message::StateText(true)));
    rows.push(Row::Divider);
    rows.push(item("Copy", "", Message::CopyText(goal_text(ws, &sg, true))));
    rows.push(item("Copy as Markdown", "", Message::CopyText(fenced(backend, &goal_text(ws, &sg, false)))));
    rows.push(match have {
        Some(h) => item("Copy as a have", "", Message::CopyText(h)),
        None => off("Copy as a have", "", "This goal has no conclusion to state"),
    });
    rows.push(Row::Divider);
    rows.push(match (&ws.caps.search, conclusion(&sg, false)) {
        (None, _) => off("Search theorems for it\u{2026}", "", "This prover has no search"),
        (Some(_), Some(c)) => item("Search theorems for it\u{2026}", "", Message::FindFor(search_query(backend, &c, true), false)),
        (Some(_), None) => off("Search theorems for it\u{2026}", "", "This goal has no conclusion to search for"),
    });
    rows
}

/// The menu of a session's row: Open, the folder's path, and Forget this session last, in red
pub fn session_rows(machine: &str, sid: &str, folder: Option<&str>) -> Vec<Row> {
    let mut rows = vec![Row::Item { label: "Open".into(), keys: "", message: Message::OpenSession(machine.into(), sid.into()), bold: true }];
    rows.push(match folder.filter(|f| !f.is_empty()) {
        Some(f) => item("Copy the folder's path", "", Message::CopyText(f.to_string())),
        None => off("Copy the folder's path", "", "The session has no folder"),
    });
    rows.push(Row::Divider);
    rows.push(Row::Danger { label: "Forget this session".into(), keys: "", message: Message::ForgetSession(machine.into(), sid.into()) });
    rows
}

/// What Edit's Copy as Markdown copies: the selection of the editor in use, else the selected
/// cell
pub fn edit_markdown(ws: &Workspace, table: &dyn Symbols) -> Result<String, &'static str> {
    let t = ws.theory.as_ref().ok_or("Open a document first")?;
    let target = if super::insert::block_in_use(t, false) { EditorTarget::Block } else { EditorTarget::Source };
    if let Some(s) = editor_selection(ws, target) {
        return Ok(fenced(ws.profile.backend(), &s));
    }
    t.selected
        .filter(|_| t.block_editor.is_none() && t.mode == Mode::Literate)
        .and_then(|i| cell_markdown(ws, table, i))
        .ok_or("Select some code, or a cell (j and k), first")
}

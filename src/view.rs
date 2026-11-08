// SPDX-License-Identifier: GPL-3.0-only

//! Deductum's window: sessions on the left, the theory in the middle in its
//! literate or source view (design C14), proof output, the prover's tools,
//! agents and history on the right. The prover's panels, tools and settings
//! show as the session's capabilities offer them (provers §5.2), laid out with
//! the profile's labels.

use std::sync::{Arc, LazyLock};

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::prelude::*;
use cosmic::widget::{self, text_editor};
use cosmic::{iced, theme};
use serde_json::Value;

use super::editor::{Mode, Theory, highlight_format};
use super::prover::{GoalBody, Setting, Tool};
use super::theory::{Block, Kind, Status};
use super::tones::{Beneath, code_selection, muted_button};
use prose_render::rich::{Typography, rich_text};
use super::{App, LeftMode, Message, Panel, Workspace, file_name};

static BLOCK_EDITOR: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);
static SOURCE_EDITOR: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);
static LITERATE_SCROLL: LazyLock<widget::Id> = LazyLock::new(widget::Id::unique);

pub fn source_editor_id() -> widget::Id {
    SOURCE_EDITOR.clone()
}

pub fn literate_scroll_id() -> widget::Id {
    LITERATE_SCROLL.clone()
}

pub fn block_editor_id() -> widget::Id {
    BLOCK_EDITOR.clone()
}

fn icon(name: &'static str) -> widget::icon::Icon {
    widget::icon::from_name(name).size(16).icon()
}

pub fn spacing() -> cosmic::cosmic_theme::Spacing {
    theme::active().cosmic().spacing
}

pub fn status_color(status: Status, theme: &cosmic::Theme) -> Color {
    let c = theme.cosmic();
    match status {
        Status::Finished => Color { a: 0.35, ..Color::from(c.success_color()) },
        Status::Warned => sorry_color(theme),
        Status::Unprocessed => Color { a: 0.3, ..Color::from(c.on_bg_color()) },
        Status::Running => c.accent_color().into(),
        Status::Failed => error_color(theme),
    }
}

/// An unfinished proof (a `sorry`): yellow. COSMIC's warning and destructive colours are almost
/// the same salmon in its dark theme, so a cell left unfinished and one that fails read alike;
/// these two are told apart by hue in both themes. Light: dark mode's hue (82.5 degrees) at OKLCH
/// lightness 0.62, the household's lightness for yellow on a light theme (S4, Juliet 8 Nov;
/// mockups/s4); dark keeps its #f0b429
pub fn sorry_color(theme: &cosmic::Theme) -> Color {
    if theme.cosmic().is_dark { Color::from_rgb8(0xf0, 0xb4, 0x29) } else { Color::from_rgb8(0xab, 0x7d, 0x01) }
}

/// A cell's status colour at full strength, in the selected cell's strip and border; none for a
/// cell not yet processed, which has no status colour of its own
fn bright_status(status: Status, theme: &cosmic::Theme) -> Option<Color> {
    let source = match status {
        Status::Finished => Color::from(theme.cosmic().success_color()),
        Status::Warned => sorry_color(theme),
        Status::Failed => error_color(theme),
        Status::Running => return Some(theme.cosmic().accent_color().into()),
        Status::Unprocessed => return None,
    };
    // (dark: one lightness and chroma, each colour's own hue)
    Some(if theme.cosmic().is_dark { super::tones::dark_status(source) } else { source })
}

/// The muted tone of a status (checked, unfinished or failed): the colour of the messages under a
/// cell, in light mode unfinished #805c00
pub fn muted_status_color(bright: Color, theme: &cosmic::Theme) -> Color {
    super::tones::muted_status(bright, theme.cosmic().is_dark)
}

/// A cell's status strip, its left edge. At rest the halfway tone between the status colour faded
/// over the page and its muted tone; selected, the status colour itself. A cell being checked is
/// the accent either way, one not yet processed a faded grey
pub fn strip_color(status: Status, selected: bool, theme: &cosmic::Theme) -> Color {
    let page = page_color(theme);
    match (status, bright_status(status, theme)) {
        (Status::Unprocessed, _) | (_, None) => super::tones::over(Color { a: 0.3, ..Color::from(theme.cosmic().on_bg_color()) }, page),
        (Status::Running, Some(accent)) => accent,
        (_, Some(bright)) if selected => bright,
        (_, Some(bright)) => super::tones::strip_at_rest(bright, page, theme.cosmic().is_dark),
    }
}

/// The 1 px border of a cell at rest, grey. Light: COSMIC's divider on the page, #bcbcbc
/// (checked by `light_card_border_is_cosmic_divider`); dark: the household's dark border, #5c5c5c
/// (household-ui's card border in dark mode), which COSMIC has no token for
pub fn card_border_color(theme: &cosmic::Theme) -> Color {
    const DARK: Color = Color::from_rgb8(0x5c, 0x5c, 0x5c);
    let c = theme.cosmic();
    if c.is_dark { DARK } else { c.primary(false).divider.into() }
}

/// A cell's border: at rest [`card_border_color`], 1 px. Selected, 2 px in its strip's bright
/// status colour, in the accent while it is checked and grey if it is not yet processed
fn card_border(status: Status, selected: bool, theme: &cosmic::Theme) -> (Color, f32) {
    if !selected {
        return (card_border_color(theme), 1.0);
    }
    (bright_status(status, theme).unwrap_or_else(|| card_border_color(theme)), 2.0)
}

/// The room a cell's border takes on each of its top, right and bottom sides whatever its state,
/// so that selecting a cell (1 px of border becoming 2) moves nothing
const CARD_RIM: f32 = 2.0;

/// The status strip, the card's left edge
const STRIP: f32 = 4.0;

/// The space between the status strip and the code, and between the code and the card's right
/// edge, in em of the code's size (so it scales with the code size and the zoom): 0.75 em, 10.5 px
/// at 14 px. Tom chose it on 8 Nov (mockups/t5e-gap-em.png: 0.78, corrected to 0.75). It is the
/// code editor's own padding, so the highlights over the code (the construct's band, the caret's
/// line) reach the card's edges: the strip on the left, the border on the right
pub const CODE_GAP_EM: f32 = 0.75;

/// What a cell takes beside its code: its status strip and the room for its right border (the gap
/// either side of the code is the code's own padding)
const CARD_CHROME: f32 = STRIP + CARD_RIM;

/// A failure: red, as against [`sorry_color`]'s yellow
pub fn error_color(theme: &cosmic::Theme) -> Color {
    if theme.cosmic().is_dark { Color::from_rgb8(0xf2, 0x5f, 0x5c) } else { Color::from_rgb8(0xc0, 0x1c, 0x28) }
}

/// The page the theory is set on. In the dark theme COSMIC's background; in the light one its
/// primary container, the lighter layer COSMIC sets content on (its background is a mid grey
/// meant for the frame around content)
pub fn page_color(theme: &cosmic::Theme) -> Color {
    let c = theme.cosmic();
    if c.is_dark { c.bg_color().into() } else { c.primary_container_color().into() }
}

/// The lightish band the window's header and sidebar and the goal panel's tools are on: COSMIC's
/// background colour in light (#d7d7d7, a step darker than the page), and in dark the colour a step
/// lighter than the page (#272727; the page is COSMIC's background, #1b1b1b)
pub fn chrome_color(theme: &cosmic::Theme) -> Color {
    let c = theme.cosmic();
    if c.is_dark { c.primary_container_color().into() } else { c.bg_color().into() }
}

/// The surface of the left sidebar and the side panel: [`chrome_color`], as the header's, the
/// plan's lightish (#d7d7d7 light, #272727 dark), edge to edge with square corners, so that
/// nothing of the window behind shows at the corners and the panels sit flush with the header
pub fn chrome_panel() -> theme::Container<'static> {
    theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(chrome_color(theme).into()),
        text_color: Some(theme.cosmic().on_bg_color().into()),
        ..Default::default()
    })
}

/// A cell (code, goals, notes) on the page: one layer up, lighter than the page in both themes,
/// as COSMIC's layers are
fn cell_color(theme: &cosmic::Theme) -> Color {
    let c = theme.cosmic();
    if c.is_dark { c.bg_component_color().into() } else { c.secondary_container_color().into() }
}

/// The region behind a paragraph being edited or selected, and its bar: no wash (a cell's
/// selection is its strip and border, and prose is not a cell), the bar in the accent
fn marked_region(theme: &cosmic::Theme) -> (Color, Option<Color>) {
    (Color::TRANSPARENT, Some(theme.cosmic().accent_color().into()))
}

/// Secondary text (a line number, a time, a hint): `tones::secondary_text`, the darkest tone on a
/// light theme, the text's colour fainter on a dark one. Taken from the
/// theme as the text is drawn, not as the view is built, so it follows the theme it is drawn in
pub fn muted() -> theme::Text {
    theme::Text::Custom(|theme| {
        let c = theme.cosmic();
        iced::widget::text::Style { color: Some(super::tones::secondary_text(theme, 0.65)), selected_fill: Color { a: 0.32, ..Color::from(c.accent.base) }, selected_text_color: None }
    })
}

/// A vertical scroll view with a thin bar and no rail, a little in from the edge (COSMIC's own is
/// an 8 px bar on a permanent rail, with 8 px of padding around the content)
fn thin<'a>(content: impl Into<Element<'a, Message>>) -> iced::widget::Scrollable<'a, Message, cosmic::Theme, cosmic::Renderer> {
    use iced::widget::scrollable::{Direction, Scrollbar};
    iced::widget::scrollable(content)
        .direction(Direction::Vertical(Scrollbar::new().width(6.0).scroller_width(6.0).margin(3.0)))
        .class(theme::iced::Scrollable::Minimal)
}

fn prose_size() -> f32 { z(super::settings::display().text_size) }

pub fn typography(theme: &cosmic::Theme) -> Typography {
    let cosmic = theme.cosmic();
    // Headings, bold text, code and captions in the darkest tone; the body in the household's tone
    // for its size (darker from 18 px, on a light theme)
    let strong: Color = cosmic.on_bg_color().into();
    let dark = cosmic.is_dark;
    let display = super::settings::display();
    let text = super::tones::body_text(theme, display.text_size);
    let link: Color = cosmic.accent_text_color().into();
    Typography {
        display_start: false,
        body: display.font.family().into(),
        body_weight: body_weight(display.font),
        mono: super::settings::code_font(),
        font_overrides: [(0xE021, 0xE07E), (0xE121, 0xE17E), (0xE211, 0xE249), (0xE311, 0xE349)]
            .into_iter().map(|(start, end)| prose_render::rich::FontOverride {
                start: char::from_u32(start).unwrap(), end: char::from_u32(end).unwrap(), family: "Isabellum Scripts".into(),
            }).collect(),
        size: display.text_size,
        line_height: display.line_height,
        paragraph_spacing: display.paragraph_spacing,
        math_scale: display.math_scale,
        justify: display.justify,
        italic: false,
        text,
        strong,
        muted: super::tones::secondary_text(theme, 0.68),
        link,
        link_strong: link,
        code: strong,
        code_background: cell_color(theme),
        page: page_color(theme),
        rule: Color { a: 0.25, ..strong },
        selection: Color { a: 0.32, ..Color::from(cosmic.accent.base) },
        error: cosmic.destructive_text_color().into(),
        added: if dark { Color::from_rgb8(0x8f, 0xd6, 0x8f) } else { Color::from_rgb8(0x1d, 0x7a, 0x2a) },
        removed: if dark { Color::from_rgb8(0xf0, 0x8a, 0x8a) } else { Color::from_rgb8(0xb0, 0x22, 0x22) },
        image_width: 520.0,
    }
}

// The header bar and the status bar are the shell's (`shell.rs`); the left sidebar is
// `sidebar.rs`'s, the new-session flow `new_session.rs`'s and the settings `settings_page.rs`'s.
/// (for the pictures of the header drawn on their own)
#[cfg(test)]
pub use super::shell::{header_center, header_start};

/// Whether the goal goes beside the text when there is room (a toolbar toggle)
static GOAL_BESIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

// Under tests, each test thread has a switch of its own: tests that turn the goal beside off for
// a frame (and on again) run side by side, and must not change another test's layout
#[cfg(test)]
thread_local! {
    static GOAL_BESIDE_HERE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub fn goal_beside_on() -> bool {
    #[cfg(test)]
    return GOAL_BESIDE_HERE.with(|g| g.get());
    #[cfg(not(test))]
    GOAL_BESIDE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_goal_beside(on: bool) {
    #[cfg(test)]
    GOAL_BESIDE_HERE.with(|g| g.set(on));
    GOAL_BESIDE.store(on, std::sync::atomic::Ordering::Relaxed);
}

// ----- the window

pub fn view(app: &App) -> Element<'_, Message> {
    warm_fonts();
    let content: Element<'_, Message> = widget::responsive(move |size| {
        // The editors' keys follow vim's mode, whoever draws the view. Published as the page is
        // laid out, where it is read, rather than when the view is built: another window's view
        // (or, in the tests, another test's) built in between would otherwise leave its mode here
        set_vim_mode(app.vim_enabled.then_some(app.vim.mode));
        super::insert::set_popout_open(app.popout.is_some());
        set_placement_override(app.goal_placement_override);
        set_view_height(size.height);
        layout(app, size.width)
    }).into();
    content.map(move |message| match message {
        Message::ReadOnlyKey(press) if !app.vim_enabled && !read_only_navigation(&press) => Message::Noop,
        Message::ReadOnlyKey(press) if app.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.mode == Mode::Source) => Message::RawVim(super::EditorTarget::Source, press),
        Message::ReadOnlyKey(press) => Message::WindowKey(press),
        other => other,
    })
}

fn read_only_navigation(press: &text_editor::KeyPress) -> bool {
    use cosmic::iced::keyboard::{Key, key::Named};
    matches!(press.key.as_ref(), Key::Named(Named::ArrowUp | Named::ArrowDown | Named::PageUp | Named::PageDown))
}

/// Whether the right sidebar shows at this window width
pub fn right_shown(app: &App, width: f32) -> bool {
    app.show_right && app.workspace.is_some() && app.new_session.is_none() && !app.shell.settings_open && width >= 960.0
}

fn layout(app: &App, width: f32) -> Element<'_, Message> {
    // Narrow windows give the theory the room: the sidebars fold away
    let left = app.show_sidebar && width >= 640.0;
    let right = right_shown(app, width);
    let mut row = widget::row::with_capacity(5).spacing(0).height(Length::Fill);
    if left {
        row = row.push(super::sidebar::sidebar(app)).push(splitter(true));
    }
    let main: Element<'_, Message> = if app.shell.settings_open {
        super::settings_page::page(app)
    } else if let Some(ns) = &app.new_session {
        super::new_session::page(app, ns)
    } else if let Some(ws) = &app.workspace {
        let main_width = width - if left { app.left_width + 6.0 } else { 0.0 } - if right { app.right_width + 6.0 } else { 0.0 };
        workspace(app, ws, right, main_width)
    } else {
        welcome(app)
    };
    row = row.push(widget::container(main).width(Length::Fill).height(Length::Fill));
    if right && let Some(ws) = &app.workspace {
        row = row.push(splitter(false)).push(side_panel(app, ws));
    }
    // While a sidebar's edge is dragged, the pointer shows it (the drag itself follows
    // window events, so a release anywhere ends it)
    let row: Element<'_, Message> = if app.dragging.is_some() {
        widget::mouse_area(row).interaction(cosmic::iced::mouse::Interaction::ResizingHorizontally).into()
    } else {
        row.into()
    };
    let mut col = widget::column::with_capacity(2).push(row);
    // The status bar, always there with a document open (so the page's height never changes for
    // it): what editing the open block means, and the app's last word
    if let Some(bar) = super::shell::status_bar(app) {
        col = col.push(bar);
    }
    // An opaque background of the theme's colour (with no inset content card, the window itself
    // would show through to the desktop)
    widget::container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(page_color(theme).into()),
            text_color: Some(theme.cosmic().on_bg_color().into()),
            ..Default::default()
        }))
        .into()
}

/// A sidebar's draggable edge
fn splitter<'a>(left: bool) -> Element<'a, Message> {
    widget::mouse_area(
        widget::container(widget::Space::new())
            .width(Length::Fixed(6.0))
            .height(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: 0.12, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
                ..Default::default()
            })),
    )
    .on_press(Message::DragStart(left))
    .interaction(cosmic::iced::mouse::Interaction::ResizingHorizontally)
    .into()
}

fn welcome(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(6)
        .spacing(sp.space_s)
        .push(widget::text::title1("Deductum"))
        .push(widget::text::body("Isabelle theories and Lean files as literate documents, worked on together with agents in one live session."))
        .push(widget::button::suggested("Open a folder…").on_press(Message::NewSession(true)));
    let any = !app.sessions.is_empty();
    if any {
        col = col.push(widget::text::body("Or pick a session on the left."));
    }
    widget::container(col.max_width(640)).center(Length::Fill).into()
}

fn nav_item<'a>(label: String, detail: Option<String>, indent: f32, color: Option<Status>, current: bool, message: Message, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(3).spacing(6).align_y(Alignment::Center);
    if let Some(status) = color {
        row = row.push(
            widget::container(widget::Space::new())
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
                .class(theme::Container::custom(move |theme| iced::widget::container::Style {
                    background: Some(status_color(status, theme).into()),
                    border: Border { radius: 4.0.into(), ..Default::default() },
                    ..Default::default()
                })),
        );
    }
    // (the one row style of the sidebar: COSMIC's wash on hover, the current row bold in the accent,
    // a long name wrapping after an underscore)
    let mut col = widget::column::with_capacity(2).width(Length::Fill).push(super::sidebar::name_text(&table.decode(&label).text, 14.0, current));
    if let Some(d) = detail {
        col = col.push(symbol_text_with_font(&d, table, 12.0, cosmic::font::default()));
    }
    row = row.push(col);
    let button = widget::button::custom(row)
        .width(Length::Fill)
        .padding(Padding { left: 6.0 + indent, right: 6.0, top: 3.0, bottom: 3.0 })
        .class(super::sidebar::nav_style(current))
        .on_press(message);
    #[cfg(test)]
    let button = widget::container(button).id(test_nav_id(&label));
    button.into()
}

/// The id a navigation row has under test, by its label
#[cfg(test)]
pub fn test_nav_id(label: &str) -> widget::Id {
    widget::Id::new(format!("deductum-test-nav-{label}"))
}

fn block_title(t: &Theory, table: &dyn super::codec::Symbols, block: &Block, max: usize) -> String {
    let text = match block.body {
        Some((s, e)) => t.source_text(s, e),
        None => t.source_text(block.start, block.end),
    };
    let first = table.decode(&text).text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    if first.chars().count() > max { format!("{}…", first.chars().take(max).collect::<String>()) } else { first }
}

pub fn outline<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(t.blocks.len()).spacing(1);
    // (the current row: the last heading at or before the caret)
    let current = t.blocks.iter().filter(|b| matches!(b.kind, Kind::Heading(_))).rev().find(|b| b.start <= t.cursor).map(|b| b.start);
    for block in &t.blocks {
        if let Kind::Heading(level) = block.kind {
            let title = block_title(t, &*table, block, 60).replace("\\<open>", "").replace("\\<close>", "");
            col = col.push(nav_item(title, None, 12.0 * (level.saturating_sub(1) as f32), None, current == Some(block.start), Message::Goto(block.start), &*table));
        }
    }
    col.into()
}

/// The theory's statements, one line each: a status mark, the name, and the statement after it
/// cut short with an ellipsis (the whole line in a tooltip), so that a long statement never
/// wraps and leaves a stray fragment on a line of its own
pub fn theorems<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(t.blocks.len()).spacing(1);
    for block in t.blocks.iter().filter(|b| b.kind == Kind::Formal && t.profile.is_statement(b, &t.kinds)) {
        let first = block_title(t, &*table, block, 200);
        let name = statement_name(&first);
        let rest = first.split_once(&name).map_or(String::new(), |(_, r)| r.trim_start_matches(':').trim().to_string());
        let sorry = block.commands.iter().any(|c| t.profile.unfinished().contains(&c.name.as_str()));
        let status = if sorry && block.status == Status::Finished { Status::Warned } else { block.status };
        let dot = widget::container(widget::Space::new()).width(Length::Fixed(8.0)).height(Length::Fixed(8.0)).class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(status_color(status, theme).into()),
            border: Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        }));
        let current = block.start <= t.cursor && t.cursor < block.end;
        let mut names = widget::column::with_capacity(2).width(Length::Fill).push(super::sidebar::name_text(&name, 13.0, current));
        if !rest.is_empty() {
            names = names.push(
                widget::text(rest)
                    .size(13.0)
                    .font(cosmic::font::mono())
                    .wrapping(iced::widget::text::Wrapping::None)
                    .ellipsize(iced::core::text::Ellipsize::End(iced::core::text::EllipsizeHeightLimit::Lines(1)))
                    .class(muted())
                    .width(Length::Fill),
            );
        }
        let row = widget::row::with_capacity(2).spacing(8.0).align_y(Alignment::Center).push(dot).push(names);
        let tip = if sorry { format!("{first}  (unfinished)") } else { first };
        let button = widget::button::custom(row).width(Length::Fill).padding([4.0, 8.0]).class(super::sidebar::nav_style(current)).on_press(Message::Goto(block.start));
        #[cfg(test)]
        let button = widget::container(button).id(test_nav_id(&statement_name(&tip)));
        col = col.push(widget::tooltip(button, widget::text(tip).font(cosmic::font::mono()).size(12.0), widget::tooltip::Position::Right));
    }
    col.into()
}

pub fn problems<'a>(app: &'a App, t: &'a Theory) -> Element<'a, Message> {
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    let mut col = widget::column::with_capacity(32).spacing(1);
    // Imported theories that cannot load hold this one up
    if let Some(info) = app.workspace.as_ref().and_then(|w| w.info.as_ref()) {
        for th in info["theories"].as_array().into_iter().flatten() {
            if let Some(errors) = th["status"]["header_errors"].as_array().filter(|e| !e.is_empty())
                && let Some(path) = th["path"].as_str()
            {
                let msg = errors.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join("; ");
                col = col.push(nav_item(format!("{} cannot load", file_name(path)), Some(msg), 0.0, Some(Status::Failed), false, Message::OpenTheory(path.to_string()), &*table));
            }
        }
    }
    for m in t.messages_in(0, usize::MAX / 2) {
        if m.kind != "error" {
            continue;
        }
        let line = super::ot::line_col(&t.local, m.start).0;
        let text: String = table.decode(&m.text).text.lines().next().unwrap_or("").chars().take(80).collect();
        col = col.push(nav_item(format!("Error, line {line}"), Some(text), 0.0, Some(Status::Failed), false, Message::Goto(m.start), &*table));
    }
    for block in &t.blocks {
        for c in block.commands.iter().filter(|c| t.profile.unfinished().contains(&c.name.as_str())) {
            let line = super::ot::line_col(&t.local, c.start).0;
            col = col.push(nav_item(format!("{}, line {line}", c.name), Some(block_title(t, &*table, block, 48)), 0.0, Some(Status::Warned), false, Message::Goto(c.start), &*table));
        }
    }
    col.into()
}

// ----- the workspace

fn workspace<'a>(app: &'a App, ws: &'a Workspace, right: bool, main_width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let mut center = widget::column::with_capacity(6).spacing(sp.space_xs).padding([sp.space_xs, sp.space_s]);
    // (the theory's page outside the padded column: its scroll view, and so its scroll bar, at the
    // window's edges)
    let mut page_body: Option<Element<'a, Message>> = None;
    VIEW_WIDTH.store(main_width.to_bits(), std::sync::atomic::Ordering::Relaxed);
    if let Some(s) = summary {
        let lc = s["lifecycle"].as_str().unwrap_or("");
        if let Some(msg) = s["message"].as_str() {
            center = center.push(banner(msg.to_string(), None));
        } else if lc == "Paused" {
            center = center.push(banner("Paused: the prover stays loaded, checking is suspended. Earlier output is historical.".into(), Some(("Resume", Message::Lifecycle("resume")))));
        }
        for r in s["requests"].as_array().into_iter().flatten() {
            center = center.push(request_banner(r));
        }
        if let Some(offer) = &ws.show_offer {
            let msg = offer["message"].as_str().unwrap_or("");
            let text = format!(
                "{} wants to show you line {} of {}{}",
                offer["from"]["label"].as_str().unwrap_or("An agent"),
                offer["line"],
                file_name(offer["path"].as_str().unwrap_or("")),
                if msg.is_empty() { String::new() } else { format!(": “{msg}”") }
            );
            let row = widget::row::with_capacity(3)
                .spacing(sp.space_s)
                .align_y(Alignment::Center)
                .push(widget::text::body(text).width(Length::Fill))
                .push(widget::button::standard("Dismiss").on_press(Message::ShowOffer(false)))
                .push(widget::button::suggested("Go there").on_press(Message::ShowOffer(true)));
            center = center.push(widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill));
        }
        if s["parent"].is_object() && s["owner"].is_object() {
            center = center.push(banner(
                format!("Independent session of {} on branch {}", s["owner"]["label"].as_str().unwrap_or("an agent"), s["parent"]["branch"].as_str().unwrap_or("")),
                Some(("Review and merge…", Message::Merge("preview".into()))),
            ));
        }
    }
    if let Some(files) = &ws.merge {
        let mut col = widget::column::with_capacity(files.len() + 2).spacing(sp.space_xxs);
        col = col.push(widget::text::heading(format!("{} changed theor{}", files.len(), if files.len() == 1 { "y" } else { "ies" })));
        for f in files {
            col = col.push(widget::text(f["diff"].as_str().unwrap_or("").to_string()).font(code_font()).size(12));
        }
        col = col.push(widget::row::with_capacity(2).spacing(sp.space_s).push(widget::button::suggested("Merge into the shared session").on_press(Message::Merge("apply".into()))));
        center = center.push(widget::container(widget::scrollable(col)).max_height(360.0).padding(sp.space_s).class(theme::Container::Card));
    }
    match &ws.theory {
        Some(t) => {
            if t.conflict.is_some() {
                center = center.push(conflict_banner());
            }
            if let Some(err) = &t.save_error {
                center = center.push(banner(format!("Saving failed: {err}"), Some(("Retry", Message::Save))));
            }
            for alt in t.alternatives.iter().take(3) {
                center = center.push(alternative_banner(alt));
            }

            // Imports that cannot load leave this theory waiting: say so
            if let Some(info) = &ws.info {
                let broken: Vec<String> = info["theories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|th| th["status"]["header_errors"].as_array().is_some_and(|e| !e.is_empty()))
                    .filter_map(|th| th["path"].as_str().map(|p| file_name(p).to_string()))
                    .collect();
                if !broken.is_empty() {
                    center = center.push(banner(
                        format!("{} cannot load (a bad import or header); theories importing it wait. See Problems.", broken.join(", ")),
                        Some(("Problems", Message::LeftMode(LeftMode::Problems))),
                    ));
                }
            }
            // The proof state (and what the tools typed in the text say) goes beside the block it
            // belongs to when there is room for both columns, else under it
            let preferred_goal = (main_width * 0.38).clamp(z(320.0), z(560.0));
            // The text column as wide as the code blocks (their code and status strip), within a
            // readable measure: long code lines wrap rather than push the goal underneath, and
            // the goal sits just past the blocks, a little apart
            let display = super::settings::display();
            let reading_width = (display.measure > 0.0).then(|| z(display.measure * display.text_size).min((main_width - SCROLLBAR_ROOM - 2.0 * z(PAGE_MARGIN)).max(z(240.0))));
            // The goal sits just past its cell, not past the column, so the room it needs is
            // measured from the widest cell's right edge: the column gives way (not below its
            // cells) to leave the goal its minimum width, and the goal stays beside in windows
            // narrower than the whole column and the goal would need. A wider window leaves the
            // column its measure
            let beside_width = match reading_width {
                Some(column) => {
                    let longest = t.block_editor.as_ref().map_or(0, |be| be.content.text().lines().map(|l| l.chars().count()).max().unwrap_or(0)).max(t.code_columns);
                    let cells = if super::settings::cell_width() == 0 { column } else { (code_width(longest) + CARD_CHROME).min(column) };
                    column.min(main_width - SCROLLBAR_ROOM - GOAL_GAP - z(320.0)).max(cells)
                }
                None => (code_width(t.code_columns) + CARD_CHROME).clamp(z(440.0), z(680.0)),
            };
            // Beside when the text column, the gap and a goal of at least its minimum width fit
            // (room for the scroll bar kept); the side margins go first, before the goal goes under
            let room = main_width - SCROLLBAR_ROOM - beside_width - GOAL_GAP;
            // (the goal narrows to keep side margins of at least PAGE_MARGIN; below its minimum
            // width the margins give way first, and only then does the goal go under)
            let goal_width = preferred_goal.min(room - 2.0 * z(PAGE_MARGIN)).max(z(320.0));
            let fits = room >= z(320.0) && goal_beside_on();
            let gutter = ((room - goal_width) / 2.0).clamp(0.0, sp.space_l as f32);
            PAGE_GUTTER.store(gutter.to_bits(), std::sync::atomic::Ordering::Relaxed);
            // The goal's column is kept whenever it fits, whether or not there is a goal now:
            // moving between blocks never changes the layout. Without it, the page is a wider
            // column, centred, the code blocks as wide as their lines allow
            let beside = fits;
            let text_width = if fits { beside_width } else { reading_width.unwrap_or_else(|| (code_width(t.code_columns) + CARD_CHROME).clamp(z(560.0), z(880.0))) };
            COLUMN_CAP.store(text_width.to_bits(), std::sync::atomic::Ordering::Relaxed);
            WIDE.store(fits, std::sync::atomic::Ordering::Relaxed);
            let inline = if beside { None } else { Some(ws) };
            let _ = right;
            let mut body: Element<'_, Message> = match t.mode {
                Mode::Literate if beside && goal_placement(true, 0) == GoalPlacement::Pinned => {
                    // the goal column beside the page, at the top of the view, not scrolling
                    let goal: Element<'_, Message> = match formal_at_caret(t).then(|| inline_goal(Some(ws), &*app.table(&ws.machine))).flatten() {
                        Some(g) => widget::container(thin(g)).width(Length::Fixed(goal_width)).into(),
                        None => widget::Space::new().width(Length::Fixed(goal_width)).into(),
                    };
                    widget::row::with_capacity(2)
                        .spacing(sp.space_m)
                        .push(widget::container(literate(app, t, None, None, text_width)).width(Length::Fill).height(Length::Fill))
                        .push(goal)
                        .into()
                }
                Mode::Literate => literate(app, t, inline, beside.then_some((ws, goal_width, text_width)), text_width),
                Mode::Source => {
                    // The source as wide as its lines (up to a readable measure), the goal beside the
                    // caret's line when there is room for both
                    let columns = t.decoded.text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
                    let source_width = source_width(columns);
                    // (the goal column narrows to the room there is, down to a readable width;
                    // it is placed against the source's own column, not the prose's measure)
                    let source_room = main_width - SCROLLBAR_ROOM - source_column(t.code_columns) - GOAL_GAP;
                    let source_goal = preferred_goal.min(source_room - 2.0 * z(PAGE_MARGIN)).max(z(320.0));
                    let room = (main_width - source_width - z(70.0)).min(source_goal);
                    // (whether or not there is a goal at the moment: a click elsewhere clears the
                    // goal until the prover's state there comes, and the layout must not change
                    // with it)
                    let fits = room >= z(320.0) && goal_beside_on();
                    SOURCE_BESIDE.store(fits, std::sync::atomic::Ordering::Relaxed);
                    source(ws, t, if fits { None } else { Some(ws) }, &*app.table(&ws.machine), fits.then_some((room, source_width)), source_width.min(main_width - z(24.0)))
                }
            };
            let _ = &mut body;
            page_body = Some(body);
        }
        None => {
            center = center.push(widget::container(widget::text::body(if ws.opening.is_some() { "Opening…" } else { "Choose a theory on the left." })).center(Length::Fill));
        }
    }
    let mut outer = widget::column::with_capacity(2).push(center);
    if let Some(body) = page_body {
        outer = outer.push(body);
    }
    widget::container(outer).width(Length::Fill).height(Length::Fill).into()
}

/// The column the Source view's goal is placed against: as wide as the theory's longest lines
/// need, between 440 and 680 px. (The Source view is laid out as it was before the Paper defaults
/// gave the Literate view its 48 em column: the goal's width does not depend on that measure)
fn source_column(columns: usize) -> f32 {
    ((columns.max(30) as f32 + 4.0) * z(13.0) * 0.6 + code_padding().left + code_padding().right + CARD_CHROME).clamp(z(440.0), z(680.0))
}

/// The workspace's width, as the view last laid it out
static VIEW_WIDTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn view_width() -> f32 {
    f32::from_bits(VIEW_WIDTH.load(std::sync::atomic::Ordering::Relaxed))
}

/// What an agent's request (or grant) covers, in words
fn scope_words(r: &Value) -> String {
    match r["scope"].as_str() {
        Some("folder") => "the whole folder".to_string(),
        Some("theory") => file_name(r["path"].as_str().unwrap_or("")).to_string(),
        Some("proof") => format!("the proof at line {} of {}", r["line"], file_name(r["path"].as_str().unwrap_or(""))),
        _ => "this session".into(),
    }
}

/// An agent asks for editing rights: Allow or Deny
fn request_banner<'a>(r: &Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = r["id"].as_str().unwrap_or("").to_string();
    let reason = r["reason"].as_str().unwrap_or("");
    let mut text = widget::column::with_capacity(2).spacing(2).width(Length::Fill).push(widget::text::body(format!(
        "{} asks to edit {}",
        r["label"].as_str().unwrap_or("An agent"),
        scope_words(r)
    )));
    if !reason.is_empty() {
        text = text.push(widget::text::caption(format!("“{reason}”")));
    }
    let row = widget::row::with_capacity(3)
        .spacing(sp.space_s)
        .align_y(Alignment::Center)
        .push(text)
        .push(widget::button::standard("Deny").on_press(Message::AnswerRequest(id.clone(), false)))
        .push(widget::button::suggested("Allow").on_press(Message::AnswerRequest(id, true)));
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

/// An agent's note: violet, apart from the statuses' green, amber and red and from the accent
pub fn note_color(theme: &cosmic::Theme) -> Color {
    if theme.cosmic().is_dark { Color::from_rgb8(0xb3, 0x9d, 0xf5) } else { Color::from_rgb8(0x63, 0x48, 0xb8) }
}

/// Between a cell and what is attached under it (its goal under it)
const ATTACHED: f32 = 6.0;

/// The agent's notes whose first character is in `start..end` of the open theory
fn notes_on<'a>(ws: &'a Workspace, t: &Theory, start: usize, end: usize) -> Vec<&'a Value> {
    ws.notes
        .iter()
        .filter(|n| {
            n["start"].as_u64().is_some_and(|s| {
                let s = super::App::local_offset_pub(t, s as usize);
                start <= s && s < end.max(start + 1)
            })
        })
        .collect()
}

/// Whether a block's notes are in its card: a cell with its code shown or open as an editor. The
/// notes of any other block have a card of their own
fn notes_in_card(t: &Theory, i: usize, block: &Block) -> bool {
    block.kind == Kind::Formal && t.block_views.get(i).is_some_and(Option::is_some)
}

/// An agent's note on the text: who and where, the text, the replies so far, and a reply or
/// Resolve. The heading is in the agent's violet with Resolve (a Tertiary link) at its right; the
/// reply field sits plain on the page, with Reply (Secondary) beside it. Nothing of its own behind
/// it: no plate and no bar
fn note_body<'a>(ws: &'a Workspace, n: &Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = n["id"].as_str().unwrap_or("").to_string();
    let mut col = widget::column::with_capacity(5).spacing(sp.space_xxxs);
    // (Resolve, which closes the note, in its heading: the reply row keeps the room for typing)
    col = col.push(
        widget::row::with_capacity(2)
            .align_y(Alignment::Center)
            .push(
                widget::text::caption_heading(format!("{} · note on line {}", n["label"].as_str().unwrap_or("Agent"), n["line"]))
                    .class(theme::Text::Custom(|theme| iced::widget::text::Style {
                        color: Some(note_color(theme)),
                        selected_fill: Color { a: 0.32, ..Color::from(theme.cosmic().accent.base) },
                        selected_text_color: None,
                    }))
                    .width(Length::Fill),
            )
            .push(tertiary("Resolve", Message::NoteReply(id.clone(), true))),
    );
    col = col.push(widget::text::body(n["text"].as_str().unwrap_or("").to_string()));
    for r in n["replies"].as_array().into_iter().flatten() {
        col = col.push(widget::text::caption(format!("{}: {}", r["by"].as_str().unwrap_or(""), r["text"].as_str().unwrap_or(""))));
    }
    let reply = ws.note_reply.get(&id).cloned().unwrap_or_default();
    let reply_id = id.clone();
    // (Reply sends what is typed, so only with something typed)
    let can_reply = !reply.trim().is_empty();
    col = col.push(
        widget::container(
            widget::row::with_capacity(3)
                .spacing(sp.space_xxs)
                .align_y(Alignment::Center)
                .push(
                    widget::text_input("Reply to the agent", reply)
                        .on_input(move |t| Message::NoteReplyInput(reply_id.clone(), t))
                        .on_submit({
                            let id = id.clone();
                            move |_| Message::NoteReply(id.clone(), false)
                        })
                        .style(plain_input())
                        .width(Length::Fill),
                )
                .push(secondary("Reply", can_reply.then(|| Message::NoteReply(id.clone(), false)))),
        )
        .padding(Padding { top: sp.space_xxxs as f32, ..Padding::ZERO }),
    );
    col.into()
}

/// A note in its cell's card: under the code and its messages, after a hairline (COSMIC's divider)
/// as wide as the card, its text where the code's is
fn note_in_card<'a>(ws: &'a Workspace, n: &Value, width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let hairline = widget::container(widget::Space::new()).width(Length::Fill).height(Length::Fixed(1.0)).class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(card_border_color(theme).into()),
        ..Default::default()
    }));
    widget::column::with_capacity(2)
        .push(hairline)
        .push(widget::container(note_body(ws, n)).padding([sp.space_xxs as f32, code_padding().left]).width(Length::Fill))
        .width(Length::Fixed(width))
        .into()
}

/// A note on a block that is not a cell with code (a text, a heading): a card of its own in the
/// same style, `width` wide (the cell's; else the text column), a little below the block
fn note_card<'a>(ws: &'a Workspace, n: &Value, width: Option<f32>) -> Element<'a, Message> {
    let sp = spacing();
    let body = widget::container(note_body(ws, n)).padding([sp.space_xxs as f32, code_padding().left]).width(Length::Fill);
    widget::container(card(body.into(), Status::Unprocessed, false)).padding(Padding { top: ATTACHED, ..Padding::ZERO }).width(width.map_or(Length::Fill, Length::Fixed)).into()
}

/// A text input plain on the page: the page's own fill and a hairline border (the accent ring when
/// it has the focus)
fn plain_input() -> theme::TextInput {
    use cosmic::widget::text_input::{Appearance, StyleSheet as _};
    fn plain(mut a: Appearance, theme: &cosmic::Theme, focused: bool) -> Appearance {
        a.background = Color::from(page_color(theme)).into();
        if !focused {
            a.border_width = 1.0;
            a.border_color = card_border_color(theme);
        }
        a
    }
    theme::TextInput::Custom {
        active: Box::new(|theme| plain(theme.active(&theme::TextInput::Default), theme, false)),
        error: Box::new(|theme| plain(theme.error(&theme::TextInput::Default), theme, false)),
        hovered: Box::new(|theme| plain(theme.hovered(&theme::TextInput::Default), theme, false)),
        focused: Box::new(|theme| plain(theme.focused(&theme::TextInput::Default), theme, true)),
        disabled: Box::new(|theme| plain(theme.disabled(&theme::TextInput::Default), theme, false)),
    }
}

/// A Secondary button (household rule 7): no fill, a 2 px accent border, an accent semibold label,
/// a 32 px pill; COSMIC's text-button washes when hovered and pressed, faded when it cannot act
fn secondary<'a>(label: &'static str, on_press: Option<Message>) -> widget::Button<'a, Message> {
    use widget::button::Catalog as _;
    fn outlined(mut style: widget::button::Style, theme: &cosmic::Theme, wash: Option<Color>, faded: bool) -> widget::button::Style {
        let c = theme.cosmic();
        let accent: Color = c.accent_text_color().into();
        let accent = if faded { Color { a: 0.5, ..accent } } else { accent };
        style.background = wash.map(Into::into);
        style.border_radius = c.corner_radii.radius_xl.into();
        style.border_width = 2.0;
        style.border_color = accent;
        style.text_color = Some(accent);
        style.icon_color = Some(accent);
        style
    }
    let text = || theme::Button::Text;
    let font = cosmic::iced::Font { weight: cosmic::iced::font::Weight::Semibold, ..cosmic::font::default() };
    widget::button::custom(widget::container(widget::text(label).size(14.0).font(font)).center_y(Length::Fill))
        .padding([6, spacing().space_s])
        .height(Length::Fixed(32.0))
        .class(theme::Button::Custom {
            active: Box::new(move |focused, theme| outlined(theme.active(focused, false, &text()), theme, None, false)),
            disabled: Box::new(move |theme| outlined(theme.disabled(&text()), theme, None, true)),
            hovered: Box::new(move |focused, theme| outlined(theme.hovered(focused, false, &text()), theme, Some(theme.cosmic().text_button.hover.into()), false)),
            pressed: Box::new(move |focused, theme| outlined(theme.pressed(focused, false, &text()), theme, Some(theme.cosmic().text_button.pressed.into()), false)),
        })
        .on_press_maybe(on_press)
}

fn banner<'a>(text: String, action: Option<(&'static str, Message)>) -> Element<'a, Message> {
    let sp = spacing();
    let mut row = widget::row::with_capacity(2).spacing(sp.space_s).align_y(Alignment::Center).push(widget::text::body(text).width(Length::Fill));
    if let Some((label, msg)) = action {
        row = row.push(widget::button::standard(label).on_press(msg));
    }
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

fn conflict_banner<'a>() -> Element<'a, Message> {
    let sp = spacing();
    widget::container(
        widget::row::with_capacity(4)
            .spacing(sp.space_s)
            .align_y(Alignment::Center)
            .push(widget::text::body("This theory changed on disk while it had unsaved edits. Autosave is holding.").width(Length::Fill))
            .push(widget::button::standard("Reload from disk").on_press(Message::Resolve("disk")))
            .push(widget::button::standard("Keep mine").on_press(Message::Resolve("buffer"))),
    )
    .padding(sp.space_xs)
    .class(theme::Container::Card)
    .width(Length::Fill)
    .into()
}

fn alternative_banner<'a>(alt: &'a Value) -> Element<'a, Message> {
    let sp = spacing();
    let id = alt["id"].as_str().unwrap_or("").to_string();
    let mut row = widget::row::with_capacity(4).spacing(sp.space_s).align_y(Alignment::Center);
    let n = alt["pieces"].as_array().map_or(0, Vec::len);
    row = row.push(
        widget::text::body(format!(
            "{} of a {} could not be applied over later edits; the text is kept here",
            if n == 1 { "One part".to_string() } else { format!("{n} parts") },
            alt["kind"].as_str().unwrap_or("change")
        ))
        .width(Length::Fill),
    );
    for (i, p) in alt["pieces"].as_array().into_iter().flatten().enumerate().take(3) {
        let restore: String = p["restore"].as_str().unwrap_or("").chars().take(24).collect();
        let current: String = p["current"].as_str().unwrap_or("").chars().take(24).collect();
        let label = match (restore.is_empty(), current.is_empty()) {
            (true, false) => format!("Remove “{current}” (line {})", p["line"]),
            (false, true) => format!("Insert “{restore}” (line {})", p["line"]),
            _ => format!("Put back “{restore}” for “{current}” (line {})", p["line"]),
        };
        row = row.push(widget::button::standard(label).on_press(Message::RestoreAlternative(id.clone(), i)));
    }
    row = row.push(widget::button::text("Dismiss").on_press(Message::DismissAlternative(id)));
    widget::container(row).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill).into()
}

fn activity_panel(ws: &Workspace) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(64).spacing(sp.space_xs);
    if let Some(act) = &ws.activity {
        let prover = &act["prover"];
        col = col.push(widget::text::heading(format!(
            "Prover {}{}{}",
            act["lifecycle"].as_str().unwrap_or(""),
            prover["cpu"].as_u64().map(|c| format!(" · CPU {c}%")).unwrap_or_default(),
            prover["rss_mb"].as_u64().map(|m| format!(" · {:.1} GB", m as f64 / 1000.0)).unwrap_or_default()
        )));
        if ws.caps.interrupt {
            col = col.push(widget::button::standard("Interrupt running proofs").on_press(Message::Lifecycle("cancel")));
        }
        let running = act["running"].as_array().cloned().unwrap_or_default();
        if !running.is_empty() {
            col = col.push(widget::text::caption_heading("Running"));
            for r in running {
                let path = r["path"].as_str().unwrap_or("").to_string();
                col = col.push(widget::text::body(format!(
                    "{} at {} line {} — {} s",
                    r["name"].as_str().unwrap_or(""),
                    file_name(&path),
                    r["line"],
                    r["seconds"]
                )));
            }
        }
        col = col.push(widget::text::caption_heading("Theories"));
        let mut theories = act["theories"].as_array().cloned().unwrap_or_default();
        theories.sort_by_key(|th| (th["consolidated"].as_bool() == Some(true), th["percentage"].as_u64().unwrap_or(0)));
        for th in theories.iter().take(80) {
            let name = th["theory"].as_str().unwrap_or("").to_string();
            let pct = th["percentage"].as_u64().unwrap_or(0);
            let failed = th["failed"].as_u64().unwrap_or(0) > 0 || th["header_errors"].as_array().is_some_and(|e| !e.is_empty());
            let label = if th["consolidated"].as_bool() == Some(true) && !failed {
                format!("{name} ✓")
            } else if failed {
                format!("{name} ✗ {pct}%")
            } else {
                format!("{name} {pct}%{}", if th["running"].as_u64().unwrap_or(0) > 0 { " · running" } else { "" })
            };
            col = col.push(widget::text::body(label));
            col = col.push(widget::progress_bar::linear::Linear::new().progress(pct as f32 / 100.0).width(Length::Fill).girth(4.0));
        }
    } else {
        col = col.push(widget::text::caption("No prover activity yet."));
    }
    col = col.push(widget::text::caption_heading("Log"));
    col = col.push(widget::text(ws.log.iter().rev().take(80).rev().cloned().collect::<Vec<_>>().join("\n")).font(cosmic::font::mono()).size(12.0));
    thin(col).height(Length::Fill).into()
}

/// The session's counterexample finders and quick proofs at the caret, without editing the
/// document (Isabelle: Quickcheck, Nitpick, try0, solve_direct)
fn counterexamples_panel<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.cex;
    let caps = &ws.caps;
    let mut col = widget::column::with_capacity(12).spacing(sp.space_xs);
    let typed: Vec<&str> = caps.counterexamples.iter().filter(|n| caps.typed_tool(n).is_some()).map(String::as_str).collect();
    let typing = if typed.is_empty() { String::new() } else { format!(" Typing {} in the proof works too.", typed.join(" or ")) };
    col = col.push(widget::text::caption(format!("Counterexamples and quick proofs for the goal at the caret.{typing}")));
    // (the finders, the first suggested, then the quick proofs)
    let label = |name: &str| caps.tool(name).map_or_else(|| name.to_string(), |t| t.label.clone());
    let names: Vec<&str> = caps.counterexamples.iter().map(String::as_str).chain(caps.tools.iter().filter(|t| t.quick).map(|t| t.name.as_str())).collect();
    let mut buttons = widget::row::with_capacity(names.len()).spacing(4);
    for (i, name) in names.into_iter().enumerate() {
        let b = if i == 0 { widget::button::suggested(label(name)) } else { widget::button::standard(label(name)) };
        buttons = buttons.push(b.on_press(Message::RunCommand(name.to_string())));
    }
    col = col.push(buttons);
    let params = caps.counterexamples.iter().filter_map(|n| caps.tool(n)).map(|t| t.params.as_str()).find(|p| !p.is_empty()).unwrap_or("options");
    col = col.push(widget::text_input(params, &ws.cex_params).on_input(Message::CommandParams));
    if !q.status.is_empty() {
        col = col.push(
            widget::row::with_capacity(2)
                .spacing(sp.space_s)
                .align_y(Alignment::Center)
                .push(widget::text::heading(if q.status == "finished" { "Finished".to_string() } else { q.status.clone() }).width(Length::Fill))
                .push(widget::button::text("Cancel").on_press_maybe((q.status != "finished" && q.status != "failed").then_some(Message::CancelQuery))),
        );
    }
    if let Some(err) = &q.error {
        col = col.push(widget::text::body(err.clone()));
    }
    for m in &q.messages {
        col = col.push(message_view(ws, m, table, &on_panel()));
        for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
            col = col.push(
                widget::row::with_capacity(2)
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .push(widget::container(symbol_text(sb, table, z(13.0))).width(Length::Fill))
                    .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
            );
        }
    }
    thin(col).height(Length::Fill).into()
}

fn theory_status(t: &Theory) -> Element<'_, Message> {
    let (finished, unprocessed, running, failed, consolidated) = t.status();
    let total = (finished + unprocessed + running + failed).max(1);
    let text = if consolidated && failed == 0 {
        "Checked".to_string()
    } else if t.prover.status.is_null() {
        "Not checked yet".into()
    } else {
        format!("{}% checked{}{}", (finished + failed) * 100 / total, if running > 0 { format!(" · {running} running") } else { String::new() }, if failed > 0 { format!(" · {failed} failed") } else { String::new() })
    };
    widget::row::with_capacity(3)
        .spacing(spacing().space_s)
        .push(widget::text::caption(file_name(&t.path).to_string()))
        .push(widget::text::caption(text))
        .push(widget::text::caption(if t.dirty { "unsaved" } else { "saved" }))
        .into()
}

fn mono_block<'a>(text: String, status: Status, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let body = widget::container(symbol_text(&text, table, 13.0)).padding(Padding { left: 4.0, right: 6.0, top: 4.0, bottom: 4.0 }).width(Length::Fill);
    card(body.into(), status, false)
}

/// A Tertiary button (household rule 7): no box, its label bold, underlined and in the accent,
/// and COSMIC's own link states for hover and press
pub(crate) fn tertiary<'a>(label: impl Into<String>, msg: Message) -> widget::Button<'a, Message> {
    widget::button::custom(link_label(label)).padding([2, 2]).class(theme::Button::Link).on_press(msg)
}

/// A Tertiary button that destroys what it acts on, in the failed colour of the cell palette (the
/// household's destructive red), the link's own shape and states otherwise
fn tertiary_destructive<'a>(label: &'static str, msg: Message) -> widget::Button<'a, Message> {
    use widget::button::Catalog as _;
    fn red(mut style: widget::button::Style, theme: &cosmic::Theme) -> widget::button::Style {
        let c = strip_color(Status::Failed, true, theme);
        style.text_color = Some(c);
        style.icon_color = Some(c);
        style
    }
    let link = || theme::Button::Link;
    widget::button::custom(link_label(label))
        .padding([z(2.0) as u16, z(2.0) as u16])
        .class(theme::Button::Custom {
            active: Box::new(move |focused, theme| red(theme.active(focused, false, &link()), theme)),
            disabled: Box::new(move |theme| red(theme.disabled(&link()), theme)),
            hovered: Box::new(move |focused, theme| red(theme.hovered(focused, false, &link()), theme)),
            pressed: Box::new(move |focused, theme| red(theme.pressed(focused, false, &link()), theme)),
        })
        .on_press(msg)
}

fn link_label<'a>(label: impl Into<String>) -> Element<'a, Message> {
    let font = cosmic::iced::Font { weight: cosmic::iced::font::Weight::Bold, ..cosmic::font::default() };
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = vec![iced::widget::span(label.into()).font(font).underline(true)];
    iced::widget::rich_text(spans).size(13.0).into()
}

/// Add a block after a block (or at the end) from the profile's templates (a text, section or
/// lemma), and delete the block: Tertiary links, short enough for the gap between blocks that
/// offers them on hover. Delete is a red link at the right edge, `width` wide (the cell's), and
/// at least `space_m` from the add links
fn block_actions<'a>(t: &Theory, after: Option<usize>, delete: bool, width: Option<f32>) -> Element<'a, Message> {
    let sp = spacing();
    let mut row = widget::row::with_capacity(5).spacing(sp.space_s).align_y(Alignment::Center);
    for template in t.profile.block_templates() {
        row = row.push(tertiary(template.label, Message::InsertBlock(after, template.kind)));
    }
    let Some(i) = after.filter(|_| delete) else { return row.into() };
    // (a space_m of room before Delete however narrow the cell, and the rest of the cell's width)
    row.push(widget::Space::new().width(Length::Fixed(sp.space_m as f32)))
        .push(widget::Space::new().width(Length::Fill))
        .push(tertiary_destructive("Delete", Message::DeleteBlock(i)))
        .width(width.map_or(Length::Fill, Length::Fixed))
        .into()
}

/// The line of the block editor's text a source offset is on
fn block_line(be: &super::editor::BlockEditor, offset: usize) -> usize {
    let shown = be.shown();
    super::editor::LineIndex::new(&shown).position(&shown, be.decoded.to_decoded(offset.saturating_sub(be.start))).line
}

/// A line of code in a block
pub fn code_line_height() -> f32 {
    code_size() * CODE_LINE
}

/// The gap between the status strip and the code (and the code and the card's right edge): `CODE_GAP_EM`
/// of the code's size at the current zoom
pub fn code_gap() -> f32 {
    CODE_GAP_EM * code_size()
}

/// Padding of code in a block, shown or edited: the same, so that opening a block moves nothing.
/// The gap at the sides is [`code_gap`]
pub fn code_padding() -> Padding {
    Padding { left: code_gap(), right: code_gap(), top: 5.0, bottom: 5.0 }
}

/// The text origin relative to a literate cell, including the code status strip.
pub fn block_text_inset(be: &super::editor::BlockEditor) -> f32 {
    if be.wrap.is_some() { 0.0 } else { STRIP + code_padding().left }
}

/// Room kept after the last character of a cell's width, in characters: the editor wraps a line
/// that only just fits, and a monospace font's advance is not exactly [`CODE_ADVANCE`]
const CODE_SLACK: f32 = 0.5;

/// The width of a cell's code, in characters: the Settings ▸ Code ▸ Cell width, or the cell's
/// longest line if that is more. Zero (Fill) is the page's whole column, not a count
pub fn cell_columns(longest_line: usize) -> usize {
    usize::from(super::settings::cell_width()).max(longest_line)
}

/// The width of a cell with this longest line: the same for every cell (the Cell width setting,
/// 80 characters of code by default), widening for a longer line; with its gap either side. The
/// page column caps it ([`block_code_width`]), and lines longer than that wrap
pub fn code_width(longest_line: usize) -> f32 {
    (cell_columns(longest_line) as f32 + CODE_SLACK) * code_size() * CODE_ADVANCE + code_padding().left + code_padding().right
}

/// The width of a cell's code on the page: as [`code_width`], but never wider than the text
/// column can be (its status strip beside it), so nothing on the page runs into the goal drawn
/// past the column; longer lines wrap. With Cell width at Fill, the column's whole width
pub fn block_code_width(longest_line: usize) -> f32 {
    let cap = f32::from_bits(COLUMN_CAP.load(std::sync::atomic::Ordering::Relaxed));
    let column = if cap > 0.0 { cap } else { z(680.0) } - CARD_CHROME;
    if super::settings::cell_width() == 0 { column } else { code_width(longest_line).min(column) }
}

/// The text column's width as the layout last chose it (blocks are never wider)
static COLUMN_CAP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The text column's width in the last layout
pub fn last_column() -> f32 {
    f32::from_bits(COLUMN_CAP.load(std::sync::atomic::Ordering::Relaxed))
}

/// How a block of code looks, shown or being edited: on the page, which is the card's fill
fn code_style() -> impl Fn(&cosmic::Theme, iced::widget::text_editor::Status) -> iced::widget::text_editor::Style {
    move |theme: &cosmic::Theme, _status| {
        let c = theme.cosmic();
        iced::widget::text_editor::Style {
            background: page_color(theme).into(),
            border: Border { radius: 0.0.into(), width: 0.0, color: Color::TRANSPARENT },
            placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
            value: c.on_bg_color().into(),
            selection: Color { a: 0.32, ..Color::from(c.accent.base) },
        }
    }
}

/// What code in a cell has beneath it, for its selection's tint: the page, the cell's own fill,
/// and, where the construct the goal is for is banded, the band over it
fn on_code(banded: bool) -> Beneath {
    let page = on_page();
    if banded { page.under(super::tones::band) } else { page }
}

/// What code on the page has beneath it, for its selection's tint
fn on_page() -> Beneath {
    Beneath::new(page_color)
}

fn blend(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a }
}

pub fn pop_out_button_id() -> widget::Id {
    widget::Id::new("deductum-pop-out")
}

/// The pop-out button's size: no taller than a line of the source view, so that, centred on the
/// proof's first line, it stays inside the proof's band there and inside a code cell's corner
pub fn pop_out_size() -> f32 {
    16.0 + 2.0 * 2.0
}

/// How far the pop-out button is inside the top right corner of the code it floats over: as far
/// from the right as from the top, the top such that it is centred on the first line
pub fn pop_out_inset() -> f32 {
    (code_padding().top + code_line_height() / 2.0 - pop_out_size() / 2.0).max(1.0)
}

/// Pop out: the proof in a window of its own, a small icon button floating over the code (in
/// both views)
fn pop_out_button<'a>() -> Element<'a, Message> {
    let button = widget::button::custom(widget::icon::from_svg_bytes(include_bytes!("../res/icons/window-pop-out-symbolic.svg").as_slice())
        .symbolic(true)
        .icon()
        .size(16))
        .padding(2.0)
        .width(Length::Fixed(pop_out_size()))
        .height(Length::Fixed(pop_out_size()))
        .class(theme::Button::Icon)
        .on_press(Message::PopOut);
    widget::container(widget::tooltip(button, widget::text("Pop out (the proof in a window of its own)"), widget::tooltip::Position::Bottom)).id(pop_out_button_id()).into()
}

/// A cell's card on the page: square, its fill the page's, a 1 px border on its top, right and
/// bottom (see [`card_border`]), and its status strip as its left edge, with no border of its own
/// on that side. The selected cell's strip and 2 px border are its bright status colour. It takes
/// the same room selected or not, the border's extra pixel coming out of [`CARD_RIM`]
fn card<'a>(content: Element<'a, Message>, status: Status, selected: bool) -> Element<'a, Message> {
    let strip = widget::container(widget::Space::new())
        .width(Length::Fixed(4.0))
        .height(Length::Fill)
        .class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: Some(strip_color(status, selected, theme).into()),
            ..Default::default()
        }));
    // (the card's fill: the code's own padding is the gap after the strip)
    let inside = widget::container(content).class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(page_color(theme).into()),
        ..Default::default()
    }));
    // (the height after the children: a pushed Fill child would make the row fill)
    let row = widget::row::with_capacity(2).push(strip).push(inside).height(Length::Shrink);
    let width = if selected { 2.0 } else { 1.0 };
    let bordered = widget::container(row).padding(Padding { top: width, right: width, bottom: width, left: 0.0 }).class(theme::Container::custom(move |theme| iced::widget::container::Style {
        background: Some(card_border(status, selected, theme).0.into()),
        ..Default::default()
    }));
    let rim = CARD_RIM - width;
    widget::container(bordered).padding(Padding { top: rim, right: rim, bottom: rim, left: 0.0 }).into()
}

/// A block's card around its code
fn framed<'a>(code: Element<'a, Message>, status: Status, marked: bool) -> Element<'a, Message> {
    card(code, status, marked)
}

/// An editor that says what is under the pointer: the pointer's place (less the editor's
/// padding) goes to the app, and what the prover says about it shows beside the pointer
fn hoverable<'a>(ws: Option<&'a Workspace>, target: super::HoverTarget, pad: Padding, el: Element<'a, Message>) -> Element<'a, Message> {
    hoverable_press(ws, target, pad, el, None)
}

/// As [`hoverable`]; with `press`, a layer over the editor takes clicks (a shown block opens at
/// the click without its read-only editor taking focus, and so a caret)
fn hoverable_press<'a>(ws: Option<&'a Workspace>, target: super::HoverTarget, pad: Padding, el: Element<'a, Message>, press: Option<Message>) -> Element<'a, Message> {
    let el: Element<'a, Message> = match press {
        Some(msg) => iced::widget::stack![el, widget::mouse_area(widget::Space::new().width(Length::Fill).height(Length::Fill)).on_press(msg)].into(),
        None => el,
    };
    let area = widget::mouse_area(el)
        .on_move(move |p| Message::HoverMove(target, iced::Point::new(p.x - pad.left, p.y - pad.top)))
        .on_exit(Message::HoverExit);
    // Always in a popover, the card added when there is one: the editor inside keeps its state
    // (focus, caret) when a tooltip comes and goes
    let pop = widget::popover(area);
    let Some(ws) = ws else { return pop.into() };
    let Some(h) = ws.hover.as_ref().filter(|h| h.target == target) else { return pop.into() };
    let Some(card) = hover_card(ws, h) else { return pop.into() };
    // (just below the line under the pointer, so that it never covers that line, and a little
    // left of the pointer, under the word)
    let lh = match target {
        super::HoverTarget::Source => source_line_height(),
        _ => code_line_height(),
    };
    let below = ((h.point.y.max(0.0) / lh).floor() + 1.0) * lh + 4.0;
    pop.popup(card)
        .position(widget::popover::Position::Point(iced::Point::new((h.point.x + pad.left - 8.0).max(0.0), below + pad.top)))
        .into()
}

/// Preserve native copying and selection; route other editor keys to this window's editor.
pub fn read_only_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    use text_editor::Binding as B;
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    if let Some(binding) = B::from_key_press(press.clone()) {
        match binding {
            B::Copy | B::Select(_) | B::SelectWord | B::SelectLine | B::SelectAll | B::Unfocus => return Some(binding),
            B::Cut | B::Paste => return Some(B::Custom(Message::Noop)),
            _ => {}
        }
    }
    if press.modifiers.alt() || press.modifiers.logo()
        || (press.modifiers.command() && matches!(press.key.as_ref(), cosmic::iced::keyboard::Key::Character("," | "=" | "+" | "-" | "0")))
    {
        return None;
    }
    Some(B::Custom(Message::ReadOnlyKey(press)))
}

/// Text with Isabelle symbols, in the code font: scripts with no Unicode form are drawn in the
/// scripts font (as the editors draw them), bold symbols bold
fn symbol_text<'a>(text: &str, table: &dyn super::codec::Symbols, size: f32) -> Element<'a, Message> {
    symbol_text_with_font(text, table, size, code_font())
}

fn symbol_text_with_font<'a>(text: &str, table: &dyn super::codec::Symbols, size: f32, normal: cosmic::iced::Font) -> Element<'a, Message> {
    symbol_text_in(text, table, size, normal, iced::widget::text::Wrapping::default())
}

/// [`symbol_text`] in the code font, wrapping as `wrapping` says
fn symbol_text_wrapped<'a>(text: &str, table: &dyn super::codec::Symbols, size: f32, wrapping: iced::widget::text::Wrapping) -> Element<'a, Message> {
    symbol_text_in(text, table, size, code_font(), wrapping)
}

fn symbol_text_in<'a>(text: &str, table: &dyn super::codec::Symbols, size: f32, normal: cosmic::iced::Font, wrapping: iced::widget::text::Wrapping) -> Element<'a, Message> {
    let d = table.decode(text);
    let style_at: std::collections::HashMap<usize, super::codec::Style> = d.styles.iter().map(|(i, st)| (*i, *st)).collect();
    let mut runs: Vec<(String, Option<super::codec::Style>)> = Vec::new();
    for (i, ch) in d.text.chars().enumerate() {
        let st = style_at.get(&i).copied();
        match runs.last_mut() {
            Some((t, k)) if *k == st => t.push(ch),
            _ => runs.push((ch.to_string(), st)),
        }
    }
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = runs
        .into_iter()
        .map(|(t, st)| iced::widget::span(t).font(match st {
            Some(super::codec::Style::Script) => cosmic::iced::Font::with_name("Isabellum Scripts"),
            Some(super::codec::Style::Bold) => bold_of(normal),
            _ => normal,
        }))
        .collect();
    iced::widget::rich_text(spans).size(size).wrapping(wrapping).into()
}

fn symbol_font(st: Option<super::codec::Style>) -> cosmic::iced::Font {
    match st {
        Some(super::codec::Style::Script) => cosmic::iced::Font::with_name("Isabellum Scripts"),
        Some(super::codec::Style::Bold) => code_font_bold(),
        _ => code_font(),
    }
}

/// A message as code, as wide as its text: coloured spans in the code font, wrapping when long
/// (decoded with the profile's own symbols, whatever table the host sent)
fn code_text<'a>(ws: &Workspace, m: &Value) -> Element<'a, Message> {
    let theme = theme::active();
    let raw = m["raw"].as_str().or_else(|| m["text"].as_str()).unwrap_or("");
    let d = ws.profile.symbols(None).decode(raw.trim_end());
    let n = d.src_of.len().saturating_sub(1);
    let mut class: Vec<Option<super::editor::Sem>> = vec![None; n];
    for sp in m["spans"].as_array().into_iter().flatten() {
        let (Some(s), Some(e), Some(sem)) = (sp[0].as_u64(), sp[1].as_u64(), sp[2].as_str().and_then(super::editor::Sem::parse)) else { continue };
        for c in class.iter_mut().take(d.to_decoded(e as usize).min(n)).skip(d.to_decoded(s as usize)) {
            *c = Some(sem);
        }
    }
    let style_at: std::collections::HashMap<usize, super::codec::Style> = d.styles.iter().map(|(i, st)| (*i, *st)).collect();
    let mut runs: Vec<(String, Option<super::editor::Sem>, Option<super::codec::Style>)> = Vec::new();
    for (i, (ch, c)) in d.text.chars().zip(class.iter().copied().chain(std::iter::repeat(None))).enumerate() {
        let st = style_at.get(&i).copied();
        match runs.last_mut() {
            Some((t, k, s)) if *k == c && *s == st => t.push(ch),
            _ => runs.push((ch.to_string(), c, st)),
        }
    }
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = runs
        .into_iter()
        .map(|(t, c, st)| {
            let mut span = iced::widget::span(t).font(symbol_font(st));
            if let Some(sem) = c
                && let Some(color) = highlight_format(&super::editor::Hl::Sem(sem), &theme).color
            {
                span = span.color(color);
            }
            span
        })
        .collect();
    iced::widget::rich_text(spans).size(13.0).into()
}

/// What Isabelle says about the place under the pointer: types, what a name is, messages there
fn hover_card<'a>(ws: &'a Workspace, h: &super::Hover) -> Option<Element<'a, Message>> {
    let info = h.info.as_ref()?;
    let items: Vec<&Value> = info["tooltip"]["items"].as_array().into_iter().flatten().collect();
    let definition = info["definition"].as_object();
    if items.is_empty() && definition.is_none() {
        return None;
    }
    let sp = spacing();
    let mut col = widget::column::with_capacity(items.len() + 1).spacing(4);
    for item in items {
        col = col.push(code_text(ws, item));
    }
    if let Some(d) = definition {
        let name = d.get("name").and_then(Value::as_str).unwrap_or("");
        let file = file_name(d.get("file").and_then(Value::as_str).unwrap_or(""));
        col = col.push(widget::text::caption(format!("Ctrl+click: go to the definition of {name} ({file}, line {})", d.get("line").cloned().unwrap_or_default())));
    }
    Some(
        widget::container(col)
            .max_width(520.0)
            .padding([sp.space_xxs as f32 + 2.0, sp.space_xs as f32])
            .class(theme::Container::custom(|theme| {
                let c = theme.cosmic();
                iced::widget::container::Style {
                    background: Some(cell_color(theme).into()),
                    border: Border { radius: 6.0.into(), width: 1.0, color: Color { a: 0.2, ..Color::from(c.on_bg_color()) } },
                    shadow: iced::Shadow { color: Color { a: 0.35, ..Color::BLACK }, offset: iced::Vector::new(0.0, 2.0), blur_radius: 8.0 },
                    ..Default::default()
                }
            }))
            .into(),
    )
}

/// The theory's title and its imports' sizes, in px at 100 %
pub const TITLE_SIZE: f32 = 26.0;
pub const IMPORTS_SIZE: f32 = 13.0;

/// The theory's title as runs of text and their fonts: "Theory" in the interface's bold sans, then
/// the theory's name in the code font, bold. A code font with no bold sets the name in its regular
/// weight (never in another family)
pub(crate) fn title_runs(name: &str) -> Vec<(String, cosmic::iced::Font)> {
    vec![
        ("Theory ".to_string(), cosmic::iced::Font { weight: cosmic::iced::font::Weight::Bold, ..cosmic::font::default() }),
        (name.to_string(), code_font_bold()),
    ]
}

/// What a theory imports, as runs: the caption "imports" in the interface's sans, then each imported
/// name in the code font. Words after `keywords` or `abbrevs` (the rest of the header) are caption
pub(crate) fn imports_runs(words: &[&str]) -> Vec<(String, cosmic::iced::Font)> {
    let caption = cosmic::font::default();
    let mut runs = Vec::new();
    let mut names = true;
    for (n, word) in words.iter().enumerate() {
        let is_caption = n == 0 && *word == "imports" || matches!(*word, "keywords" | "abbrevs");
        names = names && !matches!(*word, "keywords" | "abbrevs");
        let font = if is_caption || !names { caption } else { code_font() };
        if n > 0 {
            runs.push((if font == code_font() { "  ".to_string() } else { " ".to_string() }, caption));
        }
        runs.push((word.to_string(), font));
    }
    runs
}

/// Runs of text in their fonts, as one line of text at `size`
fn runs_text<'a>(runs: Vec<(String, cosmic::iced::Font)>, size: f32) -> Element<'a, Message> {
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = runs.into_iter().map(|(text, font)| iced::widget::span(text).font(font)).collect();
    iced::widget::rich_text(spans).size(size).into()
}

/// A formal block, shown: read-only text that a click opens for editing at the click
fn block_code<'a>(ws: Option<&'a Workspace>, t: &'a Theory, i: usize, block: &Block, marked: bool, table: &dyn super::codec::Symbols, current: bool) -> Element<'a, Message> {
    let Some(Some(v)) = t.block_views.get(i) else { return highlighted_block(t, block) };
    let editor = iced::widget::text_editor(&v.content)
        .font(code_font())
        .size(code_size())
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(code_padding())
        .width(block_code_width(v.columns))
        .on_action(move |a| Message::BlockViewAction(i, a))
        .key_binding(|_| None)
        .style(code_style())
        .highlight_with::<super::editor::Highlighter>(v.highlight.themed(), highlight_format);
    let editor = code_selection(editor, on_code(false));
    // The code, and under it inside the same card the prover's messages about it
    let mut inside = widget::column::with_capacity(2).push(hoverable_press(ws, super::HoverTarget::View(i), code_padding(), editor.into(), Some(Message::ViewPress(i))));
    if let Some(m) = messages_view(t, block, table, current, block_code_width(v.columns)) {
        inside = inside.push(m);
    }
    // and the agents' notes on it, after a hairline
    if let Some(ws) = ws {
        let width = block_code_width(v.columns);
        for n in notes_on(ws, t, block.start, block.end) {
            inside = inside.push(note_in_card(ws, n, width));
        }
    }
    let cell = framed(inside.into(), t.shown_status(block.start, block.end, block.status), marked);
    #[cfg(test)]
    { widget::container(cell).id(test_block_id(i)).into() }
    #[cfg(not(test))]
    { cell }
}

#[cfg(test)]
pub fn test_block_id(index: usize) -> widget::Id {
    widget::Id::new(format!("deductum-test-block-{index}"))
}

fn highlighted_block<'a>(t: &Theory, block: &Block) -> Element<'a, Message> {
    let theme = theme::active();
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = t
        .highlighted(block.start, block.end)
        .into_iter()
        .map(|(text, hl)| {
            let mut span = iced::widget::span(text).font(code_font());
            if let Some(hl) = hl {
                let format = highlight_format(&hl, &theme);
                if let Some(color) = format.color {
                    span = span.color(color);
                }
                if let Some(font) = format.font {
                    span = span.font(font);
                }
            }
            span
        })
        .collect();
    let text: Element<'a, Message> = iced::widget::rich_text(spans).size(13).into();
    let body = widget::container(text).padding(Padding { left: 4.0, right: 6.0, top: 4.0, bottom: 4.0 }).width(Length::Fill);
    card(body.into(), block.status, false)
}

/// The text zoom, set with Ctrl+= and Ctrl+-
static ZOOM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

pub fn set_zoom(z: f32) {
    ZOOM.store(z.clamp(0.5, 3.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn zoom() -> f32 {
    f32::from_bits(ZOOM.load(std::sync::atomic::Ordering::Relaxed))
}

/// The vim mode the editors are in, or none without vim keys
static VIM_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_vim_mode(mode: Option<super::vim::Mode>) {
    let v = match mode {
        None => 0,
        Some(super::vim::Mode::Normal) => 1,
        Some(super::vim::Mode::Insert) => 2,
        Some(super::vim::Mode::Visual) => 3,
        Some(super::vim::Mode::VisualLine) => 4,
    };
    VIM_MODE.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn vim_mode_now() -> Option<super::vim::Mode> {
    match VIM_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(super::vim::Mode::Normal),
        2 => Some(super::vim::Mode::Insert),
        3 => Some(super::vim::Mode::Visual),
        4 => Some(super::vim::Mode::VisualLine),
        _ => None,
    }
}

/// A size at the current zoom
pub fn z(px: f32) -> f32 {
    (px * zoom()).round()
}

/// The advance of a character of code, in em: Noto Sans Mono's and the usual monospace's. What
/// the layout widens and wraps by; the editors draw at the font's own advance
pub const CODE_ADVANCE: f32 = 0.6;

/// Code's size at the current zoom (Settings ▸ Code ▸ Code size): in cells, the Source view and
/// the proof window
pub fn code_size() -> f32 {
    z(super::settings::code_size())
}

/// An iced family name that lives as long as the app (`Family::Name` takes a `&'static str`; the
/// few names the human chooses are kept once)
fn interned(name: &str) -> &'static str {
    static NAMES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::LazyLock::new(Default::default);
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = names.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(name.to_string(), leaked);
    leaked
}

fn weight_of(weight: u16) -> cosmic::iced::font::Weight {
    use cosmic::iced::font::Weight;
    match weight {
        ..=499 => Weight::Normal,
        500..=649 => Weight::Semibold,
        _ => Weight::Bold,
    }
}

/// The weight regular text of `font` is drawn in: Semibold with Thicken text where the family has
/// one, else Regular
fn thick_weight(font: &cosmic::iced::Font) -> cosmic::iced::font::Weight {
    use super::text_weight::{NORMAL, SEMIBOLD, weight_in};
    if super::settings::thicken() { weight_of(weight_in(super::settings::cosmic_family(font.family), SEMIBOLD)) } else { weight_of(NORMAL) }
}

/// The body font's weight for the prose renderer
fn body_weight(font: super::settings::TextFont) -> u16 {
    use super::text_weight::{NORMAL, SEMIBOLD, has_semibold};
    let family = super::settings::cosmic_family(font.editor_font().family);
    if super::settings::thicken() && has_semibold(family) { SEMIBOLD } else { NORMAL }
}

/// The bold of `font`: Bold where its family has one, else the weight it has (Regular, or
/// Semibold when thickened), never another family's face
pub fn bold_of(font: cosmic::iced::Font) -> cosmic::iced::Font {
    use super::text_weight::{BOLD, NORMAL, weight_in};
    let bold = weight_in(super::settings::cosmic_family(font.family), BOLD);
    cosmic::iced::Font { weight: if bold == NORMAL { font.weight } else { weight_of(bold) }, ..font }
}

/// Find out, before the view is built and drawn, which weights the chosen fonts have. The answer
/// is kept; while the widgets draw they hold the font system, so a font looked up then (a
/// highlighter's bold, the prose editor's body) would wait for itself
pub fn warm_fonts() {
    use super::text_weight::{BOLD, SEMIBOLD, has_weight};
    let code = super::settings::cosmic_family(code_font_family());
    let _ = (has_weight(code, SEMIBOLD), has_weight(code, BOLD));
    for font in [super::settings::display().font] {
        let _ = has_weight(super::settings::cosmic_family(font.editor_font().family), SEMIBOLD);
    }
}

fn code_font_family() -> cosmic::iced::font::Family {
    cosmic::iced::font::Family::Name(interned(&super::settings::code_font()))
}

/// The code font (Settings ▸ Code ▸ Code font) in regular text, upright: in cells, the Source
/// view, goals, messages and the names in the text. Semibold with Thicken text where it has one
pub fn code_font() -> cosmic::iced::Font {
    let mut font = cosmic::iced::Font { family: code_font_family(), ..cosmic::font::mono() };
    font.weight = thick_weight(&font);
    font.style = cosmic::iced::font::Style::Normal;
    font
}

/// The code font in bold (the theory's name in the title, bold symbols): its Bold if the family
/// has one, else its regular weight
pub fn code_font_bold() -> cosmic::iced::Font {
    bold_of(code_font())
}

/// The code font as the highlighter wants a class: bold and/or italic
pub fn code_font_in(bold: bool, italic: bool) -> cosmic::iced::Font {
    let font = if bold { code_font_bold() } else { code_font() };
    cosmic::iced::Font { style: if italic { cosmic::iced::font::Style::Italic } else { cosmic::iced::font::Style::Normal }, ..font }
}

/// One message in a cell's card, under its code (an error, a warning). Collapsed, one line: a
/// disclosure, the message's icon and what went wrong (the first line of its summary, in the code
/// font; the goal it lists is the one the goal view shows), all in the muted status colour of the
/// message (failed, unfinished) and 8 px apart; at the right a copy icon and the line. The
/// disclosure and the summary open the prover's whole message in place, in the text colour, wrapped
/// at the card's `width`. No plate and no bar of its own, as the card's border and strip are the
/// cell's
pub(crate) fn message_strip<'a>(t: &Theory, m: &super::editor::ProverMessage, table: &dyn super::codec::Symbols, width: f32) -> Element<'a, Message> {
    let (kind, start, text) = (m.kind.as_str(), m.start, m.text.as_str());
    let error = kind == "error";
    let line = super::ot::line_col(&t.local, start).0;
    let tone = move |theme: &cosmic::Theme| muted_status_color(if error { error_color(theme) } else { sorry_color(theme) }, theme);
    let tinted = move |name: &'static str| {
        widget::icon::from_name(name).size(14).icon().class(theme::Svg::custom(move |theme| iced::widget::svg::Style { color: Some(tone(theme)) }))
    };
    let full = table.decode(text.trim_end()).text;
    let summary = t.profile.message_summary(text);
    let first = summary.lines().next().unwrap_or("").to_string();
    // (there is more to show when the whole message is more than the one line shown)
    let expandable = full.trim() != first.trim();
    let open = expandable && t.open_messages.contains(&start);
    let gap = z(8.0);
    let mut head = widget::row::with_capacity(3).spacing(gap).align_y(Alignment::Center);
    if expandable {
        head = head.push(tinted(if open { "go-down-symbolic" } else { "go-next-symbolic" }));
    }
    head = head
        .push(tinted(if error { "dialog-error-symbolic" } else { "dialog-warning-symbolic" }))
        .push(widget::container(summary_text(t, m, &first, table)).width(Length::Fill).clip(true));
    let head: Element<'a, Message> = if expandable {
        // (the whole row but its copy icon and line is the disclosure)
        let button = widget::button::custom(head)
            .padding(Padding::ZERO)
            .width(Length::Fill)
            .class(theme::Button::Custom {
                active: Box::new(move |_, theme| plain_button(tone(theme))),
                disabled: Box::new(move |theme| plain_button(tone(theme))),
                hovered: Box::new(move |_, theme| plain_button(tone(theme))),
                pressed: Box::new(move |_, theme| plain_button(tone(theme))),
            })
            .on_press(Message::ToggleMessage(start));
        #[cfg(test)]
        { widget::container(button).id(test_message_id(start, "toggle")).width(Length::Fill).into() }
        #[cfg(not(test))]
        { button.into() }
    } else {
        widget::container(head).width(Length::Fill).into()
    };
    let copy = widget::tooltip(
        widget::button::custom(widget::icon::from_name("edit-copy-symbolic").size(16).icon())
            .padding(2)
            .class(theme::Button::Link)
            .on_press(Message::CopyText(full.trim_end().to_string())),
        widget::text("Copy the whole message"),
        widget::tooltip::Position::Top,
    );
    #[cfg(test)]
    let copy = widget::container(copy).id(test_message_id(start, "copy"));
    let row = widget::row::with_capacity(3)
        .spacing(gap)
        .align_y(Alignment::Center)
        .push(head)
        .push(copy)
        .push(widget::text::caption(format!("line {line}")).class(muted()));
    let mut col = widget::column::with_capacity(2).push(
        widget::container(row).padding([z(4.0), code_padding().left]).width(Length::Fill).class(theme::Container::custom(move |theme| iced::widget::container::Style {
            text_color: Some(tone(theme)),
            ..Default::default()
        })),
    );
    if open {
        // (under the summary's text; its continuation lines further in)
        let indent = code_padding().left + 2.0 * (z(14.0) + gap);
        let columns = ((width - indent - code_padding().right) / (z(12.0) * 0.6)).floor().max(20.0) as usize;
        col = col.push(
            widget::container(symbol_text_wrapped(&wrap_message(&full, columns), table, z(12.0), iced::widget::text::Wrapping::None))
                .padding(Padding { left: indent, right: code_padding().right, top: 0.0, bottom: z(4.0) })
                .width(Length::Fill)
                .class(theme::Container::custom(|theme| iced::widget::container::Style { text_color: Some(super::tones::body_text(theme, 12.0)), ..Default::default() })),
        );
    }
    col.into()
}

/// The class of Isabelle's markup that marks a name or term (a variable, a fact, a type variable)
fn term_class(sem: super::editor::Sem) -> bool {
    use super::editor::Sem::*;
    matches!(sem, Free | Bound | Skolem | Var | TFree | TVar | Dynamic | ClassParameter | Inner)
}

/// The character ranges of `line` (a message's first line as shown) that the message's own markup
/// marks as names or terms. The spans count code points of the prover's encoding (`raw`); `line`
/// is a prefix of the shown message less the characters the summary cut (`⌂`, Isabelle's
/// position mark), which are skipped when matching
pub(crate) fn markup_terms(m: &super::editor::ProverMessage, line: &str, table: &dyn super::codec::Symbols) -> Vec<std::ops::Range<usize>> {
    if m.raw.is_empty() {
        return Vec::new();
    }
    let d = table.decode(m.raw.trim_end());
    let n = d.src_of.len().saturating_sub(1);
    let mut marked = vec![false; n];
    for &(s, e, _) in m.spans.iter().filter(|(_, _, sem)| term_class(*sem)) {
        for c in marked.iter_mut().take(d.to_decoded(e).min(n)).skip(d.to_decoded(s)) {
            *c = true;
        }
    }
    let shown: Vec<char> = d.text.chars().collect();
    let mut at = 0;
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, c) in line.chars().enumerate() {
        while at < shown.len() && shown[at] != c {
            at += 1;
        }
        if at >= shown.len() {
            break;
        }
        if marked[at] {
            match out.last_mut() {
                Some(r) if r.end == i => r.end = i + 1,
                _ => out.push(i..i + 1),
            }
        }
        at += 1;
    }
    out
}

/// The character ranges between a pair of `quote` marks (not the marks): a name or term as the
/// prover writes it. A Lean `'` counts as a mark only at a word's edge, as in `h'` it is part of a name
pub(crate) fn quoted_terms(line: &str, quote: char) -> Vec<std::ops::Range<usize>> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    for (i, &c) in chars.iter().enumerate() {
        if c != quote {
            continue;
        }
        let word = |j: Option<&char>| j.is_some_and(|c| c.is_alphanumeric());
        let mark = quote == '"' || match open {
            None => !word(i.checked_sub(1).and_then(|j| chars.get(j))),
            Some(_) => !word(chars.get(i + 1)) && chars.get(i + 1) != Some(&'\''),
        };
        if !mark {
            continue;
        }
        match open.take() {
            None => open = Some(i + 1),
            Some(s) => out.push(s..i),
        }
    }
    out
}

/// The runs of a one-line summary, each with whether it is a name or term: the words and the
/// terms apart, a quote mark beside a term dropped
pub(crate) fn summary_runs(line: &str, terms: &[std::ops::Range<usize>], quote: char) -> Vec<(String, bool)> {
    let chars: Vec<char> = line.chars().collect();
    let mut term = vec![false; chars.len()];
    let mut drop = vec![false; chars.len()];
    for r in terms {
        for i in r.clone() {
            term[i] = true;
        }
        for i in [r.start.checked_sub(1), Some(r.end)].into_iter().flatten() {
            if chars.get(i).is_some_and(|c| *c == quote || *c == '"') {
                drop[i] = true;
            }
        }
    }
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if drop[i] {
            continue;
        }
        match runs.last_mut() {
            Some((t, k)) if *k == term[i] => t.push(*c),
            _ => runs.push((c.to_string(), term[i])),
        }
    }
    runs
}

/// A collapsed error's one line: its words in the interface font, its names and terms in the code
/// font (from the message's own markup where it carries any, else from its quotes), no quote marks
fn summary_text<'a>(t: &Theory, m: &super::editor::ProverMessage, first: &str, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let line = table.decode(first).text;
    let mut terms = markup_terms(m, &line, table);
    if terms.is_empty() {
        terms = quoted_terms(&line, t.profile.message_quote());
    }
    let spans: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = summary_runs(&line, &terms, t.profile.message_quote())
        .into_iter()
        .map(|(text, term)| iced::widget::span(text).font(if term { code_font() } else { cosmic::font::default() }))
        .collect();
    iced::widget::rich_text(spans).size(z(12.0)).wrapping(iced::widget::text::Wrapping::None).into()
}

/// A button with no box, its label and icons in `tone`, whatever its state
fn plain_button(tone: Color) -> widget::button::Style {
    widget::button::Style { background: None, text_color: Some(tone), icon_color: Some(tone), ..Default::default() }
}

#[cfg(test)]
pub fn test_message_id(start: usize, part: &str) -> widget::Id {
    widget::Id::new(format!("deductum-test-message-{start}-{part}"))
}

/// A message's lines wrapped to `columns` characters at spaces, keeping each line's own
/// indentation; what a wrap leaves of a line is indented two columns further
fn wrap_message(text: &str, columns: usize) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let cont = (indent + 2).min(columns / 2);
        let mut rest: Vec<char> = line.chars().collect();
        while rest.len() > columns {
            // (the last space inside the line, after its indentation; else mid-word)
            let cut = (indent.max(cont) + 1..=columns).rev().find(|&i| rest[i] == ' ').unwrap_or(columns);
            out.push(rest[..cut].iter().collect::<String>().trim_end().to_string());
            let tail: String = rest[cut..].iter().collect::<String>().trim_start().to_string();
            rest = std::iter::repeat_n(' ', cont).chain(tail.chars()).collect();
        }
        out.push(rest.iter().collect());
    }
    out.join("\n")
}

/// The errors and warnings of a block being edited, stacked, `width` wide: in the open block's card
/// under its code, as they are under a shown block
fn edit_messages<'a>(t: &Theory, be: &super::editor::BlockEditor, table: &dyn super::codec::Symbols, width: f32) -> Option<Element<'a, Message>> {
    let msgs: Vec<_> = t.messages_in(be.start, be.end).into_iter().filter(|m| (m.kind == "error" || m.kind == "warning") && !t.expected_error(m)).collect();
    if msgs.is_empty() {
        return None;
    }
    let mut strips = widget::column::with_capacity(msgs.len()).spacing(2);
    for m in msgs.into_iter().take(6) {
        strips = strips.push(message_strip(t, &m, table, width));
    }
    Some(widget::container(strips).width(Length::Fixed(width)).into())
}

/// Messages in a block's card: all for the proof at the caret, errors for failing ones
fn messages_view<'a>(t: &Theory, block: &Block, table: &dyn super::codec::Symbols, current: bool, width: f32) -> Option<Element<'a, Message>> {
    let all = t.messages_in(block.start, block.end);
    let waiting = all.iter().any(|m| t.expected_error(m));
    let msgs: Vec<_> = all
        .into_iter()
        .filter(|m| !t.expected_error(m))
        .filter(|m| current || m.kind == "error" || m.text.contains("counterexample"))
        .collect();
    if msgs.is_empty() {
        // (a lemma after a proof that ends with a tool waits for that proof, not for a tool of its own)
        let after = t.messages_in(block.start, block.end).iter().any(|m| t.waits_on_previous(m));
        let note = if after { "Checked once the proof before it is finished" } else { "Unfinished: pick a proof the tool finds (beside the proof state)" };
        return waiting.then(|| widget::container(widget::text::caption(note)).padding([z(4.0), code_padding().left]).width(Length::Fixed(width)).into());
    }
    let mut col = widget::column::with_capacity(msgs.len()).spacing(2);
    for m in msgs.into_iter().take(6) {
        col = col.push(message_strip(t, &m, table, width));
    }
    // As wide as the code it is about, under it, inside its card
    Some(widget::container(col).width(Length::Fixed(width)).into())
}

/// The goal at the caret, under the proof being edited, when the right sidebar is hidden
/// A claude or codex job's proofs belong to the cell its line is in: shown there only
pub fn fill_here(ws: &Workspace) -> bool {
    let (Some(_), Some((start, end)), Some(t)) = (&ws.fill.job, ws.fill.replace, &ws.theory) else { return false };
    let at = super::App::local_offset_pub(t, start);
    // On its line: the caret at the end of `claude[haiku]` is past the `sorry` the prover sees
    // there, so past the end of the cell as the prover's commands make it
    if at <= t.cursor && t.cursor <= super::App::local_offset_pub(t, end) {
        return true;
    }
    t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end).is_none_or(|b| b.start <= at && at <= b.end)
}

/// The goal under its cell (no room beside it): attached a little below, as wide as the cell,
/// a coloured line where the cell's status strip is. The line is the panel's left edge, as the
/// strip is the cell's: [`GOAL_EDGE`] wide, over the panel's own left outline and the padding
/// inside it, so that the grey outline remains on the top, right and bottom only and the panel's
/// left edge is the cell's. Opaque: the accent at 60 % over the page, as it looked on the page
fn goal_under<'a>(goal: Element<'a, Message>) -> Element<'a, Message> {
    let bar = widget::container(widget::Space::new()).width(Length::Fixed(GOAL_EDGE)).height(Length::Fill).class(theme::Container::custom(|theme| {
        let (accent, page) = (Color::from(theme.cosmic().accent_color()), page_color(theme));
        let mix = |a: f32, p: f32| a * 0.6 + p * 0.4;
        iced::widget::container::Style {
            background: Some(Color::from_rgb(mix(accent.r, page.r), mix(accent.g, page.g), mix(accent.b, page.b)).into()),
            ..Default::default()
        }
    }));
    #[cfg(test)]
    let bar = bar.id(test_panel_id("edge"));
    widget::container(iced::widget::stack![goal, bar].height(Length::Shrink))
        .padding(Padding { top: ATTACHED - 4.0, ..Padding::ZERO })
        .into()
}

/// The width of the coloured line that is the left edge of the goal under a cell: the panel's
/// 2 px outline and the 2 px inside it
pub const GOAL_EDGE: f32 = 4.0;

/// Goal ownership follows the edited construct, not whether a prover reply has arrived.
fn goal_block(t: &Theory, block: &super::theory::Block) -> bool {
    if block.kind != Kind::Formal {
        return false;
    }
    if t.profile.is_statement(block, &t.kinds) {
        return true;
    }
    // Lean reports expected types in declaration bodies even without a `by` block.
    if t.profile.backend() == "lean" && matches!(block.name.as_str(), "def" | "abbrev" | "instance" | "opaque") {
        return true;
    }
    if block.name == "import" {
        return false;
    }
    let source = t.source_text(block.start, block.end);
    let lean_by = t.profile.backend() == "lean" && {
        let chars: Vec<char> = source.chars().collect();
        super::prover::lean::lexer::lex(&source).iter().any(|token| {
            token.class == super::prover::lean::lexer::Class::Keyword
                && chars.get(token.start..token.end) == Some(&['b', 'y'][..])
        })
    };
    lean_by || t.profile.lexical_commands(&source, &t.kinds).len() > 1
}

fn formal_at_caret(t: &Theory) -> bool {
    if t.mode == Mode::Source && super::insert::insert_context(t) == Some(super::prover::InsertContext::Prose) {
        return false;
    }
    if t.mode == Mode::Literate {
        if let Some(be) = &t.block_editor {
            return be.wrap.is_none() && t.blocks.iter().find(|b| b.start <= be.start && be.start < b.end).is_some_and(|b| goal_block(t, b));
        }
        if let Some(i) = t.selected {
            return t.blocks.get(i).is_some_and(|b| goal_block(t, b));
        }
    }
    t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor < b.end).is_some_and(|b| goal_block(t, b))
}

fn inline_goal<'a>(ws: Option<&'a Workspace>, table: &dyn super::codec::Symbols) -> Option<Element<'a, Message>> {
    let ws = ws?;
    // (on the page, in the panel's outline)
    if ws.output.is_none() && !fill_here(ws) {
        return Some(goal_panel(None, widget::text::caption(waiting_note(ws).unwrap_or_else(|| "Checking…".into())).into()));
    }
    Some(state_view(ws, table, &on_page()))
}

#[allow(dead_code)]
fn inline_goal_text<'a>(ws: Option<&'a Workspace>, table: &dyn super::codec::Symbols) -> Option<Element<'a, Message>> {
    let ws = ws?;
    let out = ws.output.as_ref()?;
    let msgs = out["messages"].as_array()?;
    let mut col = widget::column::with_capacity(msgs.len() + 1).spacing(4);
    if let Some(c) = out.get("command").filter(|c| c.is_object()) {
        col = col.push(widget::text::caption(format!("After {} ({})", c["name"].as_str().unwrap_or(""), c["status"].as_str().unwrap_or(""))));
    }
    if msgs.is_empty() {
        col = col.push(widget::text::caption("No goal here."));
    }
    for m in msgs {
        col = col.push(message_view(ws, m, table, &on_page().under(|theme| Color { a: 0.08, ..Color::from(theme.cosmic().accent_color()) })));
        for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
            col = col.push(
                widget::row::with_capacity(2)
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .push(widget::container(symbol_text(sb, table, z(13.0))).width(Length::Fill))
                    .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
            );
        }
    }
    Some(
        widget::container(col)
            .padding([6, 10])
            .width(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: 0.08, ..Color::from(theme.cosmic().accent_color()) }.into()),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
    )
}

fn literate<'a>(app: &'a App, t: &'a Theory, inline: Option<&'a Workspace>, beside: Option<(&'a Workspace, f32, f32)>, text_width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let table = app.table(&app.workspace.as_ref().map(|w| w.machine.clone()).unwrap_or_default());
    // With room, the goal sits beside the block it belongs to, level with it; the text keeps
    // its column on the left
    // Each item of the page: what it shows in the text column, then the goal's reserved column
    // (the goal itself is drawn by the page, past whatever is level with it)
    let side_at = move |el: Element<'a, Message>, _goal: bool, _top: f32| -> Element<'a, Message> {
        match beside {
            Some((_, width, text_width)) => widget::row::with_capacity(2)
                .spacing(GOAL_GAP)
                .push(widget::container(el).width(Length::Fixed(text_width)).height(Length::Shrink))
                .push(widget::Space::new().width(Length::Fixed(width)))
                .height(Length::Shrink)
                .into(),
            None => widget::row::with_capacity(2)
                .push(widget::container(el).width(Length::Fill).height(Length::Shrink))
                .push(widget::Space::new().width(Length::Fixed(0.0)))
                .height(Length::Shrink)
                .into(),
        }
    };
    let editing = t.block_editor.as_ref().map(|b| b.start);
    let opened = editing.and_then(|start| t.blocks.iter().position(|b| b.start <= start && start <= b.end));
    let open_item = opened.filter(|_| app.popout.is_none());
    let open_top = t.block_editor.as_ref().map_or(0.0, |be| match goal_placement(true, be.content.line_count()) {
        GoalPlacement::Construct => t.construct_at(be.start, be.end).map_or(0.0, |(cs, _)| match block_line(be, cs) { 0 => 0.0, line => code_padding().top + line as f32 * code_line_height() }),
        _ => 0.0,
    });
    let mut anchor = if beside.is_some() {
        open_item.filter(|_| formal_at_caret(t)).map(|i| (i, open_top))
            .or_else(|| editing.is_none().then(|| t.blocks.iter().enumerate().find(|(i, b)| t.selected.map_or(b.start <= t.cursor && t.cursor < b.end, |s| s == *i) && goal_block(t, b)).map(|(i, _)| (i, 0.0))).flatten())
    } else { None };
    let side = move |el: Element<'a, Message>, goal: bool| side_at(el, goal, 0.0);
    let mut typo = typography(&theme::active());
    typo.size = prose_size();
    let typo = Arc::new(typo);
    // (no spacing: the gaps between blocks are hover areas of their own)
    // (side margins as the room allows: small in a narrow window, where the text needs the width)
    let gutter = if beside.is_some() {
        f32::from_bits(PAGE_GUTTER.load(std::sync::atomic::Ordering::Relaxed)) as u16
    } else if view_width() > 0.0 && view_width() < z(900.0) {
        z(PAGE_MARGIN) as u16
    } else {
        sp.space_l
    };
    let last = t.blocks.iter().rposition(|b| b.kind != Kind::End);
    let last_selected = editing.is_none() && last.is_some() && t.selected == last && last.is_some_and(|i| t.blocks[i].kind != Kind::Header);
    let indices = super::viewport::Indices::default();
    let build_table = table.clone();
    let build = move |i: usize| -> Element<'a, Message> {
        let table = build_table.clone();
        if i == t.blocks.len() { return if last_selected { side(widget::Space::new().into(), false) } else { side(block_actions(t, None, false, None), false) }; }
        if i > t.blocks.len() { return widget::Space::new().height(Length::Fixed(if beside.is_some() { view_height() * 0.5 } else { 0.0 })).into(); }
        let block = &t.blocks[i];
        // Each block is one element of the page, whatever it shows, so that the open block's
        // editor keeps its place (and so its focus and caret) as things around it come and go
        let mut item = widget::column::with_capacity(4);
        // The open block shows as its editor (which may edit only its text, inside it)
        let opens_here = editing.is_some_and(|s| block.start <= s && s <= block.end) && t.blocks.get(i.wrapping_sub(1)).is_none_or(|p| !(p.start <= editing.unwrap_or(0) && editing.unwrap_or(0) <= p.end));
        let covered = t.block_editor.as_ref().is_some_and(|b| b.start < block.start && block.start < b.end);
        if opens_here || covered {
            if opens_here {
                if app.popout.is_some() {
                    item = item.push(side(banner("This proof is open in its own window.".into(), Some(("Bring it back", Message::PopIn))), false));
                } else {
                    let formal = t.block_editor.as_ref().is_some_and(|b| b.kind == Kind::Formal);
                    // (level with the proof's top, or with the construct at the caret in a long one)
                    let lines = t.block_editor.as_ref().map_or(0, |be| be.content.line_count());
                    let top = match goal_placement(true, lines) {
                        GoalPlacement::Construct => t
                            .block_editor
                            .as_ref()
                            .zip(t.block_editor.as_ref().and_then(|be| t.construct_at(be.start, be.end)))
                            // (a construct from the block's first line: level with the cell's
                            // top, not with the text inside its padding)
                            .map_or(0.0, |(be, (cs, _))| match block_line(be, cs) {
                                0 => 0.0,
                                line => code_padding().top + line as f32 * code_line_height(),
                            }),
                        _ => 0.0,
                    };
                    item = item.push(side_at(widget::container(block_editor_in(app.workspace.as_ref(), t, false, inline, &*table)).id(selected_block_id()).into(), formal, top));
                }
            }
            // (its notes stay with it while it is open: inside its card if it is a cell, else
            // under it; nothing after it moves)
            if opens_here && let Some(ws) = app.workspace.as_ref() && !t.block_editor.as_ref().is_some_and(|be| be.kind == Kind::Formal && be.wrap.is_none()) {
                for n in notes_on(ws, t, block.start, block.end) {
                    item = item.push(side(note_card(ws, n, None), false));
                }
            }
            // (and the space after it that a shown block has: what follows stays where it was)
            if opens_here {
                item = item.push(side(widget::Space::new().height(Length::Fixed(z(BLOCK_GAP))).into(), false));
            }
            return item.into();
        }
        let current = block.start <= t.cursor && t.cursor <= block.end;
        // The block j and k selected, else the one the caret is in: outlined
        let marked = t.selected.map_or(block.start <= t.cursor && t.cursor < block.end, |s| s == i);
        let content: Element<'a, Message> = match block.kind {
            Kind::Header => {
                let src = table.decode(&t.source_text(block.start, block.end)).text;
                // The title and, under it, what it imports, however the header's lines break
                // ("theory A imports B begin" on one line reads as the same title block)
                let words: Vec<&str> = src.split_whitespace().collect();
                // ("Theory" in the interface's bold sans, the theory's name in the code font, bold; the
                // imports as a small sans caption then each name in the code font, as a term in the
                // text is: Tom, 8 Nov, mockups/t21b)
                let (title, imports) = match words.as_slice() {
                    ["theory", name, rest @ ..] => (title_runs(name), imports_runs(rest.strip_suffix(&["begin"]).unwrap_or(rest))),
                    _ => (
                        vec![(src.lines().next().unwrap_or("").to_string(), cosmic::iced::Font { weight: cosmic::iced::font::Weight::Bold, ..cosmic::font::default() })],
                        vec![(src.lines().skip(1).map(str::trim).collect::<Vec<_>>().join(" "), cosmic::font::default())],
                    ),
                };
                widget::column::with_capacity(2)
                    .push(runs_text(title, z(TITLE_SIZE)))
                    .push(widget::container(runs_text(imports, z(IMPORTS_SIZE))).class(theme::Container::custom(|theme| iced::widget::container::Style {
                        text_color: Some(super::tones::secondary_text(theme, 0.65)),
                        ..Default::default()
                    })))
                    .into()
            }
            Kind::Heading(_) | Kind::Text => match t.docs.get(i).cloned().flatten() {
                Some(doc) => rich_text(doc, typo.clone()).on_hover(move |p| Message::ProseHover(i, p)).into(),
                None => mono_block(t.source_text(block.start, block.end), block.status, &*table),
            },
            Kind::Formal => {
                // (as wide as the cell: what is attached under it is too)
                let columns = t.block_views.get(i).and_then(Option::as_ref).map_or(0, |v| v.columns);
                let mut c = widget::column::with_capacity(3).spacing(4).width(Length::Fixed(block_code_width(columns) + CARD_CHROME)).push(block_code(app.workspace.as_ref(), t, i, block, marked, &*table, current));
                if current && goal_block(t, block) && let Some(g) = inline_goal(inline, &*table) {
                    c = c.push(goal_under(g));
                }
                c.into()
            }
            Kind::Comment => symbol_text(&t.source_text(block.start, block.end), &*table, z(12.0)),
            // (the theory's `end` is source, not reading)
            Kind::End => return widget::Space::new().into(),
        };
        // The block j and k selected, found by its id to centre it; a prose block shows it with a bar
        let content: Element<'a, Message> = if t.selected == Some(i) && t.block_editor.is_none() {
            // (prose marked as the paragraph being edited is, on a region reaching past it that
            // takes no room, so that selecting it moves nothing; a formal block is marked by
            // its own background and strip)
            let marked: Element<'a, Message> = if block.kind != Kind::Formal {
                super::gap::Halo::new(content, (z(10.0), z(2.0), z(4.0)), marked_region).into()
            } else {
                content
            };
            widget::container(marked).id(selected_block_id()).into()
        } else {
            content
        };
        // Formal blocks take their own clicks (the caret goes where the click lands)
        if block.kind == Kind::Formal && t.block_views.get(i).is_some_and(Option::is_some) {
            item = item.push(side(content, marked && editing.is_none()));
        } else {
            let area = widget::mouse_area(content).on_press(Message::EditBlock(i));
            let area = if matches!(block.kind, Kind::Heading(_) | Kind::Text) { area.on_move(move |p| Message::ProsePointer(i, p)) } else { area };
            item = item.push(side(area.into(), marked && editing.is_none() && block.kind == Kind::Formal));
        }
        let selected_here = t.selected == Some(i) && editing.is_none() && block.kind != Kind::Header;
        // Agents' notes on this block: in its card if it is a cell, else a card of their own under it
        if let Some(ws) = app.workspace.as_ref().filter(|_| !notes_in_card(t, i, block)) {
            for n in notes_on(ws, t, block.start, block.end) {
                let width = (block.kind == Kind::Formal).then(|| {
                    let columns = t.block_views.get(i).and_then(Option::as_ref).map_or(0, |v| v.columns);
                    block_code_width(columns) + CARD_CHROME
                });
                item = item.push(side(note_card(ws, n, width), false));
            }
        }
        // The selected block: what can be added after it, and Delete, set a little below it
        if selected_here {
            let width = (block.kind == Kind::Formal).then(|| {
                let columns = t.block_views.get(i).and_then(Option::as_ref).map_or(0, |v| v.columns);
                block_code_width(columns) + CARD_CHROME
            });
            item = item.push(side(widget::container(block_actions(t, Some(i), true, width)).padding(Padding { top: z(8.0), ..Padding::ZERO }).into(), false));
        }
        // The gap after it: hovered, it offers what can be added there
        if !selected_here && block.kind != Kind::End {
            let hovered = app.workspace.as_ref().is_some_and(|w| w.gap_hover == Some(i));
            let inner: Element<'a, Message> = if hovered { block_actions(t, Some(i), false, None) } else { widget::Space::new().into() };
            // (not in `side`'s row: the area spans the text column, but the goal beside is placed
            // by what the items show, and an empty area shows nothing that it must keep clear of)
            let area_width = beside.map_or(Length::Fill, |(_, _, text_width)| Length::Fixed(text_width));
            let gap = widget::mouse_area(widget::container(inner).height(Length::Fixed(z(BLOCK_GAP))).width(area_width).center_y(Length::Fixed(z(BLOCK_GAP))))
                .on_enter(Message::GapHover(Some(i)))
                .on_exit(Message::GapLeave(i))
                // (it holds the pointer, so the page's own click-outside never sees a click here)
                .on_press(Message::CloseBlock);
            item = item.push(gap);
        } else {
            item = item.push(widget::Space::new().height(Length::Fixed(z(BLOCK_GAP) - z(8.0))));
        }
        item.into()
    };
    let source = t.page_rows.clone();
    let estimate = move |i: usize, width: f32| {
        let Some(row) = t.page_rows.get(i) else { return if i == t.blocks.len() { z(42.0) } else if beside.is_some() { view_height() * 0.5 } else { 0.0 }; };
        match row.kind {
            Kind::End => 0.0,
            Kind::Formal | Kind::Comment => row.lines as f32 * code_line_height() + code_padding().top + code_padding().bottom + z(BLOCK_GAP),
            Kind::Header => z(70.0),
            Kind::Heading(_) => z(55.0),
            Kind::Text => ((row.characters as f32 / (width.min(text_width) / (prose_size() * 0.5)).max(1.0)).ceil().max(row.lines as f32) * prose_size() * super::settings::display().line_height) + z(BLOCK_GAP),
        }
    };
    let style = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let display = super::settings::display();
        display.font.hash(&mut hasher);
        display.justify.hash(&mut hasher);
        for metric in [display.text_size, display.line_height, display.paragraph_spacing, display.math_scale] { metric.to_bits().hash(&mut hasher); }
        text_width.to_bits().hash(&mut hasher);
        cosmic::iced::advanced::graphics::text::font_system().read().unwrap().version().hash(&mut hasher);
        hasher.finish()
    };
    let scope = t.page_scope;
    let col = super::viewport::Column::new(indices.clone(), scope, source, style, t.page_viewport.get(), build, estimate, move |delta| Message::LiterateAdjust(scope, delta))
        .padding([sp.space_s, gutter]).editor(open_item).pin(t.selected).pin(anchor.map(|a| a.0));
    // Clicking outside every block finishes the one being edited
    // (its side margins included, so that the page is centred, the same room on either side)
    // (with the goal under the proof, the readable text column alone, centred)
    let page_width = beside.map_or(text_width, |(_, w, tw)| tw + w + GOAL_GAP) + 2.0 * gutter as f32;
    // A click no block takes (between them, beside them, below the end) finishes editing
    // Prose uses the insert palette; a formal block keeps its goal column while state is pending.
    let palette = match (beside, open_item) {
        (Some((ws, width, _)), Some(i)) if anchor.is_none() => {
            anchor = Some((i, 0.0));
            Some(super::insert::insert_palette(ws, t, &*table, width))
        }
        _ => None,
    };
    let goal: Element<'a, Message> = match (beside, anchor) {
        _ if palette.is_some() => palette.unwrap_or_else(|| widget::Space::new().into()),
        (Some((ws, width, _)), Some(_)) => match inline_goal(Some(ws), &*table) {
            Some(g) => widget::container(g)
                .width(Length::Fixed(width))
                .class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(page_color(theme).into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }))
                .into(),
            None => widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(),
        },
        _ => widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(),
    };
    let laid = super::gap::GoalLayer::new(col, goal, anchor, GOAL_GAP).indices(indices);
    let page = widget::container(widget::container(laid).max_width(page_width).width(Length::Fill));
    let page = if super::settings::display().centred { page.center_x(Length::Fill) } else { page.width(Length::Fill) };
    widget::mouse_area(thin(page).id(literate_scroll_id()).height(Length::Fill).on_scroll(move |v| Message::LiterateScrolled(scope, v.absolute_offset().y, v.bounds().height))).on_press(Message::CloseBlock).into()
}

/// Where a scroll view is (the source view's, the proof window's), to keep the caret in sight there
struct MeasureSource {
    id: widget::Id,
    view: Option<(iced::Rectangle, iced::Rectangle, iced::Vector)>,
}

impl iced::advanced::widget::Operation<(iced::Rectangle, iced::Rectangle, iced::Vector)> for MeasureSource {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<(iced::Rectangle, iced::Rectangle, iced::Vector)>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&self.id) {
            self.view = Some((bounds, content_bounds, translation));
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<(iced::Rectangle, iced::Rectangle, iced::Vector)> {
        match self.view {
            Some(v) => iced::advanced::widget::operation::Outcome::Some(v),
            None => iced::advanced::widget::operation::Outcome::None,
        }
    }
}

/// Scroll the source view so that the caret's line is in sight
pub fn keep_source_caret(line: usize) -> cosmic::app::Task<Message> {
    keep_caret_in(source_scroll_id(), line, source_line_height(), spacing().space_s as f32)
}

/// The source editor is as tall as the document; measure its scroll viewport for paging.
pub fn source_page(scope: u64, generation: u64, down: bool, select: bool) -> cosmic::app::Task<Message> {
    struct MeasurePage(Option<f32>);
    impl iced::advanced::widget::Operation<Option<f32>> for MeasurePage {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Option<f32>>)) { f(self); }
        fn scrollable(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle, _: iced::Rectangle, _: iced::Vector, _: &mut dyn iced::advanced::widget::operation::Scrollable) {
            if id == Some(&source_scroll_id()) { self.0 = Some(bounds.height); }
        }
        fn finish(&self) -> iced::advanced::widget::operation::Outcome<Option<f32>> { iced::advanced::widget::operation::Outcome::Some(self.0) }
    }
    iced::advanced::widget::operate(MeasurePage(None)).then(move |height| {
        let rows = height.map_or(0, |h| ((h / source_line_height()).floor() as usize).saturating_sub(1).clamp(1, 1000));
        cosmic::app::Task::done(cosmic::Action::App(Message::NavPageReady(scope, generation, down, select, rows)))
    })
}

/// Scroll a scroll view holding an editor at its full height so that the caret's line is in sight
pub fn keep_caret_in(id: widget::Id, line: usize, lh: f32, top: f32) -> cosmic::app::Task<Message> {
    iced::advanced::widget::operate(MeasureSource { id: id.clone(), view: None }).then(move |(view, content, translation)| {
        let y = top + line as f32 * lh;
        let margin = lh * 3.0;
        let target = if y < translation.y + margin {
            Some(y - margin)
        } else if y + lh > translation.y + view.height - margin {
            Some(y + lh + margin - view.height)
        } else {
            None
        };
        match target {
            Some(t) => iced::widget::scrollable::scroll_to(
                id.clone(),
                iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(t.clamp(0.0, (content.height - view.height).max(0.0))) },
            ),
            None => cosmic::app::Task::none(),
        }
    })
}

pub fn selected_block_id() -> widget::Id {
    widget::Id::new("deductum-selected-block")
}

/// Wait until the current widget tree has been laid out, reporting even a missing editor.
pub fn navigation_layout(scope: u64, generation: u64) -> cosmic::app::Task<Message> {
    struct Find(bool);
    impl iced::advanced::widget::Operation<bool> for Find {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<bool>)) { f(self); }
        fn focusable(&mut self, id: Option<&widget::Id>, _: iced::Rectangle, _: &mut dyn iced::advanced::widget::operation::Focusable) {
            if id == Some(&block_editor_id()) { self.0 = true; }
        }
        fn finish(&self) -> iced::advanced::widget::operation::Outcome<bool> { iced::advanced::widget::operation::Outcome::Some(self.0) }
    }
    iced::advanced::widget::operate(Find(false)).then(move |found| {
        cosmic::app::Task::done(cosmic::Action::App(Message::NavLayoutReady(scope, generation, found)))
    })
}

/// Where the selected block and the literate view's viewport are, for centring it
#[derive(Default)]
struct MeasureSelected {
    block: Option<iced::Rectangle>,
    view: Option<(iced::Rectangle, iced::Rectangle)>,
}

type Measured = (iced::Rectangle, iced::Rectangle, iced::Rectangle);

impl iced::advanced::widget::Operation<Measured> for MeasureSelected {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Measured>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
        if id == Some(&selected_block_id()) {
            self.block = Some(bounds);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: iced::Rectangle,
        content_bounds: iced::Rectangle,
        _translation: iced::Vector,
        _state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if id == Some(&literate_scroll_id()) {
            self.view = Some((bounds, content_bounds));
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<Measured> {
        match (self.block, self.view) {
            (Some(b), Some((v, c))) => iced::advanced::widget::operation::Outcome::Some((b, v, c)),
            _ => iced::advanced::widget::operation::Outcome::None,
        }
    }
}

/// Scroll the literate view so that the selected block is in the middle (a block taller
/// than the view starts at its top)
pub fn centre_selected() -> cosmic::app::Task<Message> {
    iced::advanced::widget::operate(MeasureSelected::default()).then(|(block, view, content)| {
        let top = block.y - content.y;
        let offset = if block.height > view.height * 0.9 { top - 12.0 } else { top + block.height / 2.0 - view.height / 2.0 };
        let offset = offset.clamp(0.0, (content.height - view.height).max(0.0));
        iced::widget::scrollable::scroll_to(literate_scroll_id(), iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(offset) })
    })
}

fn block_editor_in<'a>(ws: Option<&'a Workspace>, t: &'a Theory, window: bool, inline: Option<&'a Workspace>, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let Some(be) = &t.block_editor else { return widget::Space::new().into() };
    let vim_mode = vim_mode_now();
    // The block's source, coloured as the source view colours it, without a box
    // Looks as the block did shown, outlined as the current one, so opening it moves nothing
    let columns = be.content.text().lines().map(|l| l.chars().count()).max().unwrap_or(0);
    if be.wrap.is_some() && !window {
        return prose_editor(t, be, vim_mode);
    }
    let editor = iced::widget::text_editor(&be.content)
        .id(block_editor_id())
        .font(code_font())
        .size(code_size())
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(code_padding())
        .on_action(Message::BlockAction)
        .key_binding(key_bindings(!t.completions.is_empty(), vim_mode, super::EditorTarget::Block))
        .style(code_style())
        .highlight_with::<super::editor::Highlighter>(t.block_highlight.themed(), highlight_format);
    let lines = be.content.line_count();
    // The construct the proof state is for, tinted
    // (from the cell's very top when it starts on the first line, to its very bottom when it ends
    // on the last, so that the tint and the cell's strip begin and end together)
    let band_lines = (be.kind == Kind::Formal)
        .then(|| t.construct_at(be.start, be.end))
        .flatten()
        .map(|(cs, ce)| (block_line(be, cs), block_line(be, ce.saturating_sub(1).max(cs))))
        .filter(|(l0, _)| *l0 < lines);
    let band = band_lines.map(|(l0, l1)| {
        let from = if l0 == 0 { 0.0 } else { code_padding().top + l0 as f32 * code_line_height() };
        let to = if l1 + 1 >= lines { f32::INFINITY } else { code_padding().top + (l1 + 1) as f32 * code_line_height() };
        (from, to)
    });
    // (the selection's fill is one colour: the tint over the band if most of the selected lines
    // are in it, over the page if not, so that a selection within the construct, or outside it,
    // is exactly the tint over what it lies on)
    let selected_in_band = band_lines.is_some_and(|(l0, l1)| {
        let cursor = be.content.cursor();
        let Some(other) = cursor.selection else { return false };
        let (a, b) = (cursor.position.line.min(other.line), cursor.position.line.max(other.line));
        let inside = (a..=b).filter(|l| (l0..=l1).contains(l)).count();
        inside * 2 > b - a + 1
    });
    // (a selection in the band is tinted over it, as it is drawn)
    let editor = code_selection(editor, on_code(selected_in_band));
    // A long proof in a narrow window: the goal opens a gap in the editor just after the construct
    // the caret is in (the proof stays one editor; what follows moves down, nothing is covered)
    let after = (!window && formal_at_caret(t) && inline.is_some() && goal_placement(false, lines) == GoalPlacement::AfterConstruct)
        .then(|| t.construct_at(be.start, be.end))
        .flatten()
        .and_then(|(cs, ce)| {
            // indented as the construct is
            let first = be.shown_text.split('\n').nth(block_line(be, cs)).unwrap_or("");
            let indent = first.chars().take_while(|c| *c == ' ').count();
            Some((block_line(be, ce), indent, inline_goal(inline, table)?))
        });
    let floated = after.is_some();
    let tint = construct_tint();
    // The caret's line, in COSMIC's grey wash over the band (edge to edge, as the band is: from the
    // cell's very top on its first line, to its very bottom on its last)
    let caret = {
        let line = be.content.cursor().position.line.min(lines.saturating_sub(1));
        let from = if line == 0 { 0.0 } else { code_padding().top + line as f32 * code_line_height() };
        let to = if line + 1 >= lines { f32::INFINITY } else { code_padding().top + (line + 1) as f32 * code_line_height() };
        (from, to, super::tones::caret_line(&theme::active()))
    };
    // (the bands run over the editor's padding, to the strip: the card has no fill before it)
    let reach = 0.0;
    let banded = move |g: super::gap::Gap<'a, Message, cosmic::Theme, cosmic::Renderer>| {
        let g = g.reach(reach);
        let g = match band {
            Some((from, to)) => g.band(from, to, tint),
            None => g,
        };
        g.band(caret.0, caret.1, caret.2)
    };
    let editor: Element<'_, Message> = if window {
        // At its full height in a scroll view that keeps the caret in sight, so that where its
        // lines are drawn is known for the tint
        let code = banded(super::gap::Gap::new(editor.height(Length::Shrink), widget::Space::new().height(Length::Fixed(0.0)), 0.0));
        thin(code).id(popout_scroll_id()).height(Length::Fill).into()
    } else {
        let width = block_code_width(columns);
        let code = hoverable(ws, super::HoverTarget::Block, code_padding(), editor.width(width).into());
        // (always in a gap, empty when there is nothing to put there: the editor keeps its state)
        let code: Element<'_, Message> = match after {
            Some((line, indent, goal)) => {
                // Set off from the code: indented as its construct, an accent rule on its left,
                // its own background
                let rule = widget::container(widget::Space::new())
                    .width(Length::Fixed(3.0))
                    .height(Length::Fill)
                    .class(theme::Container::custom(|theme| iced::widget::container::Style {
                        background: Some(Color::from(theme.cosmic().accent_color()).into()),
                        border: Border { radius: 2.0.into(), ..Default::default() },
                        ..Default::default()
                    }));
                let card = widget::container(goal).width(Length::Fill).class(theme::Container::custom(|theme| iced::widget::container::Style {
                    background: Some(page_color(theme).into()),
                    border: Border { radius: 6.0.into(), ..Default::default() },
                    ..Default::default()
                }));
                let inset = widget::row::with_capacity(2).spacing(6).push(rule).push(card).height(Length::Shrink);
                let left = code_padding().left + indent as f32 * code_size() * CODE_ADVANCE;
                banded(super::gap::Gap::new(
                    code,
                    widget::container(inset).padding(Padding { top: 6.0, bottom: 8.0, left, right: 6.0 }).width(Length::Fixed(width)),
                    code_padding().top + (line + 1) as f32 * code_line_height(),
                ))
                .into()
            }
            None => banded(super::gap::Gap::new(code, widget::Space::new().height(Length::Fixed(0.0)), 0.0)).into(),
        };
        // Pop out: a small button floating at the top right of the code, as in the source view
        let pop: Element<'_, Message> = if be.kind == Kind::Formal { pop_out_button() } else { widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into() };
        let inset = pop_out_inset();
        let code: Element<'_, Message> = super::gap::Beside::new(code, pop, iced::Vector::new(inset, inset)).right().fixed().into();
        // (the prover's errors and warnings are in the card under the code, as under a shown block)
        let mut inside = widget::column::with_capacity(2).push(code);
        if let Some(strips) = edit_messages(t, be, table, width) {
            inside = inside.push(strips);
        }
        if be.kind == Kind::Formal && let Some(ws) = ws {
            for n in notes_on(ws, t, be.start, be.end) {
                inside = inside.push(note_in_card(ws, n, width));
            }
        }
        framed(inside.into(), t.shown_status(be.start, be.end, t.status_in(be.start, be.end).unwrap_or(Status::Finished)), true)
    };
    let mut col = widget::column::with_capacity(6).spacing(4).push(editor);
    if let Some(bar) = completion_bar(t) {
        col = col.push(bar);
    }
    // Live feedback: the block's checking status and messages, and the goal at the caret
    let status = t.status_in(be.start, be.end);
    // (errors and warnings: the rest shows with the proof state)
    let all = t.messages_in(be.start, be.end);
    if all.iter().any(|m| t.expected_error(m)) {
        col = col.push(widget::text::caption("Unfinished while the tool runs: pick a proof it finds"));
    }
    // (in the pop-out window there is no card: its messages are under the code)
    if window && let Some(strips) = edit_messages(t, be, table, block_code_width(columns)) {
        col = col.push(strips);
    }
    // Prose has no proof state
    if !window && !floated && formal_at_caret(t) && let Some(g) = inline_goal(inline, table) {
        // (part of the cell: the space below it is the space between cells, the open block's gap)
        col = col.push(goal_under(g));
    }
    if let Some(doc) = &be.preview {
        let mut typo = typography(&theme::active());
        typo.size = prose_size();
        col = col.push(rich_text(doc.clone(), Arc::new(typo)));
    }
    // (what editing means, vim's mode and the block's checking, is in the status bar at the
    // window's foot, here and in the proof window: a row under the open block would move
    // everything after it as the open block changes)
    let _ = (status, vim_mode, sp);
    if window {
        return col.into();
    }
    col.width(Length::Fixed(block_code_width(columns) + CARD_CHROME)).into()
}

/// The size scale and relative line height the prose editor sets a heading of `level` in
pub(crate) fn prose_heading_style(level: u8, prose_line_height: f32) -> (f32, f32) {
    let scale = match level { 1 => 1.55, 2 => 1.32, 3 => 1.16, 4 => 1.05, _ => 1.0 };
    (scale, Typography::heading_line_height(level, prose_line_height))
}

/// A heading or text being edited: its text alone, set in the font, size and width it reads in,
/// on the page rather than in a box; code inside it (antiquotations) keeps code colours, and
/// what only renders (maths, antiquotations) shows rendered underneath
///
/// It is set exactly where the rendered paragraph was (no padding, the rendered paragraphs
/// around it the renderer's own paragraph spacing away), so that opening and closing it moves
/// nothing on the page; a faint region behind it, reaching past the text without taking room,
/// says that it is being edited
fn prose_editor<'a>(t: &'a Theory, be: &'a super::editor::BlockEditor, vim_mode: Option<super::vim::Mode>) -> Element<'a, Message> {
    // (a paragraph that is a Markdown heading, in a Lean module doc, is set as the heading it is)
    let markdown_heading = (be.kind == Kind::Text && be.wrap.is_some()).then(|| {
        let text = be.content.text();
        let hashes = text.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&hashes).then(|| text.chars().nth(hashes).is_some_and(|c| c == ' ')).unwrap_or(false).then_some(hashes as u8)
    });
    let level = match be.kind {
        // (as the rendered view sets it, which stops at level four)
        Kind::Heading(level) => Some(super::editor::rendered_heading_level(level)),
        _ => markdown_heading.flatten(),
    };
    let (scale, bold) = match level {
        Some(level) => (prose_heading_style(level, super::settings::display().line_height).0, true),
        None => (1.0, false),
    };
    let serif = cosmic::iced::Font {
        weight: if bold { cosmic::iced::font::Weight::Bold } else { super::settings::display().font.editor_font().weight },
        ..super::settings::display().font.editor_font()
    };
    let size = prose_size();
    let editor = iced::widget::text_editor(&be.content)
        .id(block_editor_id())
        .placeholder(if bold { "Section title" } else { "Text" })
        .font(serif)
        .size(size * scale)
        .line_height(iced::widget::text::LineHeight::Relative(match level {
            // (a heading's lines close up as it grows, as the renderer sets them, household rule 23a)
            Some(level) => prose_heading_style(level, super::settings::display().line_height).1,
            None => super::settings::display().line_height,
        }))
        .padding(Padding::ZERO)
        .on_action(Message::BlockAction)
        .key_binding(key_bindings(!t.completions.is_empty(), vim_mode, super::EditorTarget::Block))
        .style(|theme: &cosmic::Theme, _status| {
            let c = theme.cosmic();
            iced::widget::text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                value: c.on_bg_color().into(),
                selection: Color { a: 0.32, ..Color::from(c.accent.base) },
            }
        })
        .highlight_with::<super::editor::Highlighter>(t.block_highlight.themed(), prose_format);
    // (on the page, under the wash of the region marking the paragraph being edited)
    let editor = code_selection(editor, on_page());
    let editor: Element<'a, Message> = super::gap::Halo::new(editor, (z(10.0), z(2.0), z(4.0)), marked_region).into();
    // The block's other paragraphs stay as they read, above and below the one being edited
    let (before, after) = t
        .blocks
        .iter()
        .find(|b| b.start <= be.start && be.start <= b.end)
        .and_then(|b| b.body)
        .map_or((String::new(), String::new()), |(bs, bend)| (t.source_text(bs, be.start), t.source_text(be.end, bend)));
    let typo = || {
        let mut typo = typography(&theme::active());
        typo.size = size;
        Arc::new(typo)
    };
    // (the renderer's space between paragraphs, which it leaves between the rendered ones)
    let paragraph_gap = || widget::Space::new().height(Length::Fixed(size * super::settings::display().paragraph_spacing));
    let rendered = |src: &str| -> Option<Element<'a, Message>> {
        (!src.trim().is_empty()).then(|| {
            let md = t.profile.prose(src.trim(), &*t.profile.symbols(None));
            rich_text(Arc::new(prose_render::markdown::parse(&md)), typo()).into()
        })
    };
    // (all three places always there, empty or not: the editor keeps its place, and so its focus)
    let mut col = widget::column::with_capacity(5).width(Length::Fill);
    col = col.push(match rendered(&before) {
        Some(r) => widget::column::with_capacity(2).push(r).push(paragraph_gap()).into(),
        None => Element::from(widget::Space::new().height(Length::Fixed(0.0))),
    });
    col = col.push(editor);
    col = col.push(match rendered(&after) {
        Some(r) => widget::column::with_capacity(2).push(paragraph_gap()).push(r).into(),
        None => Element::from(widget::Space::new().height(Length::Fixed(0.0))),
    });
    if let Some(bar) = completion_bar(t) {
        col = col.push(widget::container(bar).padding(Padding { top: z(8.0), ..Padding::ZERO }));
    }
    // The maths or antiquotation the caret is in, set as it will read, at the column's width
    if let Some(snippet) = t.profile.snippet_at(&be.shown(), {
        let shown = be.shown();
        super::editor::LineIndex::new(&shown).offset(&shown, be.content.cursor().position)
    }) {
        let md = t.profile.prose(&snippet, &*t.profile.symbols(None));
        col = col.push(
            widget::container(
                widget::container(rich_text(Arc::new(prose_render::markdown::parse(&md)), typo()))
                    .padding([z(4.0), z(10.0)])
                    .width(Length::Fill)
                    .class(theme::Container::custom(|theme| iced::widget::container::Style {
                        background: Some(blend(page_color(theme), cell_color(theme), 0.7).into()),
                        border: Border { radius: 6.0.into(), width: 1.0, color: Color { a: 0.18, ..Color::from(theme.cosmic().accent_color()) } },
                        ..Default::default()
                    })),
            )
            .padding(Padding { top: z(8.0), ..Padding::ZERO }),
        );
    }
    col.into()
}

/// Prose being edited: plain text in the editor's own font; code inside it in mono and colour
pub fn prose_format(hl: &super::editor::Hl, theme: &cosmic::Theme) -> iced::advanced::text::highlighter::Format<cosmic::iced::Font> {
    use super::editor::Hl;
    match hl {
        // (checking status would grey the whole text: prose shows none)
        Hl::Unprocessed | Hl::Running | Hl::Comment => iced::advanced::text::highlighter::Format { color: None, font: None },
        // The prover marks a document text's body itself (as comment, raw or cartouche text):
        // that is the prose, set as prose
        Hl::Sem(super::editor::Sem::Comment | super::editor::Sem::Raw | super::editor::Sem::Inner) => {
            iced::advanced::text::highlighter::Format { color: None, font: None }
        }
        // (in one regular weight: bold keywords in a line of serif text read as shouting, and
        // the code's colours already set it apart)
        Hl::Sem(_) | Hl::Keyword => {
            let mut f = highlight_format(hl, theme);
            f.font = Some(code_font());
            f
        }
        // Rendered markup: bold and emphasis in the prose's own font
        Hl::Bold | Hl::Emph => iced::advanced::text::highlighter::Format {
            color: None,
            font: Some(cosmic::iced::Font {
                weight: if *hl == Hl::Bold { cosmic::iced::font::Weight::Bold } else { super::settings::display().font.editor_font().weight },
                style: if *hl == Hl::Emph { cosmic::iced::font::Style::Italic } else { cosmic::iced::font::Style::Normal },
                ..super::settings::display().font.editor_font()
            }),
        },
        _ => highlight_format(hl, theme),
    }
}

/// A definition in a window of its own (Alt+click): its text, and a way to it in the main window
pub fn reference_window(app: &App, id: iced::window::Id) -> Element<'_, Message> {
    let sp = spacing();
    let Some((_, r)) = app.references.iter().find(|(w, _)| *w == id) else { return widget::text("").into() };
    let mut actions: Vec<Element<'_, Message>> = Vec::new();
    if !r.back.is_empty() {
        actions.push(widget::button::icon(widget::icon::from_name("go-previous-symbolic")).tooltip("Back to the definition before").on_press(Message::RefBack(id)).into());
    }
    actions.push(
        widget::tooltip(
            widget::button::standard("Open in the main window").on_press(Message::RefOpenHere(id)),
            widget::text("Open this theory in the main window, at the definition"),
            widget::tooltip::Position::Bottom,
        )
        .into(),
    );
    let header = window_header(r.title.clone(), actions);
    let footer = super::shell::window_status_bar(None, None, &["Read-only", "Ctrl+click: go to a definition here", "Alt+click: in a new window"]);
    // The whole theory, read-only, as the source view draws it: the definition tinted, tooltips
    // under the pointer
    let content = widget::responsive(move |size| {
        let t = &r.theory;
        let pad = Padding::from(sp.space_s as f32);
        let code = iced::widget::text_editor(&t.source)
            .font(code_font())
            .size(code_size())
            .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
            .padding(pad)
            .height(Length::Shrink)
            .key_binding(|press| {
                use iced::widget::text_editor::Binding as B;
                B::from_key_press(press).filter(|b| matches!(b, B::Copy | B::Move(_) | B::Select(_) | B::SelectWord | B::SelectLine | B::SelectAll | B::Unfocus))
            })
            .on_action(move |a| Message::RefAction(id, a))
            .style(|theme: &cosmic::Theme, _status| {
                let c = theme.cosmic();
                iced::widget::text_editor::Style {
                    background: Color::TRANSPARENT.into(),
                    border: Border::default(),
                    placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                    value: c.on_bg_color().into(),
                    selection: Color { a: 0.32, ..Color::from(c.accent.base) },
                }
            })
            .highlight_with::<super::editor::Highlighter>(t.highlight.themed(), highlight_format);
        let code = code_selection(code, on_page());
        let (l0, l1) = reference_lines(r);
        let row_top = reference_rows(r, size.width - pad.left - pad.right);
        let band = construct_tint();
        let gap = super::gap::Gap::new(code, widget::Space::new().height(Length::Fixed(0.0)), 0.0).band(pad.top + row_top(l0), pad.top + row_top(l1 + 1), band);
        let el: Element<'_, Message> =
            thin(hoverable(app.workspace.as_ref(), super::HoverTarget::Ref(id), pad, gap.into())).id(reference_scroll_id(id)).height(Length::Fill).into();
        el
    });
    widget::container(widget::column::with_capacity(3).push(header).push(content).push(footer))
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(page_color(theme).into()),
            text_color: Some(theme.cosmic().on_bg_color().into()),
            ..Default::default()
        }))
        .into()
}

/// The lines of a reference's definition: from its line to the next blank one (at most 40)
fn reference_lines(r: &super::Reference) -> (usize, usize) {
    let lines: Vec<&str> = r.theory.decoded.text.split('\n').collect();
    let mut end = r.line.min(lines.len().saturating_sub(1));
    while end + 1 < lines.len() && !lines[end + 1].trim().is_empty() && end - r.line < 40 {
        end += 1;
    }
    (r.line, end)
}

/// Where a reference's lines are drawn, below its text's top, wrapped at this width
fn reference_rows(r: &super::Reference, width: f32) -> impl Fn(usize) -> f32 {
    let per_row = (width / (code_size() * CODE_ADVANCE)).floor().max(20.0) as usize;
    let rows: Vec<usize> = r.theory.decoded.text.split('\n').map(|l| l.chars().count().div_ceil(per_row).max(1)).collect();
    move |line| rows.iter().take(line).sum::<usize>() as f32 * code_line_height()
}

pub fn reference_scroll_id(id: iced::window::Id) -> widget::Id {
    widget::Id::new(format!("deductum-reference-{id:?}"))
}

/// Scroll a reference window to its definition, a few lines above it in sight
pub fn scroll_reference(id: iced::window::Id, r: &super::Reference) -> cosmic::app::Task<Message> {
    // (the window's width is not known here: wrapping is estimated at its usual width)
    let y = reference_rows(r, z(780.0))(r.line) - 3.0 * code_line_height();
    iced::widget::scrollable::scroll_to(reference_scroll_id(id), iced::widget::scrollable::AbsoluteOffset { x: None, y: Some(y.max(0.0)) })
}

/// A window's own header bar (the proof and reference windows have no header of the app's): its
/// title, then its actions, on COSMIC's header background with a rule under it
fn window_header<'a>(title: String, actions: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let sp = spacing();
    let mut row = widget::row::with_capacity(actions.len() + 1).spacing(sp.space_xxs).align_y(Alignment::Center).push(widget::text::heading(title).width(Length::Fill));
    for a in actions {
        row = row.push(a);
    }
    widget::container(row)
        .padding([0, sp.space_s])
        .height(Length::Fixed(48.0))
        .center_y(Length::Fixed(48.0))
        .width(Length::Fill)
        .class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            iced::widget::container::Style {
                background: Some(Color::from(c.bg_color()).into()),
                border: Border { width: 0.0, ..Default::default() },
                shadow: iced::Shadow { color: Color { a: 0.18, ..Color::BLACK }, offset: iced::Vector::new(0.0, 1.0), blur_radius: 0.0 },
                ..Default::default()
            }
        }))
        .into()
}

/// A statement's name, from its first line ("lemma add_zero: …" is add_zero), else the line
fn statement_name(first: &str) -> String {
    let mut words = first.split_whitespace();
    let _keyword = words.next();
    match words.next().map(|w| w.trim_end_matches(':')) {
        Some(name) if !name.is_empty() && !name.starts_with('"') && !name.starts_with('‹') => name.to_string(),
        _ => first.to_string(),
    }
}

/// A panel of a window, its corners rounded (the editor inside draws the cell's colour)
fn window_card<'a>(content: Element<'a, Message>) -> widget::Container<'a, Message, cosmic::Theme> {
    widget::container(content).class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(blend(cell_color(theme), Color::BLACK, if theme.cosmic().is_dark { 0.32 } else { 0.0 }).into()),
        border: Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }))
}

/// The proof workspace in its own window (design C4): the block's source, the
/// goal at its caret and Sledgehammer, on the same buffer as the main window
pub fn proof_window(app: &App) -> Element<'_, Message> {
    let sp = spacing();
    let Some(ws) = &app.workspace else { return widget::text("No session").into() };
    let Some(t) = &ws.theory else { return widget::text("No theory").into() };
    let Some(be) = &t.block_editor else {
        return widget::container(widget::text::body("The proof was closed in the main window."))
            .center(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(page_color(theme).into()),
                ..Default::default()
            }))
            .into();
    };
    // As the main window: the state beside the proof when there is room, else under it
    let content = widget::responsive(move |size| {
        super::insert::set_popout_open(app.popout.is_some());
        let table = app.table(&ws.machine);
        // (the state on the page, in the goal panel's own outline)
        let state = widget::container(thin(widget::container(state_view(ws, &*table, &on_page())).padding(Padding { right: z(10.0), ..Padding::ZERO }))).padding([sp.space_xs, sp.space_xs]).class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(page_color(theme).into()),
            ..Default::default()
        }));
        let editor = window_card(block_editor_in(Some(ws), t, true, None, &*table));
        let wide = size.width >= z(900.0);
        let el: Element<'_, Message> = if wide {
            widget::row::with_capacity(2)
                .spacing(sp.space_s)
                .padding(sp.space_s)
                .push(editor.width(Length::FillPortion(3)).height(Length::Fill))
                .push(state.width(Length::FillPortion(2)).height(Length::Fill))
                .into()
        } else {
            widget::column::with_capacity(2)
                .spacing(sp.space_s)
                .padding(sp.space_s)
                .push(editor.height(Length::FillPortion(3)).width(Length::Fill))
                .push(state.height(Length::FillPortion(2)).width(Length::Fill))
                .into()
        };
        el
    });
    // An opaque window: its own header, background and status bar, whatever the theme's transparency
    let table = app.table(&ws.machine);
    let block = t.blocks.iter().find(|b| b.start <= be.start && be.start <= b.end);
    let title = block.map_or_else(|| "Proof".to_string(), |b| format!("Proof of {}", statement_name(&block_title(t, &*table, b, 80))));
    let header = window_header(
        format!("{title} — {}", file_name(&t.path)),
        vec![
            widget::tooltip(
                widget::button::standard("Bring back").on_press(Message::PopIn),
                widget::text("Edit the proof in the main window again"),
                widget::tooltip::Position::Bottom,
            )
            .into(),
        ],
    );
    let footer = super::shell::window_status_bar(vim_mode_now(), t.status_in(be.start, be.end), &["Esc or Bring back to finish"]);
    let window: Element<'_, Message> = widget::container(widget::column::with_capacity(3).push(header).push(content).push(footer))
        .width(Length::Fill)
        .height(Length::Fill)
        .class(theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let on: Color = cosmic.on_bg_color().into();
            iced::widget::container::Style {
                background: Some(page_color(theme).into()),
                text_color: Some(Color { a: 1.0, ..on }),
                icon_color: Some(Color { a: 1.0, ..on }),
                ..Default::default()
            }
        }))
        .into();
    window.map(move |message| match message {
        Message::ReadOnlyKey(press) if !app.vim_enabled && !read_only_navigation(&press) => Message::Noop,
        Message::ReadOnlyKey(press) => Message::RawVim(super::EditorTarget::Block, press),
        other => other,
    })
}

fn key_bindings(completing: bool, vim: Option<super::vim::Mode>, target: super::EditorTarget) -> impl Fn(text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    move |press| key_binding(press, completing, vim, target)
}

fn key_binding(press: text_editor::KeyPress, completing: bool, vim: Option<super::vim::Mode>, target: super::EditorTarget) -> Option<text_editor::Binding<Message>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    if vim.is_some_and(|mode| mode != super::vim::Mode::Insert) {
        let handled = resolve_key_binding(press.clone(), completing, vim, target).is_some()
            || resolve_key_binding(press.clone(), completing, Some(super::vim::Mode::Insert), target).is_some();
        return handled.then_some(text_editor::Binding::Custom(Message::RawVim(target, press)));
    }
    resolve_key_binding(press, completing, vim, target)
}

pub(super) fn resolve_key_binding(press: text_editor::KeyPress, completing: bool, vim: Option<super::vim::Mode>, target: super::EditorTarget) -> Option<text_editor::Binding<Message>> {
    use cosmic::iced::keyboard::Key;
    use cosmic::iced::keyboard::key::Named;
    let m = press.modifiers;
    if m.command() && matches!(press.key.as_ref(), Key::Character("=" | "+" | "-" | "0")) {
        return None;
    }
    // The settings, from any editor and in any vim mode
    if m.command() && matches!(press.key.as_ref(), Key::Character(",")) {
        return Some(text_editor::Binding::Custom(Message::Shell(super::shell::Msg::ToggleSettings)));
    }
    // A tool's offers, in any vim mode: Alt+Enter takes the highlighted one, Alt+Up and
    // Alt+Down pick another (see `flow`)
    if m.alt() && !m.control() && !m.logo() {
        match press.key.as_ref() {
            Key::Named(Named::Enter) => return Some(text_editor::Binding::Custom(Message::AcceptOffer)),
            Key::Named(Named::ArrowDown) => return Some(text_editor::Binding::Custom(Message::PickOffer(true))),
            Key::Named(Named::ArrowUp) => return Some(text_editor::Binding::Custom(Message::PickOffer(false))),
            _ => {}
        }
    }
    // Vim: outside insert mode every key goes to the vim machine
    if let Some(mode) = vim {
        let custom = |k: &str, ctrl: bool| Some(text_editor::Binding::Custom(Message::Vim(k.to_string(), ctrl)));
        // Copy, cut and paste stay the desktop's, in every mode
        if m.control() {
            match press.key.as_ref() {
                Key::Character("c") => return Some(text_editor::Binding::Custom(Message::CopySelection(false))),
                Key::Character("x") => return Some(text_editor::Binding::Custom(Message::CopySelection(true))),
                Key::Character("v") => return Some(text_editor::Binding::Paste),
                _ => {}
            }
        }
        if mode != super::vim::Mode::Insert {
            return match press.key.as_ref() {
                Key::Named(Named::Escape) => custom("Escape", false),
                Key::Named(Named::Enter) => custom("Enter", false),
                Key::Named(Named::Backspace) => custom("Backspace", false),
                Key::Named(Named::ArrowLeft) => custom("h", false),
                Key::Named(Named::ArrowRight) => custom("l", false),
                Key::Named(Named::ArrowUp) => custom("k", false),
                Key::Named(Named::ArrowDown) => custom("j", false),
                Key::Character(c) if m.control() => custom(c, true),
                _ => match press.text.as_deref() {
                    Some(t) if !t.is_empty() && !m.control() && !m.alt() => custom(t, false),
                    _ => None,
                },
            };
        }
        if let Key::Named(Named::Escape) = press.key.as_ref() {
            return custom("Escape", false);
        }
    }
    match press.key.as_ref() {
        Key::Character("c") if m.control() && !m.shift() => Some(text_editor::Binding::Custom(Message::CopySelection(false))),
        Key::Character("x") if m.control() && !m.shift() => Some(text_editor::Binding::Custom(Message::CopySelection(true))),
        Key::Named(Named::Tab) if completing && !m.shift() => Some(text_editor::Binding::Custom(Message::Complete(0))),
        // Tab indents, as in any code editor, instead of moving the focus
        Key::Named(Named::Tab) if m.shift() => Some(text_editor::Binding::Custom(Message::Unindent)),
        Key::Named(Named::Tab) => Some(text_editor::Binding::Custom(Message::InsertSymbol(" ".repeat(super::settings::indent_width())))),
        Key::Character("z") if m.command() && m.shift() => Some(text_editor::Binding::Custom(Message::Redo)),
        Key::Character("z") if m.command() => Some(text_editor::Binding::Custom(Message::Undo)),
        Key::Character("y") if m.command() => Some(text_editor::Binding::Custom(Message::Redo)),
        Key::Character("s") if m.command() => Some(text_editor::Binding::Custom(Message::Save)),
        // Zoom is the window's: decline it, so the window-wide shortcut sees it
        Key::Character("=" | "+" | "-" | "0") if m.command() => None,
        Key::Named(Named::Escape) => Some(text_editor::Binding::Custom(Message::Escape)),
        // Up and down run on past a block's ends into its neighbours, as j and k do
        Key::Named(Named::ArrowUp) if !m.shift() && !m.control() && !m.alt() => Some(text_editor::Binding::Custom(Message::LineMove(target, false))),
        Key::Named(Named::ArrowDown) if !m.shift() && !m.control() && !m.alt() => Some(text_editor::Binding::Custom(Message::LineMove(target, true))),
        Key::Named(Named::Enter) if !m.shift() && !m.command() && !m.alt() => Some(text_editor::Binding::Custom(Message::Newline)),
        _ => text_editor::Binding::from_key_press(press),
    }
}

#[cfg(test)]
#[test]
fn deferred_vim_keys_resolve_in_the_current_mode() {
    use cosmic::iced::keyboard::{Key, Modifiers, key::{Named, NativeCode, Physical}};
    use text_editor::{Binding as B, KeyPress, Status};
    let press = |key: Key, text: Option<&str>| KeyPress {
        key: key.clone(), modified_key: key, physical_key: Physical::Unidentified(NativeCode::Unidentified),
        modifiers: Modifiers::empty(), text: text.map(Into::into), status: Status::Focused { is_hovered: false },
    };
    let tab = press(Key::Named(Named::Tab), None);
    let mut inactive_copy = press(Key::Character("c".into()), Some("c"));
    inactive_copy.modifiers = Modifiers::CTRL;
    inactive_copy.status = Status::Active;
    assert!(key_binding(inactive_copy, false, Some(super::vim::Mode::Normal), super::EditorTarget::Source).is_none(), "an unfocused source editor cannot intercept a goal's copy shortcut");
    assert!(matches!(key_binding(tab.clone(), false, Some(super::vim::Mode::Normal), super::EditorTarget::Block), Some(B::Custom(Message::RawVim(super::EditorTarget::Block, _)))));
    assert!(matches!(resolve_key_binding(tab, false, Some(super::vim::Mode::Insert), super::EditorTarget::Block), Some(B::Custom(Message::InsertSymbol(_)))));
    let letter = press(Key::Character("a".into()), Some("a"));
    assert!(matches!(resolve_key_binding(letter.clone(), false, Some(super::vim::Mode::Normal), super::EditorTarget::Block), Some(B::Custom(Message::Vim(_, false)))));
    assert!(matches!(resolve_key_binding(letter, false, Some(super::vim::Mode::Insert), super::EditorTarget::Block), Some(B::Insert('a'))));
    let right = press(Key::Named(Named::ArrowRight), None);
    assert!(matches!(resolve_key_binding(right, false, Some(super::vim::Mode::Insert), super::EditorTarget::Block), Some(B::Move(_))));
}

fn completion_bar(t: &Theory) -> Option<Element<'_, Message>> {
    if t.completions.is_empty() {
        return None;
    }
    let mut row = widget::row::with_capacity(t.completions.len() + 1).spacing(4).align_y(Alignment::Center);
    row = row.push(widget::text::caption("Complete:"));
    for (i, c) in t.completions.iter().enumerate() {
        let label = if i == 0 { format!("{}  ⇥", c.label) } else { c.label.clone() };
        row = row.push(
            widget::button::text(label)
                .class(if i == 0 { theme::Button::Suggested } else { theme::Button::Text })
                .on_press(Message::Complete(i)),
        );
    }
    Some(widget::scrollable::horizontal(row).into())
}

/// The source view's width for lines this long: their measure, within limits (longer lines wrap)
pub fn source_width(columns: usize) -> f32 {
    (columns.clamp(60, 100) as f32 + 4.0) * code_size() * CODE_ADVANCE + 2.0 * spacing().space_s as f32
}

/// Where the proof state goes relative to the proof it belongs to
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalPlacement {
    /// Beside the text, at the top of what is in view
    Pinned,
    /// Beside the text, level with the proof's top
    Proof,
    /// Beside the text, level with the construct (have, show, case, ... or the step) at the caret
    Construct,
    /// Under the whole proof
    UnderProof,
    /// Just after the construct at the caret
    AfterConstruct,
}

static VIEW_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static PLACEMENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn set_view_height(h: f32) {
    VIEW_HEIGHT.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

fn view_height() -> f32 {
    f32::from_bits(VIEW_HEIGHT.load(std::sync::atomic::Ordering::Relaxed)).max(400.0)
}

fn set_placement_override(p: Option<GoalPlacement>) {
    let v = match p {
        None => 0,
        Some(GoalPlacement::Pinned) => 1,
        Some(GoalPlacement::Proof) => 2,
        Some(GoalPlacement::Construct) => 3,
        Some(GoalPlacement::UnderProof) => 4,
        Some(GoalPlacement::AfterConstruct) => 5,
    };
    PLACEMENT.store(v, std::sync::atomic::Ordering::Relaxed);
}

static WIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the last layout had room for the goal beside the text
pub fn last_wide() -> bool {
    WIDE.load(std::sync::atomic::Ordering::Relaxed)
}

static SOURCE_BESIDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the source view, as last laid out, had the goal's column beside the source (for the
/// tests)
#[cfg(test)]
pub fn last_source_beside() -> bool {
    SOURCE_BESIDE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The rule: a proof that fits comfortably in view keeps its goal with the whole proof (level
/// with its top beside it, or under it); a longer one keeps it with the construct at the caret,
/// so that it stays near (level with it beside, or just after it)
pub fn goal_placement(wide: bool, proof_lines: usize) -> GoalPlacement {
    match PLACEMENT.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return GoalPlacement::Pinned,
        2 => return GoalPlacement::Proof,
        3 => return GoalPlacement::Construct,
        4 => return GoalPlacement::UnderProof,
        5 => return GoalPlacement::AfterConstruct,
        _ => {}
    }
    let height = proof_lines as f32 * code_line_height();
    match wide {
        // (level with the construct, moving only as far as it must to stay in sight)
        true => GoalPlacement::Construct,
        false if height <= view_height() * 0.35 => GoalPlacement::UnderProof,
        false => GoalPlacement::AfterConstruct,
    }
}

/// Between a block and the goal beside it
const GOAL_GAP: f32 = 10.0;

/// Between one block and the next (a hover area that offers what can be added there)
const BLOCK_GAP: f32 = 20.0;

/// The page's side margins, at least, while the goal beside it can keep them
const PAGE_MARGIN: f32 = 24.0;

/// Room kept for the page's scroll bar
const SCROLLBAR_ROOM: f32 = 16.0;

/// The page's side margins when the goal is beside it, as the layout computed them
static PAGE_GUTTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub const GOAL_GAP_PUB: f32 = GOAL_GAP;

/// The tint over the construct the goal is for
fn construct_tint() -> Color {
    // (fainter on a light theme, where the text over it is dark and needs the contrast)
    super::tones::band(&theme::active())
}

/// Code's line height, set on its editors and used to place things level with lines
const CODE_LINE: f32 = 1.4;

/// The source view's line height
pub fn source_line_height() -> f32 {
    code_size() * CODE_LINE
}

pub fn popout_scroll_id() -> widget::Id {
    widget::Id::new("deductum-popout-scroll")
}

/// Keep the proof window's caret in sight
pub fn keep_popout_caret(line: usize) -> cosmic::app::Task<Message> {
    keep_caret_in(popout_scroll_id(), line, code_line_height(), code_padding().top)
}

pub fn source_scroll_id() -> widget::Id {
    widget::Id::new("deductum-source-scroll")
}

fn source<'a>(ws: &'a Workspace, t: &'a Theory, inline: Option<&'a Workspace>, table: &dyn super::codec::Symbols, beside: Option<(f32, f32)>, width: f32) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(4).spacing(2).height(Length::Fill);
    let pad = Padding::from(spacing().space_s as f32);
    // The editor at its full height in a scroll view (shared with the goal when that is beside
    // it), so that where its lines are drawn is known: the construct the caret is in (the
    // statement around it, or the step if that is long) is tinted, and the goal sits level with it
    let source_width = beside.map_or(width, |(_, w)| w);
    let construct = t
        .blocks
        .iter()
        .find(|b| b.start <= t.cursor && t.cursor <= b.end)
        .and_then(|b| t.construct_at(b.start, b.end))
        .map(|(cs, ce)| {
            let line = |o: usize| t.lines.position(&t.decoded.text, t.decoded.to_decoded(o)).line;
            (line(cs), line(ce.saturating_sub(1).max(cs)))
        });
    // (in rows as drawn: a long line wraps onto several)
    let per_row = ((source_width - pad.left - pad.right) / (code_size() * CODE_ADVANCE)).floor().max(20.0) as usize;
    let rows: Vec<usize> = t.decoded.text.split('\n').map(|l| l.chars().count().div_ceil(per_row).max(1)).collect();
    let row_top = |line: usize| pad.top + rows.iter().take(line).sum::<usize>() as f32 * source_line_height();
    let line = construct.map_or(t.source.cursor().position.line, |(l0, _)| l0);
    let top = row_top(line);
    let mut code = super::gap::Gap::new(source_editor(t, Length::Shrink), widget::Space::new().height(Length::Fixed(0.0)), 0.0);
    if let Some((l0, l1)) = construct {
        code = code.band(row_top(l0), row_top(l1 + 1), construct_tint());
    }
    // (the caret's line in COSMIC's grey wash, over the band)
    let caret = t.source.cursor().position.line;
    code = code.band(row_top(caret), row_top(caret + 1), super::tones::caret_line(&theme::active()));
    let editor = widget::container(hoverable(Some(ws), super::HoverTarget::Source, pad, code.into())).width(Length::Fixed(source_width));
    // The goal level with the construct, over its reserved column (moving only as far as it
    // must to stay in sight)
    let formal = formal_at_caret(t);
    let (goal_width, goal): (f32, Element<'a, Message>) = match beside.filter(|_| formal).and_then(|(w, _)| Some((w, inline_goal(Some(ws), table)?))) {
        Some((w, g)) => (w, widget::container(g).width(Length::Fixed(w)).class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(page_color(theme).into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        })).into()),
        // No goal: in prose (a module doc, a text block), what can be inserted there, as the
        // literate view has it; beside a proof, nothing while its state comes
        None => match beside {
            Some((w, _)) if !formal && super::insert::insert_context(t).is_some() => {
                (w, super::insert::insert_palette(ws, t, table, w))
            }
            _ => (beside.map_or(0.0, |(w, _)| w), widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into()),
        },
    };
    // (the goal's column and the space before it only when the goal is beside: an empty column
    // would put the page off centre)
    let row = widget::row::with_capacity(2).spacing(if beside.is_some() { spacing().space_m } else { 0 }).push(editor).push(widget::Space::new().width(Length::Fixed(goal_width)));
    let at = iced::Vector::new(source_width + spacing().space_m as f32, top);
    // Pop out: a small button floating at the top right of the proof the caret is in
    let proof = t.blocks.iter().find(|b| b.start <= t.cursor && t.cursor <= b.end && b.kind == Kind::Formal);
    let (pop, pop_at): (Element<'a, Message>, iced::Vector) = match proof {
        Some(b) => (
            pop_out_button(),
            // (centred on the proof's first line, as far inside the editor's right edge as in a
            // code cell of the literate view)
            iced::Vector::new(
                source_width - pop_out_inset() - pop_out_size(),
                row_top(t.lines.position(&t.decoded.text, t.decoded.to_decoded(b.start)).line) + (source_line_height() - pop_out_size()) / 2.0,
            ),
        ),
        None => (widget::Space::new().width(Length::Fixed(0.0)).height(Length::Fixed(0.0)).into(), iced::Vector::ZERO),
    };
    let page = widget::container(super::gap::Beside::new(super::gap::Beside::new(row, goal, at), pop, pop_at).fixed()).center_x(Length::Fill);
    col = col.push(thin(page).id(source_scroll_id()).height(Length::Fill));
    if let Some(m) = vim_mode_now() {
        col = col.push(widget::text::caption(format!("-- {} --", m.label())));
    }
    if let Some(bar) = completion_bar(t) {
        col = col.push(bar);
    }
    // With no room beside it, the goal at the caret shows under the editor, apart from it by a
    // rule, its scroll bar clear of its contents
    if formal && let Some(g) = inline_goal(inline, table) {
        col = col.push(widget::divider::horizontal::default());
        col = col.push(widget::container(thin(widget::container(g).padding(Padding { right: z(12.0), ..Padding::ZERO }))).max_height(z(240.0)).padding([z(8.0), z(12.0)]));
    }
    col.into()
}

fn source_editor(t: &Theory, height: Length) -> Element<'_, Message> {
    let sp = spacing();
    let vim_mode = vim_mode_now();
    let scope = t.page_scope;
    let completing = !t.completions.is_empty();
    let editor = iced::widget::text_editor(&t.source)
        .id(source_editor_id())
        .font(code_font())
        .size(code_size())
        .line_height(iced::widget::text::LineHeight::Relative(CODE_LINE))
        .padding(sp.space_s)
        .height(height)
        .on_action(Message::SourceAction)
        .key_binding(move |press| {
            use iced::keyboard::{Key, key::Named};
            if !matches!(press.status, text_editor::Status::Focused { .. }) {
                return None;
            }
            if !press.modifiers.control() && !press.modifiers.alt() && !press.modifiers.logo()
                && (vim_mode.is_none() || vim_mode == Some(super::vim::Mode::Insert))
            {
                match press.key.as_ref() {
                    Key::Named(Named::PageUp) => return Some(text_editor::Binding::Custom(Message::SourcePage(scope, false, press.modifiers.shift()))),
                    Key::Named(Named::PageDown) => return Some(text_editor::Binding::Custom(Message::SourcePage(scope, true, press.modifiers.shift()))),
                    _ => {}
                }
            }
            key_binding(press, completing, vim_mode, super::EditorTarget::Source)
        })
        .style(|theme: &cosmic::Theme, _status| {
            let c = theme.cosmic();
            iced::widget::text_editor::Style {
                background: Color::TRANSPARENT.into(),
                border: Border::default(),
                placeholder: Color { a: 0.5, ..Color::from(c.on_bg_color()) },
                value: c.on_bg_color().into(),
                selection: Color { a: 0.32, ..Color::from(c.accent.base) },
            }
        })
        .highlight_with::<super::editor::Highlighter>(t.highlight.themed(), highlight_format);
    code_selection(editor, on_page()).into()
}

// ----- the side panel

fn side_panel<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    // (the prover's panels as the session's capabilities and profile offer them)
    let mut tabs: Vec<(Panel, String)> = Vec::with_capacity(6);
    if let Some(label) = ws.caps.counterexamples_label() {
        tabs.push((Panel::Counterexamples, label));
    }
    if let Some(search) = &ws.caps.search {
        tabs.push((Panel::Search, search.short().to_string()));
    }
    if ws.profile.has_symbol_table() {
        tabs.push((Panel::Symbols, "Symbols".into()));
    }
    tabs.extend([(Panel::Agents, "Agents".into()), (Panel::History, "History".into()), (Panel::Log, "Activity".into())]);
    let bar = panel_tabs(ws.panel, tabs, app.right_width - 2.0 * sp.space_s as f32);
    let table = app.table(&ws.machine);
    let content: Element<'a, Message> = match ws.panel {
        Panel::Output => output_panel(ws, &*table),
        Panel::Tool => tool_panel(ws, &*table),
        Panel::Search => find_panel(ws, &*table),
        Panel::Symbols => symbols_panel(ws, &*table),
        Panel::Agents => agents_panel(app, ws),
        Panel::History => history_panel(ws),
        Panel::Log => activity_panel(ws),
        Panel::Counterexamples => counterexamples_panel(ws, &*table),
    };
    widget::container(widget::column::with_capacity(2).spacing(sp.space_xs).push(bar).push(content))
        .padding(sp.space_s)
        .width(Length::Fixed(app.right_width))
        .height(Length::Fill)
        .class(chrome_panel())
        .into()
}

/// The side panel's tabs: one row of equal segments in a fixed-height bar, never wrapping. When
/// the panel is too narrow for all of them, the last segment is a "More" button that steps
/// through the ones left out (named in its tooltip), and the open panel always has a segment
fn panel_tabs<'a>(open: Panel, tabs: Vec<(Panel, String)>, width: f32) -> Element<'a, Message> {
    let size = 12.5;
    // (a segment's width for its label: the label as set, and room either side)
    let wanted = |label: &str| label.chars().count() as f32 * size * 0.56 + 18.0;
    let fits = |tabs: &[(Panel, String)], more: bool| tabs.iter().map(|(_, l)| wanted(l) + 2.0).sum::<f32>() + if more { wanted("More") } else { 0.0 } <= width - 4.0;
    let (shown, hidden): (Vec<(Panel, String)>, Vec<(Panel, String)>) = if fits(&tabs, false) {
        (tabs, Vec::new())
    } else {
        let mut shown = tabs.clone();
        let mut hidden = Vec::new();
        while shown.len() > 1 && !fits(&shown, true) {
            hidden.insert(0, shown.pop().expect("a segment"));
        }
        // (the open panel always has a segment: it takes the last one's place)
        if let Some(k) = hidden.iter().position(|(p, _)| *p == open) {
            let open_tab = hidden.remove(k);
            if let Some(last) = shown.pop() {
                hidden.insert(0, last);
            }
            shown.push(open_tab);
        }
        (shown, hidden)
    };
    let segment = |label: String, active: bool, msg: Message| -> Element<'a, Message> {
        let portion = label.chars().count() as u16 + 3;
        widget::button::custom(
            widget::text(label)
                .size(size)
                .wrapping(iced::widget::text::Wrapping::None)
                .ellipsize(iced::core::text::Ellipsize::End(iced::core::text::EllipsizeHeightLimit::Lines(1)))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .width(Length::FillPortion(portion))
        .height(Length::Fill)
        .padding([0, 4])
        // (the open panel's tab in the muted accent, household rule 6)
        .class(if active { muted_button(theme::Button::Icon) } else { theme::Button::Icon })
        .on_press(msg)
        .into()
    };
    let mut row = widget::row::with_capacity(shown.len() + 1).spacing(2).height(Length::Fill).align_y(Alignment::Center);
    for (panel, label) in shown {
        row = row.push(segment(label, panel == open, Message::Panel(panel)));
    }
    if let Some((first, _)) = hidden.first() {
        let names: Vec<&str> = hidden.iter().map(|(_, l)| l.as_str()).collect();
        row = row.push(widget::tooltip(
            segment("More".into(), false, Message::Panel(*first)),
            widget::text(format!("Also: {}", names.join(", "))),
            widget::tooltip::Position::Bottom,
        ));
    }
    widget::container(row)
        .height(Length::Fixed(34.0))
        .padding(2)
        .width(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: if theme.cosmic().is_dark { 0.05 } else { 0.06 }, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
            border: Border { radius: 17.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

fn message_text<'a>(m: &Value, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let kind = m["kind"].as_str().unwrap_or("").to_string();
    tinted(kind, symbol_text(m["text"].as_str().unwrap_or(""), table, z(13.0)))
}

/// Errors and warnings on a tinted background
fn tinted<'a>(kind: String, inner: Element<'a, Message>) -> Element<'a, Message> {
    match kind.as_str() {
        "error" | "warning" | "legacy" => widget::container(inner)
            .padding([2, 6])
            .width(Length::Fill)
            .class(theme::Container::custom(move |theme| iced::widget::container::Style { background: Some(message_wash(theme, kind == "error").into()), ..Default::default() }))
            .into(),
        _ => inner,
    }
}

/// The tint behind an error (or a warning)
fn message_wash(theme: &cosmic::Theme, error: bool) -> Color {
    let c = theme.cosmic();
    let color: Color = if error { c.destructive_color().into() } else { c.warning_color().into() };
    Color { a: 0.15, ..color }
}

/// What code in the side panel has beneath it, for its selection's tint: the panel's card
fn on_panel() -> Beneath {
    Beneath::new(|theme| match iced::widget::container::Catalog::style(theme, &theme::Container::Card).background {
        Some(iced::Background::Color(c)) => c,
        _ => page_color(theme),
    })
}

/// A message as code: highlighted with the prover's markup, selectable and copyable
fn message_view<'a>(ws: &'a Workspace, m: &Value, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    let key = super::message_key(m);
    let kind = m["kind"].as_str().unwrap_or("").to_string();
    // (an error or a warning on its own tint, as `tinted` lays it)
    let on = match kind.as_str() {
        "error" | "warning" | "legacy" => {
            let error = kind == "error";
            on.under(move |theme| message_wash(theme, error))
        }
        _ => on.clone(),
    };
    match ws.views.get(&key) {
        Some(v) => tinted(
            kind,
            hoverable(Some(ws), super::HoverTarget::Message(key), Padding::from(2.0), {
                let editor = iced::widget::text_editor(&v.content)
                    .font(code_font())
                    .size(z(13.0))
                    .padding(2)
                    .on_action(move |a| Message::ViewAction(key, a))
                    // Read-only: copying and selecting only, so that every other key (j, k, the
                    // arrows, d, Enter) goes on to the page's block navigation even when a click
                    // has left this view focused
                    .key_binding(read_only_binding)
                    .style(|theme: &cosmic::Theme, _status| {
                        let c = theme.cosmic();
                        iced::widget::text_editor::Style {
                            background: Color::TRANSPARENT.into(),
                            border: Border::default(),
                            placeholder: Color::TRANSPARENT,
                            value: c.on_bg_color().into(),
                            selection: Color { a: 0.32, ..Color::from(c.accent.base) },
                        }
                    })
                    .highlight_with::<super::editor::Highlighter>(v.highlight.themed(), highlight_format);
                code_selection(editor, on).into()
            }),
        ),
        None => message_text(m, table),
    }
}

fn output_panel<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    thin(state_view(ws, table, &on_panel())).height(Length::Fill).into()
}

/// Why there is no proof state at the caret yet, when the prover has not reached its command:
/// the imports it is still checking (a session that has just started checks every theory the
/// document imports first, which can take minutes), or else that it has yet to get there. A
/// tool typed there (sledgehammer, …) starts when it does. None when the command is checked
pub fn waiting_note(ws: &Workspace) -> Option<String> {
    let t = ws.theory.as_ref()?;
    let at = t.cursor.saturating_sub(1);
    let local = |o: usize| super::App::local_offset_pub(t, o);
    let command = t.prover.commands.iter().find(|c| local(c.start) <= at && at < local(c.end).max(local(c.start) + 1))?;
    if !matches!(command.status, super::theory::Status::Unprocessed) {
        return None;
    }
    let tool = ws.caps.tools.iter().any(|tool| tool.typed == command.name || tool.name == command.name);
    let starts = if tool { format!("{} starts when the prover reaches this line. ", command.name) } else { String::new() };
    let waiting = super::shell::pending_imports(ws, t);
    if waiting.is_empty() {
        return Some(format!("{starts}Waiting for the prover to reach this line…"));
    }
    let blocked: Vec<&str> = waiting.iter().filter(|w| w.2).map(|w| w.0.as_str()).collect();
    if !blocked.is_empty() {
        return Some(format!("{starts}An import cannot load: {}.", blocked.join(", ")));
    }
    let names: Vec<String> = waiting.iter().take(3).map(|(n, pct, _)| format!("{n} {pct}%")).collect();
    let more = if waiting.len() > 3 { format!(" and {} more", waiting.len() - 3) } else { String::new() };
    Some(format!(
        "{starts}Waiting for the prover to check {} imported theor{} first: {}{more}.",
        waiting.len(),
        if waiting.len() == 1 { "y" } else { "ies" },
        names.join(", ")
    ))
}

/// The goal panel: the insert bar (symbols, steps or markup for the caret, above the first
/// subgoal, where they are wanted while the goal is shown), then the proof state
fn state_view<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    goal_panel(super::insert::insert_bar(ws, table), state_body(ws, table, on))
}

/// The goal panel: outlined 2 px grey ([`card_border_color`]: #bcbcbc light, #5c5c5c dark), the
/// insert bar (the tools and symbol keys) on its lightish band at the top, edge to edge, then
/// the body
fn goal_panel<'a>(band: Option<Element<'a, Message>>, body: Element<'a, Message>) -> Element<'a, Message> {
    let sp = spacing();
    let body = widget::container(body).padding([sp.space_xs, sp.space_s]).width(Length::Fill);
    let content = widget::column::with_capacity(2).push_maybe(band).push(body);
    let panel = widget::container(content).padding(2).width(Length::Fill).clip(true).class(theme::Container::custom(|theme| iced::widget::container::Style {
        border: Border { radius: 0.0.into(), width: 2.0, color: card_border_color(theme) },
        ..Default::default()
    }));
    #[cfg(test)]
    let panel = panel.id(test_panel_id("panel"));
    panel.into()
}

/// The ids the goal panel's parts have under test
#[cfg(test)]
pub fn test_panel_id(part: &str) -> widget::Id {
    widget::Id::new(format!("deductum-test-panel-{part}"))
}

/// The proof state: as parts (the chained facts, then each subgoal as fix, assume and show,
/// with what the command changed), or as Isabelle prints it; then any other output
fn state_body<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    let sp = spacing();
    // While Isabelle has yet to check what changed at the caret, output about another command
    // (the one that was there before the edit) is not shown as if it were this one's
    if let (Some(t), Some(out)) = (&ws.theory, &ws.output)
        && t.unchecked_from().is_some_and(|from| from <= t.cursor)
        && let (Some(s), Some(e)) = (out["command"]["start"].as_u64(), out["command"]["end"].as_u64())
    {
        let (s, e) = (super::App::local_offset_pub(t, s as usize), super::App::local_offset_pub(t, e as usize));
        let at = t.cursor.saturating_sub(1);
        if at < s || at >= e.max(s + 1) {
            return widget::text::caption(waiting_note(ws).unwrap_or_else(|| "Checking…".into())).into();
        }
    }
    let g = super::goals::Goals::parse(&ws.goals.messages);
    let have_parts = g.count.is_some() || g.nogoal || !g.facts.is_empty();
    // Goals or Text: two even segments of one small control, as the side panel's tabs are
    let toggle = |label: &'static str, on: bool, msg: Message| {
        widget::button::custom(widget::text(label).size(13.0).height(Length::Fill).align_y(Alignment::Center))
            .height(Length::Fill)
            .padding([0, 12])
            // (the chosen side in the muted accent, household rule 6)
            .class(if on { muted_button(theme::Button::Icon) } else { theme::Button::Icon })
            .on_press(msg)
    };
    let switch = widget::container(widget::row::with_capacity(2).spacing(2).height(Length::Fill).push(toggle("Goals", !ws.state_text, Message::StateText(false))).push(toggle("Text", ws.state_text, Message::StateText(true))))
        .height(Length::Fixed(30.0))
        .padding(2)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: if theme.cosmic().is_dark { 0.06 } else { 0.07 }, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
            border: Border { radius: 15.0.into(), ..Default::default() },
            ..Default::default()
        }));
    // What the command did: subgoals solved, and new ones
    let prev = super::goals::Goals::parse(&ws.goals_prev.messages);
    let before: Vec<String> = prev.subgoals.iter().map(|s| s.key()).collect();
    let now: Vec<String> = g.subgoals.iter().map(|s| s.key()).collect();
    // (what the step changed only when the settings ask for it)
    let compared = prev.count.is_some() && super::settings::show_changes();
    let solved = before.iter().filter(|k| !now.contains(k)).count();
    let added = now.iter().filter(|k| !before.contains(k)).count();
    let n = g.count.unwrap_or(0);
    let headline = if g.nogoal {
        "No goal: stating facts (forward mode)".to_string()
    } else if n == 0 {
        format!("No {}s left: the proof can be finished", ws.profile.goal_noun())
    } else {
        format!("{n} {}{}", ws.profile.goal_noun(), if n == 1 { "" } else { "s" })
    };
    // The heading line: "N goals" in bold, and at its right Goals | Text (sized to its parts) and
    // Types, which stays a checkbox
    let mut controls = widget::row::with_capacity(8).spacing(sp.space_s).align_y(Alignment::Center);
    if have_parts {
        controls = controls.push(widget::text::heading(headline).font(cosmic::font::bold()));
        if matches!(ws.goals.status.as_str(), "waiting" | "running") {
            controls = controls.push(widget::text::caption("updating…"));
        }
        if compared && (solved > 0 || added > 0) {
            let mut parts = Vec::new();
            if solved > 0 {
                parts.push(format!("{solved} solved"));
            }
            if added > 0 {
                parts.push(format!("{added} new"));
            }
            controls = controls.push(widget::text::caption(format!("by this step: {}", parts.join(", "))));
        }
    }
    let controls = controls
        .push(widget::Space::new().width(Length::Fill))
        .push(switch)
        .push_maybe(ws.profile.goal_types_option().then(|| widget::checkbox(ws.show_types).label("Types").on_toggle(Message::ShowTypes)));
    #[cfg(test)]
    let controls = widget::container(controls).id(test_panel_id("heading"));
    // On a proof tool typed in the text (sledgehammer, nitpick, …): what it is doing and what it
    // found, as they come, above the goal it works on, with its settings
    let tool = ws.output.as_ref().and_then(|o| o["command"]["name"].as_str()).and_then(|n| ws.caps.typed_tool(n));
    let tool_section: Option<Element<'a, Message>> = match (tool, super::flow::report(ws)) {
        (Some(tool), Some(report)) => Some(tool_section(ws, tool, report, table, on)),
        _ => None,
    };
    // Only a message (no proof here, or the session stopped): just the message
    let only_message = !have_parts
        && tool_section.is_none()
        && !fill_here(ws)
        && ws.output.as_ref().is_none_or(|o| o.get("error").is_some() || o["messages"].as_array().is_none_or(|m| m.is_empty()));
    if only_message {
        // (while Isabelle has yet to check what changed, there is a state coming, not none)
        let checking = ws.theory.as_ref().is_some_and(|t| t.unchecked_from().is_some_and(|from| from <= t.cursor));
        let text = match ws.output.as_ref().and_then(|o| o["error"].as_str()) {
            Some(e) => e.to_string(),
            None => match waiting_note(ws) {
                Some(note) => note,
                None if checking => "Checking…".to_string(),
                None => "No proof state here.".to_string(),
            },
        };
        return widget::text::caption(text).into();
    }
    let mut col = widget::column::with_capacity(16).spacing(sp.space_xs).push(controls);
    let in_tool = tool_section.is_some();
    if let Some(section) = tool_section {
        col = col.push(section);
    }
    if ws.state_text && let Some(text) = &g.text {
        // Isabelle's own text of the state, printed with the Types setting
        if !in_tool {
            col = col.push(tool_output(ws, table, on));
        }
        col = col.push(message_view(ws, text, table, on));
        return col.into();
    }
    if ws.state_text || !have_parts {
        // (Text shows the command's output as it is; with no goal in parts, the tool's section
        // has said it already)
        if ws.state_text || !in_tool {
            col = col.push(state_text(ws, table, on));
        }
        if fill_here(ws) {
            col = col.push(fill_results(ws, table));
        }
        return col.into();
    }
    // The command's own output (a theorem it proved, a value, a warning) comes first: it is
    // what the step did
    if !in_tool {
        col = col.push(tool_output(ws, table, on));
    }
    if !g.facts.is_empty() {
        let mut facts = widget::column::with_capacity(g.facts.len()).spacing(2);
        for f in &g.facts {
            facts = facts.push(message_view(ws, f, table, on));
        }
        // (inset as the subgoal cards' contents are, so that all the labels are in one column)
        col = col.push(widget::container(labelled(ws.profile.facts_label(), facts.into())).padding(Padding { left: sp.space_xs as f32, right: sp.space_xs as f32, ..Padding::ZERO }));
    }
    let shown = if ws.all_goals { g.subgoals.len() } else { g.subgoals.len().min(4) };
    for (i, sg) in g.subgoals.iter().take(shown).enumerate() {
        let new = compared && !before.contains(&sg.key());
        if i > 0 {
            col = col.push(hairline(i));
        }
        col = col.push(subgoal_card(ws, sg, i == 0, new, table, on));
    }
    if g.subgoals.len() > shown {
        col = col.push(widget::button::text(format!("Show {} more", g.subgoals.len() - shown)).on_press(Message::AllGoals(true)));
    } else if ws.all_goals && g.subgoals.len() > 4 {
        col = col.push(widget::button::text("Show fewer").on_press(Message::AllGoals(false)));
    }
    col.into()
}

/// A typed tool's section above its goal: its name, what it is doing (how long it has been at
/// it, with Stop, which Esc also does), then what it came to (see [`super::flow`]): the proofs it
/// found, a counterexample as a table, or a plain sentence and what to do next. What else it said
/// follows, then its settings, folded
fn tool_section<'a>(ws: &'a Workspace, tool: &'a Tool, r: super::flow::Report, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    // (on its own wash)
    let on = &on.under(tool_wash);
    use super::flow::Outcome;
    let sp = spacing();
    let secs = ws.tool_started.map_or(0, |s| s.elapsed().as_secs());
    let found = r.offers.iter().filter(|o| matches!(o.act, super::flow::Act::Insert(_)) && o.ok == Some(true)).count();
    let fitting = r.offers.iter().filter(|o| matches!(o.act, super::flow::Act::Insert(_)) && o.ok.is_none()).count();
    let status = match &r.outcome {
        Outcome::Waiting(_) => "waiting".to_string(),
        // (Esc or Stop pressed: said at once, though the provers take a moment to give up)
        _ if r.running && ws.tools_stopping => "stopping…".to_string(),
        _ if r.running => format!("running · {secs} s"),
        Outcome::Proofs if found == 0 => format!("{fitting} {} that {}", if fitting == 1 { "fact" } else { "facts" }, if fitting == 1 { "fits" } else { "fit" }),
        Outcome::Proofs => format!("{found} {} found", if found == 1 { "proof" } else { "proofs" }),
        Outcome::Counterexample { .. } => "counterexample found".into(),
        Outcome::NoCounterexample(_) => "no counterexample".into(),
        Outcome::NothingFound(_) => "nothing found".into(),
        Outcome::Failed(_) => "failed".into(),
        Outcome::Stopped => "stopped".into(),
        Outcome::Running(_) | Outcome::Done(_) => "done".into(),
    };
    let mut head = widget::row::with_capacity(3)
        .spacing(sp.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::heading(r.tool.clone()))
        .push(widget::text::caption(status).class(muted()).width(Length::Fill));
    // (a tool that cannot be stopped on its own goes when its line is changed)
    if r.running && r.stoppable && !ws.tools_stopping {
        head = head.push(widget::tooltip(widget::button::text("Stop").on_press(Message::StopTools), widget::text::caption("Stop it (Esc)"), widget::tooltip::Position::Bottom));
    }
    let mut col = widget::column::with_capacity(8).spacing(sp.space_xs).push(head);
    if r.running {
        col = col.push(iced::widget::progress_bar(0.0..=1.0, ((ws.tool_started.map_or(0.0, |s| s.elapsed().as_secs_f32())) / tool.timeout(&ws.tool_settings)).min(1.0)).girth(4.0));
    }
    let said = |text: String| -> Element<'a, Message> { symbol_text_with_font(&text, table, z(14.0), cosmic::font::default()) };
    let hint = |text: &str| -> Element<'a, Message> {
        widget::container(symbol_text_with_font(text, table, z(12.0), cosmic::font::default())).class(theme::Container::custom(|theme| iced::widget::container::Style {
            text_color: Some(super::tones::secondary_text(theme, 0.65)),
            ..Default::default()
        })).into()
    };
    match &r.outcome {
        Outcome::Waiting(note) => col = col.push(hint(note)),
        Outcome::Running(note) => {
            let looking = if tool.kind == "counterexample" { "Looking for a counterexample…" } else { "Looking for a proof…" };
            col = col.push(hint(if note.is_empty() { looking } else { note }));
        }
        Outcome::Proofs => {
            col = col.push(offer_list(ws, &r.offers, true, table));
            if r.running {
                col = col.push(hint("Still looking: more may come."));
            }
        }
        Outcome::Counterexample { headline, sections } => {
            col = col.push(counterexample_table(headline, sections, table));
            let spurious = headline.contains("spurious");
            col = col.push(hint(if spurious {
                "The tool is not sure of this one: check the values against the statement."
            } else {
                "The statement is false for these values. Correct it, or add an assumption that rules them out."
            }));
        }
        Outcome::NoCounterexample(text) => {
            col = col.push(said(format!("{text}.")));
            col = col.push(hint("That does not prove it."));
        }
        Outcome::NothingFound(text) => col = col.push(said(text.clone())),
        Outcome::Failed(text) => col = col.push(tinted("error".into(), symbol_text(text, table, z(13.0)))),
        Outcome::Stopped => col = col.push(hint("Stopped. Change the line to run it again.")),
        Outcome::Done(text) => col = col.push(hint(text)),
    }
    if !matches!(r.outcome, Outcome::Proofs) && !r.offers.is_empty() {
        col = col.push(offer_list(ws, &r.offers, true, table));
    }
    for m in &r.notes {
        col = col.push(message_view(ws, m, table, on));
    }
    if tool.settings.iter().any(|s| !s.option.is_empty()) {
        col = col.push(tool_settings(ws, tool));
    }
    widget::container(col)
        .padding([sp.space_xxs, sp.space_xs])
        .width(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(tool_wash(theme).into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// The wash behind a proof tool's section
fn tool_wash(theme: &cosmic::Theme) -> Color {
    Color { a: 0.06, ..Color::from(theme.cosmic().accent_color()) }
}

/// The offers of a tool or an LLM, each a row: whether it was checked, what it is (in the code
/// font) with who found it and how long it took, and its button. The one Alt+Enter takes is
/// highlighted (when these are the offers at the caret, `active`), with the keys said under them
fn offer_list<'a>(ws: &'a Workspace, offers: &[super::flow::Offer], active: bool, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    use super::flow::Act;
    let sp = spacing();
    let picked = if active { Some(super::flow::picked(ws, offers)) } else { None };
    // (five at first, the rest a click away; a pick past them shows them all)
    let shown = if ws.all_offers || picked.is_some_and(|p| p >= 5) { offers.len() } else { offers.len().min(5) };
    let mut col = widget::column::with_capacity(offers.len() + 2).spacing(2);
    for (i, o) in offers.iter().enumerate().take(shown) {
        let on = picked == Some(i);
        // (who found it and how long it took on the same line, at its end, when that is short:
        // a list of one-line proofs stays a list of lines)
        let code = || widget::container(symbol_text(&o.code, table, z(13.0))).width(Length::Fill);
        let inline = o.detail.chars().count() <= 26;
        let what: Element<'a, Message> = if o.detail.is_empty() {
            code().into()
        } else if inline {
            widget::row::with_capacity(2)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(code())
                .push(widget::text::caption(o.detail.clone()).class(muted()).wrapping(iced::widget::text::Wrapping::None))
                .width(Length::Fill)
                .into()
        } else {
            widget::column::with_capacity(2).push(code()).push(widget::text::caption(o.detail.clone()).class(muted())).width(Length::Fill).into()
        };
        let mark: Element<'a, Message> = match (&o.act, o.ok) {
            (Act::Replace(_) | Act::Ask(_), _) | (Act::Insert(_), None) => widget::icon::from_name("go-next-symbolic").size(16).icon().class(theme::Svg::custom(|t| iced::widget::svg::Style { color: Some(super::tones::secondary_text(t, 0.7)) })).into(),
            (Act::Facts, _) => widget::icon::from_name("view-list-symbolic").size(16).icon().class(theme::Svg::custom(|t| iced::widget::svg::Style { color: Some(super::tones::secondary_text(t, 0.7)) })).into(),
            (_, ok) => verdict(ok),
        };
        let button = if on { widget::button::suggested(o.button()) } else { widget::button::standard(o.button()) };
        let row = widget::row::with_capacity(3).spacing(sp.space_xs).align_y(Alignment::Center).push(mark).push(what).push(button.on_press(o.message()));
        col = col.push(widget::container(row).padding([z(3.0), z(6.0)]).width(Length::Fill).class(theme::Container::custom(move |theme| iced::widget::container::Style {
            background: on.then(|| Color { a: 0.14, ..Color::from(theme.cosmic().accent_color()) }.into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        })));
    }
    if shown < offers.len() {
        col = col.push(widget::button::text(format!("{} more", offers.len() - shown)).on_press(Message::AllOffers(true)));
    }
    if let Some(i) = picked
        && let Some(o) = offers.get(i)
    {
        let keys = if offers.len() > 1 { format!("Alt+Enter {} the highlighted one · Alt+Up or Alt+Down picks another", o.verb()) } else { format!("Alt+Enter {} it", o.verb()) };
        col = col.push(widget::container(widget::text::caption(keys).class(muted())).padding(Padding { left: z(6.0), top: 2.0, ..Padding::ZERO }));
    }
    col.into()
}

/// A counterexample as its sections, each a table of names and values in the code font
fn counterexample_table<'a>(headline: &str, sections: &[super::flow::Section], table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(1 + 2 * sections.len()).spacing(sp.space_xxs).push(symbol_text_with_font(headline, table, z(14.0), cosmic::font::default()));
    let mono = |t: String| widget::text(t).font(code_font()).size(z(13.0));
    for s in sections {
        // (each section's names in one column, as wide as the longest of them, up to 180 px)
        let longest = s.rows.iter().map(|(n, _)| table.decode(n).text.chars().count()).max().unwrap_or(1);
        let name_width = z((longest as f32 * 7.9 + 4.0).clamp(24.0, 180.0));
        let mut rows = widget::column::with_capacity(s.rows.len()).spacing(2);
        for (name, value) in &s.rows {
            rows = rows.push(
                widget::row::with_capacity(3)
                    .spacing(sp.space_xs)
                    .push(widget::container(symbol_text(name, table, z(13.0))).width(Length::Fixed(name_width)).class(theme::Container::custom(|theme| iced::widget::container::Style {
                        text_color: Some(theme.cosmic().accent_color().into()),
                        ..Default::default()
                    })))
                    .push(mono("=".into()).class(muted()))
                    .push(widget::container(symbol_text(value, table, z(13.0))).width(Length::Fill)),
            );
        }
        col = col.push(widget::text::caption(s.title.clone()).class(muted())).push(
            widget::container(rows).padding([z(4.0), z(8.0)]).width(Length::Fill).class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: if theme.cosmic().is_dark { 0.05 } else { 0.06 }, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
                border: Border { radius: 4.0.into(), ..Default::default() },
                ..Default::default()
            })),
        );
    }
    col.into()
}

/// A typed tool's settings, folded away under the proof state: the tool itself is typed in the
/// text (where its proofs come with Insert), so only the settings that set prover options are
/// here, for they hold for it there. Built from the tool's settings: a setting without a label
/// is a line of its own, one with a label a row with its unit after it; Apply ends the last row
fn tool_settings<'a>(ws: &'a Workspace, tool: &'a Tool) -> Element<'a, Message> {
    let sp = spacing();
    let head = widget::button::text(format!("{} {} settings", if ws.settings_open { "▾" } else { "▸" }, tool.label)).on_press(Message::ToolSettings(!ws.settings_open));
    let mut col = widget::column::with_capacity(4).spacing(sp.space_xxs).push(head);
    if ws.settings_open {
        let shown: Vec<&Setting> = tool.settings.iter().filter(|s| !s.option.is_empty()).collect();
        let last_row = shown.iter().rposition(|s| !s.label.is_empty());
        let apply = || Message::ToolApply(tool.name.clone());
        for (i, s) in shown.iter().enumerate() {
            let (name, key) = (tool.name.clone(), s.key.clone());
            let input = widget::text_input(s.placeholder.as_str(), tool.value(&ws.tool_settings, &s.key))
                .on_input(move |v| Message::ToolSetting(name.clone(), key.clone(), v))
                .on_submit(move |_| apply());
            if s.label.is_empty() {
                col = col.push(input);
                continue;
            }
            let mut row = widget::row::with_capacity(4)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(widget::text::body(s.label.as_str()))
                .push(if s.kind == "number" { input.width(Length::Fixed(64.0)) } else { input.width(Length::Fill) })
                .push(widget::text::body(s.unit.as_str()).width(Length::Fill));
            if Some(i) == last_row {
                row = row.push(widget::button::standard("Apply").on_press(apply()));
            }
            col = col.push(row);
        }
        if last_row.is_none() {
            col = col.push(widget::button::standard("Apply").on_press(apply()));
        }
        if !tool.settings_note.is_empty() {
            col = col.push(widget::text::caption(tool.settings_note.as_str()));
        }
    }
    col.into()
}

/// What the command at the caret printed besides the state (a theorem, sledgehammer's or
/// nitpick's findings typed in the text, a warning), with Insert for the proofs it offers; then
/// an LLM's proofs for a hole, when one was asked for (typing `claude` or `codex`)
fn tool_output<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(8).spacing(sp.space_xxs);
    if let Some(out) = ws.output.as_ref().filter(|_| !super::flow::stale_tool_output(ws)) {
        // (a tool's Interrupt is its being stopped: said in its heading, not as an error)
        for m in out["messages"].as_array().into_iter().flatten().filter(|m| m["kind"] != "state" && !(m["kind"] == "error" && m["text"].as_str().is_some_and(|t| ws.profile.interrupted(t)))) {
            col = col.push(message_view(ws, m, table, on));
            for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
                col = col.push(
                    widget::row::with_capacity(2)
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .push(widget::container(symbol_text(sb, table, z(13.0))).width(Length::Fill))
                        .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
                );
            }
        }
    }
    // A `claude`, `codex` or `@name` line not asked yet: when it will be
    if !fill_here(ws)
        && let Some(t) = &ws.theory
        && let Some(line) = super::keyword_line(t)
    {
        let who = line.split_whitespace().find(|w| w.starts_with("claude") || w.starts_with("codex") || w.starts_with('@')).unwrap_or("claude").to_string();
        col = col.push(widget::text::caption(format!("{who}: asks when you pause typing (Enter asks now)")));
    }
    if fill_here(ws) && !ws.fill.status.is_empty() {
        col = col.push(fill_section(ws, table));
    }
    col.into()
}

/// A `claude`, `codex` or `@name` line's job, as a typed tool's section shows: who was asked,
/// what it is doing and for how long (Stop, which Esc also does), then its proofs, the checked
/// ones first, with Insert; where such queries go comes last
fn fill_section<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let f = &ws.fill;
    let busy = matches!(f.status.as_str(), "starting" | "asking" | "checking");
    let secs = f.started.map_or(0, |s| s.elapsed().as_secs());
    let checked = f.candidates.iter().filter(|c| c["ok"] == serde_json::json!(true)).count();
    let status = match f.status.as_str() {
        "starting" | "asking" => format!("asking · {secs} s"),
        "checking" => format!("checking its proofs · {secs} s"),
        "done" if f.candidates.is_empty() => "no proofs".to_string(),
        "done" => format!("{} {}, {checked} checked", f.candidates.len(), if f.candidates.len() == 1 { "proof" } else { "proofs" }),
        other => other.to_string(),
    };
    let who = match &f.asked {
        Some(name) => format!("@{name}"),
        None => format!("{}{}", if f.agent == 1 { "codex" } else { "claude" }, f.model.as_ref().map_or(String::new(), |m| format!(" · {m}"))),
    };
    let stop: Element<'a, Message> = if busy {
        widget::tooltip(widget::button::text("Stop").on_press(Message::FillDismiss), widget::text::caption("Stop it (Esc)"), widget::tooltip::Position::Bottom).into()
    } else {
        widget::button::text("Dismiss").on_press(Message::FillDismiss).into()
    };
    let head = widget::row::with_capacity(3)
        .spacing(sp.space_s)
        .align_y(Alignment::Center)
        .push(widget::text::heading(who))
        .push(widget::text::caption(status).class(muted()).width(Length::Fill))
        .push(stop);
    let mut col = widget::column::with_capacity(6).spacing(sp.space_xs).push(head);
    if busy {
        col = col.push(iced::widget::progress_bar(0.0..=1.0, (secs as f32 / 120.0).min(1.0)).girth(4.0));
    }
    if let Some(m) = &f.message {
        col = col.push(widget::text::body(m.clone()));
    }
    if let Some(err) = &f.error {
        col = col.push(widget::text::body(err.clone()));
    }
    let offers = super::flow::fill_offers(ws);
    if offers.is_empty() && busy {
        col = col.push(widget::text::caption(if f.status == "checking" { "Checking the proposed proofs…" } else { "Asking for proofs…" }).class(muted()));
    }
    if !offers.is_empty() {
        let active = super::flow::offers_here(ws).first().map(|o| &o.code) == offers.first().map(|o| &o.code);
        col = col.push(offer_list(ws, &offers, active, table));
    }
    col = col.push(query_route_row(ws));
    widget::container(col)
        .padding([sp.space_xxs, sp.space_xs])
        .width(Length::Fill)
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(Color { a: 0.06, ..Color::from(theme.cosmic().accent_color()) }.into()),
            border: Border { radius: 6.0.into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// Where `claude` and `codex` queries go: the session's helper conversation (it remembers), a
/// fresh request each time, or a listening agent; the helper's conversation, to open or start again
pub(super) fn query_route_row<'a>(ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let route = ws.activity.as_ref().and_then(|a| a["route"].as_str()).unwrap_or("helper").to_string();
    let listeners: Vec<(String, String)> = ws
        .activity
        .as_ref()
        .and_then(|a| a["listeners"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["key"].as_str()?.to_string(), l["label"].as_str()?.to_string())))
        .collect();
    // (as a row that wraps: in a narrow panel the helper's buttons go to a line of their own)
    let mut items: Vec<Element<'a, Message>> = vec![widget::container(widget::text::caption("Queries go to")).padding([6.0, 0.0]).into()];
    let choice = |label: String, value: String| -> Element<'a, Message> {
        let on = route == value;
        // (the chosen one in the muted accent, household rule 6)
        widget::button::text(label).class(if on { muted_button(theme::Button::Text) } else { theme::Button::Text }).on_press(Message::QueryRoute(value)).into()
    };
    items.push(choice("the helper".into(), "helper".into()));
    items.push(choice("a fresh request".into(), "fresh".into()));
    for (key, label) in listeners {
        items.push(choice(format!("@{label}"), format!("agent:{key}")));
    }
    if route == "helper" && ws.fill.asked.is_none() {
        if let Some(conv) = &ws.fill.conversation {
            let resume = conv["resume"].as_str().unwrap_or("").to_string();
            let machine = conv["machine"].as_str().unwrap_or("").to_string();
            items.push(
                widget::button::text("Copy resume command")
                    .tooltip(format!("Talk to the helper: run this on {machine}"))
                    .on_press(Message::CopyText(resume))
                    .into(),
            );
        }
        items.push(widget::button::text("New conversation").on_press(Message::HelperReset).into());
    }
    widget::container(widget::flex_row(items).column_spacing(sp.space_xxs).row_spacing(2)).padding([sp.space_xxs, 0]).into()
}

/// A label in a narrow column, then what it labels
fn labelled<'a>(label: &'static str, body: Element<'a, Message>) -> Element<'a, Message> {
    // A one-character label (Lean's `⊢`) is part of the line as the infoview sets it: the code
    // font at the code's size, then a space, then the target
    if label.chars().count() <= 1 {
        return widget::row::with_capacity(2)
            .spacing(z(4.0))
            .push(widget::container(widget::text(label).font(code_font()).size(z(13.0)).class(theme::Text::Accent)).padding(Padding { top: 2.0, left: 2.0, ..Padding::ZERO }))
            .push(widget::container(body).width(Length::Fill))
            .into();
    }
    widget::row::with_capacity(2)
        .spacing(spacing().space_xs)
        .push(
            widget::container(widget::text(label).font(code_font()).size(z(12.0)).class(theme::Text::Accent))
                .padding(Padding { top: 3.0, ..Padding::ZERO })
                .width(Length::Fixed(z(52.0))),
        )
        .push(widget::container(body).width(Length::Fill))
        .into()
}

/// One subgoal as the backend's users read it, laid out by the profile (Isabelle: the variables it
/// fixes, what it assumes, what it shows)
fn subgoal_card<'a>(ws: &'a Workspace, sg: &super::goals::Subgoal, first: bool, new: bool, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    let sp = spacing();
    // (flat on the panel, no plate: the first, the one the next step works on, says so in bold,
    // and its label in the accent)
    let name = format!("{}{}", if ws.profile.goal_noun() == "goal" { "Goal " } else { "Subgoal " }, sg.n);
    let name = widget::text::caption(name);
    let mut head = widget::row::with_capacity(3).spacing(sp.space_xs).push(if first { name.font(cosmic::font::bold()) } else { name });
    if first {
        head = head.push(widget::text::caption("· next").class(theme::Text::Accent).font(cosmic::font::bold()));
    }
    if new {
        head = head.push(widget::text::caption("· new").class(theme::Text::Accent));
    }
    let mut col = widget::column::with_capacity(4).spacing(2).push(head);
    let stack = |parts: &[Value]| -> Element<'a, Message> {
        let mut c = widget::column::with_capacity(parts.len()).spacing(2);
        for p in parts {
            c = c.push(message_view(ws, p, table, on));
        }
        c.into()
    };
    let shown = ws.profile.present_goal(sg);
    if let Some(heading) = shown.heading {
        col = col.push(symbol_text(&heading, table, z(13.0)));
    }
    for section in shown.sections {
        let body = match section.body {
            GoalBody::One(part) => message_view(ws, part, table, on),
            GoalBody::Many(parts) => stack(parts),
        };
        col = col.push(match section.label {
            Some(label) => labelled(label, body),
            None => body,
        });
    }
    let card = widget::container(col).padding([sp.space_xxs, sp.space_xs]).width(Length::Fill);
    #[cfg(test)]
    let card = card.id(test_panel_id(&format!("subgoal-{}", sg.n)));
    card.into()
}

/// The hairline between subgoals, in the card divider colour
fn hairline<'a>(#[allow(unused)] i: usize) -> Element<'a, Message> {
    let line = widget::container(widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .class(theme::Container::custom(|theme| iced::widget::container::Style { background: Some(card_border_color(theme).into()), ..Default::default() }));
    #[cfg(test)]
    let line = line.id(test_panel_id(&format!("hairline-{i}")));
    line.into()
}

/// The state as Isabelle prints it, with the command's other output
fn state_text<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols, on: &Beneath) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(8).spacing(6);
    match &ws.output {
        None => col = col.push(widget::text::caption("Move the caret into a proof to see its state.")),
        Some(out) if out.get("error").is_some() => col = col.push(widget::text::body(out["error"].as_str().unwrap_or("").to_string())),
        Some(_) if super::flow::stale_tool_output(ws) => col = col.push(widget::text::caption("Checking…")),
        Some(out) => {
            if let Some(c) = out.get("command").filter(|c| c.is_object()) {
                col = col.push(widget::text::caption(format!(
                    "After {} ({}){}",
                    c["name"].as_str().unwrap_or(""),
                    c["status"].as_str().unwrap_or(""),
                    if out["outdated"].as_bool() == Some(true) { " — prover catching up" } else { "" }
                )));
            }
            let msgs = out["messages"].as_array().cloned().unwrap_or_default();
            if msgs.is_empty() {
                col = col.push(widget::text::caption("No output."));
            }
            for m in msgs {
                col = col.push(message_view(ws, &m, table, on));
                for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
                    col = col.push(
                        widget::row::with_capacity(2)
                            .spacing(6)
                            .align_y(Alignment::Center)
                            .push(widget::container(symbol_text(sb, table, 13.0)).width(Length::Fill))
                            .push(widget::button::suggested("Insert").on_press(Message::InsertOutput(sb.to_string()))),
                    );
                }
            }
        }
    }
    col.into()
}

/// The proofs a tool found, each with who found it and how fast ("e, 6 ms"), from the lines that
/// offer them ("e: Try this: by simp (6 ms)")
fn proofs(q: &super::QueryState) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for m in &q.messages {
        let text = m["text"].as_str().unwrap_or("");
        let prover = text.split_once(": ").map(|(p, _)| p).filter(|p| !p.contains(' ') && !p.is_empty());
        let time = text.rsplit_once('(').and_then(|(_, t)| t.strip_suffix(')')).filter(|t| t.ends_with('s'));
        let by = match (prover, time) {
            (Some(p), Some(t)) => Some(format!("{p}, {t}")),
            (Some(p), None) => Some(p.to_string()),
            _ => None,
        };
        for sb in m["sendbacks"].as_array().into_iter().flatten() {
            let Some(s) = sb.as_str() else { continue };
            match out.iter_mut().find(|(p, _)| p == s) {
                Some((_, who)) => who.extend(by.clone()),
                None => out.push((s.to_string(), by.clone().into_iter().collect())),
            }
        }
    }
    out
}

/// A ✓ or ✗ (or … while not known), coloured
fn verdict<'a>(ok: Option<bool>) -> Element<'a, Message> {
    let (name, tone): (&'static str, fn(&cosmic::Theme) -> Color) = match ok {
        Some(true) => ("object-select-symbolic", |t| t.cosmic().success_color().into()),
        Some(false) => ("window-close-symbolic", error_color),
        None => ("content-loading-symbolic", |t| Color { a: 0.6, ..Color::from(t.cosmic().on_bg_color()) }),
    };
    widget::icon::from_name(name).size(16).icon().class(theme::Svg::custom(move |theme| iced::widget::svg::Style { color: Some(tone(theme)) })).into()
}

/// The proof tool that runs from a panel (Isabelle's Sledgehammer), built from its settings: the
/// unlabelled ones each a line of their own, the labelled ones in rows with their labels in a
/// column, the switches in a row, then Cancel and Run; then its findings, each proof with the
/// provers that found it and Insert. Under it, LLM help for the hole
fn tool_panel<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.tool_query;
    let mut col = widget::column::with_capacity(12).spacing(sp.space_xs);
    if let Some(tool) = ws.caps.panel_tool() {
        col = col.push(widget::text::heading(tool.label.as_str()));
        if !tool.panel_note.is_empty() {
            col = col.push(widget::text::caption(tool.panel_note.as_str()));
        }
        let setting = |s: &'a Setting| {
            let (name, key) = (tool.name.clone(), s.key.clone());
            widget::text_input(s.placeholder.as_str(), tool.value(&ws.tool_settings, &s.key)).on_input(move |v| Message::ToolSetting(name.clone(), key.clone(), v))
        };
        let label_width = Length::Fixed(72.0);
        for s in tool.settings.iter().filter(|s| s.kind != "bool") {
            let input = setting(s);
            if s.label.is_empty() {
                col = col.push(input.width(Length::Fill));
                continue;
            }
            col = col.push(
                widget::row::with_capacity(3)
                    .spacing(sp.space_xs)
                    .align_y(Alignment::Center)
                    .push(widget::text::body(s.label.as_str()).width(label_width))
                    .push(if s.kind == "number" { input.width(Length::Fixed(72.0)) } else { input.width(Length::Fill) })
                    .push(widget::text::body(s.unit.as_str())),
            );
        }
        let switches: Vec<&Setting> = tool.settings.iter().filter(|s| s.kind == "bool").collect();
        if !switches.is_empty() {
            let mut row = widget::row::with_capacity(switches.len()).spacing(sp.space_m);
            for s in switches {
                let (name, key) = (tool.name.clone(), s.key.clone());
                row = row.push(widget::checkbox(tool.value(&ws.tool_settings, &s.key) == "true").label(s.label.as_str()).on_toggle(move |on| Message::ToolSetting(name.clone(), key.clone(), on.to_string())));
            }
            col = col.push(row);
        }
        let busy = q.status == "running" || q.status == "waiting" || q.status == "starting";
        col = col.push(
            widget::row::with_capacity(3)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(widget::Space::new().width(Length::Fill))
                .push(widget::button::standard("Cancel").on_press_maybe(busy.then_some(Message::CancelQuery)))
                .push(widget::button::suggested("Run").on_press(Message::RunTool(tool.name.clone()))),
        );
        if !tool.typed_note.is_empty() {
            col = col.push(widget::text::caption(tool.typed_note.as_str()).class(muted()));
        }
        if !q.status.is_empty() {
            let status = match q.status.as_str() {
                "waiting" | "starting" => "Waiting for the goal to be checked…",
                "running" if !tool.running.is_empty() => tool.running.as_str(),
                "running" => "Running…",
                "finished" => "Finished",
                "canceled" => "Canceled",
                "failed" => "Failed",
                other => other,
            };
            col = col.push(widget::divider::horizontal::light());
            let mut head = widget::row::with_capacity(2).spacing(sp.space_xs).align_y(Alignment::Center);
            if busy {
                head = head.push(widget::progress_bar::circular::Circular::new().size(16.0));
            }
            col = col.push(head.push(widget::text::heading(status.to_string())));
        }
        if let Some(err) = &q.error {
            col = col.push(widget::text::body(err.clone()));
        }
        let found = proofs(q);
        if !found.is_empty() {
            col = col.push(widget::text::caption(if q.replace.is_some() {
                format!("Insert replaces the {}:", ws.profile.placeholder())
            } else {
                "Insert adds the proof after the command:".to_string()
            }));
            for (n, (p, by)) in found.into_iter().enumerate() {
                let mut text = widget::column::with_capacity(2).push(symbol_text(&p, table, 13.0));
                if !by.is_empty() {
                    text = text.push(widget::text::caption(format!("found by {}", by.join("; "))).class(muted()));
                }
                // (the first, the fastest found, is the one to take)
                let insert = if n == 0 { widget::button::suggested("Insert") } else { widget::button::standard("Insert") };
                col = col.push(
                    widget::row::with_capacity(3)
                        .spacing(sp.space_xs)
                        .align_y(Alignment::Center)
                        .push(verdict(Some(true)))
                        .push(text.width(Length::Fill))
                        .push(insert.on_press(Message::InsertProof(p.clone()))),
                );
            }
        }
        // (the provers' lines saying they found the proofs shown above)
        for m in &q.messages {
            let text = m["text"].as_str().unwrap_or("");
            if text.contains("found a proof") || m["sendbacks"].as_array().is_some_and(|s| !s.is_empty()) {
                continue;
            }
            col = col.push(message_view(ws, m, table, &on_panel()));
        }
        col = col.push(widget::container(widget::divider::horizontal::default()).padding([sp.space_xxs, 0]));
    }
    // An LLM's proofs for the hole, each checked by the prover before it is offered
    let placeholder = ws.profile.placeholder();
    col = col.push(widget::text::heading("Fill the hole with an LLM"));
    col = col.push(widget::text::caption(format!(
        "Claude or Codex on the session's machine proposes proofs for the {placeholder} at or after the caret; each is checked by the prover first. Type claude or codex (with a hint after it, if you like) on a line of its own in a proof and press Enter."
    )));
    col = col.push(fill_results(ws, table));
    thin(col).height(Length::Fill).into()
}

/// An LLM fill's progress and its proofs, each with an Insert button
fn fill_results<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let f = &ws.fill;
    let mut col = widget::column::with_capacity(f.candidates.len() + 2).spacing(sp.space_xs);
    if !f.status.is_empty() {
        let status = match f.status.as_str() {
            "starting" | "asking" => "Asking for proofs…",
            "checking" => "Checking the proposed proofs…",
            "done" => "Done",
            "failed" => "Failed",
            other => other,
        };
        col = col.push(widget::text::heading(status.to_string()));
    }
    if let Some(err) = &f.error {
        col = col.push(widget::text::body(err.clone()));
    }
    // Its proofs, the checked ones first, then the facts for Sledgehammer: highlighted for
    // Alt+Enter when nothing else at the caret offers anything
    let offers = super::flow::fill_offers(ws);
    if !offers.is_empty() {
        let active = super::flow::offers_here(ws).first().map(|o| &o.code) == offers.first().map(|o| &o.code);
        col = col.push(offer_list(ws, &offers, active, table));
    }
    col.into()
}

/// Searching for theorems, as the session's capabilities describe it
fn find_panel<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let q = &ws.find;
    let Some(search) = &ws.caps.search else { return widget::text::caption("This prover has no search.").into() };
    let mut col = widget::column::with_capacity(6).spacing(sp.space_xs);
    col = col.push(
        widget::row::with_capacity(2)
            .spacing(sp.space_xs)
            .push(widget::text_input(search.placeholder.as_str(), &ws.find_input).on_input(Message::FindInput).on_submit(|_| Message::Find).width(Length::Fill))
            .push(widget::button::suggested(search.short()).on_press(Message::Find)),
    );
    if !search.note.is_empty() {
        col = col.push(widget::text::caption(search.note.as_str()));
    }
    if q.status == "running" || q.status == "waiting" || q.status == "starting" {
        col = col.push(widget::text::caption("Searching…"));
    }
    // (ways to search the panel offers besides its query: tactics that search at the caret, typed
    // into the proof)
    let tactics: Vec<&String> = search.sources.iter().filter(|s| s.ends_with('?')).collect();
    if !tactics.is_empty() {
        let mut row = widget::row::with_capacity(tactics.len() + 1).spacing(sp.space_xs).align_y(Alignment::Center);
        row = row.push(widget::text::caption("At the caret:"));
        for t in tactics {
            row = row.push(widget::button::standard(t.as_str()).on_press(Message::Wrap(t.clone(), String::new())));
        }
        col = col.push(row);
    }
    if let Some(err) = &q.error {
        col = col.push(tinted("error".into(), widget::text(err.clone()).size(13.0).into()));
    }
    for m in &q.messages {
        col = col.push(if m["name"].is_string() { search_hit(m) } else { message_text(m, table) });
    }
    thin(col).height(Length::Fill).into()
}

/// A search result: the name to insert, its type, and the module it is in
fn search_hit<'a>(m: &Value) -> Element<'a, Message> {
    let sp = spacing();
    let name = m["name"].as_str().unwrap_or("").to_string();
    let mono = |t: String, size: f32| widget::text(t).font(code_font()).size(z(size));
    let bold = code_font_bold();
    let mut body = widget::column::with_capacity(3).spacing(1).width(Length::Fill);
    body = body.push(widget::text(name.clone()).font(bold).size(z(13.0)));
    body = body.push(mono(m["type"].as_str().unwrap_or("").to_string(), 12.0));
    if let Some(module) = m["module"].as_str().filter(|s| !s.is_empty()) {
        body = body.push(widget::text::caption(module.to_string()));
    }
    widget::container(
        widget::row::with_capacity(2)
            .spacing(sp.space_xs)
            .align_y(Alignment::Center)
            .push(body)
            .push(widget::button::suggested("Insert").on_press(Message::Wrap(name, String::new()))),
    )
    .padding([sp.space_xxs, sp.space_xs])
    .width(Length::Fill)
    .class(theme::Container::custom(|theme| iced::widget::container::Style {
        background: Some(Color { a: 0.05, ..Color::from(theme.cosmic().accent_color()) }.into()),
        border: Border { radius: 6.0.into(), ..Default::default() },
        ..Default::default()
    }))
    .into()
}

/// Isabelle symbols to insert, by group or by search (name, LaTeX name or abbreviation): glyphs
/// large enough to tell apart, in square cells that light up under the pointer, each with a
/// tooltip giving its name and how to type it
fn symbols_panel<'a>(ws: &'a Workspace, table: &dyn super::codec::Symbols) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(24).spacing(sp.space_xxs);
    col = col.push(widget::search_input("forall, leq, -->, sub…", &ws.symbol_search).on_input(Message::SymbolSearch).on_clear(Message::SymbolSearch(String::new())));
    col = col.push(widget::text::caption("In the text, type \\ and a name (or a LaTeX name) and press Tab."));
    let mut typed: std::collections::HashMap<char, Vec<String>> = std::collections::HashMap::new();
    for (c, abbrev) in table.abbreviations() {
        typed.entry(c).or_default().push(abbrev.to_string());
    }
    let cell = |c: char, sym: String, how: String| -> Element<'a, Message> {
        let mut ways: Vec<String> = typed.get(&c).cloned().unwrap_or_default();
        if !how.is_empty() && !ways.contains(&how) {
            ways.insert(0, how);
        }
        let tip = if ways.is_empty() { sym } else { format!("{sym}   type {}", ways.join(" or ")) };
        widget::tooltip(
            widget::button::custom(widget::text(c.to_string()).size(19.0).font(cosmic::font::mono()).width(Length::Fill).height(Length::Fill).align_x(Alignment::Center).align_y(Alignment::Center))
                .width(Length::Fixed(36.0))
                .height(Length::Fixed(36.0))
                .padding(0)
                .class(theme::Button::Icon)
                .on_press(Message::InsertSymbol(c.to_string())),
            widget::text(tip).font(cosmic::font::mono()).size(13.0),
            widget::tooltip::Position::Bottom,
        )
        .into()
    };
    let grid = |items: Vec<Element<'a, Message>>| -> Element<'a, Message> { widget::flex_row(items).row_spacing(2).column_spacing(2).into() };
    let q = ws.symbol_search.trim().trim_start_matches('\\');
    if !q.is_empty() {
        let found = table.search(q, 40);
        if found.is_empty() {
            col = col.push(widget::text::body(format!("No symbol matches “{q}”.")));
        }
        col = col.push(grid(found.into_iter().map(|(c, sym, how)| cell(c, sym, how)).collect()));
    } else {
        for (group, syms) in table.groups() {
            // (group names as the table has them, lower case: a heading starts with a capital)
            let mut heading = group.clone();
            if let Some(first) = heading.get(..1) {
                heading = first.to_uppercase() + &heading[1..];
            }
            col = col.push(widget::container(widget::text::heading(heading)).padding(Padding { top: sp.space_xxs as f32, ..Padding::ZERO }));
            col = col.push(grid(syms.into_iter().map(|(c, sym)| cell(c, sym, String::new())).collect()));
        }
    }
    thin(col).height(Length::Fill).into()
}

/// The agents that have used the session: each a card with who and where it is, its editing
/// rights (each revocable), the rights it can be given, and following it
fn agents_panel<'a>(app: &'a App, ws: &'a Workspace) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(24).spacing(sp.space_xs);
    let summary = app.sessions.get(&(ws.machine.clone(), ws.sid.clone()));
    let grants: Vec<Value> = summary.and_then(|s| s["grants"].as_array().cloned()).unwrap_or_default();
    let clients: Vec<Value> = summary.and_then(|s| s["clients"].as_array().cloned()).unwrap_or_default();
    let seen = ws.info.as_ref().and_then(|i| i["seen"].as_object().cloned()).unwrap_or_default();
    col = col.push(widget::text::caption("Agents use this session through Deductum's MCP tools. They edit only where you grant rights; grants last until you revoke them."));
    if seen.is_empty() {
        col = col.push(widget::text::body("No agent has used this session yet."));
    }
    for (key, info) in seen {
        let label = info["label"].as_str().unwrap_or(&key).to_string();
        let live = clients.iter().any(|c| c["key"].as_str() == Some(key.as_str()));
        let pos = info["position"].clone();
        let where_ = pos["path"].as_str().map(|p| format!("{} line {}", file_name(p), pos["line"])).unwrap_or_default();
        let mut card = widget::column::with_capacity(8).spacing(sp.space_xxs);
        // Who, whether attached now, and where it last was
        let mut head = widget::row::with_capacity(2).spacing(sp.space_xxs).align_y(Alignment::Center).push(widget::text::heading(label.clone()).width(Length::Fill));
        if live {
            head = head.push(widget::text::caption("attached").class(theme::Text::Accent));
        }
        card = card.push(head);
        card = card.push(widget::text::caption(format!("{key}{}", if where_.is_empty() { String::new() } else { format!(" · at {where_}") })).class(muted()));
        // Its rights, each with Revoke
        let mine: Vec<&Value> = grants.iter().filter(|g| g["agent"].as_str() == Some(key.as_str())).collect();
        card = card.push(widget::container(widget::text::caption_heading("Editing rights")).padding(Padding { top: sp.space_xxs as f32, ..Padding::ZERO }));
        if mine.is_empty() {
            card = card.push(widget::text::body("None: it can read and query, not edit."));
        }
        for g in &mine {
            let scope = match g["scope"].as_str() {
                Some("folder") => "The whole folder".to_string(),
                Some("theory") => file_name(g["path"].as_str().unwrap_or("")).to_string(),
                Some("proof") => format!("The proof at line {} of {}", g["line"], file_name(g["path"].as_str().unwrap_or(""))),
                _ => "Some of the session".into(),
            };
            card = card.push(
                widget::row::with_capacity(2)
                    .spacing(sp.space_xs)
                    .align_y(Alignment::Center)
                    .push(widget::text::body(scope).width(Length::Fill))
                    .push(widget::button::destructive("Revoke").on_press(Message::Revoke(g["id"].as_str().unwrap_or("").to_string()))),
            );
        }
        // The rights it can be given, labelled, as one group
        card = card.push(widget::text::caption("Allow it to edit:"));
        card = card.push(
            widget::row::with_capacity(3)
                .spacing(sp.space_xxs)
                .push(widget::button::standard("Folder").on_press(Message::Grant(key.clone(), label.clone(), "folder")))
                .push(widget::button::standard("This theory").on_press_maybe(ws.theory.as_ref().map(|_| Message::Grant(key.clone(), label.clone(), "theory"))))
                .push(widget::button::standard("Proof at caret").on_press_maybe(ws.theory.as_ref().map(|_| Message::Grant(key.clone(), label.clone(), "proof")))),
        );
        let following = ws.follow.as_deref() == Some(key.as_str());
        card = card.push(widget::divider::horizontal::light());
        card = card.push(
            widget::row::with_capacity(2)
                .spacing(sp.space_xs)
                .align_y(Alignment::Center)
                .push(
                    widget::column::with_capacity(2)
                        .push(widget::text::body("Follow this agent"))
                        .push(widget::text::caption("Go where it shows you, as it does").class(muted()))
                        .width(Length::Fill),
                )
                .push(widget::toggler(following).on_toggle(move |on| Message::Follow(on.then(|| key.clone())))),
        );
        col = col.push(widget::container(card).padding(sp.space_xs).class(theme::Container::Card).width(Length::Fill));
    }
    thin(col).height(Length::Fill).into()
}

/// How long ago a Unix time was, in words
fn ago(time: f64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let s = (now - time).max(0.0) as u64;
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

/// The theory's edits, newest first: who, what and where, when; an agent's edit can be reverted
/// on its own (your own are undone with Ctrl+Z)
fn history_panel(ws: &Workspace) -> Element<'_, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(ws.history.len() * 2 + 1).spacing(0);
    col = col.push(widget::container(widget::text::caption("Ctrl+Z undoes your own edits. An agent's change is reverted on its own; later edits are kept.")).padding(Padding { bottom: sp.space_xs as f32, ..Padding::ZERO }));
    if ws.history.is_empty() {
        col = col.push(widget::text::body("No edits yet."));
    }
    for (n, h) in ws.history.iter().enumerate() {
        let author = h["author"]["label"].as_str().unwrap_or("?").to_string();
        let agent = h["author"]["kind"] == "agent";
        let kind = h["kind"].as_str().unwrap_or("").to_string();
        let reverted = h["reverted_by"].is_number();
        if n > 0 {
            col = col.push(widget::divider::horizontal::light());
        }
        let mut what = format!("{kind} at line {}", h["line"]);
        if let Some(t) = h["time"].as_f64() {
            what.push_str(&format!(" · {}", ago(t)));
        }
        let mut text = widget::column::with_capacity(3)
            .spacing(2)
            .push(
                widget::row::with_capacity(2)
                    .spacing(sp.space_xxs)
                    .push(widget::text::body(author).font(cosmic::iced::Font { weight: cosmic::iced::font::Weight::Semibold, ..cosmic::font::default() }))
                    .push(widget::text::body(what).class(muted())),
            )
            .push(widget::text(h["summary"].as_str().unwrap_or("").to_string()).font(code_font()).size(z(12.0)))
            .width(Length::Fill);
        if reverted {
            text = text.push(widget::text::caption("reverted").class(muted()));
        }
        let mut row = widget::row::with_capacity(2).spacing(sp.space_xs).align_y(Alignment::Center).push(text);
        if agent && !reverted
            && let Some(id) = h["id"].as_u64()
        {
            row = row.push(widget::button::standard("Revert").on_press(Message::Revert(id)));
        }
        col = col.push(widget::container(row).padding([sp.space_xxs, 0]));
    }
    thin(col).height(Length::Fill).into()
}

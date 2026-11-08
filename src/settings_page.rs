// SPDX-License-Identifier: GPL-3.0-only

//! The Settings page (household rule 21a): one scrolling page in the window with Back, a search
//! across every row (matches highlighted), a Show changed only switch, and the sections as cards.
//! From about 900 px of room a section list stands at the left (it scrolls to a section and
//! marks the one being read); from about 1,250 px the cards flow into two columns, none split.
//! Each row is a label with a line saying what it does, and its control: a segmented bar for a
//! few values, a dropdown for many, the household's muted switch for on and off. A dot marks a
//! row changed from its default; right-click offers Reset to default.
//!
//! The page is built from [`rows`], plain data (label, section, control, changed or not), so that
//! search, Show changed only and the layout are tested without drawing. Rows have one fixed
//! height and cards one fixed shape, so where each section starts is known without measuring
//! (the list scrolls to it and follows the scroll with the same numbers).
//!
//! Every change applies at once and is written at once; there is nothing to confirm. Keyboard
//! shortcuts and About are pages of their own, reached from rows at the end.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use cosmic::app::Task;
use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::prelude::*;
use cosmic::widget::{self, menu, settings};
use cosmic::{iced, theme};

use super::settings::{Change, CODE_SIZES, DisplayChange, INTERFACE_SIZES, Loogle, Rights, Settings, TextFont, ThemeChoice};
use super::shell::{self, Msg, sp};
use super::tones;
use super::{App, Message};

// ----- the layout

/// Room from which the section list stands at the left
pub const LIST_AT: f32 = 900.0;
/// Room from which the cards flow into two columns
pub const TWO_COLUMNS_AT: f32 = 1250.0;
const LIST_WIDTH: f32 = 220.0;
const ROW_HEIGHT: f32 = 64.0;
const TITLE_HEIGHT: f32 = 44.0;
const CARD_BOTTOM: f32 = 8.0;
const CARD_GAP: f32 = 24.0;
const PAGE_TOP: f32 = 8.0;

fn scroll_id() -> widget::Id {
    widget::Id::new("deductum-settings-scroll")
}

/// How a card with `rows` rows is tall (a thin line between rows)
fn card_height(rows: usize) -> f32 {
    TITLE_HEIGHT + rows as f32 * ROW_HEIGHT + rows.saturating_sub(1) as f32 + CARD_BOTTOM
}

/// How the cards are placed for `width` of room: whether the section list shows, and which
/// sections (by position among those shown) each column holds. Two columns split the sections,
/// in reading order, where the taller column is shortest; a card is never split
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub list: bool,
    pub columns: Vec<Vec<usize>>,
}

pub fn plan(width: f32, heights: &[f32]) -> Plan {
    let list = width >= LIST_AT;
    if width < TWO_COLUMNS_AT || heights.len() < 2 {
        return Plan { list, columns: vec![(0..heights.len()).collect()] };
    }
    let total: f32 = heights.iter().map(|h| h + CARD_GAP).sum();
    let mut best = (1, f32::MAX);
    let mut left = 0.0;
    for k in 1..heights.len() {
        left += heights[k - 1] + CARD_GAP;
        let tall = left.max(total - left);
        if tall < best.1 {
            best = (k, tall);
        }
    }
    Plan { list, columns: vec![(0..best.0).collect(), (best.0..heights.len()).collect()] }
}

/// Where each section's card starts in the scrolled content, by position among those shown
pub fn starts(plan: &Plan, heights: &[f32]) -> Vec<f32> {
    let mut at = vec![0.0; heights.len()];
    for column in &plan.columns {
        let mut y = PAGE_TOP;
        for &i in column {
            at[i] = y;
            y += heights[i] + CARD_GAP;
        }
    }
    at
}

// ----- the page's own state and messages

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sub {
    #[default]
    Settings,
    Shortcuts,
    About,
}

/// What the page keeps between frames
#[derive(Default)]
pub struct PageState {
    pub query: String,
    pub changed_only: bool,
    /// The section being read (the list marks it)
    pub current: Option<Section>,
    pub sub: Sub,
    /// The room the page had when last drawn (`f32` bits): where the scroll positions come from
    width: AtomicU32,
}

impl PageState {
    fn width(&self) -> f32 {
        f32::from_bits(self.width.load(Ordering::Relaxed))
    }
}

#[derive(Clone, Debug)]
pub enum PageMsg {
    Search(String),
    ChangedOnly(bool),
    /// Scroll to a section
    Jump(Section),
    /// The page scrolled: the offset, the view's height and the content's
    Scrolled(f32, f32, f32),
    /// Put a row back to its default
    Reset(&'static str),
    Open(Sub),
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ResetRow {
    Row(&'static str),
}

impl menu::Action for ResetRow {
    type Message = Message;

    fn message(&self) -> Message {
        let ResetRow::Row(id) = *self;
        Message::Shell(Msg::Page(PageMsg::Reset(id)))
    }
}

pub fn update(app: &mut App, msg: PageMsg) -> Task<Message> {
    match msg {
        PageMsg::Search(q) => app.shell.page.query = q,
        PageMsg::ChangedOnly(on) => app.shell.page.changed_only = on,
        PageMsg::Jump(section) => {
            app.shell.page.current = Some(section);
            let (sections, heights) = shown_sections(app);
            if let Some(i) = sections.iter().position(|s| *s == section) {
                let plan = plan(app.shell.page.width(), &heights);
                let y = starts(&plan, &heights)[i];
                return iced::widget::scrollable::scroll_to(
                    scroll_id(),
                    iced::widget::scrollable::AbsoluteOffset { x: None, y: Some((y - PAGE_TOP).max(0.0)) },
                );
            }
        }
        PageMsg::Scrolled(offset, view, content) => {
            let (sections, heights) = shown_sections(app);
            if sections.is_empty() {
                return Task::none();
            }
            let plan = plan(app.shell.page.width(), &heights);
            let at = starts(&plan, &heights);
            let last = (0..at.len()).max_by(|a, b| at[*a].total_cmp(&at[*b]).then(a.cmp(b))).unwrap_or(0);
            let reading = offset + PAGE_TOP + 4.0;
            let i = if offset > 0.0 && offset + view >= content - 1.0 {
                last
            } else {
                (0..at.len()).filter(|i| at[*i] <= reading).max_by(|a, b| at[*a].total_cmp(&at[*b]).then(a.cmp(b))).unwrap_or(0)
            };
            app.shell.page.current = Some(sections[i]);
        }
        PageMsg::Reset(id) => {
            if let Some(reset) = rows(app).into_iter().find(|r| r.id == id).and_then(|r| r.reset) {
                return app.shell_update(reset);
            }
        }
        PageMsg::Open(sub) => app.shell.page.sub = sub,
        PageMsg::Back => {
            if app.shell.page.sub == Sub::Settings {
                return app.shell_update(Msg::Settings(false));
            }
            app.shell.page.sub = Sub::Settings;
        }
    }
    Task::none()
}

// ----- the rows

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Appearance,
    Paper,
    Code,
    Editing,
    ProofState,
    Agents,
    Lean,
    Machines,
    Shortcuts,
    About,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Appearance, Section::Paper, Section::Code, Section::Editing, Section::ProofState,
        Section::Agents, Section::Lean, Section::Machines, Section::Shortcuts, Section::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Paper => "Paper",
            Section::Code => "Code",
            Section::Editing => "Editing",
            Section::ProofState => "Proof state",
            Section::Agents => "Agents",
            Section::Lean => "Lean",
            Section::Machines => "Machines",
            Section::Shortcuts => "Keyboard shortcuts",
            Section::About => "About",
        }
    }
}

/// How a row's value is set
#[derive(Clone, Debug)]
pub enum Control {
    /// A segmented bar, sized to its parts: each part's label and what choosing it does
    Seg { options: Vec<(String, Msg)>, selected: Option<usize> },
    /// A dropdown, for many values
    Drop { options: Vec<String>, selected: Option<usize>, msgs: Vec<Msg> },
    /// The household's muted switch
    Switch { on: bool, set: fn(bool) -> Msg },
    Spin { text: String, value: i32, step: i32, min: i32, max: i32, set: fn(i32) -> Msg },
    Text { placeholder: &'static str, value: String, set: fn(String) -> Msg },
    /// A button that does something (opens a page)
    Open { label: &'static str, msg: Msg },
    /// A machine's connection (the nth of the app's)
    Machine(usize),
    /// Words in place of a control
    Info(String),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: &'static str,
    pub section: Section,
    pub label: &'static str,
    pub desc: String,
    pub control: Control,
    /// Different from its default (the dot)
    pub changed: bool,
    /// What puts it back to its default; none for a row with no default
    pub reset: Option<Msg>,
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.001
}

/// A segmented choice among `values` (the current one added if it is not among them), the zero
/// value, if any, last and called Fill
fn seg_f32(values: &[f32], current: f32, label: impl Fn(f32) -> String, make: impl Fn(f32) -> Msg) -> Control {
    let mut values = values.to_vec();
    if !values.iter().any(|v| near(*v, current)) {
        values.push(current);
    }
    values.sort_by(|a, b| {
        let key = |v: &f32| if *v == 0.0 { f32::MAX } else { *v };
        key(a).total_cmp(&key(b))
    });
    Control::Seg {
        selected: values.iter().position(|v| near(*v, current)),
        options: values.iter().map(|v| (if *v == 0.0 { "Fill".to_string() } else { label(*v) }, make(*v))).collect(),
    }
}

fn seg_u32(values: &[u32], current: u32, make: impl Fn(u32) -> Msg) -> Control {
    seg_f32(&values.iter().map(|v| *v as f32).collect::<Vec<_>>(), current as f32, |v| format!("{}", v as u32), |v| make(v as u32))
}

fn switch(on: bool, set: fn(bool) -> Msg) -> Control {
    Control::Switch { on, set }
}

/// The code fonts offered (each with a bold; the choice is kept as a name, so another family in
/// the settings file stays as it is)
pub const CODE_FONTS: [&str; 5] = ["Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", "Source Code Pro", "JetBrains Mono"];

/// Every row of the page, with its current value. The Agents' default rights row is not on
/// Windows
pub fn rows(app: &App) -> Vec<Row> {
    let st = &app.shell.settings;
    let def = Settings::default();
    let d = &st.display;
    let dd = &def.display;
    let zoom = (super::view::zoom() * 100.0).round() as i32;
    let goal_beside = super::view::goal_beside_on();
    let mut out: Vec<Row> = Vec::new();
    #[allow(clippy::too_many_arguments)]
    let mut add = |id: &'static str, section: Section, label: &'static str, desc: &str, control: Control, changed: bool, reset: Option<Msg>| {
        out.push(Row { id, section, label, desc: desc.to_string(), control, changed, reset });
    };
    use Section::*;

    // Appearance
    add("theme", Appearance, "Theme", "Match the desktop, or always light or dark",
        Control::Seg {
            options: ThemeChoice::ALL.iter().map(|t| (t.label().to_string(), Msg::Theme(*t))).collect(),
            selected: ThemeChoice::ALL.iter().position(|t| *t == st.appearance.theme),
        },
        st.appearance.theme != def.appearance.theme, Some(Msg::Theme(def.appearance.theme)));
    add("interface_size", Appearance, "Interface size", "Labels, menus and buttons, in px; Ctrl+= zooms the content, not these",
        seg_u32(&INTERFACE_SIZES.map(u32::from), u32::from(st.appearance.interface_size), |v| Msg::Setting(Change::InterfaceSize(v as u8))),
        st.appearance.interface_size != def.appearance.interface_size, Some(Msg::Setting(Change::InterfaceSize(def.appearance.interface_size))));
    add("thicken_text", Appearance, "Thicken text", "Regular text drawn in semibold",
        switch(st.appearance.thicken_text, |v| Msg::Setting(Change::ThickenText(v))),
        st.appearance.thicken_text != def.appearance.thicken_text, Some(Msg::Setting(Change::ThickenText(def.appearance.thicken_text))));
    add("zoom", Appearance, "Zoom", "Also Ctrl+= and Ctrl+−; Ctrl+0 for 100%",
        Control::Spin { text: format!("{zoom}%"), value: zoom, step: 10, min: 50, max: 300, set: Msg::TextSize },
        zoom != 100, Some(Msg::TextSize(100)));
    add("goal_beside", Appearance, "The goal beside the text", "When the window has room; otherwise under the proof, with the page centred",
        switch(goal_beside, Msg::GoalBeside), !goal_beside, Some(Msg::GoalBeside(true)));

    // Paper
    add("prose_font", Paper, "Prose font", "The literate document and its prose editor; Sans matches Terminal Agents threads",
        Control::Drop {
            options: TextFont::ALL.iter().map(|f| f.label().to_string()).collect(),
            selected: TextFont::ALL.iter().position(|f| *f == d.font),
            msgs: TextFont::ALL.iter().map(|f| Msg::Display(DisplayChange::Font(*f))).collect(),
        },
        d.font != dd.font, Some(Msg::Display(DisplayChange::Font(dd.font))));
    add("prose_size", Paper, "Prose size", "In px",
        seg_f32(&[17.0, 18.0, 20.0, 22.0], d.text_size, |v| format!("{v}"), |v| Msg::Display(DisplayChange::TextSize(v))),
        !near(d.text_size, dd.text_size), Some(Msg::Display(DisplayChange::TextSize(dd.text_size))));
    add("line_spacing", Paper, "Line spacing", "",
        seg_f32(&[1.4, 1.5, 1.6], d.line_height, |v| format!("{v}"), |v| Msg::Display(DisplayChange::LineHeight(v))),
        !near(d.line_height, dd.line_height), Some(Msg::Display(DisplayChange::LineHeight(dd.line_height))));
    add("prose_width", Paper, "Prose column width", "Maximum width of the text, in em; Fill follows the proof layout",
        seg_f32(&[40.0, 48.0, 60.0, 0.0], d.measure, |v| format!("{v}"), |v| Msg::Display(DisplayChange::Measure(v))),
        !near(d.measure, dd.measure), Some(Msg::Display(DisplayChange::Measure(dd.measure))));
    add("paragraph_spacing", Paper, "Paragraph spacing", "",
        {
            let mut values = vec![0.4, 0.65, 0.7, 0.9, 1.2, 1.5];
            if !values.iter().any(|v| near(*v, d.paragraph_spacing)) {
                values.push(d.paragraph_spacing);
                values.sort_by(f32::total_cmp);
            }
            Control::Drop {
                options: values.iter().map(|v| format!("{v} em")).collect(),
                selected: values.iter().position(|v| near(*v, d.paragraph_spacing)),
                msgs: values.iter().map(|v| Msg::Display(DisplayChange::ParagraphSpacing(*v))).collect(),
            }
        },
        !near(d.paragraph_spacing, dd.paragraph_spacing), Some(Msg::Display(DisplayChange::ParagraphSpacing(dd.paragraph_spacing))));
    add("maths_size", Paper, "Maths size", "Formulas against the text, in steps of 0.05",
        {
            let mut values: Vec<f32> = (80..=150).step_by(5).map(|n| n as f32 / 100.0).collect();
            if !values.iter().any(|v| near(*v, d.math_scale)) {
                values.push(d.math_scale);
                values.sort_by(f32::total_cmp);
            }
            Control::Drop {
                options: values.iter().map(|v| format!("× {v:.2}")).collect(),
                selected: values.iter().position(|v| near(*v, d.math_scale)),
                msgs: values.iter().map(|v| Msg::Display(DisplayChange::MathScale(*v))).collect(),
            }
        },
        !near(d.math_scale, dd.math_scale), Some(Msg::Display(DisplayChange::MathScale(dd.math_scale))));
    add("justify", Paper, "Justify paragraphs", "", switch(d.justify, |v| Msg::Display(DisplayChange::Justify(v))),
        d.justify != dd.justify, Some(Msg::Display(DisplayChange::Justify(dd.justify))));
    add("centre", Paper, "Centre the column", "", switch(d.centred, |v| Msg::Display(DisplayChange::Centred(v))),
        d.centred != dd.centred, Some(Msg::Display(DisplayChange::Centred(dd.centred))));

    // Code
    add("code_font", Code, "Code font", "Cells, the Source view, goals, and names in the text",
        {
            let mut fonts: Vec<String> = CODE_FONTS.iter().map(|f| f.to_string()).collect();
            if !fonts.contains(&st.code.font) {
                fonts.push(st.code.font.clone());
            }
            Control::Drop {
                selected: fonts.iter().position(|f| *f == st.code.font),
                msgs: fonts.iter().map(|f| Msg::Setting(Change::CodeFont(f.clone()))).collect(),
                options: fonts,
            }
        },
        st.code.font != def.code.font, Some(Msg::Setting(Change::CodeFont(def.code.font.clone()))));
    add("code_size", Code, "Code size", "In px",
        seg_u32(&CODE_SIZES.map(u32::from), u32::from(st.code.size), |v| Msg::Setting(Change::CodeSize(v as u8))),
        st.code.size != def.code.size, Some(Msg::Setting(Change::CodeSize(def.code.size))));
    add("cell_width", Code, "Cell width", "In characters; longer lines widen the cell",
        seg_u32(&[72, 80, 100, 0], u32::from(st.code.cell_width), |v| Msg::Setting(Change::CellWidth(v as u16))),
        st.code.cell_width != def.code.cell_width, Some(Msg::Setting(Change::CellWidth(def.code.cell_width))));
    add("indent_width", Code, "Indent width", "Spaces that Tab inserts and Shift+Tab takes away",
        seg_u32(&[2, 4], u32::from(st.editing.indent_width), |v| Msg::IndentWidth(v as u8)),
        st.editing.indent_width != def.editing.indent_width, Some(Msg::IndentWidth(def.editing.indent_width)));

    // Editing
    add("vim", Editing, "Vim keys", "Normal, insert and visual modes in every editor; j and k move between blocks",
        switch(app.vim_enabled, Msg::Vim), app.vim_enabled != def.editing.vim, Some(Msg::Vim(def.editing.vim)));
    add("completion", Editing, "Completion while typing", "Words and steps from the goal; \\ symbols and @names are always offered",
        switch(st.editing.complete_while_typing, Msg::CompleteWhileTyping),
        st.editing.complete_while_typing != def.editing.complete_while_typing, Some(Msg::CompleteWhileTyping(def.editing.complete_while_typing)));
    add("keywords", Editing, "Keyword lines ask for proofs", "A line reading claude, codex or @name asks that LLM or agent for a proof",
        switch(st.editing.keywords, Msg::Keywords), st.editing.keywords != def.editing.keywords, Some(Msg::Keywords(def.editing.keywords)));
    add("keyword_pause", Editing, "Pause before asking", "How long typing pauses on a keyword line before it asks; Enter asks at once",
        Control::Spin {
            text: format!("{:.1} s", f64::from(st.editing.keyword_pause_ms) / 1000.0),
            value: st.editing.keyword_pause_ms as i32, step: 500, min: 500, max: 10_000, set: |ms| Msg::KeywordPause(ms as u32),
        },
        st.editing.keyword_pause_ms != def.editing.keyword_pause_ms, Some(Msg::KeywordPause(def.editing.keyword_pause_ms)));
    add("spelling", Editing, "Spelling", "Check the spelling of the theory's prose",
        switch(st.editing.spelling, |v| Msg::Setting(Change::Spelling(v))),
        st.editing.spelling != def.editing.spelling, Some(Msg::Setting(Change::Spelling(def.editing.spelling))));

    // Proof state
    add("show_types", ProofState, "Show types", "Where the prover can print them; the Types switch beside the goal starts here",
        switch(st.proof_state.show_types, Msg::ShowTypes), st.proof_state.show_types != def.proof_state.show_types, Some(Msg::ShowTypes(def.proof_state.show_types)));
    add("show_changes", ProofState, "Show what a step changed", "The subgoals the step at the caret solved and the new ones",
        switch(st.proof_state.show_changes, Msg::ShowChanges), st.proof_state.show_changes != def.proof_state.show_changes, Some(Msg::ShowChanges(def.proof_state.show_changes)));

    // Agents: where this session's keyword lines go is the session's (the host keeps it)
    match &app.workspace {
        Some(ws) => {
            let routes = shell::routes(ws);
            let current = shell::current_route(ws);
            add("route", Agents, "Where keyword lines go", "For this session: its helper conversation remembers earlier attempts",
                Control::Drop {
                    selected: routes.iter().position(|(v, _)| *v == current),
                    options: routes.iter().map(|(_, l)| l.clone()).collect(),
                    msgs: (0..routes.len()).map(Msg::Route).collect(),
                },
                false, None);
        }
        None => add("route", Agents, "Where keyword lines go", "Each session's own choice: open a session to set it", Control::Info(String::new()), false, None),
    }
    add("claude_model", Agents, "Claude's model", "When a claude line names none, as claude[opus] does",
        Control::Text { placeholder: "Claude's default", value: st.llm.claude_model.clone(), set: Msg::ClaudeModel },
        !st.llm.claude_model.is_empty(), Some(Msg::ClaudeModel(String::new())));
    add("codex_model", Agents, "Codex's model", "When a codex line names none",
        Control::Text { placeholder: "Codex's default", value: st.llm.codex_model.clone(), set: Msg::CodexModel },
        !st.llm.codex_model.is_empty(), Some(Msg::CodexModel(String::new())));
    #[cfg(not(windows))]
    add("default_rights", Agents, "Default editing rights", "What a new agent session may edit until you say otherwise; not on Windows",
        Control::Drop {
            options: Rights::ALL.iter().map(|r| r.label().to_string()).collect(),
            selected: Rights::ALL.iter().position(|r| *r == st.agents.default_rights),
            msgs: Rights::ALL.iter().map(|r| Msg::Setting(Change::DefaultRights(*r))).collect(),
        },
        st.agents.default_rights != def.agents.default_rights, Some(Msg::Setting(Change::DefaultRights(def.agents.default_rights))));

    // Lean
    add("loogle", Lean, "Loogle search", "Finding lemmas by name and type pattern: off, a local Loogle, or the public service",
        Control::Drop {
            options: Loogle::ALL.iter().map(|l| l.label().to_string()).collect(),
            selected: Loogle::ALL.iter().position(|l| *l == st.lean.loogle),
            msgs: Loogle::ALL.iter().map(|l| Msg::Loogle(*l)).collect(),
        },
        st.lean.loogle != def.lean.loogle, Some(Msg::Loogle(def.lean.loogle)));

    // Machines (their names are not static; the row's label says what they are)
    if app.links.is_empty() {
        add("machines_none", Machines, "No machines", "Machines come from the Terminal Agents app", Control::Info(String::new()), false, None);
    }
    for (i, link) in app.links.iter().enumerate() {
        let mut destination = match &link.target.destination {
            Some(d) => format!("Over ssh: {d}"),
            None => "This computer".to_string(),
        };
        // (its host's version, once it has said hello)
        if let Some(v) = link.hello.as_ref().and_then(|h| h["version"].as_str()) {
            destination.push_str(&format!(" · session host {v}"));
        }
        add("machine", Machines, "Machine", &destination, Control::Machine(i), false, None);
    }

    // The pages of their own
    add("shortcuts", Shortcuts, "Keyboard shortcuts", "Every shortcut, by area",
        Control::Open { label: "Show", msg: Msg::Page(PageMsg::Open(Sub::Shortcuts)) }, false, None);
    add("about", About, "About Deductum", "Version, licence and where the settings are kept",
        Control::Open { label: "Show", msg: Msg::Page(PageMsg::Open(Sub::About)) }, false, None);
    out
}

/// The words of a search, lower case
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(|t| t.to_lowercase()).collect()
}

/// Whether a row answers a search: every word is in its label, its line, or its section's name
pub fn matches(row: &Row, terms: &[String]) -> bool {
    let hay = format!("{} {} {}", row.label, row.desc, row.section.title()).to_lowercase();
    terms.iter().all(|t| hay.contains(t.as_str()))
}

/// The rows the search and Show changed only leave
pub fn visible(rows: Vec<Row>, query: &str, changed_only: bool) -> Vec<Row> {
    let terms = terms(query);
    rows.into_iter().filter(|r| (!changed_only || r.changed) && matches(r, &terms)).collect()
}

fn shown_rows(app: &App) -> Vec<Row> {
    visible(rows(app), &app.shell.page.query, app.shell.page.changed_only)
}

/// The sections that have a row to show, in order, and the height of each one's card
fn shown_sections(app: &App) -> (Vec<Section>, Vec<f32>) {
    sections_of(&shown_rows(app))
}

fn sections_of(rows: &[Row]) -> (Vec<Section>, Vec<f32>) {
    let mut sections = Vec::new();
    let mut heights = Vec::new();
    for s in Section::ALL {
        let n = rows.iter().filter(|r| r.section == s).count();
        if n > 0 {
            sections.push(s);
            heights.push(card_height(n));
        }
    }
    (sections, heights)
}

// ----- drawing

fn set(m: Msg) -> Message {
    Message::Shell(m)
}

fn page_msg(m: PageMsg) -> Message {
    Message::Shell(Msg::Page(m))
}

pub fn page(app: &App) -> Element<'_, Message> {
    match app.shell.page.sub {
        Sub::Settings => settings_page(app),
        Sub::Shortcuts => shortcuts_page(app),
        Sub::About => about_page(app),
    }
}

/// The top of a sub page: Back and the page's title
fn back_head<'a>(title: &'static str) -> Element<'a, Message> {
    widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .spacing(sp().space_s)
        .push(back_button())
        .push(widget::text::title3(title).width(Length::Fill))
        .into()
}

fn back_button<'a>() -> Element<'a, Message> {
    widget::button::text("Back")
        .leading_icon(widget::icon::from_name("go-previous-symbolic").size(16).handle())
        .on_press(page_msg(PageMsg::Back))
        .into()
}

fn settings_page(app: &App) -> Element<'_, Message> {
    let s = sp();
    let page = &app.shell.page;
    let head = widget::row::with_capacity(5)
        .align_y(Alignment::Center)
        .spacing(s.space_s)
        .padding([s.space_s, s.space_l])
        .push(back_button())
        .push(widget::text::title3("Settings").width(Length::Fill))
        .push(
            widget::container(
                widget::search_input("Search settings", page.query.clone())
                    .on_input(|q| page_msg(PageMsg::Search(q)))
                    .on_clear(page_msg(PageMsg::Search(String::new())))
                    .width(Length::Fill),
            )
            .max_width(280)
            .width(Length::Fill),
        )
        .push(widget::text::body("Show changed only"))
        .push(muted_switch(page.changed_only, |on| page_msg(PageMsg::ChangedOnly(on))));
    let body = widget::responsive(move |size| {
        page.width.store(size.width.to_bits(), Ordering::Relaxed);
        body(app, size.width)
    });
    widget::column::with_capacity(2).push(head).push(body).height(Length::Fill).into()
}

fn muted_switch<'a>(on: bool, f: impl Fn(bool) -> Message + 'static) -> Element<'a, Message> {
    widget::toggler(on).class(theme::Toggler::accent(|t| tones::muted(t).0)).on_toggle(f).into()
}

fn body(app: &App, width: f32) -> Element<'_, Message> {
    let s = sp();
    let rows = shown_rows(app);
    if rows.is_empty() {
        let none = if app.shell.page.changed_only && app.shell.page.query.trim().is_empty() { "No setting differs from its default" } else { "No setting matches" };
        return widget::container(widget::text::body(none)).center(Length::Fill).into();
    }
    let (sections, heights) = sections_of(&rows);
    let plan = plan(width, &heights);
    let terms = terms(&app.shell.page.query);
    let mut by_section: HashMap<Section, Vec<Row>> = HashMap::new();
    for r in rows {
        by_section.entry(r.section).or_default().push(r);
    }
    let mut cards: Vec<Option<Element<'_, Message>>> = sections.iter().map(|sec| Some(card(app, *sec, by_section.remove(sec).unwrap_or_default(), &terms))).collect();
    let mut columns = widget::row::with_capacity(plan.columns.len()).spacing(CARD_GAP);
    for column in &plan.columns {
        let mut col = widget::column::with_capacity(column.len()).spacing(CARD_GAP).width(Length::Fill);
        for &i in column {
            if let Some(card) = cards[i].take() {
                col = col.push(card);
            }
        }
        columns = columns.push(col);
    }
    let scroll = iced::widget::scrollable(widget::container(columns).padding(Padding { top: PAGE_TOP, right: s.space_l as f32, bottom: s.space_l as f32, left: s.space_l as f32 }).width(Length::Fill))
        .id(scroll_id())
        .on_scroll(|v| page_msg(PageMsg::Scrolled(v.absolute_offset().y, v.bounds().height, v.content_bounds().height)))
        .width(Length::Fill)
        .height(Length::Fill);
    let current = app.shell.page.current.filter(|c| sections.contains(c)).unwrap_or(sections[0]);
    if plan.list {
        let mut list = widget::column::with_capacity(sections.len()).spacing(2).width(Length::Fixed(LIST_WIDTH - 24.0));
        for sec in &sections {
            let on = *sec == current;
            list = list.push(
                widget::button::custom(
                    widget::container(
                        widget::text::body(sec.title())
                            .font(if on { cosmic::font::bold() } else { cosmic::font::default() })
                            .class(if on { theme::Text::Accent } else { theme::Text::Default }),
                    )
                    .padding([6, 12])
                    .width(Length::Fill)
                    .class(theme::Container::custom(move |t| iced::widget::container::Style {
                        background: on.then(|| Color { a: 0.12, ..Color::from(t.cosmic().on_bg_color()) }.into()),
                        border: Border { radius: t.cosmic().radius_s().into(), ..Default::default() },
                        ..Default::default()
                    })),
                )
                .class(theme::Button::Text)
                .padding(0)
                .width(Length::Fill)
                .on_press(page_msg(PageMsg::Jump(*sec))),
            );
        }
        widget::row::with_capacity(2)
            .push(widget::container(list).padding(Padding { top: PAGE_TOP, right: 12.0, bottom: 0.0, left: 12.0 }).width(Length::Fixed(LIST_WIDTH)))
            .push(scroll)
            .height(Length::Fill)
            .into()
    } else {
        // Narrow: the sections as a row of chips above the page
        let mut chips = Vec::new();
        for sec in &sections {
            let on = *sec == current;
            chips.push(
                widget::button::text(sec.title())
                    .class(if on { tones::muted_button(theme::Button::Text) } else { theme::Button::Text })
                    .on_press(page_msg(PageMsg::Jump(*sec)))
                    .into(),
            );
        }
        widget::column::with_capacity(2)
            .push(widget::container(widget::flex_row(chips).spacing(4)).padding([0, s.space_l]))
            .push(scroll)
            .height(Length::Fill)
            .into()
    }
}

fn card<'a>(app: &'a App, section: Section, rows: Vec<Row>, terms: &[String]) -> Element<'a, Message> {
    let mut col = widget::column::with_capacity(rows.len() * 2 + 1).width(Length::Fill);
    col = col.push(
        widget::container(widget::text::heading(section.title()))
            .padding([0, 20])
            .center_y(Length::Fixed(TITLE_HEIGHT))
            .width(Length::Fill),
    );
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            col = col.push(
                widget::container(
                    widget::container(widget::Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
                        .class(theme::Container::custom(|t| iced::widget::container::Style {
                            background: Some(Color { a: 0.14, ..Color::from(t.cosmic().on_bg_color()) }.into()),
                            ..Default::default()
                        })),
                )
                .padding([0, 20]),
            );
        }
        col = col.push(row_view(app, row, terms));
    }
    widget::container(col.padding(Padding { top: 0.0, right: 0.0, bottom: CARD_BOTTOM, left: 0.0 }))
        .width(Length::Fill)
        .class(theme::Container::List)
        .into()
}

fn row_view<'a>(app: &'a App, row: Row, terms: &[String]) -> Element<'a, Message> {
    let dot: Element<'a, Message> = if row.changed {
        widget::container(widget::Space::new().width(Length::Fixed(6.0)).height(Length::Fixed(6.0)))
            .class(theme::Container::custom(|t| iced::widget::container::Style {
                background: Some(Color::from(t.cosmic().accent_color()).into()),
                border: Border { radius: 3.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into()
    } else {
        widget::Space::new().width(Length::Fixed(6.0)).into()
    };
    let label = marked(row.label, terms, 14.0);
    let mut text = widget::column::with_capacity(2).spacing(2).width(Length::Fill).push(label);
    if !row.desc.is_empty() {
        text = text.push(marked(&row.desc, terms, 12.0));
    }
    let id = row.id;
    let changed = row.changed && row.reset.is_some();
    let line = widget::row::with_capacity(4)
        .align_y(Alignment::Center)
        .spacing(8)
        .padding(Padding { top: 0.0, right: 20.0, bottom: 0.0, left: 8.0 })
        .height(Length::Fixed(ROW_HEIGHT))
        .push(widget::container(dot).width(Length::Fixed(12.0)).center_x(Length::Fixed(12.0)))
        .push(widget::container(text).width(Length::Fill).clip(true))
        .push(control(app, row.control));
    let reset = if changed {
        menu::Item::Button("Reset to default", None, ResetRow::Row(id))
    } else {
        menu::Item::ButtonDisabled("Reset to default", None, ResetRow::Row(id))
    };
    let tree = menu::items(&HashMap::new(), vec![reset]);
    let mut menu = widget::context_menu(line, Some(tree));
    menu.item_width = menu::ItemWidth::Uniform(200);
    menu.into()
}

/// `text` with the places a search word occurs drawn highlighted
fn marked<'a>(text: &str, terms: &[String], size: f32) -> Element<'a, Message> {
    let lower = text.to_ascii_lowercase();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for t in terms.iter().filter(|t| t.is_ascii()) {
        let mut from = 0;
        while let Some(at) = lower[from..].find(t.as_str()) {
            spans.push((from + at, from + at + t.len()));
            from += at + t.len();
        }
    }
    if spans.is_empty() {
        return widget::text(text.to_string()).size(size).wrapping(iced::widget::text::Wrapping::Word).into();
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let accent: Color = cosmic::theme::active().cosmic().accent_color().into();
    let mut out: Vec<iced::widget::text::Span<'a, (), cosmic::iced::Font>> = Vec::new();
    let mut at = 0;
    for (a, b) in merged {
        if a > at {
            out.push(iced::widget::span(text[at..a].to_string()));
        }
        out.push(iced::widget::span(text[a..b].to_string()).background(Color { a: 0.35, ..accent }).padding(0));
        at = b;
    }
    if at < text.len() {
        out.push(iced::widget::span(text[at..].to_string()));
    }
    iced::widget::rich_text(out).size(size).wrapping(iced::widget::text::Wrapping::Word).into()
}

fn control<'a>(app: &'a App, c: Control) -> Element<'a, Message> {
    match c {
        Control::Seg { options, selected } => seg(options, selected),
        Control::Drop { options, selected, msgs } => widget::dropdown(options, selected, move |i| set(msgs[i].clone())).into(),
        Control::Switch { on, set: make } => muted_switch(on, move |v| set(make(v))),
        Control::Spin { text, value, step, min, max, set: make } => widget::spin_button(text, value, step, min, max, move |v| set(make(v))).into(),
        Control::Text { placeholder, value, set: make } => widget::text_input(placeholder, value).on_input(move |v| set(make(v))).width(Length::Fixed(200.0)).into(),
        Control::Open { label, msg } => widget::button::standard(label).on_press(set(msg)).into(),
        Control::Machine(i) => app.links.get(i).map(machine_state).unwrap_or_else(|| widget::Space::new().into()),
        Control::Info(words) if words.is_empty() => widget::Space::new().into(),
        Control::Info(words) => widget::text::body(words).into(),
    }
}

/// A segmented bar sized to its parts, the chosen one filled with the muted accent
fn seg<'a>(options: Vec<(String, Msg)>, selected: Option<usize>) -> Element<'a, Message> {
    let mut bar = widget::row::with_capacity(options.len()).spacing(2).align_y(Alignment::Center);
    for (i, (label, msg)) in options.into_iter().enumerate() {
        let on = selected == Some(i);
        bar = bar.push(
            widget::button::text(label)
                .class(if on { tones::muted_button(theme::Button::Text) } else { theme::Button::Text })
                .on_press(set(msg)),
        );
    }
    widget::container(bar)
        .padding(2)
        .class(theme::Container::custom(|t| iced::widget::container::Style {
            background: Some(Color { a: 0.12, ..Color::from(t.cosmic().on_bg_color()) }.into()),
            border: Border { radius: t.cosmic().radius_s().into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

// ----- the pages of their own

fn sub_page<'a>(head: Element<'a, Message>, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let s = sp();
    let col = widget::column::with_capacity(2).spacing(s.space_l).padding([s.space_l, s.space_l]).max_width(760).push(head).push(body);
    widget::scrollable(widget::container(col).center_x(Length::Fill)).height(Length::Fill).into()
}

/// Keyboard shortcuts: a page of its own (rule 21b). Only the keys the app has so far; the
/// grouped, searchable list comes with the shortcut work
fn shortcuts_page(_app: &App) -> Element<'_, Message> {
    let key = |keys: &'static str, what: &'static str| settings::item::builder(keys).control(widget::text::body(what));
    let section = settings::section()
        .title("Everywhere")
        .add(key("Ctrl+,", "Settings"))
        .add(key("Ctrl+=  ·  Ctrl+−  ·  Ctrl+0", "Larger, smaller and default content size"))
        .add(key("Alt+Left", "Back to where Ctrl+click came from"))
        .add(key("Ctrl+1  \u{b7}  Ctrl+2", "The document as Literate, as Source"))
        .add(key("F9", "Show or hide the sidebar"))
        .add(key("F8  \u{b7}  Shift+F8", "The next failed cell, the next unfinished one"));
    sub_page(back_head("Keyboard shortcuts"), section)
}

fn about_page(_app: &App) -> Element<'_, Message> {
    let about = settings::section()
        .title("Deductum")
        .add(settings::item::builder("Version").description("Literate proof documents for Isabelle and Lean, shared with agents").control(widget::text::body(env!("CARGO_PKG_VERSION"))))
        .add(settings::item::builder("Licence").control(widget::text::body("GPL-3.0-only")))
        .add(settings::item::builder("Settings file").control(widget::text::caption(super::settings::path().display().to_string()).wrapping(iced::widget::text::Wrapping::WordOrGlyph)));
    sub_page(back_head("About"), about)
}

/// A machine's connection, in a word with a coloured dot, as the sidebar and the status bar say it
fn machine_state<'a>(link: &super::Link) -> Element<'a, Message> {
    let conn = shell::connection(link);
    let mut word = conn.word().to_string();
    word[..1].make_ascii_uppercase();
    if let Some(d) = conn.detail() {
        word = format!("{word}: {d}");
    }
    widget::row::with_capacity(2)
        .spacing(6)
        .align_y(Alignment::Center)
        .push(conn.dot(8.0))
        .push(widget::text::body(word).wrapping(iced::widget::text::Wrapping::None))
        .into()
}


#[cfg(test)]
mod tests {
    use cosmic::iced::Size;

    use super::super::editor::Mode;
    use super::super::snapshot::{Harness, demo_app, draw_element, ui_turn};
    use super::*;

    /// The demo app on the Settings page, with the sidebar away so that the page has the window
    fn settings_app(sidebar: bool) -> App {
        let mut app = demo_app(Mode::Literate);
        app.shell.settings_open = true;
        app.show_sidebar = sidebar;
        app
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    /// Process-wide view state a row reads, back to its defaults (taken under the UI's turn)
    fn plain_view_state() {
        super::super::view::set_zoom(1.0);
        super::super::view::set_goal_beside(true);
    }

    #[test]
    fn the_defaults_change_nothing_and_every_row_has_an_id_and_a_section() {
        let _turn = ui_turn();
        plain_view_state();
        let app = settings_app(false);
        let rows = rows(&app);
        assert!(rows.iter().all(|r| !r.changed), "{:?}", ids(&rows.iter().filter(|r| r.changed).cloned().collect::<Vec<_>>()));
        let mut seen = std::collections::HashSet::new();
        for r in rows.iter().filter(|r| r.id != "machine") {
            assert!(seen.insert(r.id), "{} twice", r.id);
        }
        for s in Section::ALL {
            assert!(rows.iter().any(|r| r.section == s), "{s:?} has no row");
        }
        // the stated defaults
        let get = |id: &str| rows.iter().find(|r| r.id == id).unwrap().control.clone();
        let chosen = |c: Control| match c {
            Control::Seg { options, selected } => options[selected.unwrap()].0.clone(),
            Control::Drop { options, selected, .. } => options[selected.unwrap()].clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(chosen(get("interface_size")), "14");
        assert_eq!(chosen(get("prose_width")), "48", "Paper's measure (Q5)");
        assert_eq!(chosen(get("code_font")), "Noto Sans Mono");
        assert_eq!(chosen(get("code_size")), "14");
        assert_eq!(chosen(get("cell_width")), "80");
        assert_eq!(chosen(get("maths_size")), "× 1.05", "Juliet approved x1.05 (household-kit#3)");
        #[cfg(not(windows))]
        assert_eq!(chosen(get("default_rights")), "This theory");
    }

    #[test]
    fn maths_size_offers_080_to_150_in_steps_of_005() {
        let _turn = ui_turn();
        let app = settings_app(false);
        let Control::Drop { options, .. } = rows(&app).into_iter().find(|r| r.id == "maths_size").unwrap().control else { panic!() };
        assert_eq!(options.len(), 15);
        assert_eq!((options[0].as_str(), options[14].as_str()), ("× 0.80", "× 1.50"));
        assert!(options.contains(&"× 1.05".to_string()));
    }

    #[test]
    fn search_filters_the_rows_across_every_section() {
        let _turn = ui_turn();
        let app = settings_app(false);
        let all = rows(&app);
        assert_eq!(visible(all.clone(), "", false).len(), all.len(), "no search shows every row");
        assert_eq!(ids(&visible(all.clone(), "maths", false)), ["maths_size"]);
        assert_eq!(ids(&visible(all.clone(), "  MATHS  ", false)), ["maths_size"], "case and spacing do not matter");
        let fonts = visible(all.clone(), "font", false);
        assert!(ids(&fonts).contains(&"prose_font") && ids(&fonts).contains(&"code_font"));
        assert!(fonts.iter().all(|r| r.section != Section::ProofState), "rows without it are away");
        // every word must be found, in the label, the line or the section's name
        assert_eq!(ids(&visible(all.clone(), "code size", false)), ["code_size"]);
        assert!(visible(all.clone(), "paper", false).iter().filter(|r| r.section == Section::Paper).count() >= 8, "a section's name finds its rows");
        assert!(visible(all, "zzzz-nothing", false).is_empty());
    }

    #[test]
    fn show_changed_only_lists_exactly_the_changed_rows() {
        let _turn = ui_turn();
        plain_view_state();
        let mut app = settings_app(false);
        assert!(visible(rows(&app), "", true).is_empty());
        for m in [
            Msg::Theme(ThemeChoice::Light),
            Msg::Display(DisplayChange::MathScale(1.1)),
            Msg::Display(DisplayChange::Measure(60.0)),
            Msg::Setting(Change::CodeSize(16)),
            Msg::Setting(Change::ThickenText(true)),
            Msg::Setting(Change::DefaultRights(Rights::Folder)),
            Msg::ClaudeModel("opus".into()),
            Msg::IndentWidth(4),
        ] {
            let _ = app.shell_update(m);
        }
        let mut changed = ids(&visible(rows(&app), "", true));
        changed.sort();
        let mut expected = vec!["theme", "maths_size", "prose_width", "code_size", "thicken_text", "claude_model", "indent_width"];
        #[cfg(not(windows))]
        expected.push("default_rights");
        expected.sort();
        assert_eq!(changed, expected);
        // with a search too: the changed rows that match
        assert_eq!(ids(&visible(rows(&app), "size", true)).len(), 2, "maths size and code size");
        // and Reset to default puts each back
        for id in &expected {
            let reset = rows(&app).into_iter().find(|r| r.id == *id).and_then(|r| r.reset).unwrap();
            let _ = app.shell_update(reset);
        }
        assert!(visible(rows(&app), "", true).is_empty(), "{:?}", ids(&visible(rows(&app), "", true)));
        assert_eq!(app.shell.settings.display, Settings::default().display);
    }

    #[test]
    fn the_page_messages_drive_search_and_the_changed_switch() {
        let _turn = ui_turn();
        plain_view_state();
        let mut app = settings_app(false);
        let _ = app.shell_update(Msg::Page(PageMsg::Search("loogle".into())));
        assert_eq!(ids(&shown_rows(&app)), ["loogle"]);
        let _ = app.shell_update(Msg::Page(PageMsg::ChangedOnly(true)));
        assert!(shown_rows(&app).is_empty(), "loogle is at its default");
        let _ = app.shell_update(Msg::Page(PageMsg::Search(String::new())));
        let _ = app.shell_update(Msg::Page(PageMsg::ChangedOnly(false)));
        assert!(shown_rows(&app).len() > 20);
        // Back from a page of its own returns to Settings; Back from Settings closes it
        let _ = app.shell_update(Msg::Page(PageMsg::Open(Sub::About)));
        assert_eq!(app.shell.page.sub, Sub::About);
        let _ = app.shell_update(Msg::Page(PageMsg::Back));
        assert_eq!((app.shell.page.sub, app.shell.settings_open), (Sub::Settings, true));
        let _ = app.shell_update(Msg::Page(PageMsg::Back));
        assert!(!app.shell.settings_open);
    }

    #[test]
    fn the_section_list_stands_from_900_and_two_columns_from_1250() {
        let heights = [card_height(3), card_height(8), card_height(4), card_height(4), card_height(2), card_height(5)];
        for (width, list, columns) in [(640.0, false, 1), (899.0, false, 1), (900.0, true, 1), (1249.0, true, 1), (1250.0, true, 2), (1920.0, true, 2)] {
            let p = plan(width, &heights);
            assert_eq!((p.list, p.columns.len()), (list, columns), "at {width}");
            // every card once, in reading order, none split
            let flat: Vec<usize> = p.columns.iter().flatten().copied().collect();
            assert_eq!(flat, (0..heights.len()).collect::<Vec<_>>());
        }
        let two = plan(1400.0, &heights);
        assert!(!two.columns[0].is_empty() && !two.columns[1].is_empty());
        // the taller column is as short as the split allows
        let tall = |c: &Vec<usize>| c.iter().map(|i| heights[*i] + CARD_GAP).sum::<f32>();
        let best = (1..heights.len())
            .map(|k| (0..k).map(|i| heights[i] + CARD_GAP).sum::<f32>().max((k..heights.len()).map(|i| heights[i] + CARD_GAP).sum::<f32>()))
            .fold(f32::MAX, f32::min);
        assert_eq!(tall(&two.columns[0]).max(tall(&two.columns[1])), best);
        // one section is one column
        assert_eq!(plan(1400.0, &heights[..1]).columns.len(), 1);
        // where the sections start: the second column begins again at the top
        let at = starts(&two, &heights);
        assert_eq!(at[0], PAGE_TOP);
        assert_eq!(at[two.columns[1][0]], PAGE_TOP);
    }

    /// How the page laid itself out when drawn at `width`, and the frame
    fn drawn(width: f32, app: &App) -> (Plan, Vec<u8>) {
        let mut h = Harness::new(Size::new(width, 900.0));
        let _ = h.draw(app, "settings-layout-warm");
        let frame = h.draw(app, "settings-layout");
        let (_, heights) = shown_sections(app);
        (plan(app.shell.page.width(), &heights), frame)
    }

    #[test]
    fn the_page_lays_itself_out_by_the_room_it_has() {
        let app = settings_app(false);
        let (narrow, _) = drawn(800.0, &app);
        let (list, list_frame) = drawn(1000.0, &app);
        let (two, two_frame) = drawn(1400.0, &app);
        assert!(!narrow.list && narrow.columns.len() == 1, "{narrow:?}");
        assert!(list.list && list.columns.len() == 1, "{list:?}");
        assert!(two.list && two.columns.len() == 2, "{two:?}");
        // (the sidebar takes its room from the page)
        let with_sidebar = settings_app(true);
        let (beside, _) = drawn(1000.0, &with_sidebar);
        assert!(!beside.list, "a sidebar leaves 1000 px under the list's room: {beside:?}");
        // the section list is drawn: its current entry is washed, at the left of the page
        let px = |frame: &[u8], width: u32, x: u32, y: u32| -> [u8; 4] {
            let i = ((y * width + x) * 4) as usize;
            [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
        };
        let washed = |frame: &[u8], width: u32| (60..200).any(|y| px(frame, width, 100, y) != px(frame, width, 2, 800));
        assert!(washed(&list_frame, 1000), "the list shows its current section at 1000");
        assert!(washed(&two_frame, 1400));
    }

    #[test]
    fn jumping_to_a_section_marks_it_and_scrolling_follows_the_cards() {
        let _turn = ui_turn();
        plain_view_state();
        let mut app = settings_app(false);
        app.shell.page.width.store(1000.0f32.to_bits(), Ordering::Relaxed);
        let _ = app.shell_update(Msg::Page(PageMsg::Jump(Section::Code)));
        assert_eq!(app.shell.page.current, Some(Section::Code));
        let (sections, heights) = shown_sections(&app);
        let at = starts(&plan(1000.0, &heights), &heights);
        let y = |s: Section| at[sections.iter().position(|x| *x == s).unwrap()] - PAGE_TOP;
        assert_eq!(y(Section::Appearance), 0.0);
        for s in [Section::Paper, Section::Code, Section::Agents] {
            let _ = app.shell_update(Msg::Page(PageMsg::Scrolled(y(s) + 3.0, 700.0, 5000.0)));
            assert_eq!(app.shell.page.current, Some(s), "reading {s:?}");
        }
        let _ = app.shell_update(Msg::Page(PageMsg::Scrolled(4300.0, 700.0, 5000.0)));
        assert_eq!(app.shell.page.current, Some(Section::About), "at the bottom the last section is the one read");
        // in two columns the second column's first card is read at the top too
        app.shell.page.width.store(1400.0f32.to_bits(), Ordering::Relaxed);
        let p = plan(1400.0, &heights);
        let _ = app.shell_update(Msg::Page(PageMsg::Scrolled(0.0, 700.0, 5000.0)));
        assert!(sections.iter().position(|s| Some(*s) == app.shell.page.current).is_some_and(|i| p.columns.iter().any(|c| c[0] == i)));
    }

    #[test]
    fn each_kind_of_control_renders() {
        let fill = {
            let _turn = ui_turn();
            tones::muted(&cosmic::theme::active()).0
        };
        let app = settings_app(false);
        let count = |frame: &[u8], c: Color| {
            let [r, g, b, _] = c.into_rgba8();
            frame.chunks(4).filter(|p| p[0].abs_diff(r) < 3 && p[1].abs_diff(g) < 3 && p[2].abs_diff(b) < 3).count()
        };
        let msg = Msg::Theme(ThemeChoice::Light);
        let controls = [
            ("seg", Control::Seg { options: vec![("Match desktop".into(), msg.clone()), ("Light".into(), msg.clone()), ("Dark".into(), msg.clone())], selected: Some(1) }),
            ("drop", Control::Drop { options: vec!["One".into(), "Two".into()], selected: Some(0), msgs: vec![msg.clone(), msg.clone()] }),
            ("switch-on", Control::Switch { on: true, set: |_| Msg::Theme(ThemeChoice::Dark) }),
            ("switch-off", Control::Switch { on: false, set: |_| Msg::Theme(ThemeChoice::Dark) }),
            ("spin", Control::Spin { text: "100%".into(), value: 100, step: 10, min: 50, max: 300, set: Msg::TextSize }),
            ("text", Control::Text { placeholder: "Claude's default", value: String::new(), set: Msg::ClaudeModel }),
            ("open", Control::Open { label: "Show", msg: msg.clone() }),
            ("info", Control::Info("Some words".into())),
            ("machine", Control::Machine(0)),
        ];
        for (name, control) in controls {
            let element: Element<'_, Message> = widget::container(super::control(&app, control)).padding(8).into();
            let frame = draw_element(element, &format!("settings-control-{name}"), Size::new(360.0, 60.0));
            let first = [frame[0], frame[1], frame[2]];
            assert!(frame.chunks(4).any(|p| [p[0], p[1], p[2]] != first), "{name} draws something");
            match name {
                // the chosen segment and the switch that is on are the household's muted accent
                "seg" | "switch-on" => assert!(count(&frame, fill) > 100, "{name} is filled with the muted accent"),
                "switch-off" => assert_eq!(count(&frame, fill), 0, "an off switch has no accent"),
                _ => {}
            }
        }
    }

    #[test]
    fn a_page_with_search_changes_and_the_pages_of_their_own_draw() {
        let mut app = settings_app(false);
        // (the harness holds the UI's turn, so that the published settings are ours meanwhile)
        let mut h = Harness::new(Size::new(1400.0, 900.0));
        let _ = app.shell_update(Msg::Display(DisplayChange::MathScale(1.1)));
        let _ = app.shell_update(Msg::Page(PageMsg::Search("size".into())));
        let _ = h.draw(&app, "settings-search-warm");
        let frame = h.draw(&app, "settings-search");
        assert!(frame.chunks(4).any(|p| p[0] != frame[0]));
        let _ = app.shell_update(Msg::Page(PageMsg::Search("zzz".into())));
        let _ = h.draw(&app, "settings-none");
        let _ = app.shell_update(Msg::Page(PageMsg::Search(String::new())));
        for sub in [Sub::Shortcuts, Sub::About] {
            let _ = app.shell_update(Msg::Page(PageMsg::Open(sub)));
            let _ = h.draw(&app, &format!("settings-{sub:?}"));
        }
        let _ = app.shell_update(Msg::Display(DisplayChange::MathScale(1.05)));
    }

    /// The Settings page at the review widths, light and dark, with the sidebar as the demo has
    /// it and without, saved under `CT_SNAPSHOTS` as `settings-<width>-<light|dark>[-nosidebar].png`
    #[test]
    fn settings_frames() {
        for width in [1000.0, 1400.0] {
            for light in [true, false] {
                for sidebar in [true, false] {
                    let mut app = settings_app(sidebar);
                    let mut h = Harness::new(Size::new(width, 900.0));
                    if !sidebar {
                        // (a few rows changed from their defaults, so the dots show)
                        let _ = app.shell_update(Msg::Display(DisplayChange::MathScale(1.1)));
                        let _ = app.shell_update(Msg::Setting(Change::CodeSize(15)));
                    }
                    cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
                    let name = format!("settings-{}-{}{}", width as u32, if light { "light" } else { "dark" }, if sidebar { "" } else { "-nosidebar" });
                    let _ = h.draw(&app, &format!("{name}-warm"));
                    let frame = h.draw(&app, &name);
                    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
                    assert!(frame.chunks(4).any(|p| p[0] != frame[0]), "the page is not blank");
                    let _ = app.shell_update(Msg::Display(DisplayChange::MathScale(1.05)));
                    let _ = app.shell_update(Msg::Setting(Change::CodeSize(14)));
                }
            }
        }
    }
}

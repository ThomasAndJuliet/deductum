// SPDX-License-Identifier: GPL-3.0-only

//! What can be inserted at the caret, and the two places that offer it: the insert palette
//! beside the text when there is no goal to show there (prose being edited, a proof with no
//! state yet), and the insert bar at the top of the goal panel, above the first subgoal.
//!
//! Inside a proof, a definition's body or a theorem's statement there is almost always a goal
//! (or Lean's expected type) beside the text, so a palette that only takes the goal's place is
//! never there when a symbol or a step is wanted. The bar is: one row (a search field, the
//! entries most wanted at the caret, and More) that leaves the goals where they are; More opens
//! the whole palette in place, pushing the goals down.
//!
//! Both are built from the same entries ([`groups`]) and the same chips ([`chip`]), and both
//! insert with [`Message::Wrap`], which wraps a selection, keeps the editor's focus and caret
//! and is one undo step. What the caret is in ([`insert_context`]) comes from the profile:
//! [`Profile::formal_context`](super::prover::Profile::formal_context) in a formal block, and the
//! maths delimiters in prose.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::prelude::*;
use cosmic::{iced, theme, widget};

use super::codec::Symbols;
use super::editor::{Mode, Theory};
use super::prover::{InsertContext, PaletteEntry};
use super::theory::Kind;
use super::view::{muted, page_color, spacing, vim_mode_now, z};
use super::{Message, Workspace};

/// Whether the proof window is open, as the view last drew it: the palette then edits the block
/// in it, whichever view the main window shows
static POPOUT: AtomicBool = AtomicBool::new(false);

pub fn set_popout_open(open: bool) {
    POPOUT.store(open, Ordering::Relaxed);
}

/// Whether insertions go to the open block's editor (the literate view's, or the proof window's)
/// rather than the source view's editor
pub fn block_in_use(t: &Theory, popout: bool) -> bool {
    t.block_editor.is_some() && (t.mode == Mode::Literate || popout)
}

/// What prose before the caret leaves it in: maths (an unclosed `$` or `\(`), where symbols are
/// typed, or text, where markup is
fn prose_context(before: &str) -> InsertContext {
    let dollars = before.matches('$').count() - before.matches("\\$").count();
    let parens = before.matches("\\(").count() > before.matches("\\)").count();
    if dollars % 2 == 1 || parens { InsertContext::Symbols } else { InsertContext::Prose }
}

/// What the caret is in, for the palette and the bar: maths or a term (symbols), prose (markup
/// and antiquotations), or a place for a proof step; none where nothing is edited (the literate
/// view with no block open) or nothing can be inserted (a theory's header)
pub fn insert_context(t: &Theory) -> Option<InsertContext> {
    if block_in_use(t, POPOUT.load(Ordering::Relaxed)) {
        let be = t.block_editor.as_ref()?;
        // (the source as decoded, which the editor shows one character for one: maths shown set
        // still has its `$` here)
        let before: String = be.decoded.text.chars().take(be.caret()).collect();
        if be.wrap.is_some() {
            return Some(prose_context(&before));
        }
        return (be.kind == Kind::Formal).then(|| t.profile.formal_context(&before));
    }
    if t.mode != Mode::Source {
        return None;
    }
    // The source view: the block the caret is in, read up to the caret
    let caret = t.lines.offset(&t.decoded.text, t.source.cursor().position);
    let block = t.blocks.iter().find(|b| t.decoded.to_decoded(b.start) <= caret && caret <= t.decoded.to_decoded(b.end))?;
    let start = t.decoded.to_decoded(block.start);
    let before: String = t.decoded.text.chars().skip(start).take(caret - start).collect();
    match block.kind {
        Kind::Formal => Some(t.profile.formal_context(&before)),
        Kind::Text | Kind::Heading(_) | Kind::Comment => Some(prose_context(&before)),
        Kind::Header | Kind::End => None,
    }
}

/// The palette's list entries for the prose being edited: the profile's, for the caret's line
pub fn list_entries(t: &Theory, table: &dyn Symbols) -> Vec<PaletteEntry> {
    let Some(be) = t.block_editor.as_ref().filter(|be| be.wrap.is_some()) else { return Vec::new() };
    let before: String = be.decoded.text.chars().take(be.caret()).collect();
    t.profile.lists(before.rsplit('\n').next().unwrap_or(""), table)
}

/// An entry as offered: what it inserts, what it is called beyond its label (a symbol's name and
/// how it is typed), and whether its label is a symbol (set in the code font)
pub struct Item {
    pub entry: PaletteEntry,
    pub detail: String,
    pub symbol: bool,
}

/// Entries under a heading
pub struct Group {
    pub heading: &'static str,
    /// Whether its symbols show as their characters alone (the common ones), not with names
    pub compact: bool,
    pub items: Vec<Item>,
}

/// The query as typed, without the `\` symbols are typed with, in lower case
fn normalised(query: &str) -> String {
    query.trim().trim_start_matches('\\').to_lowercase()
}

/// Each symbol's name, by character, from the table's groups (the Symbols panel's)
fn symbol_names(table: &dyn Symbols) -> HashMap<char, String> {
    let mut names = HashMap::new();
    for (_, syms) in table.groups() {
        for (c, name) in syms {
            names.entry(c).or_insert(name);
        }
    }
    names
}

/// A profile's entry as offered: among symbols, one labelled with a symbol (or a pair of
/// brackets) is set in the code font, its name (the first character's) its detail. Elsewhere an
/// entry is a step or markup, whatever its label (Lean's `·` is a step)
fn as_item(entry: PaletteEntry, names: &HashMap<char, String>, context: InsertContext) -> Item {
    let first = entry.label.chars().next();
    let symbol = context == InsertContext::Symbols && first.is_some_and(|c| !c.is_ascii()) && entry.label.chars().count() <= 2;
    let detail = if symbol { first.and_then(|c| names.get(&c).cloned()).unwrap_or_default() } else { String::new() };
    Item { entry, detail, symbol }
}

/// Symbols matching a search: by name, abbreviation or LaTeX name (the table's search), or the
/// character itself when the query has one (`∀` finds ∀)
fn symbol_hits(table: &dyn Symbols, query: &str, limit: usize) -> Vec<Item> {
    let item = |c: char, name: String, how: String| Item {
        entry: PaletteEntry::new(&c.to_string(), &c.to_string(), ""),
        detail: if how.is_empty() { name } else { format!("{name} {how}") },
        symbol: true,
    };
    let mut out: Vec<Item> = Vec::new();
    if query.chars().any(|c| !c.is_ascii()) {
        let names = symbol_names(table);
        for c in query.chars().filter(|c| !c.is_ascii()) {
            out.push(item(c, names.get(&c).cloned().unwrap_or_default(), String::new()));
        }
    } else {
        out.extend(table.search(query, limit).into_iter().map(|(c, name, how)| item(c, name, how)));
    }
    out.truncate(limit);
    out
}

/// The entries for a context, grouped as the palette shows them: with no search, lists (in
/// prose) and the context's own entries; with one, the entries whose labels match and the
/// symbols that match. Empty groups are left out
pub fn groups(ws: &Workspace, t: &Theory, table: &dyn Symbols, context: InsertContext, query: &str) -> Vec<Group> {
    let query = normalised(query);
    let names = symbol_names(table);
    let item = |e: PaletteEntry| as_item(e, &names, context);
    let matches = |e: &PaletteEntry| query.is_empty() || e.label.to_lowercase().contains(&query);
    let mut groups = Vec::new();
    match context {
        InsertContext::Symbols if !query.is_empty() => {
            // (the bar's own entries first, by label or name: a pair of brackets is one of them)
            let mut items: Vec<Item> = t.profile.quick_entries(context, &ws.caps).into_iter().map(item).filter(|i| i.entry.label.to_lowercase().contains(&query) || i.detail.to_lowercase().contains(&query)).collect();
            for hit in symbol_hits(table, &query, 40) {
                if !items.iter().any(|i| i.entry.before == hit.entry.before) {
                    items.push(hit);
                }
            }
            groups.push(Group { heading: "Symbols", compact: false, items });
        }
        InsertContext::Symbols => groups.push(Group { heading: "Common symbols", compact: true, items: t.profile.palette(context, &ws.caps).into_iter().map(item).collect() }),
        InsertContext::Prose | InsertContext::Proof => {
            // Lists: in one, its next item (and a nested one); elsewhere, a list to begin
            if context == InsertContext::Prose {
                groups.push(Group { heading: "Lists", compact: false, items: list_entries(t, table).into_iter().filter(matches).map(item).collect() });
            }
            let heading = if context == InsertContext::Prose { "Markup and antiquotations" } else { "Steps and tools" };
            groups.push(Group { heading, compact: false, items: t.profile.palette(context, &ws.caps).into_iter().filter(matches).map(item).collect() });
            if !query.is_empty() {
                groups.push(Group { heading: "Symbols", compact: false, items: symbol_hits(table, &query, 12) });
            }
        }
    }
    groups.retain(|g| !g.items.is_empty());
    groups
}

/// The entries the bar shows on its row: with no search, the profile's quick entries for the
/// context; with one, the best matches of every group, in the groups' order
fn quick_items(ws: &Workspace, t: &Theory, table: &dyn Symbols, context: InsertContext) -> Vec<Item> {
    if normalised(&ws.palette_query).is_empty() {
        let names = symbol_names(table);
        return t.profile.quick_entries(context, &ws.caps).into_iter().map(|e| as_item(e, &names, context)).collect();
    }
    groups(ws, t, table, context, &ws.palette_query).into_iter().flat_map(|g| g.items).collect()
}

/// An entry as a button that inserts it at the caret (around the selection, if the entry has
/// something to put after it). A symbol shown `compact` (on the bar's row, among the common
/// symbols) is its character alone, its name in a tooltip; found by a search, its character and
/// its name
pub fn chip<'a>(item: Item, compact: bool) -> Element<'a, Message> {
    let wrap = Message::Wrap(item.entry.before, item.entry.after);
    // A symbol on its own: its character, large enough to read, on a chip of one width so that
    // a row of them is even
    let glyph = item.symbol && (compact || item.detail.is_empty());
    let button = if glyph {
        widget::button::custom(
            widget::text(item.entry.label.clone())
                .size(z(16.0))
                .font(cosmic::font::mono())
                .wrapping(iced::widget::text::Wrapping::None)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .padding([z(3.0), z(2.0)])
        .width(Length::Fixed(z(if item.entry.label.chars().count() > 1 { 36.0 } else { 28.0 })))
    } else {
        let label = if compact || item.detail.is_empty() { item.entry.label.clone() } else { format!("{}  {}", item.entry.label, item.detail) };
        widget::button::custom(widget::text(label).size(z(13.0)).font(if item.symbol { cosmic::font::mono() } else { cosmic::font::default() }).wrapping(iced::widget::text::Wrapping::None)).padding([z(4.0), z(9.0)])
    };
    let button = button.class(theme::Button::Standard).on_press(wrap);
    // (its name, when only its character shows, in a tooltip)
    if glyph && !item.detail.is_empty() {
        widget::tooltip(button, widget::text(item.detail).font(cosmic::font::mono()).size(z(13.0)), widget::tooltip::Position::Bottom).into()
    } else {
        button.into()
    }
}

/// The groups as the palette lists them: each heading, then its chips, wrapping
fn grouped<'a>(groups: Vec<Group>) -> Element<'a, Message> {
    let sp = spacing();
    let mut col = widget::column::with_capacity(2 * groups.len()).spacing(sp.space_xxs);
    for g in groups {
        col = col.push(widget::container(widget::text::caption_heading(g.heading).class(muted())).padding(Padding { top: sp.space_xxs as f32, ..Padding::ZERO }));
        let chips: Vec<Element<'a, Message>> = g.items.into_iter().map(|i| chip(i, g.compact)).collect();
        col = col.push(widget::flex_row(chips).row_spacing(sp.space_xxs).column_spacing(sp.space_xxs));
    }
    col.into()
}

/// Whether the selection is text to wrap (vim's block cursor selects the character under it,
/// which is not)
fn selected(t: &Theory) -> bool {
    let block_cursor = vim_mode_now() == Some(super::vim::Mode::Normal);
    let content = match &t.block_editor {
        Some(be) if block_in_use(t, POPOUT.load(Ordering::Relaxed)) => &be.content,
        _ => &t.source,
    };
    !block_cursor && content.selection().is_some_and(|s| !s.trim().is_empty())
}

/// The palette's search field, its clear button only when there is something to clear (on the
/// bar's row the room is the chips')
fn search_field<'a>(placeholder: &'static str, query: &'a str) -> widget::TextInput<'a, Message> {
    let input = widget::search_input(placeholder, query).on_input(Message::PaletteQuery);
    if query.is_empty() { input } else { input.on_clear(Message::PaletteQuery(String::new())) }
}

/// Beside the block being edited, when there is no goal: what can be inserted at the caret, by
/// context, searchable, under a title saying where it goes
pub fn insert_palette<'a>(ws: &'a Workspace, t: &'a Theory, table: &dyn Symbols, width: f32) -> Element<'a, Message> {
    let sp = spacing();
    let context = insert_context(t).unwrap_or(InsertContext::Prose);
    let selected = selected(t);
    let mut col = widget::column::with_capacity(4).spacing(sp.space_xxs);
    let (title, hint) = match context {
        InsertContext::Symbols => ("Insert a symbol", "Or type \\ and its name, then Tab"),
        InsertContext::Prose if selected => ("Insert in the text", "Wraps the selection"),
        InsertContext::Prose => ("Insert in the text", "At the caret, ready to type into"),
        InsertContext::Proof if selected => ("Insert in the proof", "Wraps the selection"),
        InsertContext::Proof => ("Insert in the proof", "At the caret"),
    };
    col = col.push(widget::row::with_capacity(2).align_y(Alignment::Center).push(widget::text::heading(title).width(Length::Fill)).push(widget::text::caption(hint).class(muted())));
    col = col.push(widget::container(search_field("Search", &ws.palette_query)).padding([sp.space_xxxs, 0]));
    let groups = groups(ws, t, table, context, &ws.palette_query);
    if groups.is_empty() {
        col = col.push(widget::container(widget::text::body(format!("Nothing matches “{}”.", ws.palette_query.trim()))).padding([sp.space_xxs, 0]));
    }
    col = col.push(grouped(groups));
    // (on the page, outlined: a panel of its own beside the text, not a part of it)
    widget::container(col)
        .padding(sp.space_s)
        .width(Length::Fixed(width))
        .class(theme::Container::custom(|theme| iced::widget::container::Style {
            background: Some(page_color(theme).into()),
            border: Border { radius: 8.0.into(), width: 1.0, color: Color { a: 0.14, ..Color::from(theme.cosmic().on_bg_color()) } },
            ..Default::default()
        }))
        .into()
}

pub fn insert_bar_id() -> widget::Id {
    widget::Id::new("deductum-insert-bar")
}

/// The insert bar at the top of the goal panel: a search field, as many of the entries most
/// wanted at the caret as fit on the row, and More, which opens the whole palette under the row
/// (the goals move down only then). None where nothing can be inserted
pub fn insert_bar<'a>(ws: &'a Workspace, table: &dyn Symbols) -> Option<Element<'a, Message>> {
    let t = ws.theory.as_ref()?;
    let context = insert_context(t)?;
    let sp = spacing();
    // (what the row offers, said by its search field while it is empty)
    let placeholder = match context {
        InsertContext::Symbols => "Symbol",
        InsertContext::Prose => "Markup",
        InsertContext::Proof => "Step",
    };
    let search = widget::container(search_field(placeholder, &ws.palette_query)).width(Length::Fixed(z(if ws.palette_query.is_empty() { 110.0 } else { 160.0 })));
    let items = quick_items(ws, t, table, context);
    let none = items.is_empty();
    let chips: Vec<Element<'a, Message>> = items.into_iter().map(|i| chip(i, true)).collect();
    let row_chips: Element<'a, Message> = if none {
        widget::text::caption(format!("Nothing matches “{}”", ws.palette_query.trim())).class(muted()).width(Length::Fill).into()
    } else {
        super::gap::OneRow::new(chips, z(3.0)).into()
    };
    let more = widget::button::icon(widget::icon::from_name(if ws.insert_open { "pan-up-symbolic" } else { "pan-down-symbolic" }))
        .tooltip(if ws.insert_open { "Fewer (just this row)" } else { "More: everything that can be inserted here" })
        .on_press(Message::InsertBar(!ws.insert_open));
    let row = widget::row::with_capacity(3).spacing(sp.space_xs).align_y(Alignment::Center).push(search).push(widget::container(row_chips).width(Length::Fill).clip(true)).push(more);
    let mut col = widget::column::with_capacity(2).spacing(sp.space_xxs).push(row);
    if ws.insert_open {
        let groups = groups(ws, t, table, context, &ws.palette_query);
        if !groups.is_empty() {
            let hint = match context {
                InsertContext::Symbols => "Or type \\ and its name, then Tab",
                _ if selected(t) => "Wraps the selection",
                _ => "At the caret",
            };
            col = col.push(widget::text::caption(hint).class(muted()));
            col = col.push(grouped(groups));
        }
    }
    // (a band of its own above the state, faintly set off from it)
    Some(
        widget::container(col)
            .id(insert_bar_id())
            .padding([z(4.0), z(6.0)])
            .width(Length::Fill)
            .class(theme::Container::custom(|theme| iced::widget::container::Style {
                background: Some(Color { a: if theme.cosmic().is_dark { 0.05 } else { 0.06 }, ..Color::from(theme.cosmic().on_bg_color()) }.into()),
                border: Border { radius: 8.0.into(), ..Default::default() },
                ..Default::default()
            }))
            .into(),
    )
}

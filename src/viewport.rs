// SPDX-License-Identifier: GPL-3.0-only

//! A retained document column. The model supplies row identities and cheap
//! height estimates; widgets are constructed only near the viewport or for
//! pinned interaction targets. Offscreen rows keep geometry, not widget views.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

pub type Indices = Arc<Mutex<Vec<usize>>>;

#[cfg(test)]
static LAST_BUILT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub fn last_built_count() -> usize {
    LAST_BUILT.load(std::sync::atomic::Ordering::Relaxed)
}

use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::advanced::widget::{Operation, Tree, tree};
use cosmic::Element;
use cosmic::iced::{Event, Length, Padding, Rectangle, Size, Vector};
use prose_render::layout::HeightIndex;

#[derive(Clone, Debug)]
pub struct Row {
    pub key: u64,
    pub characters: usize,
    pub lines: usize,
    pub kind: super::theory::Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Content(u64),
    Editor,
    Footer(usize),
}

struct State {
    scope: u64,
    source: Arc<Vec<Row>>,
    keys: Vec<Key>,
    heights: HeightIndex,
    width: u32,
    style: u64,
    active: Option<usize>,
    viewport: (f32, f32),
    external_viewport: (f32, f32),
    pending_adjust: f32,
    pending_origin: Option<f32>,
    built: Vec<usize>,
}

impl Default for State {
    fn default() -> Self {
        Self { scope: 0, source: Arc::new(Vec::new()), keys: Vec::new(), heights: HeightIndex::new(Vec::new()),
            width: 0, style: 0, active: None, viewport: (0.0, 800.0), external_viewport: (0.0, 0.0),
            pending_adjust: 0.0, pending_origin: None, built: Vec::new() }
    }
}

pub struct Column<'a, Message> {
    scope: u64,
    source: Arc<Vec<Row>>,
    style: u64,
    active: Option<usize>,
    pinned: Vec<usize>,
    viewport: (f32, f32),
    padding: Padding,
    build: Box<dyn Fn(usize) -> Element<'a, Message> + 'a>,
    estimate: Box<dyn Fn(usize, f32) -> f32 + 'a>,
    adjust: Box<dyn Fn(f32) -> Message + 'a>,
    indices: Indices,
    rows: Vec<(usize, Element<'a, Message>)>,
}

impl<'a, Message> Column<'a, Message> {
    pub fn new(
        indices: Indices,
        scope: u64,
        source: Arc<Vec<Row>>,
        style: u64,
        viewport: (f32, f32),
        build: impl Fn(usize) -> Element<'a, Message> + 'a,
        estimate: impl Fn(usize, f32) -> f32 + 'a,
        adjust: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self { indices, scope, source, style, active: None, pinned: Vec::new(), viewport, padding: Padding::ZERO,
            build: Box::new(build), estimate: Box::new(estimate), adjust: Box::new(adjust), rows: Vec::new() }
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn pin(mut self, index: Option<usize>) -> Self {
        self.pinned.extend(index);
        self
    }

    pub fn editor(mut self, index: Option<usize>) -> Self {
        self.active = index;
        self.pinned.extend(index);
        self
    }

    fn key(&self, i: usize) -> Key {
        if self.active == Some(i) { Key::Editor }
        else { self.source.get(i).map_or_else(|| Key::Footer(i - self.source.len()), |row| Key::Content(row.key)) }
    }

    fn wanted(&self, state: &State) -> Vec<usize> {
        let len = self.source.len() + 2;
        if self.source.len() <= 64 {
            return (0..len).collect();
        }
        let (top, height) = state.viewport;
        let height = height.max(400.0);
        let mut wanted: Vec<_> = state.heights.visible_range((top - height).max(0.0), top + 2.0 * height).collect();
        wanted.extend(self.pinned.iter().copied().filter(|i| *i < len));
        wanted.sort_unstable();
        wanted.dedup();
        wanted
    }
}

impl<Message> Widget<Message, cosmic::Theme, cosmic::Renderer> for Column<'_, Message> {
    fn tag(&self) -> tree::Tag { tree::Tag::of::<State>() }
    fn state(&self) -> tree::State { tree::State::new(State::default()) }
    fn children(&self) -> Vec<Tree> { Vec::new() }
    fn diff(&mut self, _tree: &mut Tree) {}
    fn size(&self) -> Size<Length> { Size::new(Length::Fill, Length::Shrink) }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let inner = (width - self.padding.left - self.padding.right).max(1.0);
        let state = tree.state.downcast_mut::<State>();
        if state.scope != self.scope {
            *state = State::default();
            state.scope = self.scope;
            tree.children.clear();
        }
        if state.external_viewport != self.viewport {
            state.external_viewport = self.viewport;
            state.viewport = (self.viewport.0.max(0.0), self.viewport.1.max(400.0));
        }
        let frame_changed = state.width != inner.to_bits() || state.style != self.style;
        let source_changed = !Arc::ptr_eq(&state.source, &self.source) || state.active != self.active;
        let anchor = state.heights.index_at(state.viewport.0).and_then(|i| {
            Some((i, *state.keys.get(i)?, state.viewport.0 - state.heights.top(i)))
        });
        if frame_changed || source_changed {
            let mut kept: HashMap<Key, VecDeque<f32>> = HashMap::new();
            for (i, key) in state.keys.iter().enumerate() {
                kept.entry(*key).or_default().push_back(state.heights.get(i).unwrap_or(0.0));
            }
            let old_keys = state.keys.clone();
            let mut retained: HashMap<Key, Tree> = std::mem::take(&mut tree.children).into_iter()
                .zip(state.built.iter()).filter_map(|(child, i)| old_keys.get(*i).map(|key| (*key, child))).collect();
            state.keys = (0..self.source.len() + 2).map(|i| self.key(i)).collect();
            let heights = state.keys.iter().enumerate().map(|(i, key)| {
                if !frame_changed { kept.get_mut(key).and_then(VecDeque::pop_front).unwrap_or_else(|| (self.estimate)(i, inner)) }
                else { (self.estimate)(i, inner) }
            }).collect();
            let mut built = Vec::new();
            for (i, key) in state.keys.iter().enumerate() {
                if let Some(child) = retained.remove(key) { tree.children.push(child); built.push(i); }
            }
            state.built = built;
            state.heights = HeightIndex::new(heights);
            state.source = self.source.clone();
            state.active = self.active;
            state.width = inner.to_bits();
            state.style = self.style;
        }
        self.rows.clear();
        let wanted = self.wanted(state);
        let mut retained: HashMap<usize, Tree> = state.built.iter().copied().zip(std::mem::take(&mut tree.children)).collect();
        for i in wanted.iter().copied() {
            let mut row = (self.build)(i);
            let mut child = retained.remove(&i).unwrap_or_else(Tree::empty);
            child.diff(row.as_widget_mut());
            tree.children.push(child);
            self.rows.push((i, row));
        }
        let child_limits = layout::Limits::new(Size::ZERO, Size::new(inner, f32::INFINITY));
        let mut laid = Vec::with_capacity(self.rows.len());
        for ((i, row), child) in self.rows.iter_mut().zip(&mut tree.children) {
            let node = row.as_widget_mut().layout(child, renderer, &child_limits);
            state.heights.set(*i, node.size().height);
            laid.push(node);
        }
        // Heights before the visible source may change after an edit or resize.
        // Keep that source at its old distance below the viewport's top.
        if let Some((old_index, key, within)) = anchor
            && let Some(i) = if source_changed { state.keys.iter().position(|k| *k == key) } else { Some(old_index) }
        {
            let target = state.heights.top(i) + within;
            let delta = target - state.viewport.0;
            if delta.abs() > 0.5 && state.viewport.0 > 0.0 {
                if state.pending_origin.is_none() { state.pending_origin = Some(state.viewport.0); }
                state.pending_adjust += delta;
                state.viewport.0 = target.max(0.0);
            }
        }
        state.built = wanted;
        #[cfg(test)]
        LAST_BUILT.store(state.built.len(), std::sync::atomic::Ordering::Relaxed);
        *self.indices.lock().unwrap_or_else(|e| e.into_inner()) = state.built.clone();
        let children = laid.into_iter().zip(&state.built).map(|(node, i)| {
            node.move_to((self.padding.left, self.padding.top + state.heights.top(*i)))
        }).collect();
        layout::Node::with_children(Size::new(width, self.padding.top + state.heights.total() + self.padding.bottom), children)
    }

    fn update(&mut self, tree: &mut Tree, event: &Event, layout: Layout<'_>, cursor: mouse::Cursor,
        renderer: &cosmic::Renderer, clipboard: &mut dyn Clipboard, shell: &mut Shell<'_, Message>, viewport: &Rectangle) {
        let state = tree.state.downcast_mut::<State>();
        let actual = ((viewport.y - layout.bounds().y - self.padding.top).max(0.0), viewport.height);
        if actual.1.is_finite() && actual.1 > 0.0 {
            state.viewport = actual;
        }
        let adjustment = std::mem::take(&mut state.pending_adjust);
        let origin = state.pending_origin.take();
        if adjustment.abs() > 0.5 && origin.is_some_and(|origin| (actual.0 - origin).abs() <= 1.0) {
            shell.publish((self.adjust)(adjustment));
            shell.request_redraw();
        }
        if self.wanted(state) != state.built {
            shell.invalidate_layout();
            shell.request_redraw();
        }
        for (slot, (_, row)) in self.rows.iter_mut().enumerate() {
            let Some(child) = layout.children().nth(slot) else { continue };
            row.as_widget_mut().update(&mut tree.children[slot], event, child, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn draw(&self, tree: &Tree, renderer: &mut cosmic::Renderer, theme: &cosmic::Theme,
        style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        for (slot, (_, row)) in self.rows.iter().enumerate() {
            if let Some(child) = layout.children().nth(slot)
                && child.bounds().intersection(viewport).is_some()
            {
                row.as_widget().draw(&tree.children[slot], renderer, theme, style, child, cursor, viewport);
            }
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle,
        renderer: &cosmic::Renderer) -> mouse::Interaction {
        self.rows.iter().enumerate().filter_map(|(slot, (_, row))| {
            let child = layout.children().nth(slot)?;
            child.bounds().intersection(viewport)?;
            Some(row.as_widget().mouse_interaction(&tree.children[slot], child, cursor, viewport, renderer))
        }).max().unwrap_or_default()
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &cosmic::Renderer, operation: &mut dyn Operation) {
        operation.traverse(&mut |operation| {
            for (slot, (_, row)) in self.rows.iter_mut().enumerate() {
                if let Some(child) = layout.children().nth(slot) {
                    row.as_widget_mut().operate(&mut tree.children[slot], child, renderer, operation);
                }
            }
        });
    }

    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &cosmic::Renderer,
        viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        let mut overlays = Vec::new();
        let mut children = tree.children.iter_mut().enumerate();
        for (slot, (_, row)) in self.rows.iter_mut().enumerate() {
            let (_, child_tree) = children.next()?;
            if let Some(child) = layout.children().nth(slot)
                && let Some(element) = row.as_widget_mut().overlay(child_tree, child, renderer, viewport, translation)
            {
                overlays.push(element);
            }
        }
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).into())
    }
}

impl<'a, Message: 'a> From<Column<'a, Message>> for Element<'a, Message> {
    fn from(column: Column<'a, Message>) -> Self { Element::new(column) }
}

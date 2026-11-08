// SPDX-License-Identifier: GPL-3.0-only

//! An editor with a gap between two of its lines, holding another element (the proof state, just
//! after the construct the caret is in). The editor stays one editor: above the gap it is drawn
//! as it is, below the gap it is drawn moved down by the gap's height, and the pointer's position
//! below the gap is moved back up before the editor sees it, so clicks, drags and hovers land on
//! the characters drawn under them.

use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{self, Operation, Tree, Widget};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::{Element, Event, Length, Rectangle, Size, Vector};

pub struct Gap<'a, Message, Theme, Renderer> {
    editor: Element<'a, Message, Theme, Renderer>,
    inset: Element<'a, Message, Theme, Renderer>,
    /// Where the gap opens, below the editor's top
    at: f32,
    /// Bands of the editor's text tinted (from, to, below its top; the colour), laid one over the
    /// other in the order given: the construct the proof state is for, then the caret's line
    washes: Vec<(f32, f32, cosmic::iced::Color)>,
    /// How far the bands reach past the editor's left edge, to the left edge of the card the
    /// editor is padded inside (they run edge to edge; the text does not move)
    reach: f32,
}

impl<'a, Message, Theme, Renderer> Gap<'a, Message, Theme, Renderer> {
    pub fn new(editor: impl Into<Element<'a, Message, Theme, Renderer>>, inset: impl Into<Element<'a, Message, Theme, Renderer>>, at: f32) -> Self {
        Self { editor: editor.into(), inset: inset.into(), at, washes: Vec::new(), reach: 0.0 }
    }

    /// Tint the editor's text from `from` to `to` below its top
    pub fn band(mut self, from: f32, to: f32, color: cosmic::iced::Color) -> Self {
        self.washes.push((from, to, color));
        self
    }

    /// Let the bands reach `left` past the editor's left edge
    pub fn reach(mut self, left: f32) -> Self {
        self.reach = left;
        self
    }
}

/// Where things are, from the layout: the editor's bounds, the inset's, where the gap opens and
/// how tall it is
fn parts(layout: Layout<'_>) -> (Rectangle, Rectangle, f32, f32) {
    let mut children = layout.children();
    let editor = children.next().map(|l| l.bounds()).unwrap_or_default();
    let inset = children.next().map(|l| l.bounds()).unwrap_or_default();
    (editor, inset, inset.y - editor.y, inset.height)
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Gap<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.editor), Tree::new(&self.inset)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.editor, &mut self.inset]);
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.editor.as_widget().size().width, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let editor = self.editor.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let width = editor.size().width;
        let inset = self.inset.as_widget_mut().layout(&mut tree.children[1], renderer, &layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY)));
        let at = self.at.clamp(0.0, editor.size().height);
        let gap = inset.size().height;
        let size = Size::new(width, editor.size().height + gap);
        layout::Node::with_children(size, vec![editor, inset.move_to((0.0, at))])
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let mut children = layout.children();
        let (Some(editor_layout), Some(inset_layout)) = (children.next(), children.next()) else { return };
        let (editor, _, at, gap) = parts(layout);
        let moved = |c: mouse::Cursor| match c.position() {
            Some(p) if p.y >= editor.y + at + gap => mouse::Cursor::Available(p - Vector::new(0.0, gap)),
            Some(p) if p.y >= editor.y + at => mouse::Cursor::Unavailable,
            _ => c,
        };
        // The construct's band, over the text (where the gap opens in it, its lower part moves
        // down), and under a selection: in a layer of its own made inside the editor's, after the
        // layer that takes the editor's background and text and before those the editor makes
        // for its selection (layers are drawn in the order they are made), so a selection, which
        // libcosmic draws opaque in layers of its own, lies over the band
        let reach = self.reach;
        // (the layers reach left too, as the bands do)
        let wide = |r: Rectangle| Rectangle { x: r.x - reach, width: r.width + reach, ..r };
        let bands = |r: &mut Renderer, below: bool| {
            for (from, to, color) in &self.washes {
                // (no further than the editor's own bottom)
                let to = to.min(editor.height);
                let (y0, y1) = if below { (editor.y + from.max(at) + gap, editor.y + to.max(at) + gap) } else { (editor.y + from.min(at), editor.y + to.min(at)) };
                if y1 > y0 {
                    let bounds = wide(Rectangle { y: y0, height: y1 - y0, ..editor });
                    r.with_layer(bounds, |r| {
                        r.fill_quad(renderer::Quad { bounds, ..Default::default() }, cosmic::iced::Background::Color(*color));
                    });
                }
            }
        };
        // Above the gap: as it is
        renderer.with_layer(wide(Rectangle { height: at, ..editor }), |r| {
            bands(r, false);
            self.editor.as_widget().draw(&tree.children[0], r, theme, style, editor_layout, moved(cursor), viewport);
        });
        // Below it: moved down by the gap
        renderer.with_layer(wide(Rectangle { y: editor.y + at + gap, height: (editor.height - at).max(0.0), ..editor }), |r| {
            bands(r, true);
            r.with_translation(Vector::new(0.0, gap), |r| {
                self.editor.as_widget().draw(&tree.children[0], r, theme, style, editor_layout, moved(cursor), viewport);
            });
        });
        self.inset.as_widget().draw(&tree.children[1], renderer, theme, style, inset_layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut children = layout.children();
        let (Some(editor_layout), Some(inset_layout)) = (children.next(), children.next()) else { return };
        let (editor, _, at, gap) = parts(layout);
        // The inset first: over the gap, the pointer is its
        self.inset.as_widget_mut().update(&mut tree.children[1], event, inset_layout, cursor, renderer, clipboard, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        let editor_cursor = match cursor.position() {
            Some(p) if p.y >= editor.y + at + gap => mouse::Cursor::Available(p - Vector::new(0.0, gap)),
            // over the gap: not over the editor (a click there is not in the text)
            Some(p) if p.y >= editor.y + at => mouse::Cursor::Available(cosmic::iced::Point::new(p.x, f32::MAX / 4.0)),
            _ => cursor,
        };
        self.editor.as_widget_mut().update(&mut tree.children[0], event, editor_layout, editor_cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let mut children = layout.children();
        let (Some(editor_layout), Some(inset_layout)) = (children.next(), children.next()) else { return mouse::Interaction::None };
        let (editor, _, at, gap) = parts(layout);
        match cursor.position() {
            Some(p) if p.y >= editor.y + at && p.y < editor.y + at + gap => {
                self.inset.as_widget().mouse_interaction(&tree.children[1], inset_layout, cursor, viewport, renderer)
            }
            Some(p) if p.y >= editor.y + at + gap => self.editor.as_widget().mouse_interaction(
                &tree.children[0],
                editor_layout,
                mouse::Cursor::Available(p - Vector::new(0.0, gap)),
                viewport,
                renderer,
            ),
            _ => self.editor.as_widget().mouse_interaction(&tree.children[0], editor_layout, cursor, viewport, renderer),
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let mut children = layout.children();
        let (Some(editor_layout), Some(inset_layout)) = (children.next(), children.next()) else { return };
        operation.traverse(&mut |operation| {
            self.editor.as_widget_mut().operate(&mut tree.children[0], editor_layout, renderer, operation);
            self.inset.as_widget_mut().operate(&mut tree.children[1], inset_layout, renderer, operation);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut children = layout.children();
        let (editor_layout, inset_layout) = (children.next()?, children.next()?);
        let (editor_tree, inset_tree) = tree.children.split_at_mut(1);
        let editor = self.editor.as_widget_mut().overlay(&mut editor_tree[0], editor_layout, renderer, viewport, translation);
        let inset = self.inset.as_widget_mut().overlay(&mut inset_tree[0], inset_layout, renderer, viewport, translation);
        match (editor, inset) {
            (Some(a), Some(b)) => Some(overlay::Group::with_children(vec![a, b]).into()),
            (a, b) => a.or(b),
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Gap<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(gap: Gap<'a, Message, Theme, Renderer>) -> Self {
        Element::new(gap)
    }
}

#[allow(dead_code)]
fn _uses(_: &dyn Fn(&mut widget::Tree)) {}

/// An element with another beside it, at an offset from its top left, drawn over whatever is
/// there without taking room: the page does not move for it (as it would for a column of its
/// own), and it scrolls with the page, level with what it belongs to (an overlay is kept inside
/// the window instead, and jumps up when it does not fit below). It moves only as far as it
/// must to stay in sight: up when it would run off the bottom (not above the element's top or
/// the view's), down when what it belongs to has scrolled up out of the view
pub struct Beside<'a, Message, Theme, Renderer> {
    main: Element<'a, Message, Theme, Renderer>,
    side: Element<'a, Message, Theme, Renderer>,
    at: Vector,
    /// `at.x` from the main element's right edge, inward, rather than from its left
    right: bool,
    /// Whether it moves to stay in sight (a goal does; a button on a block stays on it)
    follow: bool,
}

impl<'a, Message, Theme, Renderer> Beside<'a, Message, Theme, Renderer> {
    pub fn new(main: impl Into<Element<'a, Message, Theme, Renderer>>, side: impl Into<Element<'a, Message, Theme, Renderer>>, at: Vector) -> Self {
        Self { main: main.into(), side: side.into(), at, right: false, follow: true }
    }

    /// Placed from the main element's top right corner (inward by `at.x`), as laid out
    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }

    /// Where it is put, scrolling with the main element (not moving to stay in sight)
    pub fn fixed(mut self) -> Self {
        self.follow = false;
        self
    }
}

/// How far the side moves from its place to stay in sight
fn nudge(main: Rectangle, side: Rectangle, viewport: &Rectangle) -> f32 {
    const MARGIN: f32 = 8.0;
    let (top, bottom) = (viewport.y + MARGIN, viewport.y + viewport.height - MARGIN);
    let mut y = side.y;
    if y + side.height > bottom {
        y = (bottom - side.height).max(top.max(main.y).min(side.y));
    }
    if y < top {
        // (scrolled past its place: follows down, while what it belongs to is still in sight)
        y = top.min((main.y + main.height - side.height.min(48.0)).max(side.y));
    }
    y - side.y
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Beside<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.main), Tree::new(&self.side)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.main, &mut self.side]);
    }

    fn size(&self) -> Size<Length> {
        self.main.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let main = self.main.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let side = self.side.as_widget_mut().layout(&mut tree.children[1], renderer, &layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY)));
        let x = if self.right { main.size().width - side.size().width - self.at.x } else { self.at.x };
        // (what stays in sight may be the goal, which the end of the page must not cut off)
        let reach = if self.follow { self.at.y + side.size().height + GOAL_ROOM } else { 0.0 };
        let size = Size::new(main.size().width, main.size().height.max(reach));
        layout::Node::with_children(size, vec![main, side.move_to((x, self.at.y))])
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let mut children = layout.children();
        let (Some(main), Some(side)) = (children.next(), children.next()) else { return };
        self.main.as_widget().draw(&tree.children[0], renderer, theme, style, main, cursor, viewport);
        let dy = if self.follow { nudge(main.bounds(), side.bounds(), viewport) } else { 0.0 };
        let cursor = match cursor.position() {
            Some(p) => mouse::Cursor::Available(p - Vector::new(0.0, dy)),
            None => cursor,
        };
        // (in a layer of its own: over what the main element draws in its layers, such as an editor)
        renderer.with_layer(*viewport, |r| {
            r.with_translation(Vector::new(0.0, dy), |r| {
                self.side.as_widget().draw(&tree.children[1], r, theme, style, side, cursor, &Rectangle { y: viewport.y - dy, ..*viewport });
            });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut children = layout.children();
        let (Some(main), Some(side)) = (children.next(), children.next()) else { return };
        let dy = if self.follow { nudge(main.bounds(), side.bounds(), viewport) } else { 0.0 };
        let moved = |c: mouse::Cursor| match c.position() {
            Some(p) => mouse::Cursor::Available(p - Vector::new(0.0, dy)),
            None => c,
        };
        self.side.as_widget_mut().update(&mut tree.children[1], event, side, moved(cursor), renderer, clipboard, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        // (over the side, the pointer is not over what is under it)
        let main_cursor = match moved(cursor).position() {
            Some(p) if side.bounds().contains(p) => mouse::Cursor::Unavailable,
            _ => cursor,
        };
        self.main.as_widget_mut().update(&mut tree.children[0], event, main, main_cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let mut children = layout.children();
        let (Some(main), Some(side)) = (children.next(), children.next()) else { return mouse::Interaction::None };
        let dy = if self.follow { nudge(main.bounds(), side.bounds(), viewport) } else { 0.0 };
        match cursor.position().map(|p| p - Vector::new(0.0, dy)) {
            Some(p) if side.bounds().contains(p) => {
                self.side.as_widget().mouse_interaction(&tree.children[1], side, mouse::Cursor::Available(p), viewport, renderer)
            }
            _ => self.main.as_widget().mouse_interaction(&tree.children[0], main, cursor, viewport, renderer),
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let mut children = layout.children();
        let (Some(main), Some(side)) = (children.next(), children.next()) else { return };
        operation.traverse(&mut |operation| {
            self.main.as_widget_mut().operate(&mut tree.children[0], main, renderer, operation);
            self.side.as_widget_mut().operate(&mut tree.children[1], side, renderer, operation);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut children = layout.children();
        let (main, side) = (children.next()?, children.next()?);
        let (main_tree, side_tree) = tree.children.split_at_mut(1);
        let a = self.main.as_widget_mut().overlay(&mut main_tree[0], main, renderer, viewport, translation);
        let b = self.side.as_widget_mut().overlay(&mut side_tree[0], side, renderer, viewport, translation);
        match (a, b) {
            (Some(a), Some(b)) => Some(overlay::Group::with_children(vec![a, b]).into()),
            (a, b) => a.or(b),
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Beside<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(b: Beside<'a, Message, Theme, Renderer>) -> Self {
        Element::new(b)
    }
}

/// A page (a column of items, each a column of rows whose first element holds what the item
/// shows) with the goal drawn beside one of its items, level with what it is for: just past the
/// items level with it, however wide they are drawn (so close to a code block among code blocks,
/// past a paragraph where one is beside it), never over any of them. It takes no room in the page
/// and moves only as far as it must to stay in sight
pub struct GoalLayer<'a, Message, Theme, Renderer> {
    page: Element<'a, Message, Theme, Renderer>,
    goal: Element<'a, Message, Theme, Renderer>,
    /// The item the goal is for, and how far below its top
    anchor: Option<(usize, f32)>,
    gap: f32,
    indices: Option<super::viewport::Indices>,
}

/// How far the goal was moved from where it was laid out, when last drawn: where the widgets in
/// it are drawn (and take the pointer) is that far from where their layout says
static GOAL_MOVED: std::sync::Mutex<Vector> = std::sync::Mutex::new(Vector::new(0.0, 0.0));

#[cfg(test)]
pub static GOAL_VIEWPORT: std::sync::Mutex<Rectangle> = std::sync::Mutex::new(Rectangle { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });

#[cfg(test)]
pub fn goal_moved() -> Vector {
    GOAL_MOVED.lock().map_or(Vector::ZERO, |v| *v)
}

/// What is left under the goal when the page has gone on to show it
const GOAL_ROOM: f32 = 16.0;

impl<'a, Message, Theme, Renderer> GoalLayer<'a, Message, Theme, Renderer> {
    pub fn new(page: impl Into<Element<'a, Message, Theme, Renderer>>, goal: impl Into<Element<'a, Message, Theme, Renderer>>, anchor: Option<(usize, f32)>, gap: f32) -> Self {
        Self { page: page.into(), goal: goal.into(), anchor, gap, indices: None }
    }
}

impl<Message, Theme, Renderer> GoalLayer<'_, Message, Theme, Renderer> {
    pub fn indices(mut self, indices: super::viewport::Indices) -> Self { self.indices = Some(indices); self }
}

/// How far right an item's content reaches: the widest element its rows show
fn item_right(item: Layout<'_>) -> f32 {
    let mut right = f32::MIN;
    for row in item.children() {
        // (a row's first element holds what it shows; spacers and reserved room hold nothing)
        if let Some(holder) = row.children().next()
            && let Some(shown) = holder.children().next()
        {
            right = right.max(shown.bounds().x + shown.bounds().width);
        }
    }
    right
}

impl<Message, Theme, Renderer> GoalLayer<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Where the goal goes (its offset from where it was laid out), in this viewport
    fn place(&self, layout: Layout<'_>, viewport: &Rectangle) -> Option<Vector> {
        let (index, top) = self.anchor?;
        let mut children = layout.children();
        let (page, goal) = (children.next()?, children.next()?);
        let items: Vec<Layout<'_>> = page.children().collect();
        let slot = match &self.indices {
            Some(indices) => indices.lock().unwrap_or_else(|e| e.into_inner()).binary_search(&index).ok()?,
            None => index,
        };
        let anchor = *items.get(slot)?;
        let (gw, gh) = (goal.bounds().width, goal.bounds().height);
        let wanted = Rectangle { x: goal.bounds().x, y: anchor.bounds().y + top, width: gw, height: gh };
        let y = wanted.y + nudge(anchor.bounds(), wanted, viewport);
        let extents: Vec<Extent> = items.iter().map(|it| Extent { top: it.bounds().y, bottom: it.bounds().y + it.bounds().height, right: item_right(*it) }).collect();
        let (y, right) = settle(&extents, slot, y, gh, anchor.bounds().x);
        Some(Vector::new(right + self.gap - goal.bounds().x, y - goal.bounds().y))
    }
}

/// Where an item is on the page and how far right its content reaches
#[derive(Clone, Copy, Debug)]
struct Extent {
    top: f32,
    bottom: f32,
    right: f32,
}

/// Where a goal `height` tall, wanted at `y` beside the item `slot`, goes: its top, and the right
/// edge of what it goes past. Close to its own item, just past the items level with it. Where
/// that would put it over a wider item below (a paragraph reaching further right than the item),
/// it slides up, as far as the gap the item is in goes: the space beside it and any neighbouring
/// items that are no wider, between the wider items above and below. Only a goal taller than
/// the whole gap goes where it was wanted, past the wider items level with it. `from` is where
/// the page's items begin on the left
fn settle(items: &[Extent], slot: usize, wanted: f32, height: f32, from: f32) -> (f32, f32) {
    let base = items[slot].right;
    let wider = |it: &Extent| it.right != f32::MIN && it.right > base + 0.5;
    let above = items[..slot].iter().rev().find(|it| wider(it)).map_or(f32::NEG_INFINITY, |it| it.bottom);
    let below = items[slot + 1..].iter().find(|it| wider(it)).map_or(f32::INFINITY, |it| it.top);
    let slid = wanted.min(below - height).max(above);
    // (a gap that holds the goal: it stays in it, and its items are all no wider than the item's)
    let y = if slid + height <= below + 0.01 { slid } else { wanted };
    // past whatever is level with it
    let right = items.iter().filter(|it| it.top < y + height && y < it.bottom).map(|it| it.right).fold(from, f32::max);
    (y, right)
}

#[cfg(test)]
mod settle_tests {
    use super::{Extent, settle};

    fn cells_and_prose() -> Vec<Extent> {
        // prose (wide), a cell above (narrow), the anchor cell (narrow), then prose (wide)
        vec![
            Extent { top: 0.0, bottom: 100.0, right: 900.0 },
            Extent { top: 110.0, bottom: 200.0, right: 700.0 },
            Extent { top: 210.0, bottom: 300.0, right: 700.0 },
            Extent { top: 310.0, bottom: 400.0, right: 900.0 },
        ]
    }

    /// A goal that fits where it is wanted is not moved, and goes just past its cell
    #[test]
    fn a_goal_that_fits_without_sliding_is_unchanged() {
        let (y, right) = settle(&cells_and_prose(), 2, 215.0, 80.0, 24.0);
        assert_eq!((y, right), (215.0, 700.0));
    }

    /// A goal that would reach the paragraph below slides up, beside the cells, to its bottom
    #[test]
    fn a_goal_that_would_reach_the_prose_below_slides_up_and_stays_beside_the_cell() {
        let (y, right) = settle(&cells_and_prose(), 2, 280.0, 150.0, 24.0);
        assert_eq!(y, 160.0, "its bottom at the paragraph's top");
        assert_eq!(right, 700.0, "still just past the cells, not past the paragraph");
        // and it never goes above the gap's top (the paragraph above ends at 100)
        let (y, right) = settle(&cells_and_prose(), 2, 280.0, 205.0, 24.0);
        assert_eq!((y, right), (105.0, 700.0));
    }

    /// Sliding is continuous as the page scrolls (the wanted top moves with the nudge): no jump
    #[test]
    fn sliding_does_not_jump_as_the_wanted_top_moves() {
        let mut last = settle(&cells_and_prose(), 2, 100.0, 150.0, 24.0).0;
        for w in 101..400 {
            let y = settle(&cells_and_prose(), 2, w as f32, 150.0, 24.0).0;
            assert!((y - last).abs() <= 1.0 + 1e-3, "{last} then {y} at {w}");
            last = y;
        }
    }

    /// A goal taller than the whole gap goes past the paragraphs level with it, as it was wanted
    #[test]
    fn a_goal_taller_than_the_gap_goes_past_the_prose() {
        let (y, right) = settle(&cells_and_prose(), 2, 215.0, 260.0, 24.0);
        assert_eq!(y, 215.0);
        assert_eq!(right, 900.0, "past the paragraph it overlaps");
    }

    /// With no wider item at all, the goal is where it was wanted
    #[test]
    fn with_no_wider_item_the_goal_is_where_it_was_wanted() {
        let items = vec![Extent { top: 0.0, bottom: 90.0, right: 700.0 }, Extent { top: 100.0, bottom: 190.0, right: 700.0 }];
        assert_eq!(settle(&items, 1, 120.0, 500.0, 24.0), (120.0, 700.0));
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for GoalLayer<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.page), Tree::new(&self.goal)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.page, &mut self.goal]);
    }

    fn size(&self) -> Size<Length> {
        self.page.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let page = self.page.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        let goal = self.goal.as_widget_mut().layout(&mut tree.children[1], renderer, &layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, f32::INFINITY)));
        // The page goes on far enough to show all of the goal: under the theory's last cell, there
        // may be less page than there is goal
        let reach = self.anchor.and_then(|(index, top)| {
            let slot = match &self.indices {
                Some(indices) => indices.lock().unwrap_or_else(|e| e.into_inner()).binary_search(&index).ok()?,
                None => index,
            };
            Some(page.children().get(slot)?.bounds().y + top + goal.size().height + GOAL_ROOM)
        });
        let size = Size::new(page.size().width, page.size().height.max(reach.unwrap_or(0.0)));
        layout::Node::with_children(size, vec![page, goal])
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let mut children = layout.children();
        let (Some(page), Some(goal)) = (children.next(), children.next()) else { return };
        self.page.as_widget().draw(&tree.children[0], renderer, theme, style, page, cursor, viewport);
        let Some(at) = self.place(layout, viewport) else { return };
        if let Ok(mut last) = GOAL_MOVED.lock() {
            *last = at;
        }
        #[cfg(test)]
        if let Ok(mut last) = GOAL_VIEWPORT.lock() {
            *last = *viewport;
        }
        let cursor = match cursor.position() {
            Some(p) => mouse::Cursor::Available(p - at),
            None => cursor,
        };
        // (in a layer of its own: over the editors' layers)
        renderer.with_layer(*viewport, |r| {
            r.with_translation(at, |r| {
                self.goal.as_widget().draw(&tree.children[1], r, theme, style, goal, cursor, &Rectangle { x: viewport.x - at.x, y: viewport.y - at.y, ..*viewport });
            });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let at = self.place(layout, viewport);
        let mut children = layout.children();
        let (Some(page), Some(goal)) = (children.next(), children.next()) else { return };
        let over_goal = |p: cosmic::iced::Point| at.is_some_and(|at| goal.bounds().contains(p - at));
        if let Some(at) = at {
            let moved = match cursor.position() {
                Some(p) => mouse::Cursor::Available(p - at),
                None => cursor,
            };
            self.goal.as_widget_mut().update(&mut tree.children[1], event, goal, moved, renderer, clipboard, shell, viewport);
            if shell.is_event_captured() {
                return;
            }
        }
        // (over the goal, the pointer is not over the page under it)
        let page_cursor = match cursor.position() {
            Some(p) if over_goal(p) => mouse::Cursor::Unavailable,
            _ => cursor,
        };
        self.page.as_widget_mut().update(&mut tree.children[0], event, page, page_cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let at = self.place(layout, viewport);
        let mut children = layout.children();
        let (Some(page), Some(goal)) = (children.next(), children.next()) else { return mouse::Interaction::None };
        match (cursor.position(), at) {
            (Some(p), Some(at)) if goal.bounds().contains(p - at) => {
                self.goal.as_widget().mouse_interaction(&tree.children[1], goal, mouse::Cursor::Available(p - at), viewport, renderer)
            }
            _ => self.page.as_widget().mouse_interaction(&tree.children[0], page, cursor, viewport, renderer),
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let mut children = layout.children();
        let (Some(page), Some(goal)) = (children.next(), children.next()) else { return };
        operation.traverse(&mut |operation| {
            self.page.as_widget_mut().operate(&mut tree.children[0], page, renderer, operation);
            self.goal.as_widget_mut().operate(&mut tree.children[1], goal, renderer, operation);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let at = self.place(layout, viewport).unwrap_or(Vector::ZERO);
        let mut children = layout.children();
        let (page, goal) = (children.next()?, children.next()?);
        let (page_tree, goal_tree) = tree.children.split_at_mut(1);
        let a = self.page.as_widget_mut().overlay(&mut page_tree[0], page, renderer, viewport, translation);
        let b = self.goal.as_widget_mut().overlay(&mut goal_tree[0], goal, renderer, viewport, translation + at);
        match (a, b) {
            (Some(a), Some(b)) => Some(overlay::Group::with_children(vec![a, b]).into()),
            (a, b) => a.or(b),
        }
    }
}

impl<'a, Message, Theme, Renderer> From<GoalLayer<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(g: GoalLayer<'a, Message, Theme, Renderer>) -> Self {
        Element::new(g)
    }
}

/// An element on a tinted, rounded region a little larger than itself, which takes no room: the
/// paragraph being edited (or a selected one) is marked without its text moving at all
pub struct Halo<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    /// How far the region reaches past the content: left, right, and above and below
    reach: (f32, f32, f32),
    /// The region's colour and its bar's (down its left edge), from the theme it is drawn in
    colors: Box<dyn Fn(&Theme) -> (cosmic::iced::Color, Option<cosmic::iced::Color>) + 'a>,
}

impl<'a, Message, Theme, Renderer> Halo<'a, Message, Theme, Renderer> {
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        reach: (f32, f32, f32),
        colors: impl Fn(&Theme) -> (cosmic::iced::Color, Option<cosmic::iced::Color>) + 'a,
    ) -> Self {
        Self { content: content.into(), reach, colors: Box::new(colors) }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Halo<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let content = self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits);
        layout::Node::with_children(content.size(), vec![content])
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let Some(content) = layout.children().next() else { return };
        let b = content.bounds();
        let (left, right, down) = self.reach;
        let region = Rectangle { x: b.x - left, y: b.y - down, width: b.width + left + right, height: b.height + 2.0 * down };
        let (color, bar) = (self.colors)(theme);
        renderer.fill_quad(
            renderer::Quad { bounds: region, border: cosmic::iced::Border { radius: 6.0.into(), ..Default::default() }, ..Default::default() },
            cosmic::iced::Background::Color(color),
        );
        if let Some(bar) = bar {
            renderer.fill_quad(
                renderer::Quad { bounds: Rectangle { width: 3.0, ..region }, border: cosmic::iced::Border { radius: 2.0.into(), ..Default::default() }, ..Default::default() },
                cosmic::iced::Background::Color(bar),
            );
        }
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, content, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Some(content) = layout.children().next() else { return };
        self.content.as_widget_mut().update(&mut tree.children[0], event, content, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        let Some(content) = layout.children().next() else { return mouse::Interaction::None };
        self.content.as_widget().mouse_interaction(&tree.children[0], content, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let Some(content) = layout.children().next() else { return };
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(&mut tree.children[0], content, renderer, operation);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let content = layout.children().next()?;
        self.content.as_widget_mut().overlay(&mut tree.children[0], content, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<Halo<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(h: Halo<'a, Message, Theme, Renderer>) -> Self {
        Element::new(h)
    }
}

/// A line with a heading at its left and controls at its right, in whichever of two forms fits:
/// the heading as long as it is and the controls whole, if the two fit the width together; else
/// the heading wrapping in the room left of the compact controls. What fits is the heading's actual
/// width, so a long heading gives the controls their compact form before they would be cut off
pub struct HeadingLine<'a, Message, Theme, Renderer> {
    /// The heading unwrapped, the whole controls, the heading that wraps, the compact controls
    children: [Element<'a, Message, Theme, Renderer>; 4],
    spacing: f32,
    /// Whether the whole form is shown, as last laid out
    whole: bool,
}

impl<'a, Message, Theme, Renderer> HeadingLine<'a, Message, Theme, Renderer> {
    pub fn new(
        heading: Element<'a, Message, Theme, Renderer>,
        controls: Element<'a, Message, Theme, Renderer>,
        wrapping: Element<'a, Message, Theme, Renderer>,
        compact: Element<'a, Message, Theme, Renderer>,
        spacing: f32,
    ) -> Self {
        Self { children: [heading, controls, wrapping, compact], spacing, whole: true }
    }

    fn shown(&self) -> [bool; 4] {
        [self.whole, self.whole, !self.whole, !self.whole]
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for HeadingLine<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let free = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let [heading, controls, wrapping, compact] = &mut self.children;
        let [t0, t1, t2, t3] = &mut tree.children[..] else { return layout::Node::new(Size::ZERO) };
        let heading = heading.as_widget_mut().layout(t0, renderer, &free);
        let controls = controls.as_widget_mut().layout(t1, renderer, &free);
        let compact = compact.as_widget_mut().layout(t3, renderer, &free);
        self.whole = heading.size().width + self.spacing + controls.size().width <= width;
        let room = layout::Limits::new(Size::ZERO, Size::new((width - compact.size().width - self.spacing).max(0.0), limits.max().height));
        let wrapping = wrapping.as_widget_mut().layout(t2, renderer, &room);
        let (left, right, others) = if self.whole { (heading, controls, [wrapping, compact]) } else { (wrapping, compact, [heading, controls]) };
        let height = left.size().height.max(right.size().height);
        let (lh, rw, rh) = (left.size().height, right.size().width, right.size().height);
        let left = left.move_to((0.0, (height - lh) / 2.0));
        let right = right.move_to((width - rw, (height - rh) / 2.0));
        let [a, b] = others;
        let nodes = if self.whole { vec![left, right, a, b] } else { vec![a, b, left, right] };
        layout::Node::with_children(Size::new(width, height), nodes)
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        for ((((child, tree), layout), _), _) in self.children.iter().zip(&tree.children).zip(layout.children()).zip(self.shown()).zip(0..).filter(|((_, shown), _)| *shown) {
            child.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let shown = self.shown();
        for (((child, tree), layout), shown) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()).zip(shown) {
            if shown {
                child.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
            }
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .zip(self.shown())
            .filter(|(_, shown)| *shown)
            .map(|(((child, tree), layout), _)| child.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer))
            .max()
            .unwrap_or_default()
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let shown = self.shown();
        operation.traverse(&mut |operation| {
            for (((child, tree), layout), shown) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()).zip(shown) {
                if shown {
                    child.as_widget_mut().operate(tree, layout, renderer, operation);
                }
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let shown = self.shown();
        let overlays: Vec<_> = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .zip(shown)
            .filter(|(_, shown)| *shown)
            .filter_map(|(((child, tree), layout), _)| child.as_widget_mut().overlay(tree, layout, renderer, viewport, translation))
            .collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).into())
    }
}

impl<'a, Message, Theme, Renderer> From<HeadingLine<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(h: HeadingLine<'a, Message, Theme, Renderer>) -> Self {
        Element::new(h)
    }
}

/// Elements left to right on one row, in order, as many as fit its width (with `spacing` between
/// them): one too wide for the room left is skipped and a narrower one after it may still fit.
/// Those left out are not drawn and given no events, so that nothing hidden can be clicked. The
/// insert bar's chips, which must stay one row whatever the panel's width
pub struct OneRow<'a, Message, Theme, Renderer> {
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    spacing: f32,
    /// Which fit, as last laid out
    shown: Vec<bool>,
}

impl<'a, Message, Theme, Renderer> OneRow<'a, Message, Theme, Renderer> {
    pub fn new(children: Vec<Element<'a, Message, Theme, Renderer>>, spacing: f32) -> Self {
        Self { children, spacing, shown: Vec::new() }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for OneRow<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let width = limits.max().width;
        let free = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        self.shown.clear();
        let mut nodes = Vec::with_capacity(self.children.len());
        for (child, tree) in self.children.iter_mut().zip(tree.children.iter_mut()) {
            let node = child.as_widget_mut().layout(tree, renderer, &free);
            let w = node.size().width;
            // (those left out keep their layout, unused, for their widgets' sake)
            let fits = x + w <= width;
            self.shown.push(fits);
            if fits {
                height = height.max(node.size().height);
                nodes.push(node.move_to((x, 0.0)));
                x += w + self.spacing;
            } else {
                nodes.push(node);
            }
        }
        // (centred on the row's height)
        let nodes = nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| {
                let dy = (height - n.size().height) / 2.0;
                let at = n.bounds().position();
                if self.shown[i] { n.move_to((at.x, at.y + dy)) } else { n }
            })
            .collect();
        layout::Node::with_children(Size::new(width, height), nodes)
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        for (((child, tree), layout), _) in self.children.iter().zip(&tree.children).zip(layout.children()).zip(&self.shown).filter(|(_, shown)| **shown) {
            child.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        for (((child, tree), layout), _) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()).zip(&self.shown).filter(|(_, shown)| **shown) {
            child.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .zip(&self.shown)
            .filter(|(_, shown)| **shown)
            .map(|(((child, tree), layout), _)| child.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer))
            .max()
            .unwrap_or_default()
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        operation.traverse(&mut |operation| {
            for ((child, tree), layout) in self.children.iter_mut().zip(&mut tree.children).zip(layout.children()) {
                child.as_widget_mut().operate(tree, layout, renderer, operation);
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let overlays: Vec<_> = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .zip(&self.shown)
            .filter(|(_, shown)| **shown)
            .filter_map(|(((child, tree), layout), _)| child.as_widget_mut().overlay(tree, layout, renderer, viewport, translation))
            .collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).into())
    }
}

impl<'a, Message, Theme, Renderer> From<OneRow<'a, Message, Theme, Renderer>> for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(r: OneRow<'a, Message, Theme, Renderer>) -> Self {
        Element::new(r)
    }
}

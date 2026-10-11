// SPDX-License-Identifier: GPL-3.0-only

//! An image you can drag and zoom, for popped-out images.
//!
//! The image may be dragged until its edge reaches the middle of the window
//! (50% of the view beyond the image's edge), at any zoom. The scroll wheel
//! (or a trackpoint/touchpad scroll) zooms towards the pointer; a double
//! click fits the image to the window again.

use std::time::{Duration, Instant};

use cosmic::Renderer;
use cosmic::iced::advanced::image::{self as adv_image, Renderer as _};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer, widget::Tree, widget::tree};
use cosmic::iced::{Element, Event, Length, Point, Rectangle, Size, Vector};
use cosmic::widget::image::Handle;

pub struct PanZoom {
    handle: Handle,
    /// Bumped to fit the image again.
    reset: u32,
}

pub fn pan_zoom(handle: Handle, reset: u32) -> PanZoom {
    PanZoom { handle, reset }
}

#[derive(Default)]
struct State {
    fitted: Option<(u32, Size)>,
    scale: f32,
    /// The image's top-left corner, relative to the view's.
    offset: Vector,
    drag: Option<(Point, Vector)>,
    last_click: Option<Instant>,
    image: Size,
    view: Size,
}

impl State {
    fn fit(&mut self) {
        if self.image.width <= 0.0 || self.image.height <= 0.0 {
            return;
        }
        self.scale = (self.view.width / self.image.width).min(self.view.height / self.image.height).min(2.0);
        let shown = Size::new(self.image.width * self.scale, self.image.height * self.scale);
        self.offset = Vector::new((self.view.width - shown.width) / 2.0, (self.view.height - shown.height) / 2.0);
    }

    /// Keeps the image within reach: each edge may go up to half the view
    /// beyond the view's edge, but no further.
    fn clamp(&mut self) {
        let (w, h) = (self.image.width * self.scale, self.image.height * self.scale);
        let (vw, vh) = (self.view.width, self.view.height);
        self.offset.x = self.offset.x.clamp(vw * 0.5 - w, vw * 0.5);
        self.offset.y = self.offset.y.clamp(vh * 0.5 - h, vh * 0.5);
    }

    fn zoom_at(&mut self, at: Point, factor: f32) {
        let new = (self.scale * factor).clamp(0.05, 32.0);
        let k = new / self.scale;
        self.offset = Vector::new(at.x - (at.x - self.offset.x) * k, at.y - (at.y - self.offset.y) * k);
        self.scale = new;
        self.clamp();
    }
}

impl<Message> Widget<Message, cosmic::Theme, Renderer> for PanZoom {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let size = limits.max();
        let state = tree.state.downcast_mut::<State>();
        let dims = renderer.measure_image(&self.handle).unwrap_or_default();
        state.image = Size::new(dims.width as f32, dims.height as f32);
        let resized = state.view != size;
        state.view = size;
        match state.fitted {
            Some((reset, _)) if reset == self.reset => {
                if resized {
                    state.clamp();
                }
            }
            _ => {
                state.fit();
                state.fitted = Some((self.reset, size));
            }
        }
        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let Some(p) = cursor.position_in(bounds) else { return };
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 40.0,
                };
                if lines != 0.0 {
                    state.zoom_at(p, 1.15f32.powf(lines));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position_in(bounds) else { return };
                if state.last_click.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
                    state.fit();
                    state.last_click = None;
                } else {
                    state.last_click = Some(Instant::now());
                    state.drag = Some((p, state.offset));
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some((from, start)), Some(p)) = (state.drag, cursor.position()) {
                    let p = Point::new(p.x - bounds.x, p.y - bounds.y);
                    state.offset = start + (p - from);
                    state.clamp();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag = None;
            }
            _ => {}
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, _viewport: &Rectangle, _renderer: &Renderer) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &cosmic::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let rect = Rectangle::new(
            Point::new(bounds.x + state.offset.x, bounds.y + state.offset.y),
            Size::new(state.image.width * state.scale, state.image.height * state.scale),
        );
        renderer.draw_image(adv_image::Image::new(self.handle.clone()), rect, bounds);
    }
}

impl<'a, Message: 'a> From<PanZoom> for Element<'a, Message, cosmic::Theme, Renderer> {
    fn from(w: PanZoom) -> Self {
        Element::new(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        let mut s = State { image: Size::new(400.0, 200.0), view: Size::new(800.0, 600.0), ..State::default() };
        s.fit();
        s
    }

    #[test]
    fn fits_and_centres() {
        let s = state();
        assert_eq!(s.scale, 2.0);
        assert_eq!(s.offset, Vector::new(0.0, 100.0));
    }

    #[test]
    fn pans_at_most_half_the_view_past_an_edge() {
        let mut s = state();
        s.offset = Vector::new(10_000.0, -10_000.0);
        s.clamp();
        // Left edge stops at the middle; bottom edge stops at the middle.
        assert_eq!(s.offset.x, 400.0);
        assert_eq!(s.offset.y, 300.0 - 400.0);
    }

    #[test]
    fn zooms_towards_the_pointer() {
        let mut s = state();
        let at = Point::new(400.0, 300.0);
        let before = (at.x - s.offset.x) / s.scale;
        s.zoom_at(at, 2.0);
        let after = (at.x - s.offset.x) / s.scale;
        assert!((before - after).abs() < 1e-3, "the point under the pointer stays put");
    }
}

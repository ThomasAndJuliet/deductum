// SPDX-License-Identifier: GPL-3.0-only

//! An image or a diagram from a thread, in a window of its own.
//!
//! Laid out as Marginalium lays out a page: a toolbar with the zoom (fit,
//! actual size, out, in, and the percentage), and the picture on a neutral
//! ground that scrolls both ways when it is larger than the window.
//! Ctrl+scroll zooms; the window goes full screen, and Escape brings it
//! back. An image is decoded only for this window, and freed when it
//! closes. A diagram is drawn again in the window's colours, and shows its
//! Mermaid source in place of the drawing at the press of a button, as in
//! Zed.

use std::path::PathBuf;
use std::sync::Arc;

use cosmic::iced::advanced::widget::{Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::keyboard::{self, key::Named};
use cosmic::iced::{Alignment, ContentFit, Event, Length, Rectangle, Size, Vector, window};
use cosmic::widget::{self, button, text_editor};
use cosmic::{Element, theme};

use super::diagram::{self, Diagram, Palette};
use crate::Message;
use crate::fl;

pub struct ImageWindow {
    /// The image's file; empty for a diagram
    pub path: PathBuf,
    pub size: Option<(u32, u32)>,
    /// `None` fits the window
    pub zoom: Option<f32>,
    /// A diagram's Mermaid source, for a diagram's window
    pub diagram: Option<String>,
    /// The source, shown in place of the drawing
    pub source: Option<text_editor::Content>,
    pub full_screen: bool,
    /// The colours of the paper the diagram was on, when the thread is set
    /// on white or sepia rather than in the theme's
    pub paper: Option<Palette>,
}

#[derive(Clone, Copy, Debug)]
pub enum Zoom {
    Fit,
    Actual,
    In,
    Out,
    Scroll(f32),
}

/// What the window's controls ask for
#[derive(Clone, Debug)]
pub enum Action {
    Zoom(Zoom),
    /// The file's path, or the diagram's source
    Copy,
    /// The file, or the diagram as SVG, in another app
    OpenExternally,
    /// Into or out of full screen
    FullScreen,
    /// Escape: out of full screen
    Escape,
    /// The diagram's source, or its drawing
    Source,
    /// A selection in the source
    Edit(text_editor::Action),
}

const STEPS: [f32; 13] = [0.1, 0.25, 0.33, 0.5, 0.67, 0.8, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 8.0];

/// The colours a diagram is drawn in, in a window of `theme`'s: those of a
/// thread set in the theme
pub fn palette(theme: &cosmic::Theme) -> Palette {
    let cosmic = theme.cosmic();
    let page: cosmic::iced::Color = cosmic.bg_color().into();
    let text: cosmic::iced::Color = cosmic.on_bg_color().into();
    let over = |color: cosmic::iced::Color, alpha: f32| cosmic::iced::Color {
        r: page.r + (color.r - page.r) * alpha * color.a,
        g: page.g + (color.g - page.g) * alpha * color.a,
        b: page.b + (color.b - page.b) * alpha * color.a,
        a: 1.0,
    };
    Palette {
        page: cosmic::iced::Color { a: 1.0, ..page },
        surface: over(cosmic.bg_component_color().into(), 1.0),
        text: over(text, 1.0),
        muted: over(text, 0.68),
        rule: over(text, 0.25),
        accent: over(cosmic.accent_text_color().into(), 1.0),
        error: over(cosmic.destructive_text_color().into(), 1.0),
    }
}

impl ImageWindow {
    pub fn new(path: PathBuf) -> Self {
        let size = super::images::dimensions(&path);
        Self { path, size, zoom: None, diagram: None, source: None, full_screen: false, paper: None }
    }

    /// A window for the diagram of `source`, on `paper` if it was on one
    pub fn diagram(source: String, paper: Option<Palette>) -> Self {
        let size = diagram::render(&source, &paper.unwrap_or_else(|| palette(&theme::active())))
            .ok()
            .map(|d| (d.width.ceil() as u32, d.height.ceil() as u32));
        Self { path: PathBuf::new(), size, zoom: None, diagram: Some(source), source: None, full_screen: false, paper }
    }

    /// The window's size to start with: the picture's, within reason
    pub fn initial_size(&self) -> cosmic::iced::Size {
        let (w, h) = self.size.unwrap_or((800, 600));
        let scale = (1400.0 / w as f32).min(1000.0 / h as f32).min(1.0);
        // Wide enough for the toolbar, whatever the picture
        cosmic::iced::Size::new((w as f32 * scale).max(640.0), (h as f32 * scale + 56.0).max(360.0))
    }

    pub fn title(&self) -> String {
        if self.diagram.is_some() {
            return fl!("diagram");
        }
        self.path.file_name().map_or_else(|| fl!("image"), |n| n.to_string_lossy().into_owned())
    }

    pub fn zoom(&mut self, zoom: Zoom) {
        let current = self.zoom.unwrap_or(1.0);
        self.zoom = match zoom {
            Zoom::Fit => None,
            Zoom::Actual => Some(1.0),
            Zoom::In => Some(STEPS.iter().copied().find(|s| *s > current + 0.001).unwrap_or(current)),
            Zoom::Out => Some(STEPS.iter().rev().copied().find(|s| *s < current - 0.001).unwrap_or(current)),
            Zoom::Scroll(lines) => Some((current * 1.1f32.powf(lines)).clamp(0.05, 16.0)),
        };
    }

    /// The diagram, drawn in `theme`'s colours, or why it can't be
    fn drawn(&self, theme: &cosmic::Theme) -> Option<Result<Arc<Diagram>, String>> {
        let palette = self.paper.unwrap_or_else(|| palette(theme));
        self.diagram.as_ref().map(|source| diagram::render(source, &palette))
    }

    /// Do what a control asks, for the window `id`
    pub fn act(&mut self, id: window::Id, action: Action) -> cosmic::app::Task<Message> {
        match action {
            Action::Zoom(zoom) => self.zoom(zoom),
            Action::Copy => {
                let text = self.diagram.clone().unwrap_or_else(|| self.path.display().to_string());
                return cosmic::iced::clipboard::write(text);
            }
            Action::OpenExternally => {
                let path = match self.drawn(&theme::active()) {
                    Some(Ok(drawn)) => {
                        let path = std::env::temp_dir().join(format!("cosmic-term-diagram-{}.svg", std::process::id()));
                        if let Err(err) = std::fs::write(&path, drawn.svg.as_bytes()) {
                            log::warn!("failed to write {}: {err}", path.display());
                            return cosmic::app::Task::none();
                        }
                        path
                    }
                    Some(Err(_)) => return cosmic::app::Task::none(),
                    None => self.path.clone(),
                };
                if let Err(err) = open::that_detached(&path) {
                    log::warn!("failed to open {}: {err}", path.display());
                }
            }
            Action::FullScreen => {
                self.full_screen = !self.full_screen;
                let mode = if self.full_screen { window::Mode::Fullscreen } else { window::Mode::Windowed };
                return window::set_mode(id, mode);
            }
            Action::Escape if self.full_screen => {
                self.full_screen = false;
                return window::set_mode(id, window::Mode::Windowed);
            }
            Action::Escape => {}
            Action::Source => {
                self.source = match (&self.source, &self.diagram) {
                    (None, Some(source)) => Some(text_editor::Content::with_text(source)),
                    _ => None,
                };
            }
            Action::Edit(action) => {
                // The source is read-only: it selects, but does not change
                if let Some(content) = &mut self.source
                    && !action.is_edit()
                {
                    content.perform(action);
                }
            }
        }
        cosmic::app::Task::none()
    }
}

/// The window: its image or diagram and the toolbar, whose controls `on`
/// turns into the app's messages
pub fn view_with<'a>(
    window: &'a ImageWindow,
    on: impl Fn(Action) -> Option<Message> + Clone + 'a,
) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let active = theme::active();
    let icon = |name| crate::icon_cache_get(name, 16);
    let percent = window.zoom.map_or_else(|| fl!("image-fit"), |z| format!("{:.0}%", z * 100.0));
    let dimensions = window.size.map(|(w, h)| format!("{w} × {h}")).unwrap_or_default();
    let tool = |name, tip: String, action: Action| -> Option<Element<'a, Message>> {
        let message = on(action)?;
        Some(
            widget::tooltip(
                button::custom(icon(name)).class(theme::Button::Icon).padding(spacing.space_xxs).on_press(message),
                widget::text::body(tip),
                widget::tooltip::Position::Bottom,
            )
            .into(),
        )
    };
    let diagram = window.diagram.is_some();
    let showing_source = window.source.is_some();
    let mut tools: Vec<Option<Element<'a, Message>>> = vec![
        Some(widget::text::body(window.title()).into()),
        Some(widget::text::caption(dimensions).into()),
        Some(widget::Space::new().width(Length::Fill).into()),
    ];
    if diagram && let Some(message) = on(Action::Source) {
        // Zed's Source and Preview, as one toggle
        let label = if showing_source { fl!("diagram-preview") } else { fl!("diagram-source") };
        tools.push(Some(button::text(label).on_press(message).into()));
    }
    if !showing_source {
        tools.extend([
            tool("zoom-fit-best-symbolic", fl!("image-fit"), Action::Zoom(Zoom::Fit)),
            tool("zoom-original-symbolic", fl!("image-actual"), Action::Zoom(Zoom::Actual)),
            tool("zoom-out-symbolic", fl!("image-zoom-out"), Action::Zoom(Zoom::Out)),
            Some(widget::text::body(percent).width(Length::Fixed(52.0)).align_x(Alignment::Center).into()),
            tool("zoom-in-symbolic", fl!("image-zoom-in"), Action::Zoom(Zoom::In)),
        ]);
    }
    tools.extend([
        if diagram {
            tool("edit-copy-symbolic", fl!("diagram-copy-source"), Action::Copy)
        } else {
            tool("edit-copy-symbolic", fl!("image-copy-path"), Action::Copy)
        },
        if diagram {
            tool("document-open-symbolic", fl!("diagram-open-externally"), Action::OpenExternally)
        } else {
            tool("document-open-symbolic", fl!("image-open-externally"), Action::OpenExternally)
        },
        if window.full_screen {
            tool("view-restore-symbolic", fl!("image-leave-full-screen"), Action::FullScreen)
        } else {
            tool("view-fullscreen-symbolic", fl!("image-full-screen"), Action::FullScreen)
        },
    ]);
    let toolbar = widget::row::with_children(tools.into_iter().flatten().collect::<Vec<_>>())
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .padding([spacing.space_xxs, spacing.space_s]);

    let drawn = window.drawn(&active);
    // An image at `size`, or filling the space; a drawing stays sharp at
    // any zoom
    let picture = move |size: Option<(f32, f32)>, smooth: bool| -> Element<'a, Message> {
        let (width, height, fit) = match size {
            Some((w, h)) => (Length::Fixed(w), Length::Fixed(h), ContentFit::Fill),
            None => (Length::Fill, Length::Fill, ContentFit::Contain),
        };
        match &drawn {
            Some(Ok(drawn)) => widget::svg(drawn.full.clone()).width(width).height(height).content_fit(fit).into(),
            Some(Err(err)) => widget::text::body(format!("mermaid: {err}")).into(),
            None if super::images::is_svg(&window.path) => {
                widget::svg(widget::svg::Handle::from_path(window.path.clone()))
                    .width(width)
                    .height(height)
                    .content_fit(fit)
                    .into()
            }
            None => widget::image(widget::image::Handle::from_path(window.path.clone()))
                .width(width)
                .height(height)
                .content_fit(fit)
                .filter_method(if smooth {
                    widget::image::FilterMethod::Linear
                } else {
                    widget::image::FilterMethod::Nearest
                })
                .into(),
        }
    };
    let content: Element<'a, Message> = match (&window.source, window.zoom, window.size) {
        (Some(source), _, _) => {
            let mut editor = text_editor::text_editor(source).font(cosmic::font::mono()).padding(spacing.space_s).height(Length::Fill);
            let edit = on.clone();
            if on(Action::Edit(text_editor::Action::SelectAll)).is_some() {
                editor = editor.on_action(move |action| edit(Action::Edit(action)).expect("an edit message"));
            }
            editor.into()
        }
        (None, Some(zoom), Some((w, h))) => widget::scrollable(
            widget::container(picture(Some(((w as f32 * zoom).floor(), (h as f32 * zoom).floor())), zoom < 2.0))
                .center(Length::Shrink)
                .padding(spacing.space_s),
        )
        .direction(cosmic::iced::widget::scrollable::Direction::Both {
            vertical: Default::default(),
            horizontal: Default::default(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        // A drawing fitted at its fitting size, as zoomed ones are: drawn
        // fitted by the GPU renderer, its thinnest lines are lost
        (_, _, Some((w, h))) if window.diagram.is_some() || super::images::is_svg(&window.path) => {
            let pad = f32::from(spacing.space_s);
            widget::responsive(move |space| {
                let scale = ((space.width - 2.0 * pad) / w as f32).min((space.height - 2.0 * pad) / h as f32).max(0.01);
                // In whole pixels, as the zoomed sizes are
                widget::container(picture(Some(((w as f32 * scale).floor(), (h as f32 * scale).floor())), true))
                    .center(Length::Fill)
                    .into()
            })
            .into()
        }
        _ => widget::container(picture(None, true)).padding(spacing.space_s).into(),
    };
    let column = widget::column::with_children(vec![
        toolbar.into(),
        widget::divider::horizontal::default().into(),
        widget::container(content).width(Length::Fill).height(Length::Fill).into(),
    ]);
    // Opaque, whatever the window's transparency: the toolbar would show
    // the desktop through it
    let paper = window.paper;
    let window_element: Element<'a, Message> = widget::container(column)
        .class(theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            // The paper's colours, else the theme's
            let background: cosmic::iced::Color = paper.map_or_else(|| cosmic.bg_color().into(), |p| p.page);
            let text: cosmic::iced::Color = paper.map_or_else(|| cosmic.on_bg_color().into(), |p| p.text);
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Color { a: 1.0, ..background }.into()),
                text_color: Some(cosmic::iced::Color { a: 1.0, ..text }),
                icon_color: Some(cosmic::iced::Color { a: 1.0, ..text }),
                ..Default::default()
            }
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    match on(Action::Escape) {
        Some(message) if window.full_screen => Element::new(OnEscape { content: window_element, message }),
        _ => window_element,
    }
}

/// Its content, and a message for Escape that nothing in it took
struct OnEscape<'a> {
    content: Element<'a, Message>,
    message: Message,
}

impl Widget<Message, cosmic::Theme, cosmic::Renderer> for OnEscape<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &cosmic::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation,
    ) {
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
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        if let Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(Named::Escape), .. }) = event {
            shell.publish(self.message.clone());
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_through_the_ladder() {
        let mut window = ImageWindow::new(PathBuf::new());
        window.size = Some((100, 100));
        window.zoom(Zoom::In);
        assert_eq!(window.zoom, Some(1.25));
        window.zoom(Zoom::Out);
        window.zoom(Zoom::Out);
        assert_eq!(window.zoom, Some(0.8));
        window.zoom(Zoom::Fit);
        assert_eq!(window.zoom, None);
    }

    #[test]
    fn a_diagram_window_shows_its_source_and_goes_full_screen() {
        let id = window::Id::unique();
        let mut window = ImageWindow::diagram("flowchart LR\n    A --> B".into(), None);
        assert!(window.size.is_some_and(|(w, h)| w > 20 && h > 10), "{:?}", window.size);
        assert_eq!(window.title(), fl!("diagram"));
        let _ = window.act(id, Action::Source);
        assert_eq!(window.source.as_ref().map(|s| s.text()).as_deref().map(str::trim_end), Some("flowchart LR\n    A --> B"));
        // Read-only: typing changes nothing
        let _ = window.act(id, Action::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        assert!(!window.source.as_ref().unwrap().text().contains('x'));
        let _ = window.act(id, Action::Source);
        assert!(window.source.is_none());
        let _ = window.act(id, Action::FullScreen);
        assert!(window.full_screen);
        let _ = window.act(id, Action::Escape);
        assert!(!window.full_screen);
    }
}

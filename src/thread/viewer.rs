// SPDX-License-Identifier: GPL-3.0-only

//! An image from a thread, in a window of its own.
//!
//! Laid out as Marginalium lays out a page: a toolbar with the zoom (fit,
//! actual size, out, in, and the percentage), and the image on a neutral
//! ground that scrolls both ways when it is larger than the window.
//! Ctrl+scroll zooms. The image is decoded only for this window, and freed
//! when it closes.

use std::path::PathBuf;

use cosmic::iced::{Alignment, ContentFit, Length};
use cosmic::widget::{self, button};
use cosmic::{Element, theme};

use crate::Message;
use crate::fl;

pub struct ImageWindow {
    pub path: PathBuf,
    pub size: Option<(u32, u32)>,
    /// `None` fits the window
    pub zoom: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub enum Zoom {
    Fit,
    Actual,
    In,
    Out,
    Scroll(f32),
}

const STEPS: [f32; 13] = [0.1, 0.25, 0.33, 0.5, 0.67, 0.8, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0, 8.0];

impl ImageWindow {
    pub fn new(path: PathBuf) -> Self {
        let size = super::images::dimensions(&path);
        Self { path, size, zoom: None }
    }

    /// The window's size to start with: the image's, within reason
    pub fn initial_size(&self) -> cosmic::iced::Size {
        let (w, h) = self.size.unwrap_or((800, 600));
        let scale = (1400.0 / w as f32).min(1000.0 / h as f32).min(1.0);
        cosmic::iced::Size::new((w as f32 * scale).max(360.0), (h as f32 * scale + 56.0).max(240.0))
    }

    pub fn title(&self) -> String {
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
}

pub fn view(window: &ImageWindow, id: cosmic::iced::window::Id) -> Element<'_, Message> {
    let spacing = theme::active().cosmic().spacing;
    let icon = |name| crate::icon_cache_get(name, 16);
    let zoom = move |zoom| Message::ImageZoom(id, zoom);
    let percent = window.zoom.map_or_else(|| fl!("image-fit"), |z| format!("{:.0}%", z * 100.0));
    let dimensions = window.size.map(|(w, h)| format!("{w} × {h}")).unwrap_or_default();
    let tool = |name, tip: String, message| -> Element<'_, Message> {
        widget::tooltip(button::custom(icon(name)).class(theme::Button::Icon).padding(spacing.space_xxs).on_press(message), widget::text::body(tip), widget::tooltip::Position::Bottom)
            .into()
    };
    let toolbar = widget::row::with_children(vec![
        widget::text::body(window.title()).into(),
        widget::text::caption(dimensions).into(),
        widget::Space::new().width(Length::Fill).into(),
        tool("zoom-fit-best-symbolic", fl!("image-fit"), zoom(Zoom::Fit)),
        tool("zoom-original-symbolic", fl!("image-actual"), zoom(Zoom::Actual)),
        tool("zoom-out-symbolic", fl!("image-zoom-out"), zoom(Zoom::Out)),
        widget::text::body(percent).width(Length::Fixed(52.0)).align_x(Alignment::Center).into(),
        tool("zoom-in-symbolic", fl!("image-zoom-in"), zoom(Zoom::In)),
        tool("edit-copy-symbolic", fl!("image-copy-path"), Message::ImageCopyPath(id)),
        tool("document-open-symbolic", fl!("image-open-externally"), Message::ImageOpenExternally(id)),
    ])
    .spacing(spacing.space_xxs)
    .align_y(Alignment::Center)
    .padding([spacing.space_xxs, spacing.space_s]);

    // An image at `size`, or filling the space; a drawing stays sharp at
    // any zoom
    let picture = |size: Option<(f32, f32)>, smooth: bool| -> Element<'_, Message> {
        let (width, height, fit) = match size {
            Some((w, h)) => (Length::Fixed(w), Length::Fixed(h), ContentFit::Fill),
            None => (Length::Fill, Length::Fill, ContentFit::Contain),
        };
        if super::images::is_svg(&window.path) {
            widget::svg(widget::svg::Handle::from_path(window.path.clone()))
                .width(width)
                .height(height)
                .content_fit(fit)
                .into()
        } else {
            widget::image(widget::image::Handle::from_path(window.path.clone()))
                .width(width)
                .height(height)
                .content_fit(fit)
                .filter_method(if smooth {
                    widget::image::FilterMethod::Linear
                } else {
                    widget::image::FilterMethod::Nearest
                })
                .into()
        }
    };
    let content: Element<'_, Message> = match (window.zoom, window.size) {
        (Some(zoom), Some((w, h))) => widget::scrollable(
            widget::container(picture(Some((w as f32 * zoom, h as f32 * zoom)), zoom < 2.0))
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
        _ => widget::container(picture(None, true)).padding(spacing.space_s).into(),
    };
    let column = widget::column::with_children(vec![
        toolbar.into(),
        widget::divider::horizontal::default().into(),
        widget::container(content).width(Length::Fill).height(Length::Fill).into(),
    ]);
    // Opaque, whatever the window's transparency: the toolbar would show
    // the desktop through it
    widget::container(column)
        .class(theme::Container::custom(|theme| {
            let cosmic = theme.cosmic();
            let background: cosmic::iced::Color = cosmic.bg_color().into();
            let text: cosmic::iced::Color = cosmic.on_bg_color().into();
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Color { a: 1.0, ..background }.into()),
                text_color: Some(cosmic::iced::Color { a: 1.0, ..text }),
                icon_color: Some(cosmic::iced::Color { a: 1.0, ..text }),
                ..Default::default()
            }
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_through_the_ladder() {
        let mut window = ImageWindow { path: PathBuf::new(), size: Some((100, 100)), zoom: None };
        window.zoom(Zoom::In);
        assert_eq!(window.zoom, Some(1.25));
        window.zoom(Zoom::Out);
        window.zoom(Zoom::Out);
        assert_eq!(window.zoom, Some(0.8));
        window.zoom(Zoom::Fit);
        assert_eq!(window.zoom, None);
    }
}

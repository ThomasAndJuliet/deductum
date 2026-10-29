// SPDX-License-Identifier: GPL-3.0-only

//! The household's tones in Deductum (household-ui RULES.md rule 6, "What is highlighted, and
//! how"): code selected in an editor is marked with the tint, the accent at 32 % over what is
//! beneath, opaque, its default text through the fill rule against that tint ("use the tint for
//! deductum code", Juliet 25 Oct); a chosen tab or segment and a toggle that is on are filled with
//! the muted accent, their label in its on colour, hovered and pressed with COSMIC's wash over
//! it. Glue only: the colours are household-tones' (the household's shared tone maths), so every
//! app shows the same tint and the same fill for the same accent.

use std::rc::Rc;

use cosmic::iced::Color;
use household_tones::{Mode, Role, Srgb, Srgba};

fn srgb(c: Color) -> Srgb {
    Srgb::new(c.r, c.g, c.b)
}

fn color(c: Srgb) -> Color {
    Color::from_rgb(c.r, c.g, c.b)
}

/// `top`, translucent, laid over `under` (source-over, as the renderer draws it)
pub fn over(top: Color, under: Color) -> Color {
    let made = household_tones::over(Srgba::new(top.r, top.g, top.b, top.a), Srgba::new(under.r, under.g, under.b, under.a));
    Color::from_rgba(made.r, made.g, made.b, made.a)
}

fn accent(theme: &cosmic::Theme) -> Srgb {
    let a = theme.cosmic().accent.base;
    Srgb::new(a.red, a.green, a.blue)
}

/// The page's strongest text colour: a light theme's darkest text (#222222, not black), a dark
/// theme's lightest. Greys on the tint go to it
fn strongest(theme: &cosmic::Theme) -> Srgb {
    let mode = if theme.cosmic().is_dark { Mode::Dark } else { Mode::Light };
    Role::Darkest.tone(mode).unwrap_or(if theme.cosmic().is_dark { Srgb::WHITE } else { Srgb::BLACK })
}

// ----- the tint on code

/// What a view of code is drawn on, from the theme it is drawn in: the page, a cell, a card, with
/// any translucent washes laid over it. A code editor's own background is clear, so the tint of a
/// selection in it is worked out over this
#[derive(Clone)]
pub struct Beneath(Rc<dyn Fn(&cosmic::Theme) -> Color>);

impl Beneath {
    pub fn new(surface: impl Fn(&cosmic::Theme) -> Color + 'static) -> Self {
        Self(Rc::new(surface))
    }

    /// This, with `wash` laid over it (a translucent card or tint between it and the code)
    pub fn under(&self, wash: impl Fn(&cosmic::Theme) -> Color + 'static) -> Self {
        let below = self.0.clone();
        Self(Rc::new(move |theme| over(wash(theme), below(theme))))
    }

    /// The opaque colour beneath the code in `theme`
    pub fn color(&self, theme: &cosmic::Theme) -> Color {
        let c = (self.0)(theme);
        if c.a < 1.0 { over(c, theme.cosmic().bg_color().into()) } else { c }
    }
}

/// The selection tint: the accent at 32 % over `beneath`, as the opaque colour it shows as
/// (`household_tones::tint`)
pub fn tint(theme: &cosmic::Theme, beneath: Color) -> Color {
    color(household_tones::tint(accent(theme), srgb(beneath)))
}

/// What `text` is drawn in on the tint over `beneath`: the fill rule against the tint, greys going
/// to the page's strongest text colour, and a colour that already reads left as it is
/// (`household_tones::on_tint`)
pub fn on_tint(theme: &cosmic::Theme, text: Color, beneath: Color) -> Color {
    let text = if text.a < 1.0 { over(text, beneath) } else { text };
    color(household_tones::on_tint(srgb(text), accent(theme), srgb(beneath), strongest(theme)))
}

/// A code editor's selection, marked with the tint over what is beneath it: its fill, and its
/// default text colour on that fill (spans the highlighter coloured keep their colours, as
/// libcosmic can only recolour the default; stage 2 of the standard)
pub fn code_selection<'a, H, M>(
    editor: cosmic::iced::widget::TextEditor<'a, H, M, cosmic::Theme, cosmic::Renderer>,
    beneath: Beneath,
) -> cosmic::iced::widget::TextEditor<'a, H, M, cosmic::Theme, cosmic::Renderer>
where
    H: cosmic::iced::advanced::text::Highlighter,
{
    let under = beneath.clone();
    editor
        .selection_fill(move |theme, _| tint(theme, beneath.color(theme)))
        .selected_text_color(move |theme, style| on_tint(theme, style.value, under.color(theme)))
}

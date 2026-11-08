// SPDX-License-Identifier: GPL-3.0-only

//! The household's tones in Deductum (household-ui RULES.md rule 6, "What is highlighted, and
//! how"): code selected in an editor is marked with the tint, the accent at 32 % over what is
//! beneath, opaque, its default text through the fill rule against that tint ("use the tint for
//! deductum code", Juliet 25 Oct); a chosen tab or segment and a toggle that is on are filled with
//! the muted accent, their label in its on colour, hovered and pressed with COSMIC's wash over
//! it. Glue only: the colours are household-tones' (the household's shared tone maths), so every
//! app shows the same tint and the same fill for the same accent.

use std::rc::Rc;

use cosmic::iced::{Background, Color};
use cosmic::widget::button;
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

// ----- the household's text tones

/// Whether the household's light-mode tones apply to `theme`: a light theme, not high contrast
/// (COSMIC's high-contrast themes keep their own text)
fn household_light(theme: &cosmic::cosmic_theme::Theme) -> bool {
    !theme.is_dark && !theme.is_high_contrast
}

/// The theme Deductum hands libcosmic: COSMIC's, with the household's text tones (rule 25d; the
/// 16 Oct UI plan §1, "darkest #222222 … no pure black"). In a light theme the text COSMIC derives
/// for its containers, their components and its buttons is darkest, #222222, instead of black, so
/// every label, line of code and caption drawn in the theme's text colour takes it. Dark and
/// high-contrast themes keep COSMIC's own for now. The theme stays a system theme if it was one, so
/// libcosmic goes on following the desktop; it is applied again to each theme the desktop sends
/// (`system_theme_update`), and applying it twice changes nothing
pub fn household(theme: cosmic::Theme) -> cosmic::Theme {
    use cosmic::theme::ThemeType;
    let mut inner = theme.cosmic().clone();
    if !household_light(&inner) {
        return theme;
    }
    let darkest = Role::Darkest.tone(Mode::Light).expect("darkest has a tone");
    let on = cosmic::cosmic_theme::palette::Srgba::new(darkest.r, darkest.g, darkest.b, 1.0);
    for component in [&mut inner.button, &mut inner.icon_button, &mut inner.text_button, &mut inner.list_button] {
        component.on = on;
    }
    let inner = std::sync::Arc::new(with_container_text(inner, on));
    let theme_type = match theme.theme_type {
        ThemeType::System { prefer_dark, .. } => ThemeType::System { prefer_dark, theme: inner },
        _ => ThemeType::Custom(inner),
    };
    cosmic::Theme { theme_type, ..theme }
}

/// `theme` with `on` as the text of its containers (the background, primary and secondary layers,
/// opaque and translucent) and of their components. cosmic-theme keeps the containers private to
/// itself, with no way to change them but rebuilding the theme from a builder, which derives its
/// text from a palette and cannot give an exact tone; the theme is a plain serde record, so the
/// fields are set through it. The record writes each colour as #RRGGBBAA, so the theme's other
/// colours come back rounded to the 8 bits a screen shows (as a theme the desktop saved already
/// is). Should a revision rename them, the theme is left as it was (and the
/// test `household_text_is_darkest_in_light_themes` fails)
fn with_container_text(theme: cosmic::cosmic_theme::Theme, on: cosmic::cosmic_theme::palette::Srgba) -> cosmic::cosmic_theme::Theme {
    let (Ok(mut record), Ok(on)) = (serde_json::to_value(&theme), serde_json::to_value(on)) else { return theme };
    for layer in ["background", "primary", "secondary", "transparent_background", "transparent_primary", "transparent_secondary"] {
        if let Some(container) = record.get_mut(layer) {
            container["on"] = on.clone();
            if let Some(component) = container.get_mut("component") {
                component["on"] = on.clone();
            }
        }
    }
    serde_json::from_value(record).unwrap_or(theme)
}

/// Body text read in paragraphs (the Literate view's prose) at `size_px`: on a light theme darkest
/// below 18 px and dark (#525252) from 18 px, where the larger letters carry the contrast
/// (`household_tones::text_role`); otherwise the theme's text
pub fn body_text(theme: &cosmic::Theme, size_px: f32) -> Color {
    let c = theme.cosmic();
    if !household_light(c) {
        return c.on_bg_color().into();
    }
    household_tones::roles::text_role(size_px, true).tone(Mode::Light).map_or_else(|| c.on_bg_color().into(), color)
}

/// Secondary text and the icons that go with it (an imports line, a hint, a caption, a detail
/// line): on a light theme the text's own darkest tone, as every caption is ("caption text should
/// be darkest (as it is in Cosmic UI) rather than grey", Juliet 16 Oct; size and weight set it
/// apart, not a lighter grey); on other themes, as before, the text faded to `alpha`
pub fn secondary_text(theme: &cosmic::Theme, alpha: f32) -> Color {
    let c = theme.cosmic();
    let text: Color = c.on_bg_color().into();
    if household_light(c) { text } else { Color { a: alpha, ..text } }
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

// ----- the cells' status tones

/// How much of a status colour shows in its faded tone: the colour at 35 % over the page (Q4,
/// Juliet 8 Nov; mockups/r2)
const FADED: f32 = 0.35;

/// The automatic highlight, the accent tint laid over what is beneath it, in a light theme and in
/// a dark one ("contrast not being symmetric in light vs dark", Juliet 8 Nov, S3). The user's own
/// selection stays `tint` (32 %), so over the band it is a step darker
const BAND_LIGHT: f32 = 0.10;
const BAND_DARK: f32 = 0.14;

/// The colour that marks the construct the current goal is for: the accent at [`BAND_LIGHT`] or
/// [`BAND_DARK`], translucent, to be laid over what is beneath it
pub fn band(theme: &cosmic::Theme) -> Color {
    let a = accent(theme);
    Color { a: if theme.cosmic().is_dark { BAND_DARK } else { BAND_LIGHT }, ..color(a) }
}

/// The lightness and chroma of a selected cell's status colour in a dark theme, one pair for all
/// three, each keeping its own hue (Tom, 8 Nov: checked #58a672, unfinished #b38b39, failed
/// #cd7670 from the theme's success, #f0b429 and #f25f5c)
const DARK_STATUS_LIGHTNESS: f32 = 0.66;
const DARK_STATUS_CHROMA: f32 = 0.11;

/// `source`'s hue at [`DARK_STATUS_LIGHTNESS`] and [`DARK_STATUS_CHROMA`]: a status colour for a
/// dark theme
pub fn dark_status(source: Color) -> Color {
    let hue = srgb(source).to_oklch().hue;
    color(household_tones::Oklch::new(DARK_STATUS_LIGHTNESS, DARK_STATUS_CHROMA, hue).to_srgb())
}

/// `bright` at 35 % over `page`, opaque: a status colour faded
pub fn faded(bright: Color, page: Color) -> Color {
    over(Color { a: FADED, ..bright }, page)
}

/// The muted tone of a status colour, as [`muted`] makes one of the accent
/// (`household_tones::muted_accent`): the colour of the messages under a cell
pub fn muted_status(bright: Color, dark: bool) -> Color {
    color(household_tones::muted_accent(srgb(bright), dark))
}

/// A cell's status strip at rest: halfway between the status colour faded over the page and its
/// muted tone (S2(e), Juliet 8 Nov; light: checked #609677, unfinished #aa904c, failed #b97676)
pub fn strip_at_rest(bright: Color, page: Color, dark: bool) -> Color {
    let (a, b) = (faded(bright, page), muted_status(bright, dark));
    Color { r: (a.r + b.r) / 2.0, g: (a.g + b.g) / 2.0, b: (a.b + b.b) / 2.0, a: 1.0 }
}

// ----- the muted accent

/// The muted accent of the theme's accent, and the colour of text and symbolic icons on it
/// (`household_tones::{muted_accent, on_muted}`)
pub fn muted(theme: &cosmic::Theme) -> (Color, Color) {
    let fill = household_tones::muted_accent(accent(theme), theme.cosmic().is_dark);
    (color(fill), color(household_tones::on_muted(fill)))
}

/// `wash`, one of COSMIC's translucent hover or pressed washes, laid over the muted accent: how a
/// chosen part hovers and is pressed ("hover and selected are cosmic choices", rule 6a)
pub fn muted_washed(theme: &cosmic::Theme, wash: impl Into<Color>) -> Color {
    over(wash.into(), muted(theme).0)
}

/// What a label or icon is drawn in on the full accent: the fill rule on a solid fill, greys going
/// to its on colour (`household_tones::on_solid`)
pub fn on_accent(theme: &cosmic::Theme, text: Color) -> Color {
    color(household_tones::on_solid(srgb(text), accent(theme)))
}

/// A button filled with the muted accent, its label and icon in the fill's on colour: the chosen
/// tab or segment of a row, a toggle that is on. It keeps `class`'s shape, padding and focus ring;
/// hovered and pressed, COSMIC's washes for that class lie over the fill; disabled, fill and
/// label are faded as every disabled control is
pub fn muted_button(class: cosmic::theme::Button) -> cosmic::theme::Button {
    use button::Catalog as _;
    let class = Rc::new(class);
    fn filled(mut style: button::Style, fill: Color, on: Color) -> button::Style {
        style.background = Some(Background::Color(fill));
        style.text_color = Some(on);
        style.icon_color = Some(on);
        style
    }
    // The wash COSMIC lays over a button of the class when it is hovered and pressed
    fn washes(theme: &cosmic::Theme, class: &cosmic::theme::Button) -> (Color, Color) {
        let c = theme.cosmic();
        let component = match class {
            cosmic::theme::Button::Text => &c.text_button,
            cosmic::theme::Button::Standard => &c.button,
            _ => &c.icon_button,
        };
        (component.hover.into(), component.pressed.into())
    }
    let (active, disabled, hovered, pressed) = (class.clone(), class.clone(), class.clone(), class);
    cosmic::theme::Button::Custom {
        active: Box::new(move |focused, theme| {
            let (fill, on) = muted(theme);
            filled(theme.active(focused, false, &active), fill, on)
        }),
        disabled: Box::new(move |theme| {
            let (fill, on) = muted(theme);
            let page: Color = theme.cosmic().bg_color().into();
            let faded = |c: Color, under: Color| color(household_tones::dimmed_over(srgb(c), srgb(under)));
            filled(theme.disabled(&disabled), faded(fill, page), faded(on, fill))
        }),
        hovered: Box::new(move |focused, theme| {
            let fill = muted_washed(theme, washes(theme, &hovered).0);
            filled(theme.hovered(focused, false, &hovered), fill, muted(theme).1)
        }),
        pressed: Box::new(move |focused, theme| {
            let fill = muted_washed(theme, washes(theme, &pressed).1);
            filled(theme.pressed(focused, false, &pressed), fill, muted(theme).1)
        }),
    }
}

/// A segmented control in COSMIC's Control look, its chosen part filled with the muted accent and
/// labelled in the fill's on colour (rules 5 and 6); the other parts, the bar and the hover and
/// pressed washes are COSMIC's
pub fn muted_segmented() -> cosmic::theme::SegmentedButton {
    use cosmic::widget::segmented_button::StyleSheet as _;
    cosmic::theme::SegmentedButton::Custom(Box::new(|theme| {
        let mut appearance = theme.horizontal(&cosmic::theme::SegmentedButton::Control);
        let (fill, on) = muted(theme);
        appearance.active.background = Some(Background::Color(fill));
        appearance.active.text_color = on;
        appearance
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: impl Into<Color>) -> String {
        let c: Color = c.into();
        let [r, g, b, _] = c.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    fn texts(theme: &cosmic::Theme) -> Vec<String> {
        let c = theme.cosmic();
        let mut all = Vec::new();
        for transparent in [false, true] {
            for container in [c.background(transparent), c.primary(transparent), c.secondary(transparent)] {
                all.push(hex(container.on));
                all.push(hex(container.component.on));
            }
        }
        all.extend([&c.button, &c.icon_button, &c.text_button, &c.list_button].map(|b| hex(b.on)));
        all
    }

    /// No pure black (rule 25d, plan §1): every text colour a light theme derives is darkest,
    /// #222222, whether the theme is COSMIC's built-in one or the desktop's, and the desktop's
    /// stays a system theme so libcosmic goes on following the desktop
    #[test]
    fn household_text_is_darkest_in_light_themes() {
        let light = cosmic::Theme::light();
        assert!(texts(&light).iter().any(|t| t != "#222222"), "COSMIC's own light text is not already darkest");
        let toned = household(light);
        assert!(texts(&toned).iter().all(|t| t == "#222222"), "{:?}", texts(&toned));
        assert_eq!(hex(toned.cosmic().on_bg_color()), "#222222");
        let system = cosmic::Theme::system(std::sync::Arc::new(cosmic::Theme::light().cosmic().clone()));
        let toned = household(system);
        assert!(matches!(toned.theme_type, cosmic::theme::ThemeType::System { .. }));
        assert!(texts(&toned).iter().all(|t| t == "#222222"));
        // Applied twice, nothing more changes (it is laid again on each theme the desktop sends)
        assert_eq!(household(toned.clone()), toned);
        // The rest of the theme is COSMIC's, to the 8 bits a channel is saved in (the theme's
        // record writes colours as #RRGGBBAA, as the desktop's saved themes are)
        let (before, after) = (cosmic::Theme::light(), household(cosmic::Theme::light()));
        let (b, a) = (before.cosmic(), after.cosmic());
        assert_eq!(hex(b.accent_color()), hex(a.accent_color()));
        assert_eq!(hex(b.bg_color()), hex(a.bg_color()));
        assert_eq!(hex(b.primary_container_color()), hex(a.primary_container_color()));
        assert_eq!(hex(b.secondary_container_color()), hex(a.secondary_container_color()));
    }

    /// Dark and high-contrast themes keep COSMIC's text for now
    #[test]
    fn other_themes_keep_cosmic_text() {
        for theme in [cosmic::Theme::dark(), cosmic::Theme::light_hc(), cosmic::Theme::dark_hc()] {
            assert_eq!(household(theme.clone()), theme);
        }
    }

    /// Prose is darkest below 18 px and dark (#525252) from 18 px on a light theme; captions and
    /// secondary text are darkest, not a translucent grey
    #[test]
    fn body_and_secondary_text_take_the_household_tones() {
        let light = household(cosmic::Theme::light());
        assert_eq!(hex(body_text(&light, 17.0)), "#222222");
        assert_eq!(hex(body_text(&light, 20.0)), "#525252");
        assert_eq!(secondary_text(&light, 0.65), Color::from(light.cosmic().on_bg_color()));
        let dark = cosmic::Theme::dark();
        assert_eq!(body_text(&dark, 20.0), Color::from(dark.cosmic().on_bg_color()));
        assert_eq!(secondary_text(&dark, 0.65).a, 0.65);
    }
}

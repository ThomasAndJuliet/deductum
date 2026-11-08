// SPDX-License-Identifier: GPL-3.0-only

//! The cells of the literate view, drawn headless in the light theme, and what their pixels
//! show: square cards on the page, a status strip as each card's left edge, the selected cell
//! in its bright status colour, the construct's band and a selection over it. `CT_SNAPSHOTS=<dir>`
//! keeps the frames (`cells-*`, `band-*`) as PNG files.

use cosmic::iced::Size;
use cosmic::widget::text_editor::{Action, Motion};

use super::editor::Mode;
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;
use super::App;

const SIZE: Size = Size::new(900.0, 760.0);
const BAND_SIZE: Size = Size::new(900.0, 560.0);

/// A harness (which holds the UI's turn, so that no other test changes the theme under it) with
/// the light theme set, for `f`; the dark theme, the tests' default, is set again after
fn in_light<T>(size: Size, f: impl FnOnce(&mut Harness) -> T) -> T {
    let mut h = Harness::new(size);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
    let out = f(&mut h);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    out
}

/// The demo theory (a checked, an unfinished and a failed proof) with `select` of its formal
/// blocks (in order) the selected cell, or none, and no sidebars
fn cells_app(select: Option<usize>) -> App {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    let ws = app.workspace.as_mut().unwrap();
    ws.output = None;
    let t = ws.theory.as_mut().unwrap();
    t.cursor = 0;
    t.selected = select.map(|n| t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal).map(|(i, _)| i).nth(n).expect("a formal block"));
    app
}

/// A frame of `app`, drawn twice (the first not kept) as the window's glyph cache catches up
fn frame(app: &App, name: &str) -> Vec<u8> {
    in_light(SIZE, |h| {
        let _ = h.draw(app, &format!("{name}-warm"));
        h.draw(app, name)
    })
}

/// The frames the redesign is reviewed from: every cell at rest, then each of the checked, the
/// unfinished and the failed one selected
#[test]
fn cells_frames() {
    for (name, select) in [("cells-rest", None), ("cells-selected-checked", Some(0)), ("cells-selected-sorry", Some(1)), ("cells-selected-failed", Some(2))] {
        frame(&cells_app(select), name);
    }
    // (the dark theme, which the harness draws by default, is kept for a look, not measured)
    for (name, select) in [("cells-rest-dark", None), ("cells-selected-sorry-dark", Some(1))] {
        let mut h = Harness::new(SIZE);
        let app = cells_app(select);
        let _ = h.draw(&app, &format!("{name}-warm"));
        h.draw(&app, name);
    }
}

/// The proof being edited with the caret in its `have` step (so its construct is banded), and
/// part of a line selected
fn band_app(select: bool) -> App {
    let mut app = super::highlight_main_tests::proof_app(Mode::Literate);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let i = t.blocks.iter().position(|b| b.kind == Kind::Formal && b.start <= t.cursor && t.cursor < b.end).expect("the proof's block");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    if select {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        for _ in 0..4 {
            be.content.perform(Action::Move(Motion::Down));
        }
        be.content.perform(Action::Move(Motion::Home));
        for _ in 0..10 {
            be.content.perform(Action::Select(Motion::Right));
        }
    }
    app
}

fn band_frame(app: &App, name: &str) -> Vec<u8> {
    in_light(BAND_SIZE, |h| {
        h.focus(app, super::view::block_editor_id());
        let _ = h.draw(app, &format!("{name}-warm"));
        h.draw(app, name)
    })
}

#[test]
fn band_frames() {
    band_frame(&band_app(false), "band-caret");
    band_frame(&band_app(true), "band-selection");
}

// ----- what the pixels show

use cosmic::iced::{Color, Rectangle};

use super::theory::Status;
use super::view::{card_border_color, muted_status_color, sorry_color, strip_color};

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn px(frame: &[u8], width: u32, x: f32, y: f32) -> [u8; 3] {
    let i = ((y as u32 * width + x as u32) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2]]
}

/// Whether a pixel is `c`, to a unit of rounding in each channel
fn is(p: [u8; 3], c: Color) -> bool {
    let [r, g, b, _] = c.into_rgba8();
    [r, g, b].iter().zip(p).all(|(a, b)| a.abs_diff(b) <= 1)
}

/// Whether a pixel within a pixel of (`x`, `y`) is `c`: a cell sits at a fractional place on the
/// page (the prose above it is not a whole number of pixels tall), so its 1 px border is drawn
/// across two rows of pixels, and the one nearest the edge may be a blend
fn near(frame: &[u8], width: u32, x: f32, y: f32, c: Color) -> bool {
    (-1..=1).any(|dy| (-1..=1).any(|dx| is(px(frame, width, x + dx as f32, y + dy as f32), c)))
}

/// How much of a 1 px line of `line` over `page` crosses (`x`, `y`), as a share of its full
/// strength, summed over the five pixels across it (`vertical`: the line runs up and down). A
/// cell sits at a fractional place on the page (the prose above it is not a whole number of
/// pixels tall), so the renderer spreads its 1 px border over two rows of pixels, each a blend
fn line_strength(frame: &[u8], width: u32, x: f32, y: f32, vertical: bool, line: Color, page: Color) -> f32 {
    let (l, p) = (line.into_rgba8()[0] as f32, page.into_rgba8()[0] as f32);
    (-2..=2)
        .map(|d| {
            let (dx, dy) = if vertical { (d as f32, 0.0) } else { (0.0, d as f32) };
            (p - px(frame, width, x + dx, y + dy)[0] as f32) / (p - l)
        })
        .sum()
}

/// The containers of a frame with ids, with their bounds
struct Bounds(Vec<(cosmic::widget::Id, Rectangle)>);

impl cosmic::iced::advanced::widget::Operation<()> for Bounds {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
        f(self);
    }
    fn container(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle) {
        if let Some(id) = id {
            self.0.push((id.clone(), bounds));
        }
    }
}

/// The bounds of the `n`th formal cell (the checked, unfinished and failed one of the demo) and
/// the frame, with the `select`th of them selected
fn cell(n: usize, select: Option<usize>) -> (Rectangle, Vec<u8>) {
    let app = cells_app(select);
    in_light(SIZE, |h| {
        let _ = h.draw(&app, "cells-probe");
        let frame = h.draw(&app, "cells-probe");
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal).map(|(i, _)| i).nth(n).unwrap();
        let mut all = Bounds(Vec::new());
        h.operate(&app, &mut all);
        let id = super::view::test_block_id(i);
        (all.0.iter().find(|(x, _)| *x == id).expect("the cell is on the page").1, frame)
    })
}

/// The strip's tone at rest, checked, unfinished and failed, as the design records them
#[test]
fn strips_at_rest_are_halfway_between_faded_and_muted() {
    let t = cosmic::Theme::light();
    for (status, want) in [(Status::Finished, 0x609677), (Status::Warned, 0xaa904c), (Status::Failed, 0xb97676)] {
        let got = strip_color(status, false, &t);
        assert!(is([(want >> 16) as u8, (want >> 8) as u8, want as u8], got), "{status:?}: {} is not #{want:06x}", hex(got));
    }
}

#[test]
fn the_bright_status_colours_are_the_designs() {
    let t = cosmic::Theme::light();
    assert_eq!(hex(strip_color(Status::Finished, true, &t)), "#00572c");
    assert_eq!(hex(strip_color(Status::Warned, true, &t)), "#ab7d01");
    assert_eq!(hex(strip_color(Status::Failed, true, &t)), "#c01c28");
    // (the messages under a cell: the muted tone, unfinished #805c00)
    assert_eq!(hex(muted_status_color(sorry_color(&t), &t)), "#805c00");
    // (the grey border at rest is COSMIC's divider on the page, #bcbcbc)
    assert_eq!(hex(card_border_color(&t)), "#bcbcbc");
}

/// Tom's dark status colours (8 Nov): OKLCH lightness 0.66, chroma 0.11, each in the hue of the
/// theme's success colour, #f0b429 and #f25f5c; to 8 bits
#[test]
fn the_dark_status_colours_are_one_lightness_and_chroma_in_each_hue() {
    let t = cosmic::Theme::dark();
    assert_eq!(hex(strip_color(Status::Finished, true, &t)), "#58a672");
    assert_eq!(hex(strip_color(Status::Warned, true, &t)), "#b38b39");
    assert_eq!(hex(strip_color(Status::Failed, true, &t)), "#cd7670");
    // (the strip at rest follows from them by the same halfway rule, so is not the selected tone)
    for status in [Status::Finished, Status::Warned, Status::Failed] {
        assert_ne!(strip_color(status, false, &t), strip_color(status, true, &t));
    }
}

/// A cell at rest: a grey 1 px border on its top, right and bottom, and its status strip, in the
/// at-rest tone, as its left edge with no border beside it
#[test]
fn a_cell_at_rest_has_a_grey_border_and_its_strip_at_the_left_edge() {
    let t = cosmic::Theme::light();
    let (b, frame) = cell(0, None);
    let w = SIZE.width as u32;
    let mid = (b.y + b.height / 2.0).floor();
    let grey = card_border_color(&t);
    // (1 px in from the cell's top, bottom and right, where the border is when not selected)
    let at = |x: f32, y: f32| px(&frame, w, x, y);
    let page = super::view::page_color(&t);
    for (what, x, y, vertical) in [("top", b.x + b.width / 2.0, b.y + 1.0, false), ("bottom", b.x + b.width / 2.0, b.y + b.height - 2.0, false), ("right", b.x + b.width - 2.0, mid, true)] {
        let strength = line_strength(&frame, w, x, y, vertical, grey, page);
        assert!((0.75..=1.05).contains(&strength), "{what}: a grey line of {strength:.2} of its strength (one line, #bcbcbc, is 1)");
    }
    // (the card's fill is the page's, not the old plate)
    assert!(is(at(b.x + b.width - 6.0, mid), page), "the card is not on the page: {:?}", at(b.x + b.width - 6.0, mid));
    // the strip: the card's left edge, 4 px wide, in the at-rest tone to the top and bottom borders
    let strip = strip_color(Status::Finished, false, &t);
    for dx in [0.0, 3.0] {
        for y in [b.y + 4.0, mid, b.y + b.height - 5.0] {
            assert!(is(at(b.x + dx, y), strip), "strip at +{dx},{y}: {:?}", at(b.x + dx, y));
        }
    }
    // no border on the left, and none between the strip and the card
    assert!(!is(at(b.x - 1.0, mid), grey));
    assert!(!is(at(b.x + 4.0, mid), grey));
}

/// The selected cell, checked and unfinished: its strip and its 2 px border in the bright status
/// colour, no accent
#[test]
fn a_selected_cell_has_its_strip_and_border_in_its_bright_status_colour() {
    let t = cosmic::Theme::light();
    for (n, status) in [(0, Status::Finished), (1, Status::Warned)] {
        let (b, frame) = cell(n, Some(n));
        let w = SIZE.width as u32;
        let bright = strip_color(status, true, &t);
        let mid = (b.y + b.height / 2.0).floor();
        for (what, x, y) in [
            ("strip", b.x + 1.0, mid),
            ("top", b.x + b.width / 2.0, b.y + 1.0),
            ("bottom", b.x + b.width / 2.0, b.y + b.height - 1.0),
            ("right", b.x + b.width - 1.0, mid),
        ] {
            assert!(near(&frame, w, x, y, bright), "{status:?} {what}: {:?} is not {}", px(&frame, w, x, y), hex(bright));
        }
        // (the fill is still the page's: no accent wash on the card)
        assert!(is(px(&frame, w, b.x + b.width - 6.0, mid), super::view::page_color(&t)));
    }
}

/// How many pixels of a frame are `c`
fn count(frame: &[u8], c: Color) -> usize {
    frame.chunks(4).filter(|p| is([p[0], p[1], p[2]], c)).count()
}

fn luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// The construct the goal is for is banded with the accent at 10 % over the page; a selection of
/// the user's, over the band, is the 32 % tint computed over the band, a step darker
#[test]
fn the_construct_band_is_the_ten_percent_tint_and_a_selection_is_darker_over_it() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let band = super::tones::over(super::tones::band(&t), page);
    assert_eq!(super::tones::band(&t).a, 0.10);
    let caret = band_frame(&band_app(false), "band-probe");
    assert!(count(&caret, band) > 3000, "the band is not {} (10 % of the accent over the page)", hex(band));
    let selected = band_frame(&band_app(true), "band-probe");
    let tint_over_band = super::tones::tint(&t, band);
    assert!(count(&selected, tint_over_band) > 300, "the selection over the band is not the tint over the band, {}", hex(tint_over_band));
    assert!(luminance(tint_over_band) < luminance(band), "{} is not darker than the band {}", hex(tint_over_band), hex(band));
}

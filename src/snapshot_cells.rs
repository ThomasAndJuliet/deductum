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
const TALL: Size = Size::new(900.0, 1000.0);
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

/// A failed cell's message is inside its card: under the code, in the muted status colour, enclosed
/// by the same border, its strip running down beside it, and with no plate of its own
#[test]
fn a_failed_cells_message_is_inside_its_border_with_no_plate() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let muted = muted_status_color(super::view::error_color(&t), &t);
    assert_eq!(hex(muted), "#964844", "the muted failed colour");
    assert_eq!(hex(muted_status_color(sorry_color(&t), &t)), "#805c00", "the muted unfinished colour");
    let (b, frame) = cell(2, None);
    let w = SIZE.width as u32;
    // (the card is taller than its two lines of code: the message is in it)
    let code_only = 2.0 * super::view::code_line_height() + super::view::CODE_PADDING.top + super::view::CODE_PADDING.bottom;
    assert!(b.height > code_only + 14.0, "the card is {} tall, as tall as its code alone ({code_only})", b.height);
    // the message's text, in the muted colour, within the card's bounds
    let [r, g, bl, _] = muted.into_rgba8();
    let mut text_pixels = 0;
    for y in (b.y as u32)..((b.y + b.height) as u32) {
        for x in (b.x as u32)..((b.x + b.width) as u32) {
            let p = px(&frame, w, x as f32, y as f32);
            if p.iter().zip([r, g, bl]).all(|(a, b)| a.abs_diff(b) <= 24) && y as f32 > b.y + code_only - 4.0 {
                text_pixels += 1;
            }
        }
    }
    assert!(text_pixels > 30, "only {text_pixels} pixels of the message's colour {} inside the card", hex(muted));
    // no plate: beside the message's text, in its row, the card's fill is the page
    let row = b.y + b.height - 12.0;
    assert!(is(px(&frame, w, b.x + b.width * 0.7, row), page), "the message's row is not the page: {:?}", px(&frame, w, b.x + b.width * 0.7, row));
    // the strip runs down beside the message, and the bottom border is below it
    let strip = strip_color(Status::Failed, false, &t);
    assert!(is(px(&frame, w, b.x + 1.0, row), strip), "the strip beside the message: {:?}", px(&frame, w, b.x + 1.0, row));
    let grey = card_border_color(&t);
    let ink = line_strength(&frame, w, b.x + b.width * 0.7, b.y + b.height - 2.0, false, grey, page);
    assert!((0.75..=1.05).contains(&ink), "the border under the message: {ink:.2} of a grey line");
}

/// The proof being edited with the caret on line `line` of it (its construct, `show` to `qed`,
/// is lines 4 to 7): the frame, and the open editor's bounds
fn caret_frame(line: usize) -> (Vec<u8>, Rectangle) {
    struct Focusables(Vec<(Option<cosmic::widget::Id>, Rectangle)>);
    impl cosmic::iced::advanced::widget::Operation<()> for Focusables {
        fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
            f(self);
        }
        fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle, _: &mut dyn cosmic::iced::advanced::widget::operation::Focusable) {
            self.0.push((id.cloned(), bounds));
        }
    }
    let mut app = super::highlight_main_tests::proof_app(Mode::Literate);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let i = t.blocks.iter().position(|b| b.kind == Kind::Formal && b.start <= t.cursor && t.cursor < b.end).expect("the proof's block");
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
    for _ in 0..line {
        be.content.perform(Action::Move(Motion::Down));
    }
    in_light(BAND_SIZE, |h| {
        h.focus(&app, super::view::block_editor_id());
        let _ = h.draw(&app, "caret-line-warm");
        let frame = h.draw(&app, &format!("caret-line-{line}"));
        let mut found = Focusables(Vec::new());
        h.operate(&app, &mut found);
        let id = super::view::block_editor_id();
        (frame, found.0.iter().find(|(i, _)| i.as_ref() == Some(&id)).expect("the editor is on the page").1)
    })
}

/// The pixel at the middle of code line `line` of an editor, `dx` from its left edge
fn on_line(frame: &[u8], editor: Rectangle, line: usize, dx: f32) -> [u8; 3] {
    px(frame, BAND_SIZE.width as u32, editor.x + dx, editor.y + super::view::CODE_PADDING.top + (line as f32 + 0.5) * super::view::code_line_height())
}

/// The automatic band runs from the strip's right edge to the card's inner right edge, with no
/// page-coloured gap, and the text does not move: at the left, 4 px of the card's padding lie
/// between the strip and the editor, and the band covers them
#[test]
fn the_band_runs_edge_to_edge_inside_the_card() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let band = super::tones::over(super::tones::band(&t), page);
    let (frame, editor) = caret_frame(0);
    // (line 5 is in the construct, and the caret is not on it)
    for dx in [-4.0, -3.0, -1.0, 0.0, 2.0, editor.width - 2.0, editor.width - 1.0] {
        let p = on_line(&frame, editor, 5, dx);
        assert!(is(p, band), "line 5 at {dx} from the editor's left edge is {p:?}, not the band {}", hex(band));
    }
    // (and the status strip is the card's left edge, right of which the band begins)
    let strip = on_line(&frame, editor, 5, -6.0);
    assert!(is(strip, strip_color(Status::Finished, true, &t)), "the strip: {strip:?}");
    // (the page, not the band, outside the construct: the band is not the whole editor)
    assert!(is(on_line(&frame, editor, 1, -4.0), page), "line 1 is outside the construct");
    // (the text did not move: the first character of line 5 is 10 px in, as the padding is)
    assert_eq!(super::view::CODE_PADDING.left, 10.0);
}

/// The caret's line is COSMIC's grey hover wash: over the band where the line is in the construct,
/// darker than the band, and over the page outside it; edge to edge as the band is
#[test]
fn the_caret_line_is_the_grey_wash_over_the_band_or_the_page() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let band = super::tones::over(super::tones::band(&t), page);
    let wash = super::tones::caret_line(&t);
    assert_eq!(hex(Color { a: 1.0, ..wash }), "#636363");
    assert!((wash.a - 0.2).abs() < 0.01, "COSMIC's hover wash is 20 %, not {}", wash.a);
    let (frame, editor) = caret_frame(6);
    let on_band = super::tones::over(wash, band);
    for dx in [-4.0, 0.0, editor.width - 1.0] {
        let p = on_line(&frame, editor, 6, dx);
        assert!(is(p, on_band), "the caret's line at {dx}: {p:?}, not the wash over the band {}", hex(on_band));
    }
    assert!(luminance(on_band) < luminance(band), "the wash is not darker than the band");
    // (the lines either side are the band alone)
    assert!(is(on_line(&frame, editor, 5, -4.0), band) && is(on_line(&frame, editor, 7, 0.0), band));
    // outside a construct: the grey wash over the page
    let (frame, editor) = caret_frame(1);
    let on_page = super::tones::over(wash, page);
    for dx in [-4.0, editor.width - 1.0] {
        let p = on_line(&frame, editor, 1, dx);
        assert!(is(p, on_page), "outside a construct, the caret's line at {dx}: {p:?}, not the wash over the page {}", hex(on_page));
    }
}

// ----- the colours of code

/// A block of code being edited, all of it one Isabelle token class, in the light theme: the
/// frame, drawn headless
fn class_frame(sem: super::editor::Sem) -> Vec<u8> {
    use super::editor::{HighlightData, HighlightSettings, Hl, next_generation};
    const TEXT: &str = "MMMMMMMM WWWWWWWW\nHHHHHHHH MMMMMMMM\nWWWWWWWW HHHHHHHH";
    let mut app = cells_app(None);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let i = t.blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let be = t.block_editor.as_mut().expect("an open block");
    be.content = cosmic::widget::text_editor::Content::with_text(TEXT);
    be.preview = None;
    let lines = TEXT.split('\n').map(|l| vec![(0..l.len(), Hl::Sem(sem))]).collect();
    t.block_highlight = HighlightSettings(std::sync::Arc::new(HighlightData { generation: next_generation(), lines }), false);
    in_light(BAND_SIZE, |h| {
        h.focus(&app, super::view::block_editor_id());
        let _ = h.draw(&app, "code-warm");
        h.draw(&app, &format!("code-{sem:?}").to_lowercase())
    })
}

/// Isabelle's classes are drawn in the household's code palette (rule 25b): keywords, a free
/// variable, bound and inner syntax, fixed and schematic variables and type variables,
/// in their light-mode colours
#[test]
fn isabelle_tokens_are_drawn_in_the_household_colours() {
    use super::editor::Sem;
    for (sem, rgb) in [
        (Sem::Keyword1, 0x7a2fd6),
        (Sem::Keyword2, 0x7a2fd6),
        (Sem::Free, 0x1a5aa8),
        (Sem::Bound, 0x2a6e2e),
        (Sem::Inner, 0x2a6e2e),
        (Sem::Skolem, 0xa64500),
        (Sem::Var, 0xa64500),
        (Sem::TFree, 0x7f5500),
        (Sem::Dynamic, 0x7f5500),
    ] {
        let frame = class_frame(sem);
        let want = Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        // (the caret's line has COSMIC's grey wash over it, so its stems are a little off the colour)
        // (text is anti-aliased and its gamma darkens it a little: the pixels within 16 units of
        // the colour in each channel are the stems of the capitals; no other class's colour is
        // that near, and the page and the cell's strip are not)
        let [r, g, b, _] = want.into_rgba8();
        let n = frame.chunks(4).filter(|p| [r, g, b].iter().zip(p.iter()).all(|(a, b)| a.abs_diff(*b) <= 16)).count();
        assert!(n > 20, "{sem:?}: only {n} pixels are {}", hex(want));
    }
}

// ----- a cell's message, folded

fn bounds_of(h: &mut Harness, app: &App, id: cosmic::widget::Id) -> Rectangle {
    let mut all = Bounds(Vec::new());
    h.operate(app, &mut all);
    all.0.iter().find(|(x, _)| *x == id).unwrap_or_else(|| panic!("{id:?} is not on the page")).1
}

/// The index of the demo's failed block, and where its error starts and what it says
fn failed_cell(app: &App) -> (usize, usize, String) {
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let i = t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal).map(|(i, _)| i).nth(2).unwrap();
    let m = t.messages_in(t.blocks[i].start, t.blocks[i].end).into_iter().find(|m| m.kind == "error").expect("the failed cell's error");
    (i, m.start, m.text)
}

fn centre(r: Rectangle) -> cosmic::iced::Point {
    cosmic::iced::Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// A long error is one line while collapsed (the card is its code and one row), opens in place
/// inside the card's border when its summary is pressed, and its copy icon puts the prover's whole
/// message on the clipboard
#[test]
fn a_long_error_is_one_line_opens_inside_the_card_and_copies_whole() {
    let t = cosmic::Theme::light();
    let mut app = cells_app(None);
    let (i, start, text) = failed_cell(&app);
    let w = TALL.width as u32;
    in_light(TALL, |h| {
        let _ = h.draw(&app, "msg-warm");
        let _ = h.draw(&app, "msg-collapsed");
        let cell_id = super::view::test_block_id(i);
        let before = bounds_of(h, &app, cell_id.clone());
        // one row of message under two lines of code
        let code_only = 2.0 * super::view::code_line_height() + super::view::CODE_PADDING.top + super::view::CODE_PADDING.bottom;
        assert!(before.height < code_only + 40.0, "the collapsed card is {} tall, code alone {code_only}: more than one line of message", before.height);
        assert!(text.lines().count() > 1, "the demo's error is long");
        let toggle = bounds_of(h, &app, super::view::test_message_id(start, "toggle"));
        let copy = bounds_of(h, &app, super::view::test_message_id(start, "copy"));
        assert!(copy.x > toggle.x + toggle.width - 1.0, "the copy icon is at the right of the summary");

        // pressing the summary opens it
        let pressed = h.click(&app, centre(toggle));
        assert!(pressed.iter().any(|m| matches!(m, super::Message::ToggleMessage(s) if *s == start)), "pressing the summary toggles the message");
        let _ = cosmic::Application::update(&mut app, super::Message::ToggleMessage(start));
        let _ = h.draw(&app, "msg-open-warm");
        let open = h.draw(&app, "msg-expanded");
        let after = bounds_of(h, &app, cell_id);
        assert!(after.height > before.height + 30.0, "the card did not grow: {} then {}", before.height, after.height);
        // the detail is in the text colour inside the card's border, under the summary's row
        let page = super::view::page_color(&t);
        let mut dark = 0;
        for y in ((before.y + before.height - 8.0) as u32)..((after.y + after.height - 4.0) as u32) {
            for x in (after.x as u32 + 6)..((after.x + after.width) as u32 - 2) {
                let p = px(&open, w, x as f32, y as f32);
                if luminance(Color::from_rgb8(p[0], p[1], p[2])) < luminance(page) - 0.4 {
                    dark += 1;
                }
            }
        }
        assert!(dark > 40, "only {dark} pixels of text colour in the opened detail");
        // the border encloses it: the bottom border is under the detail, at the card's new bottom
        let grey = card_border_color(&t);
        let strength = line_strength(&open, w, after.x + after.width * 0.9, after.y + after.height - 2.0, false, grey, page);
        assert!((0.75..=1.05).contains(&strength), "the card's border under the detail: {strength:.2}");
        let strip = strip_color(Status::Failed, false, &t);
        assert!(is(px(&open, w, after.x + 1.0, after.y + after.height - 8.0), strip), "the strip runs beside the detail");

        // the copy icon copies the whole message as plain text
        let copy = bounds_of(h, &app, super::view::test_message_id(start, "copy"));
        let pressed = h.click(&app, centre(copy));
        let Some(super::Message::CopyText(copied)) = pressed.into_iter().find(|m| matches!(m, super::Message::CopyText(_))) else { panic!("the copy icon sends no copy") };
        use super::codec::Symbols as _;
        let table = super::prover::isabelle::symbols::Table::fallback();
        assert_eq!(copied, table.decode(text.trim_end()).text);
        assert!(copied.contains("goal (1 subgoal):") && copied.contains("1. False"), "the whole message, not its summary: {copied:?}");
        let task = cosmic::Application::update(&mut app, super::Message::CopyText(copied.clone()));
        h.run_task(&app, task);
        assert_eq!(h.copied_text(), Some(copied.as_str()), "the clipboard has the whole message");
        // pressing it again folds it
        let _ = cosmic::Application::update(&mut app, super::Message::ToggleMessage(start));
        let _ = h.draw(&app, "msg-fold-warm");
        let folded = bounds_of(h, &app, super::view::test_block_id(i));
        assert!((folded.height - before.height).abs() < 1.0, "folded again: {} vs {}", folded.height, before.height);
    });
}

// ----- the actions under a cell

/// Whether `p` is on the line from `page` to `ink` (a pixel of text in `ink` over the page, or one
/// of its anti-aliased edges), to a few units
fn on_ink_line(p: [u8; 3], page: Color, ink: Color) -> bool {
    let (page, ink) = (page.into_rgba8(), ink.into_rgba8());
    let d: Vec<f32> = (0..3).map(|c| ink[c] as f32 - page[c] as f32).collect();
    let n: f32 = d.iter().map(|x| x * x).sum();
    if n < 1.0 {
        return true;
    }
    let t = (0..3).map(|c| (p[c] as f32 - page[c] as f32) * d[c]).sum::<f32>() / n;
    let t = t.clamp(0.0, 1.0);
    (0..3).all(|c| (page[c] as f32 + t * d[c] - p[c] as f32).abs() <= 14.0)
}

/// Under the selected failed cell: "+ Text", "+ Section" and "+ Lemma" are links in the accent (no
/// pill fill behind them, an underline under each), and Delete is a link in the destructive red at
/// the cell's right edge, a space_m or more from the last of them
#[test]
fn the_add_links_are_accent_with_no_fill_and_delete_is_red_at_the_right() {
    let t = cosmic::Theme::light();
    let app = cells_app(Some(2));
    let w = TALL.width as u32;
    in_light(TALL, |h| {
        let _ = h.draw(&app, "actions-warm");
        let frame = h.draw(&app, "actions-selected");
        let (i, ..) = failed_cell(&app);
        let b = bounds_of(h, &app, super::view::test_block_id(i));
        let page = super::view::page_color(&t);
        let accent: Color = cosmic::theme::active().cosmic().accent_text_color().into();
        let red = strip_color(Status::Failed, true, &t);
        assert_eq!(hex(red), "#c01c28");
        // the band under the card where the row is
        let (top, bottom) = ((b.y + b.height + 3.0) as u32, (b.y + b.height + 40.0) as u32);
        let (mut left_ink, mut right_ink) = (Vec::new(), Vec::new());
        let mut stray = Vec::new();
        for y in top..bottom {
            for x in (b.x as u32)..((b.x + b.width) as u32 + 4) {
                let p = px(&frame, w, x as f32, y as f32);
                if is(p, page) {
                    continue;
                }
                // (the accent is teal, so its blue is over its red; the destructive red is the other way)
                if on_ink_line(p, page, accent) && p[2] >= p[0] {
                    left_ink.push((x, y));
                } else if on_ink_line(p, page, red) && p[0] > p[2] {
                    right_ink.push((x, y));
                } else {
                    stray.push((x, y, p));
                }
            }
        }
        assert!(left_ink.len() > 150, "only {} pixels of the accent {} in the row under the cell", left_ink.len(), hex(accent));
        assert!(stray.len() < 6, "{} pixels that are neither the page, the accent nor the red (a pill's fill?): {:?}", stray.len(), &stray[..stray.len().min(4)]);
        // no fill: the pixels between the links, and just above each label, are the page
        let ink_rows: Vec<u32> = left_ink.iter().map(|p| p.1).collect();
        let (y0, y1) = (*ink_rows.iter().min().unwrap(), *ink_rows.iter().max().unwrap());
        let cy = (y0 + y1) / 2;
        let lefts: Vec<u32> = left_ink.iter().filter(|p| p.1 == cy).map(|p| p.0).collect();
        let gaps = lefts.windows(2).filter(|p| p[1] - p[0] > 8).count();
        assert!(gaps >= 2, "three links leave two gaps between them on the row: {gaps}");
        for x in (b.x as u32 + 400)..(b.x as u32 + 440) {
            assert!(is(px(&frame, w, x as f32, cy as f32), page), "the row has a fill at {x}: {:?}", px(&frame, w, x as f32, cy as f32));
        }
        // an underline: a long horizontal run of the accent under the labels
        let underline = (y0..=y1).any(|y| {
            let mut run = 0;
            let mut best = 0;
            for x in (b.x as u32)..((b.x + b.width) as u32) {
                let p = px(&frame, w, x as f32, y as f32);
                if !is(p, page) && on_ink_line(p, page, accent) { run += 1; best = best.max(run) } else { run = 0 }
            }
            best >= 20
        });
        assert!(underline, "no underline under the links");
        // Delete: red, at the cell's right edge, a space_m or more from the links
        assert!(right_ink.len() > 40, "only {} pixels of the destructive red", right_ink.len());
        let (rmin, rmax) = (right_ink.iter().map(|p| p.0).min().unwrap() as f32, right_ink.iter().map(|p| p.0).max().unwrap() as f32);
        let right_edge = b.x + b.width;
        assert!(right_edge - rmax < 8.0, "Delete ends {} px short of the cell's right edge", right_edge - rmax);
        let lmax = left_ink.iter().map(|p| p.0).max().unwrap() as f32;
        assert!(rmin - lmax >= super::view::spacing().space_m as f32, "Delete is {} px from the add links", rmin - lmax);
    });
}

// ----- an agent's note, in its cell's card

/// The agent's note on a cell is inside the cell's card: under its code after a hairline, enclosed
/// by the same border (the 2 px selected one too), the cell's strip running down beside it, with
/// no tinted plate and no violet bar; its heading is the agent's violet and Resolve an accent link
#[test]
fn a_note_is_inside_its_cells_border_with_no_plate_and_a_violet_heading() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let violet = super::view::note_color(&t);
    assert_eq!(hex(violet), "#6348b8");
    let w = SIZE.width as u32;
    let grey = card_border_color(&t);
    // (the demo's note is on its first cell, the checked one)
    let (b, frame) = cell(0, None);
    let code_only = 3.0 * super::view::code_line_height() + super::view::CODE_PADDING.top + super::view::CODE_PADDING.bottom;
    assert!(b.height > code_only + 80.0, "the card is {} tall, as tall as its code alone ({code_only}): the note is not in it", b.height);
    // the heading: pixels of the violet, in the card under the code
    let mut rows = Vec::new();
    for y in ((b.y + code_only) as u32)..((b.y + b.height) as u32) {
        for x in (b.x as u32 + 6)..((b.x + b.width) as u32) {
            if is(px(&frame, w, x as f32, y as f32), violet) {
                rows.push((x, y));
            }
        }
    }
    assert!(rows.len() > 25, "only {} pixels of the agent's violet {} in the card", rows.len(), hex(violet));
    let (top, bottom) = (rows.iter().map(|p| p.1).min().unwrap(), rows.iter().map(|p| p.1).max().unwrap());
    assert!(bottom - top < 16, "the violet is in one heading line, not a plate or bar: rows {top} to {bottom}");
    // no plate, no bar: down the note's rows the card is the page, and its left edge the status strip
    let strip = strip_color(Status::Finished, false, &t);
    for y in ((b.y + code_only + 4.0) as u32)..((b.y + b.height - 6.0) as u32) {
        assert!(is(px(&frame, w, b.x + b.width - 8.0, y as f32), page), "the note has a plate at {y}: {:?}", px(&frame, w, b.x + b.width - 8.0, y as f32));
        assert!(is(px(&frame, w, b.x + 1.0, y as f32), strip), "the strip at {y}: {:?}", px(&frame, w, b.x + 1.0, y as f32));
        // (the card's own 4 px after the strip, where a bar would be, is the page's)
        assert!(is(px(&frame, w, b.x + 6.0, y as f32), page), "a bar beside the note at {y}: {:?}", px(&frame, w, b.x + 6.0, y as f32));
    }
    // the border encloses it: grey on the right beside the note and along the bottom below it
    let note_y = (top + bottom) as f32 / 2.0;
    let right = line_strength(&frame, w, b.x + b.width - 2.0, note_y, true, grey, page);
    assert!((0.75..=1.05).contains(&right), "the card's right border beside the note: {right:.2}");
    let under = line_strength(&frame, w, b.x + b.width * 0.7, b.y + b.height - 2.0, false, grey, page);
    assert!((0.75..=1.05).contains(&under), "the card's bottom border under the note: {under:.2}");
    // the hairline between the code and the note: one grey line across the card
    let hair = (((b.y + code_only) as u32)..top).find(|&y| line_strength(&frame, w, b.x + b.width * 0.5, y as f32, false, grey, page) > 0.75);
    assert!(hair.is_some(), "no hairline between the code and the note");
    // Resolve is an accent link at the heading's right
    let accent: Color = t.cosmic().accent_text_color().into();
    let resolve = (((b.x + b.width * 0.6) as u32)..((b.x + b.width) as u32)).filter(|&x| (top - 3..=bottom + 6).any(|y| { let p = px(&frame, w, x as f32, y as f32); !is(p, page) && on_ink_line(p, page, accent) && p[2] >= p[0] && (p[0] as i32 - accent.into_rgba8()[0] as i32).abs() < 40 })).count();
    assert!(resolve > 10, "Resolve is not drawn in the accent at the heading's right ({resolve} columns)");

    // selected: the 2 px border in the bright status colour encloses the note as well
    let (b, frame) = cell(0, Some(0));
    let bright = strip_color(Status::Finished, true, &t);
    let y = b.y + b.height - 30.0;
    assert!(near(&frame, w, b.x + b.width - 1.0, y, bright), "the selected border beside the note: {:?}", px(&frame, w, b.x + b.width - 1.0, y));
    assert!(near(&frame, w, b.x + b.width * 0.7, b.y + b.height - 1.0, bright), "the selected border under the note");
    assert!(is(px(&frame, w, b.x + 1.0, y), bright), "the strip beside the note is the status colour");
}

// ----- the goal panel

/// The demo's proof being edited with the caret at a step (`apply`), where the bar offers the proof
/// tools, and its two goals shown under the proof
fn goal_app() -> App {
    let mut app = demo_app(Mode::Literate);
    app.show_right = false;
    app.show_sidebar = false;
    app.window_width = 1000.0;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma").unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    super::snapshot::caret_in_block(&mut app, "apply", 0);
    app
}

const PANEL: Size = Size::new(1000.0, 900.0);

/// The goal panel's frame and the bounds of its identified parts, in the theme asked for (the
/// dark one is the tests' default, so it is set again after the light)
fn goal_frame(light: bool) -> (Vec<u8>, Vec<(cosmic::widget::Id, Rectangle)>) {
    let app = goal_app();
    let mut h = Harness::new(PANEL);
    cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
    super::view::set_goal_beside(false);
    let _ = h.draw(&app, "panel-warm");
    let frame = h.draw(&app, if light { "panel-light" } else { "panel-dark" });
    let mut all = Bounds(Vec::new());
    h.operate(&app, &mut all);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    (frame, all.0)
}

fn part(all: &[(cosmic::widget::Id, Rectangle)], id: cosmic::widget::Id) -> Rectangle {
    all.iter().find(|(x, _)| *x == id).unwrap_or_else(|| panic!("{id:?} is not on the page")).1
}

/// The panel is outlined 2 px grey (#bcbcbc light, #5c5c5c dark), and its tools sit on a band in
/// the window chrome's lightish colour (#d7d7d7 light, #272727 dark) that runs edge to edge inside it
#[test]
fn the_goal_panel_has_a_two_pixel_grey_outline_and_a_lightish_band() {
    for light in [true, false] {
        let (frame, all) = goal_frame(light);
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let w = PANEL.width as u32;
        let panel = part(&all, super::view::test_panel_id("panel"));
        let grey = card_border_color(&theme);
        assert_eq!(hex(grey), if light { "#bcbcbc" } else { "#5c5c5c" });
        // two pixels of grey at each side: the panel's edges are at fractions of a pixel, so the
        // strength of the line is summed across the edge (2 is two whole pixels of grey; at the
        // top, the band beside it adds a little)
        let page = super::view::page_color(&theme);
        let (mx, my) = (panel.x + panel.width / 2.0, panel.y + panel.height / 2.0);
        for (what, n) in [
            ("left", line_strength(&frame, w, panel.x + 1.0, my, true, grey, page)),
            ("right", line_strength(&frame, w, panel.x + panel.width - 1.0, my, true, grey, page)),
            ("top", line_strength(&frame, w, mx, panel.y + 1.0, false, grey, page)),
            ("bottom", line_strength(&frame, w, mx, panel.y + panel.height - 1.0, false, grey, page)),
        ] {
            // (the top edge is at a fraction of a pixel under Paper's larger headings: a little more)
            let most = if what == "top" { 2.7 } else { 2.5 };
            assert!((1.6..=most).contains(&n), "light={light}: the {what} outline is {n:.2} px of grey (2 wanted)");
        }
        // the band: COSMIC's background colour, inside the outline, at the panel's top
        let band = part(&all, super::insert::insert_bar_id());
        let bg = super::view::chrome_color(&theme);
        assert_eq!(hex(bg), if light { "#d7d7d7" } else { "#272727" });
        assert!((band.x - (panel.x + 2.0)).abs() < 1.0 && (band.width - (panel.width - 4.0)).abs() < 1.5, "the band is {band:?} in {panel:?}");
        assert!((band.y - (panel.y + 2.0)).abs() < 1.0, "the band is at the panel's top: {band:?} in {panel:?}");
        for x in [band.x + 2.0, band.x + band.width - 3.0] {
            assert!(is(px(&frame, w, x, band.y + 2.0), bg), "light={light}: the band at {x}: {:?}", px(&frame, w, x, band.y + 2.0));
        }
        // and the body under it is not the band's colour
        let sub = part(&all, super::view::test_panel_id("subgoal-1"));
        assert!(!is(px(&frame, w, sub.x + sub.width - 10.0, sub.y + sub.height / 2.0), bg), "light={light}: the body is on the band's colour");
    }
}

/// The tools and the symbol keys are accent links: bold, underlined, with no chip fill (only the
/// search field, a field, is filled)
#[test]
fn the_tools_are_accent_links_with_no_chip_fill() {
    for light in [true, false] {
        let (frame, all) = goal_frame(light);
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let w = PANEL.width as u32;
        let band = part(&all, super::insert::insert_bar_id());
        let bg = super::view::chrome_color(&theme);
        let accent: Color = theme.cosmic().accent_text_color().into();
        // right of the search field (110 px and its padding): the links
        let (x0, x1) = ((band.x + 150.0) as u32, (band.x + band.width - 40.0) as u32);
        let (y0, y1) = (band.y as u32 + 2, (band.y + band.height) as u32 - 2);
        let (mut ink, mut stray) = (0, Vec::new());
        for y in y0..=y1 {
            for x in x0..x1 {
                let p = px(&frame, w, x as f32, y as f32);
                if is(p, bg) {
                    continue;
                }
                if on_ink_line(p, bg, accent) {
                    ink += 1;
                } else {
                    stray.push((x, y, p));
                }
            }
        }
        assert!(ink > 150, "light={light}: only {ink} pixels of the accent {} on the band", hex(accent));
        assert!(stray.len() < 8, "light={light}: {} pixels that are neither the band nor the accent (a chip's fill?): {:?}", stray.len(), &stray[..stray.len().min(4)]);
        // an underline: a row with a long run of the accent
        let underline = (y0..=y1).any(|y| {
            let (mut run, mut best) = (0, 0);
            for x in x0..x1 {
                let p = px(&frame, w, x as f32, y as f32);
                if !is(p, bg) && on_ink_line(p, bg, accent) { run += 1; best = best.max(run) } else { run = 0 }
            }
            best >= 30
        });
        assert!(underline, "light={light}: no underline under the tools");
    }
}

/// Where along the heading line's middle a click sends `wanted`, scanning from the right
fn click_x(h: &mut Harness, app: &App, heading: Rectangle, wanted: impl Fn(&super::Message) -> bool) -> f32 {
    let y = heading.y + heading.height / 2.0;
    let mut x = heading.x + heading.width - 2.0;
    while x > heading.x {
        if h.click(app, cosmic::iced::Point::new(x, y)).iter().any(&wanted) {
            return x;
        }
        x -= 3.0;
    }
    panic!("nothing on the heading line sends the message, in {heading:?}")
}

/// "N goals" is a bold heading at the left of its line; Goals | Text, sized to its two parts, and
/// Types are on the same line at the right, and no row of their own is above the first subgoal
#[test]
fn goals_text_and_types_are_on_the_headings_line() {
    let app = goal_app();
    let mut h = Harness::new(PANEL);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
    super::view::set_goal_beside(false);
    let _ = h.draw(&app, "heading-warm");
    let frame = h.draw(&app, "heading-line");
    let mut all = Bounds(Vec::new());
    h.operate(&app, &mut all);
    let heading = part(&all.0, super::view::test_panel_id("heading"));
    let first = part(&all.0, super::view::test_panel_id("subgoal-1"));
    let (goals, text, types) = (
        click_x(&mut h, &app, heading, |m| matches!(m, super::Message::StateText(false))),
        click_x(&mut h, &app, heading, |m| matches!(m, super::Message::StateText(true))),
        click_x(&mut h, &app, heading, |m| matches!(m, super::Message::ShowTypes(_))),
    );
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    assert!(goals < text && text < types, "Goals {goals}, Text {text}, Types {types}: in that order along the line");
    assert!(types > heading.x + heading.width - 120.0 && goals > heading.x + heading.width * 0.5, "the controls are at the right of the line");
    // sized to their parts: Goals and Text are no more than a part's width apart
    assert!(text - goals < 90.0, "Goals and Text are {} px apart: not sized to their parts", text - goals);
    // one line: it is under 40 px, and the first subgoal follows it directly
    assert!(heading.height < 40.0, "the heading line is {} px tall: two rows", heading.height);
    let band = part(&all.0, super::insert::insert_bar_id());
    assert!(heading.y >= band.y + band.height && heading.y - (band.y + band.height) < 16.0, "the heading line at {} is not directly under the band ending at {}", heading.y, band.y + band.height);
    assert!(first.y >= heading.y + heading.height, "the first subgoal is above the heading line");
    // the heading itself is bold ink at the line's left
    let w = PANEL.width as u32;
    let page = super::view::page_color(&cosmic::Theme::light());
    let mut dark = 0;
    for x in (heading.x as u32 + 4)..(heading.x as u32 + 120) {
        for y in (heading.y as u32)..((heading.y + heading.height) as u32) {
            let p = px(&frame, w, x as f32, y as f32);
            if luminance(Color::from_rgb8(p[0], p[1], p[2])) < luminance(page) - 0.4 {
                dark += 1;
            }
        }
    }
    assert!(dark > 80, "the heading's text: {dark} dark pixels");
}

/// Subgoals are flat: no plate fill (the first has no accent tint either), a hairline in the card
/// divider colour between them, and the labels in the accent
#[test]
fn subgoals_are_flat_with_a_hairline_and_accent_labels() {
    for light in [true, false] {
        let (frame, all) = goal_frame(light);
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let w = PANEL.width as u32;
        let (one, two) = (part(&all, super::view::test_panel_id("subgoal-1")), part(&all, super::view::test_panel_id("subgoal-2")));
        // no plate: well inside each card at the right, away from its text, the colour is the same
        // and the page's
        let page = super::view::page_color(&theme);
        for card in [one, two] {
            for dy in [4.0, card.height / 2.0, card.height - 4.0] {
                let p = px(&frame, w, card.x + card.width - 30.0, card.y + dy);
                assert!(is(p, page), "light={light}: a plate at {dy} of a subgoal: {p:?} on {}", hex(page));
            }
        }
        // the hairline between them: one pixel, in the divider colour, the cards' width
        let line = part(&all, super::view::test_panel_id("hairline-1"));
        assert!((line.height - 1.0).abs() < 0.1 && line.y >= one.y + one.height - 1.0 && line.y + 1.0 <= two.y + 1.0, "the hairline {line:?} between {one:?} and {two:?}");
        let grey = card_border_color(&theme);
        for x in [line.x + 10.0, line.x + line.width / 2.0, line.x + line.width - 10.0] {
            let strength = line_strength(&frame, w, x, line.y, false, grey, page);
            assert!((0.75..=1.1).contains(&strength), "light={light}: the hairline at {x} is {strength:.2} of a grey line");
        }
        // labels in the accent: "· next" beside Subgoal 1's name, "show" in each card's column
        let accent: Color = theme.cosmic().accent_color().into();
        let in_accent = |r: Rectangle| {
            let mut n = 0;
            for y in (r.y as u32)..((r.y + r.height) as u32) {
                for x in (r.x as u32)..((r.x + r.width) as u32) {
                    let p = px(&frame, w, x as f32, y as f32);
                    if !is(p, page) && on_ink_line(p, page, accent) && p[2] >= p[0] {
                        n += 1;
                    }
                }
            }
            n
        };
        let name_row = |c: Rectangle, x0: f32, x1: f32| Rectangle { x: c.x + x0, y: c.y, width: x1 - x0, height: 18.0 };
        assert!(in_accent(name_row(one, 70.0, 200.0)) > 25, "light={light}: '· next' is not in the accent");
        assert_eq!(in_accent(name_row(two, 70.0, 200.0)), 0, "light={light}: the second subgoal has an accent label beside its name");
        let label = |c: Rectangle| Rectangle { x: c.x + 4.0, y: c.y + 18.0, width: 52.0, height: c.height - 18.0 };
        assert!(in_accent(label(one)) > 20 && in_accent(label(two)) > 20, "light={light}: the 'show' labels are not in the accent");
    }
}

/// Subgoal 1 is bold while it is next, and the second is not
#[test]
fn the_next_subgoals_name_is_bold() {
    let (frame, all) = goal_frame(true);
    let w = PANEL.width as u32;
    let page = super::view::page_color(&cosmic::Theme::light());
    let ink = |c: Rectangle| {
        let mut n = 0.0;
        for y in (c.y as u32)..(c.y as u32 + 18) {
            for x in (c.x as u32 + 8)..(c.x as u32 + 62) {
                let p = px(&frame, w, x as f32, y as f32);
                n += 1.0 - luminance(Color::from_rgb8(p[0], p[1], p[2])) / luminance(page).max(0.01);
            }
        }
        n
    };
    let (one, two) = (ink(part(&all, super::view::test_panel_id("subgoal-1"))), ink(part(&all, super::view::test_panel_id("subgoal-2"))));
    assert!(one > two * 1.12, "the next subgoal's name has {one:.1} of ink, the other {two:.1}: not bolder");
}

// ----- a collapsed error's one line: words in the interface font, terms in the code font

use super::editor::{ProverMessage, Sem};

fn words(runs: &[(String, bool)]) -> Vec<(&str, bool)> {
    runs.iter().map(|(t, term)| (t.as_str(), *term)).collect()
}

/// Isabelle quotes terms with "…", Lean with '…': the quotes go and the terms are the code font's
#[test]
fn an_error_summary_sets_quoted_names_and_terms_in_the_code_font_without_the_quotes() {
    let isabelle = |line: &str| {
        let terms = super::view::quoted_terms(line, '"');
        words(&super::view::summary_runs(line, &terms, '"')).into_iter().map(|(t, k)| (t.to_string(), k)).collect::<Vec<_>>()
    };
    assert_eq!(isabelle("Undefined fact: \"map_def\""), vec![("Undefined fact: ".to_string(), false), ("map_def".to_string(), true)]);
    assert_eq!(
        isabelle("Type unification failed: Clash of types \"nat\" and \"bool\""),
        vec![("Type unification failed: Clash of types ".into(), false), ("nat".into(), true), (" and ".into(), false), ("bool".into(), true)]
    );
    assert_eq!(isabelle("Inner syntax error at \"x <\""), vec![("Inner syntax error at ".to_string(), false), ("x <".to_string(), true)]);
    assert_eq!(isabelle("Failed to finish proof"), vec![("Failed to finish proof".to_string(), false)]);
    let lean = |line: &str| {
        let terms = super::view::quoted_terms(line, '\'');
        super::view::summary_runs(line, &terms, '\'')
    };
    assert_eq!(lean("unknown identifier 'candidate'"), vec![("unknown identifier ".to_string(), false), ("candidate".to_string(), true)]);
    // (an apostrophe inside a name is not a quote)
    assert_eq!(lean("the hypothesis h' is unused"), vec![("the hypothesis h' is unused".to_string(), false)]);
    assert_eq!(lean("unknown constant 'h''"), vec![("unknown constant ".to_string(), false), ("h'".to_string(), true)]);
}

/// Where the message carries Isabelle's markup (its colour classes for names, variables and
/// facts), those parts are the terms, whatever the quotes; the position mark the summary cuts
/// does not shift them
#[test]
fn an_error_summary_takes_its_terms_from_the_messages_markup_where_it_has_it() {
    let table = super::prover::isabelle::symbols::Table::fallback();
    let line = "Undefined fact: \"map_def\"";
    let message = ProverMessage { raw: line.into(), text: line.into(), kind: "error".into(), spans: vec![(17, 24, Sem::Dynamic), (0, 9, Sem::Keyword1)], ..Default::default() };
    let terms = super::view::markup_terms(&message, line, &table);
    assert_eq!(terms, vec![17..24], "only the term classes count, not a keyword's");
    let runs = super::view::summary_runs(line, &terms, '"');
    assert_eq!(words(&runs), vec![("Undefined fact: ", false), ("map_def", true)]);
    // markup on something with no quotes
    let line = "Variable x is not fixed";
    let message = ProverMessage { raw: line.into(), text: line.into(), kind: "error".into(), spans: vec![(9, 10, Sem::Free)], ..Default::default() };
    let terms = super::view::markup_terms(&message, line, &table);
    assert_eq!(words(&super::view::summary_runs(line, &terms, '"')), vec![("Variable ", false), ("x", true), (" is not fixed", false)]);
    // the position mark ⌂ (\<^here>), cut from the summary, is not in its line: the spans still land
    let raw = "Undefined fact\u{2302}: \"map_def\"";
    let line = "Undefined fact: \"map_def\"";
    let message = ProverMessage { raw: raw.into(), text: raw.into(), kind: "error".into(), spans: vec![(18, 25, Sem::Dynamic)], ..Default::default() };
    assert_eq!(super::view::markup_terms(&message, line, &table), vec![17..24]);
    // no markup: nothing, and the quotes decide
    let bare = ProverMessage { raw: String::new(), text: line.into(), ..Default::default() };
    assert!(super::view::markup_terms(&bare, line, &table).is_empty());
}

/// The collapsed row as drawn: its words are not in the code font's even advance, its terms are
/// (the same line drawn all in the code font is wider than the words and terms mixed)
#[test]
fn a_collapsed_error_row_draws_sans_words_and_code_terms() {
    let app = cells_app(None);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let table = super::prover::isabelle::symbols::Table::fallback();
    let width = 700.0;
    let draw = |light: bool, text: &str, name: &str| {
        let m = ProverMessage { kind: "error".into(), start: 3, end: 6, text: text.into(), raw: text.into(), ..Default::default() };
        let _turn = super::snapshot::ui_turn();
        cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
        let frame = super::snapshot::render(cosmic::widget::container(super::view::message_strip(t, &m, &table, width)).width(cosmic::iced::Length::Fixed(width)).into(), name, Size::new(width, 40.0));
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
        frame
    };
    // The words are the same in both rows (so they are drawn alike: the interface font), and the
    // term differs (the code font in one, the interface font in the other)
    let differing = |a: &[u8], b: &[u8], from: u32, to: u32| {
        let mut n = 0;
        for y in 4..36 {
            for x in from..to {
                if px(a, width as u32, x as f32, y as f32).iter().zip(px(b, width as u32, x as f32, y as f32)).any(|(p, q)| p.abs_diff(q) > 40) {
                    n += 1;
                }
            }
        }
        n
    };
    for light in [true, false] {
        let name = if light { "error-summary-light" } else { "error-summary-dark" };
        let mixed = draw(light, "Undefined fact: \"map_def\"", name);
        // (no quotes in the text, so no term is found: all the interface font)
        let sans = draw(light, "Undefined fact: map_def", &format!("{name}-sans"));
        assert_eq!(differing(&mixed, &sans, 30, 112), 0, "light={light}: the words are drawn differently");
        let term = differing(&mixed, &sans, 118, 190);
        assert!(term > 60, "light={light}: the term is drawn as the words are ({term} pixels differ): not in the code font");
    }
}

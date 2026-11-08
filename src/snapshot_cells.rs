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
    let code_only = 2.0 * super::view::code_line_height() + super::view::code_padding().top + super::view::code_padding().bottom;
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
    px(frame, BAND_SIZE.width as u32, editor.x + dx, editor.y + super::view::code_padding().top + (line as f32 + 0.5) * super::view::code_line_height())
}

/// The automatic band runs from the strip's right edge to the card's inner right edge, with no
/// page-coloured gap: at the left the editor's own padding (the gap, 0.75 em of the code's size)
/// lies between the strip and the text, and the band covers it
#[test]
fn the_band_runs_edge_to_edge_inside_the_card() {
    let t = cosmic::Theme::light();
    let page = super::view::page_color(&t);
    let band = super::tones::over(super::tones::band(&t), page);
    let (frame, editor) = caret_frame(0);
    // (line 5 is in the construct, and the caret is not on it)
    for dx in [0.0, 1.0, 5.0, 9.0, editor.width - 2.0, editor.width - 1.0] {
        let p = on_line(&frame, editor, 5, dx);
        assert!(is(p, band), "line 5 at {dx} from the editor's left edge is {p:?}, not the band {}", hex(band));
    }
    // (and the status strip is the card's left edge, right of which the band begins)
    let strip = on_line(&frame, editor, 5, -2.0);
    assert!(is(strip, strip_color(Status::Finished, true, &t)), "the strip: {strip:?}");
    // (the page, not the band, outside the construct: the band is not the whole editor)
    assert!(is(on_line(&frame, editor, 1, 1.0), page), "line 1 is outside the construct");
    // (the gap between the strip and the code is the editor's own padding, 0.75 em of the code's
    // size: the band, which covers the whole editor, reaches the strip with no fill between)
    assert!((super::view::code_padding().left - 10.5).abs() < 1e-3, "{}", super::view::code_padding().left);
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
    for dx in [1.0, 0.0, editor.width - 1.0] {
        let p = on_line(&frame, editor, 6, dx);
        assert!(is(p, on_band), "the caret's line at {dx}: {p:?}, not the wash over the band {}", hex(on_band));
    }
    assert!(luminance(on_band) < luminance(band), "the wash is not darker than the band");
    // (the lines either side are the band alone)
    assert!(is(on_line(&frame, editor, 5, 1.0), band) && is(on_line(&frame, editor, 7, 0.0), band));
    // outside a construct: the grey wash over the page
    let (frame, editor) = caret_frame(1);
    let on_page = super::tones::over(wash, page);
    for dx in [1.0, editor.width - 1.0] {
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
        let code_only = 2.0 * super::view::code_line_height() + super::view::code_padding().top + super::view::code_padding().bottom;
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
    let code_only = 3.0 * super::view::code_line_height() + super::view::code_padding().top + super::view::code_padding().bottom;
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
            // (the left edge is the coloured line, below)
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

/// The goal under its cell: the coloured line is the panel's left edge, as a cell's status strip is
/// the cell's. It is [`GOAL_EDGE`] (4 px) wide, with no gap and no grey outline on that side, and
/// the page colour immediately to its left; the panel's left edge is the cell's. The grey outline
/// stays on the top, right and bottom (the test above)
#[test]
fn the_goal_under_a_cell_has_the_coloured_line_as_its_left_edge() {
    for light in [true, false] {
        let (frame, all) = goal_frame(light);
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let w = PANEL.width as u32;
        let panel = part(&all, super::view::test_panel_id("panel"));
        let edge = part(&all, super::view::test_panel_id("edge"));
        let cell = part(&all, super::view::selected_block_id());
        let page = super::view::page_color(&theme);
        let grey = card_border_color(&theme);
        assert!((panel.x - cell.x).abs() < 1.0, "light={light}: the panel's left edge {} is not the cell's {}", panel.x, cell.x);
        assert!((edge.x - panel.x).abs() < 0.5 && (edge.width - super::view::GOAL_EDGE).abs() < 0.5, "light={light}: the line is {edge:?} on the panel {panel:?}");
        // (the line's colour: the accent at 60 % over the page)
        let accent = Color::from(theme.cosmic().accent_color());
        let want = |a: f32, p: f32| a * 0.6 + p * 0.4;
        let line = Color::from_rgb(want(accent.r, page.r), want(accent.g, page.g), want(accent.b, page.b));
        // several rows down the panel, in its band and its body
        for y in [panel.y + 8.0, panel.y + panel.height / 2.0, panel.y + panel.height - 8.0] {
            let x0 = panel.x as u32;
            // the page, immediately left of the line
            for dx in 1..=3 {
                assert!(is(px(&frame, w, (x0 - dx) as f32, y), page), "light={light}, y={y}: {} left of the line is not the page colour: {:?}", dx, px(&frame, w, (x0 - dx) as f32, y));
            }
            // one line of four pixels, in its colour, with no grey in or beside it
            for dx in 0..4 {
                let p = px(&frame, w, (x0 + dx) as f32, y);
                assert!(is(p, line), "light={light}, y={y}: pixel {dx} of the line is {p:?}, not {}", hex(line));
                assert!(!is(p, grey), "light={light}: the grey outline shows on the left edge");
            }
            // and right after it, the panel's own contents, not a second line
            assert!(!is(px(&frame, w, (x0 + 4) as f32, y), line) && !is(px(&frame, w, (x0 + 4) as f32, y), grey), "light={light}, y={y}: a second line after the first");
        }
        // the top, right and bottom keep their grey outline
        let mx = panel.x + panel.width / 2.0;
        assert!(line_strength(&frame, w, mx, panel.y + panel.height - 1.0, false, grey, page) > 1.6, "light={light}: the bottom outline");
    }
}

/// A long proof in a narrow window opens the goal as a gap inside the editor: there too the coloured
/// line is the panel's left border, with no gap between them and no grey on that side (the separate
/// accent rule, the 6 px gap and the panel's full outline were a double line). The panel keeps its
/// grey outline on the other three sides
#[test]
fn the_goal_in_the_open_editor_has_the_coloured_line_as_its_left_edge() {
    let mut app = goal_app();
    app.goal_placement_override = Some(super::view::GoalPlacement::AfterConstruct);
    for light in [true, false] {
        let mut h = Harness::new(PANEL);
        cosmic::theme::set_active(if light { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
        super::view::set_goal_beside(false);
        let _ = h.draw(&app, "gap-panel-warm");
        let frame = h.draw(&app, if light { "gap-panel-light" } else { "gap-panel-dark" });
        let mut all = Bounds(Vec::new());
        h.operate(&app, &mut all);
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
        let theme = if light { cosmic::Theme::light() } else { cosmic::Theme::dark() };
        let all = all.0;
        let w = PANEL.width as u32;
        let panel = part(&all, super::view::test_panel_id("panel"));
        let edge = part(&all, super::view::test_panel_id("edge"));
        let page = super::view::page_color(&theme);
        let grey = card_border_color(&theme);
        assert!((edge.x - panel.x).abs() < 0.5 && (edge.width - super::view::GOAL_EDGE).abs() < 0.5, "light={light}: the line is {edge:?} on the panel {panel:?}");
        assert!((edge.y - panel.y).abs() < 0.5 && (edge.height - panel.height).abs() < 0.5, "light={light}: the line runs the panel's whole height: {edge:?} on {panel:?}");
        let accent = Color::from(theme.cosmic().accent_color());
        let want = |a: f32, p: f32| a * 0.6 + p * 0.4;
        let line = Color::from_rgb(want(accent.r, page.r), want(accent.g, page.g), want(accent.b, page.b));
        let x0 = panel.x as u32;
        for y in [panel.y + 8.0, panel.y + panel.height / 2.0, panel.y + panel.height - 8.0] {
            for dx in 0..4 {
                let p = px(&frame, w, (x0 + dx) as f32, y);
                assert!(is(p, line), "light={light}, y={y}: pixel {dx} of the line is {p:?}, not {}", hex(line));
                assert!(!is(p, grey), "light={light}: the grey outline shows on the left edge");
            }
            // right after it the panel's own contents, not a second line
            let after = px(&frame, w, (x0 + 4) as f32, y);
            assert!(!is(after, line) && !is(after, grey), "light={light}, y={y}: a second line after the first: {after:?}");
            // and no accent rule, or the old 6 px gap, before it
            for dx in 1..=6 {
                assert!(!is(px(&frame, w, (x0 - dx) as f32, y), line), "light={light}, y={y}: the line stands apart from the panel {dx} px before it");
            }
        }
        // the top, right and bottom keep their grey outline
        let (mx, my) = (panel.x + panel.width / 2.0, panel.y + panel.height / 2.0);
        assert!(line_strength(&frame, w, mx, panel.y + panel.height - 1.0, false, grey, page) > 1.6, "light={light}: the bottom outline");
        assert!(line_strength(&frame, w, panel.x + panel.width - 1.0, my, true, grey, page) > 1.6, "light={light}: the right outline");
    }
}

// ----- the goal beside the proof sits just past its cell

/// Where things are with the goal beside the proof at `width`: the open cell, the goal panel as
/// drawn (the layout's place and how far the page moved it), the other cells on the page, whether
/// the page had room for the goal beside, the frame
struct Beside {
    /// The goal's heading (its count and controls), where it is drawn, if it has come
    heading: Option<Rectangle>,
    cell: Rectangle,
    panel: Rectangle,
    others: Vec<Rectangle>,
    wide: bool,
    column: f32,
    /// The cell's width in characters, and the rows of the open cell's lines, read under the test's
    /// turn (another test's view changes what they are worked out from)
    characters: f32,
    rows: Vec<usize>,
    frame: Vec<u8>,
    size: Size,
}

fn beside(app: &App, width: f32) -> Beside {
    let size = Size::new(width, 900.0);
    in_light(size, |h| {
        super::view::set_goal_beside(true);
        let _ = h.draw(app, "beside-warm");
        let frame = h.draw(app, &format!("beside-{width}"));
        // (only a goal beside is moved: under the cell, what a goal was last moved by is another's)
        let moved = if super::view::last_wide() { super::gap::goal_moved() } else { cosmic::iced::Vector::ZERO };
        let mut all = Bounds(Vec::new());
        h.operate(app, &mut all);
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let panel = part(&all.0, super::view::test_panel_id("panel"));
        let others = t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal)
            .filter_map(|(i, _)| all.0.iter().find(|(x, _)| *x == super::view::test_block_id(i)).map(|(_, r)| *r)).collect();
        let heading = all.0.iter().find(|(x, _)| *x == super::view::test_panel_id("heading")).map(|(_, r)| Rectangle { x: r.x + moved.x, y: r.y + moved.y, ..*r });
        Beside {
            heading,
            cell: part(&all.0, super::view::selected_block_id()),
            panel: Rectangle { x: panel.x + moved.x, y: panel.y + moved.y, ..panel },
            others,
            wide: super::view::last_wide(),
            column: super::view::last_column(),
            characters: super::view::cell_characters(part(&all.0, super::view::selected_block_id()).width),
            rows: t.block_editor.as_ref().map_or_else(Vec::new, super::view::open_rows),
            frame,
            size,
        }
    })
}

fn overlaps(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Nothing but the page's colour on any row of the panel, in the gap before it
fn gap_is_clear(b: &Beside) {
    let page = super::view::page_color(&cosmic::Theme::light());
    let w = b.size.width as u32;
    let (from, to) = (b.panel.x as u32 - super::view::GOAL_GAP_PUB as u32 + 2, b.panel.x as u32 - 1);
    for y in b.panel.y as u32..(b.panel.y + b.panel.height) as u32 {
        for x in from..to {
            assert!(is(px(&b.frame, w, x as f32, y as f32), page), "something on the page reaches into the goal's gap at ({x}, {y}): {:?}", px(&b.frame, w, x as f32, y as f32));
        }
    }
}

/// The goal panel's left edge is the right edge of its cell and the gap, 1,400 px wide, not the
/// edge of the prose column; and nothing on the page, cells or prose, is under it
#[test]
fn the_goal_beside_sits_just_past_its_cell() {
    let b = beside(&goal_app(), 1400.0);
    assert!(b.wide, "1,400 px has room for the goal beside");
    assert!(b.column > b.cell.x + b.cell.width + 100.0, "the column ({}) is wider than the cell, ending at {}", b.column, b.cell.x + b.cell.width);
    let gap = super::view::GOAL_GAP_PUB;
    assert!((b.panel.x - (b.cell.x + b.cell.width + gap)).abs() < 1.0, "the panel is at {}, the cell's right edge is {} and the gap {gap}", b.panel.x, b.cell.x + b.cell.width);
    assert!(b.panel.x + b.panel.width <= b.size.width, "the panel is in the window");
    for other in &b.others {
        assert!(!overlaps(b.panel, *other), "the panel {:?} covers a cell {other:?}", b.panel);
    }
    gap_is_clear(&b);
}

/// A long line wraps at the cell's width instead of widening the cell: the panel is where it is
/// beside a short one, and the cell is a row taller for each row the line takes
#[test]
fn a_long_line_wraps_instead_of_widening_the_cell() {
    let open = |app: &mut App, name: char| {
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().position(|b| b.kind == Kind::Formal && t.source_text(b.start, b.end).starts_with(&format!("lemma {name}:"))).unwrap();
        let _ = cosmic::Application::update(app, super::Message::EditBlock(i));
    };
    let gap = super::view::GOAL_GAP_PUB;
    let mut seen = Vec::new();
    // (one token, too long for a row: it still wraps, within the word; 300 characters in 80 a row)
    for (name, long) in [('a', 100), ('b', 100), ('b', 300)] {
        let mut app = widths_app(long);
        open(&mut app, name);
        let b = beside(&app, 1400.0);
        assert!(b.wide);
        assert!((b.panel.x - (b.cell.x + b.cell.width + gap)).abs() < 1.0, "lemma {name}: the panel is at {}, past a cell ending at {} and the gap {gap}", b.panel.x, b.cell.x + b.cell.width);
        for other in &b.others {
            assert!(!overlaps(b.panel, *other), "lemma {name}: the panel {:?} covers a cell {other:?}", b.panel);
        }
        gap_is_clear(&b);
        seen.push((b.cell.width, b.panel.x, b.cell.height, b.rows[0]));
    }
    let near = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(near(seen[0].0, seen[1].0) && near(seen[0].0, seen[2].0), "the long line's cell is as wide as the short one's: {seen:?}");
    assert!(near(seen[0].1, seen[1].1) && near(seen[0].1, seen[2].1), "the panel does not move: {seen:?}");
    let lh = 14.0 * 1.4;
    // (`lemma b: ` then the token, too long for a row, from a row of its own)
    assert_eq!((seen[0].3, seen[1].3, seen[2].3), (1, 3, 5), "{seen:?}");
    assert!(near(seen[1].2 - seen[0].2, 2.0 * lh), "the cell is taller by the rows the line takes: {seen:?}");
}

/// What the view reckons a wrapped line takes is what the editor takes: the cell's height, at the
/// widths the page gives it, for a line of words, a line of one token and a line with symbols
#[test]
fn the_rows_reckoned_for_a_line_are_the_rows_the_editor_draws() {
    let words = "lemma w: \"".to_string() + &"alpha beta gamma delta epsilon ".repeat(12) + "\"";
    let symbols = "lemma w: \"".to_string() + &"\\<forall>x y. \\<lbrakk>x \\<in> A; y \\<in> B\\<rbrakk> \\<Longrightarrow> f x \\<le> g y \\<and> ".repeat(8) + "True\"";
    let token = format!("lemma w: \"{}\"", "x".repeat(400));
    for line in [words, symbols, token] {
        let text = format!("theory W imports Main begin\n\n{line}\n  by simp\n\nend\n");
        for width in [1400.0, 1000.0, 900.0, 800.0] {
            let mut app = demo_app(Mode::Literate);
            let doc = serde_json::json!({"text": text, "revision": 1, "prover": super::snapshot::prover_json(&text)});
            app.workspace.as_mut().unwrap().theory = Some(super::editor::Theory::new("/tmp/W.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), Mode::Literate, super::prover::profile(None)));
            app.show_right = false;
            app.show_sidebar = false;
            app.workspace.as_mut().unwrap().output = None;
            let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
            let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
            let (editor, reckoned) = in_light(Size::new(width, 900.0), |h| {
                let _ = h.draw(&app, "rows-warm");
                let _ = h.draw(&app, "rows");
                let mut all = Focusable(None);
                h.operate(&app, &mut all);
                let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
                (all.0.expect("the open editor"), super::view::open_rows(be))
            });
            let drawn = (editor.height - super::view::code_padding().top - super::view::code_padding().bottom) / super::view::code_line_height();
            assert!(reckoned[0] > 1, "{width} px: the line wraps");
            assert!((drawn - reckoned.iter().sum::<usize>() as f32).abs() < 0.1, "{width} px: the editor draws {drawn} rows, the view reckons {reckoned:?}: {}", &line[..40.min(line.len())]);
        }
    }
}

/// The open editor's bounds
struct Focusable(Option<Rectangle>);

impl cosmic::iced::advanced::widget::Operation<()> for Focusable {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) {
        f(self);
    }
    fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle, _: &mut dyn cosmic::iced::advanced::widget::operation::Focusable) {
        if id == Some(&super::view::block_editor_id()) {
            self.0 = Some(bounds);
        }
    }
}

/// What is level with a line follows the rows above it: with a long first line wrapped, the wash of
/// the caret's line on the line after it is that many rows down, not one
#[test]
fn the_caret_line_wash_follows_the_rows_above_it() {
    let mut app = widths_app(200);
    let i = {
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        t.blocks.iter().position(|b| b.kind == Kind::Formal && t.source_text(b.start, b.end).starts_with("lemma b:")).unwrap()
    };
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
    super::editor::place(&mut be.content, cosmic::widget::text_editor::Position { line: 1, column: 2 });
    let size = Size::new(1400.0, 700.0);
    let (frame, editor, rows) = in_light(size, |h| {
        let _ = h.draw(&app, "wash-warm");
        let frame = h.draw(&app, "wash");
        let mut all = Focusable(None);
        h.operate(&app, &mut all);
        let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap();
        (frame, all.0.expect("the open editor"), super::view::open_rows(be))
    });
    assert!(rows[0] >= 3, "{rows:?}");
    let lh = super::view::code_line_height();
    // past the text, in the editor's padding, where only the tints can be
    let x = editor.x + editor.width - 4.0;
    let at = |row: f32| px(&frame, size.width as u32, x, editor.y + super::view::code_padding().top + (row + 0.5) * lh);
    let (washed, above) = (at(rows[0] as f32), at(rows[0] as f32 - 1.0));
    assert!(washed != above, "the caret's line, {} rows down, has its wash and the wrapped line's last row does not: {washed:?} and {above:?}", rows[0]);
    assert!(at(1.0) == above, "the wrapped line's rows have no wash: {:?} and {above:?}", at(1.0));
}

/// The goal stays beside down to where a 60 character cell, the gap and a 300 px goal no longer fit
/// between the margins (893 px). The prose column gives way to the goal, not below its cells, and
/// nothing is covered at any width
#[test]
fn the_goal_stays_beside_down_to_the_narrowest_cell_and_goal() {
    for width in [1400.0, 1250.0, 1150.0, 1100.0, 1000.0, 900.0] {
        let b = beside(&goal_app(), width);
        assert!(b.wide, "{width} px: the goal is beside");
        assert!(b.panel.x + b.panel.width <= width - 6.0, "{width} px: the panel ({:?}) is within the window", b.panel);
        assert!((b.panel.x - (b.cell.x + b.cell.width + super::view::GOAL_GAP_PUB)).abs() < 1.0, "{width} px: the panel at {} is not just past the cell ending at {}", b.panel.x, b.cell.x + b.cell.width);
        assert!(b.column >= b.cell.width - 1.0, "{width} px: the column ({}) is not narrower than its cells ({})", b.column, b.cell.width);
        for other in &b.others {
            assert!(!overlaps(b.panel, *other), "{width} px: the panel covers a cell");
        }
        gap_is_clear(&b);
    }
    // too narrow for those: under the cell
    let b = beside(&goal_app(), 880.0);
    assert!(!b.wide, "880 px has no room for a 60 character cell, the gap and a 300 px goal");
}

/// Cells have whole characters' widths, the page's width alone decides them
fn characters(card: f32) -> f32 {
    super::view::cell_characters(card)
}

/// As the page narrows: the goal from 360 to 300 px with the cell at 80 characters, then the cell
/// to 60 characters with the goal at 300 px, then the goal under the cell, the cell back at its full
/// width. The prose column keeps 48 em (960 px) while it, the gap and the goal fit, and then gives
/// way, not below the cell; under the cell it keeps 48 em while it fits the page. The thresholds
/// come from an 80 character card of 703.2 px, a 60 character one of 535.2 px, margins of 24 px and
/// the gap of 10 px
#[test]
fn the_page_at_a_table_of_widths() {
    let _turn = super::snapshot::ui_turn();
    let layout = |page: f32| super::view::page_layout(page, 80, Some(960.0), true);
    let near = |a: f32, b: f32| (a - b).abs() < 0.11;
    // page, the goal (none: under), the cell in characters, the column
    let table: &[(f32, Option<f32>, f32, f32)] = &[
        (1700.0, Some(360.0), 80.0, 960.0),
        (1378.0, Some(360.0), 80.0, 960.0),  // the column, the gap and the widest goal just fit
        (1377.0, Some(360.0), 80.0, 959.0),  // and the column begins to give way
        (1200.0, Some(360.0), 80.0, 782.0),
        (1121.3, Some(360.0), 80.0, 703.3),  // the goal still at its widest
        (1121.0, Some(359.8), 80.0, 703.2),  // and it begins to narrow
        (1100.0, Some(338.8), 80.0, 703.2),
        (1061.3, Some(300.1), 80.0, 703.2),
        (1061.2, Some(300.0), 80.0, 703.2),  // the goal at its narrowest, the cell still 80
        (1061.0, Some(300.0), 79.0, 703.0),  // and the cell begins to narrow, by whole characters
        (1000.0, Some(300.0), 72.0, 642.0),
        (950.0, Some(300.0), 66.0, 592.0),
        (893.3, Some(300.0), 60.0, 535.3),   // the narrowest beside
        (893.0, None, 80.0, 845.0),          // under, the cell back at 80, the column filling the page
        (800.0, None, 80.0, 752.0),
        (760.0, None, 80.0, 712.0),
        (700.0, None, 73.0, 652.0),          // under, the cell narrows with the column
    ];
    for &(page, goal, chars, column) in table {
        let l = layout(page);
        assert!(match (l.goal, goal) { (Some(a), Some(b)) => near(a, b), (None, None) => true, _ => false }, "{page}: the goal is {:?}, wanted {goal:?}", l.goal);
        assert_eq!(characters(l.cell), chars, "{page}: the cell is {} wide", l.cell);
        assert!(near(l.column, column), "{page}: the column is {}, wanted {column}", l.column);
        assert!(l.column >= l.cell - 0.01, "{page}: no cell leaves the column");
        // everything fits between margins of at least 24 px
        let taken = l.column + l.goal.map_or(0.0, |g| g + super::view::GOAL_GAP_PUB);
        assert!(l.gutter >= 24.0 && taken + 2.0 * 24.0 <= page + 0.01, "{page}: {taken} and the margins {} do not fit", l.gutter);
    }
    // the page's width alone: going back over the widths gives the same layouts
    let down: Vec<_> = (880..1720).rev().map(|w| layout(w as f32)).collect();
    let up: Vec<_> = (880..1720).map(|w| layout(w as f32)).collect();
    assert!(down.iter().rev().eq(up.iter()), "the same at each width, narrowing or widening");
    // and each thing only narrows as the page does, each in its turn
    for pair in up.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if let (Some(ga), Some(gb)) = (a.goal, b.goal) {
            assert!(gb >= ga && b.cell >= a.cell && b.column >= a.column, "{a:?} then {b:?}");
            assert!(a.cell >= b.cell - 0.01 || ga <= 300.0 + 0.01, "the cell narrows only once the goal is at its narrowest: {a:?} then {b:?}");
        }
    }
}

/// The settings against the table: a Cell width over 60 narrows to 60, one below 60 is honoured and
/// does not narrow, Fill is the column's width at every width, and the zoom scales it all
#[test]
fn the_page_layout_under_the_settings_and_the_zoom() {
    let _turn = super::snapshot::ui_turn();
    let near = |a: f32, b: f32| (a - b).abs() < 0.11;
    // 100: full width while it fits
    let l = super::view::page_layout(1700.0, 100, Some(960.0), true);
    assert_eq!((characters(l.cell), l.goal), (100.0, Some(360.0)));
    // and narrowing to 60 like 80 does
    let l = super::view::page_layout(900.0, 100, Some(960.0), true);
    assert_eq!((characters(l.cell), l.goal), (60.0, Some(300.0)));
    // 70 goes to 60 at the same width
    let l = super::view::page_layout(900.0, 70, Some(960.0), true);
    assert_eq!((characters(l.cell), l.goal), (60.0, Some(300.0)));
    // 40: as set at every width, the goal beside for longer
    for page in [1700.0, 1000.0, 800.0, 726.0] {
        let l = super::view::page_layout(page, 40, Some(960.0), true);
        assert_eq!(characters(l.cell), 40.0, "{page}");
        assert!(l.goal.is_some(), "{page}: a 40 character cell leaves room for the goal");
    }
    // and under when even that does not fit
    let l = super::view::page_layout(725.0, 40, Some(960.0), true);
    assert_eq!((characters(l.cell), l.goal), (40.0, None));
    // Fill is the column, beside or under
    for page in [1700.0, 1400.0, 1200.0, 900.0, 880.0, 700.0] {
        let l = super::view::page_layout(page, 0, Some(960.0), true);
        assert!(near(l.cell, l.column), "{page}: Fill is the column: {l:?}");
    }
    assert!(super::view::page_layout(1700.0, 0, Some(960.0), true).column == 960.0);
    // with the goal not allowed beside, it is under at every width
    assert_eq!(super::view::page_layout(1700.0, 80, Some(960.0), false).goal, None);
    // the prose measure caps the cell, and no measure leaves the column as wide as the cell
    assert!(near(super::view::page_layout(1700.0, 80, Some(600.0), true).cell, 600.0));
    let l = super::view::page_layout(1700.0, 80, None, true);
    assert!(near(l.column, l.cell) && characters(l.cell) == 80.0, "{l:?}");
    // at 150 %, everything is half as large again: the same layout at a page half as large again
    let _zoomed = Zoomed::new(1.5);
    let code = (14.0f32 * 1.5).round();
    let card = |n: f32| (n + 0.5) * code * 0.6 + 2.0 * 0.75 * code + 6.0;
    let l = super::view::page_layout(1700.0, 80, Some(1440.0), true);
    assert!(near(l.cell, card(80.0)), "{l:?}");
    assert_eq!(characters(l.cell), 80.0);
    assert!(near(l.goal.unwrap(), 540.0), "{l:?}");
    let l = super::view::page_layout(36.0 * 2.0 + 10.0 + 450.0 + card(60.0), 80, Some(1440.0), true);
    assert_eq!((characters(l.cell), l.goal), (60.0, Some(450.0)));
    let l = super::view::page_layout(36.0 * 2.0 + 10.0 + 450.0 + card(60.0) - 1.0, 80, Some(1440.0), true);
    assert_eq!((characters(l.cell), l.goal), (80.0, None));
}

/// On the page, at the windows of the review, with the sidebar open and closed: the cell and the goal
/// as the layout has them, the panel just past the cell, nothing covered. The 290 px sidebar and its
/// 6 px splitter come off the window's width
#[test]
fn the_page_on_screen_with_the_sidebar_open_and_closed() {
    // window, sidebar, the goal, characters
    let table: &[(f32, bool, Option<f32>, f32)] = &[
        (1700.0, true, Some(360.0), 80.0),
        (1500.0, true, Some(360.0), 80.0),
        (1400.0, true, Some(342.8), 80.0),
        (1344.0, true, Some(300.0), 78.0),
        (1280.0, true, Some(300.0), 70.0),
        (1200.0, true, Some(300.0), 61.0),
        (1190.0, true, Some(300.0), 60.0),
        (1180.0, true, None, 80.0),
        (1100.0, true, None, 80.0),
        (1700.0, false, Some(360.0), 80.0),
        (1100.0, false, Some(338.8), 80.0),
        (1000.0, false, Some(300.0), 72.0),
        (900.0, false, Some(300.0), 60.0),
        (880.0, false, None, 80.0),
    ];
    for &(window, sidebar, goal, chars) in table {
        let mut app = goal_app();
        app.show_sidebar = sidebar;
        let b = beside(&app, window);
        let name = format!("{window} px, sidebar {sidebar}");
        assert_eq!(b.wide, goal.is_some(), "{name}");
        assert_eq!(b.characters, chars, "{name}: the cell is {} wide", b.cell.width);
        match goal {
            Some(goal) => {
                assert!((b.panel.width - goal).abs() < 0.2, "{name}: the panel is {} wide, wanted {goal}", b.panel.width);
                assert!((b.panel.x - (b.cell.x + b.cell.width + super::view::GOAL_GAP_PUB)).abs() < 1.0, "{name}: the panel at {} is past the cell ending at {}", b.panel.x, b.cell.x + b.cell.width);
                assert!(b.panel.x + b.panel.width <= window - 6.0, "{name}: the panel is in the window");
                gap_is_clear(&b);
            }
            None => {
                assert!((b.panel.x - b.cell.x).abs() < 1.0 && (b.panel.width - b.cell.width).abs() < 1.0, "{name}: under the cell, as wide as it: {:?} {:?}", b.panel, b.cell);
                // (the open cell's element holds the goal under its code)
                assert!(b.panel.y > b.cell.y + 2.0 * super::view::code_line_height(), "{name}: under the cell's code: {:?} {:?}", b.panel, b.cell);
            }
        }
        for other in &b.others {
            assert!(!overlaps(b.panel, *other), "{name}: the panel {:?} covers a cell {other:?}", b.panel);
        }
    }
}

/// At its narrowest the panel still shows all of its heading: on two rows, Types not cut off at its
/// edge; with room for them, on one
#[test]
fn the_goal_heading_stays_inside_a_narrow_panel() {
    let app = goal_app();
    let b = beside(&app, 1000.0);
    assert!((b.panel.width - 300.0).abs() < 0.5, "{:?}", b.panel);
    let heading = b.heading.expect("the goal's heading");
    assert!(heading.height > 45.0, "two rows at 300 px: {heading:?}");
    let page = super::view::page_color(&cosmic::Theme::light());
    // the panel's own padding, inside its outline, is clear down the heading's whole height
    let x = b.panel.x + b.panel.width - 8.0;
    for y in heading.y as u32..(heading.y + heading.height) as u32 {
        assert!(is(px(&b.frame, b.size.width as u32, x, y as f32), page), "something runs into the panel's edge at ({x}, {y})");
    }
    // and Types is drawn: there is ink in the last part of the second row
    let row = heading.y + heading.height - 15.0;
    assert!((0..60).any(|d| !is(px(&b.frame, b.size.width as u32, x - 2.0 - d as f32, row), page)), "Types is on the second row, at the right");
    let b = beside(&app, 1400.0);
    let heading = b.heading.expect("the goal's heading");
    assert!(b.panel.width > 340.0 && heading.height < 40.0, "one row with room: {:?} {heading:?}", b.panel);
}

/// Widening the window undoes narrowing it, to the pixel: nothing about the page remembers the
/// widths it has been through (rule 45)
#[test]
fn widening_the_window_undoes_narrowing_it() {
    let app = goal_app();
    let frame = |width: f32| {
        let b = beside(&app, width);
        (b.frame, b.cell, b.panel)
    };
    let first = frame(1400.0);
    let narrow = frame(1000.0);
    let under = frame(880.0);
    assert!(narrow.1.width < first.1.width && narrow.2.width < first.2.width, "narrower at 1,000 px");
    assert!(under.2.y > under.1.y, "the goal under the cell at 880 px");
    let again = frame(1400.0);
    assert_eq!((first.1, first.2), (again.1, again.2), "the same places");
    assert!(first.0 == again.0, "the same pixels");
}

/// Prose level with the panel is never covered: the panel goes past it (the page's mechanism for a
/// goal beside, which moves past whatever is level with it), and the column keeps the panel in the window
#[test]
fn the_goal_beside_never_covers_prose_level_with_it() {
    let para = "We prove that the reactive design is healthy, and that its refinement follows from the refinement of its pre and postconditions, which is the main theorem of this section and is used throughout the rest of the development without further comment.";
    let text = format!("theory Fit imports Main begin\n\nlemma first: \"x + 0 = x\"\n  apply simp\n  done\n\ntext \\<open>{para} {para} {para}\\<close>\n\nlemma second: \"True\"\n  by simp\n\nend\n");
    for width in [1500.0, 1400.0, 1150.0, 1000.0, 900.0] {
        let mut app = demo_app(Mode::Literate);
        let doc = serde_json::json!({"text": text, "revision": 1, "prover": super::snapshot::prover_json(&text)});
        app.workspace.as_mut().unwrap().theory = Some(super::editor::Theory::new("/tmp/Fit.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), Mode::Literate, super::prover::profile(None)));
        app.show_right = false;
        app.show_sidebar = false;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        super::snapshot::caret_in_block(&mut app, "apply", 0);
        let b = beside(&app, width);
        assert!(b.wide);
        assert!(b.panel.x + b.panel.width <= width - 6.0, "{width} px: the panel is in the window: {:?}", b.panel);
        // (the paragraph is the page's width, so level with it the panel is past the column)
        gap_is_clear(&b);
        for other in &b.others {
            assert!(!overlaps(b.panel, *other));
        }
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


// ----- Paper: the code font and Thicken text

use super::settings::{self, Settings};

/// Settings published for the length of a test (under its harness's turn), the defaults again after
struct Published;

impl Published {
    fn new(settings: &Settings) -> Self {
        settings::publish(settings);
        Published
    }
}

impl Drop for Published {
    fn drop(&mut self) {
        settings::publish(&Settings::default());
    }
}

/// How many pixels of `rect` differ between two frames of the page's width
fn differing(a: &[u8], b: &[u8], width: u32, rect: Rectangle) -> usize {
    let mut n = 0;
    for y in rect.y as u32..(rect.y + rect.height) as u32 {
        for x in rect.x as u32..(rect.x + rect.width) as u32 {
            if px(a, width, x as f32, y as f32) != px(b, width, x as f32, y as f32) {
                n += 1;
            }
        }
    }
    n
}

/// The cells' bounds and the frame of the demo theory under `settings`
fn cells_under(settings: &Settings) -> (Vec<Rectangle>, Vec<u8>) {
    let app = cells_app(None);
    in_light(SIZE, |h| {
        let _published = Published::new(settings);
        let _ = h.draw(&app, "paper-warm");
        let frame = h.draw(&app, "paper-cells");
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let mut all = Bounds(Vec::new());
        h.operate(&app, &mut all);
        let cells = t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal)
            .map(|(i, _)| all.0.iter().find(|(x, _)| *x == super::view::test_block_id(i)).expect("the cell is on the page").1).collect();
        (cells, frame)
    })
}

/// The code font of Settings ▸ Code reaches the cells, the Source view and the terms in the prose,
/// and nothing else of the prose
#[test]
fn the_code_font_reaches_cells_the_source_view_and_inline_terms() {
    let mut other = Settings::default();
    other.change(settings::Change::CodeFont("NewComputerModern".into()));
    let width = SIZE.width as u32;
    let (cells, before) = cells_under(&Settings::default());
    let (cells_after, after) = cells_under(&other);
    assert_eq!(cells, cells_after.clone(), "the cells keep their place: the font changes only how they are drawn");
    for (n, cell) in cells.iter().enumerate() {
        assert!(differing(&before, &after, width, *cell) > 200, "cell {n}: not drawn in the chosen code font");
    }
    // the Source view
    let source = |settings: &Settings| {
        let mut app = demo_app(Mode::Source);
        app.show_right = false;
        app.show_sidebar = false;
        let mut h = Harness::new(SIZE);
        let _published = Published::new(settings);
        let _ = h.draw(&app, "paper-source-warm");
        h.draw(&app, "paper-source")
    };
    let page = Rectangle { x: 0.0, y: 60.0, width: SIZE.width, height: 400.0 };
    assert!(differing(&source(&Settings::default()), &source(&other), width, page) > 500, "the Source view is not in the code font");

    // an inline term in the prose: the same sentence drawn with the term in two families differs
    // where the term is, and a sentence with no term does not differ at all
    let sentence = |settings: &Settings, md: &str| {
        let _turn = super::snapshot::ui_turn();
        let _published = Published::new(settings);
        cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
        let typo = super::view::typography(&cosmic::theme::active());
        assert_eq!(typo.mono, settings.code.font, "the prose's terms are set in the code font");
        let doc = std::sync::Arc::new(prose_render::markdown::parse(md));
        let frame = super::snapshot::render(prose_render::rich::rich_text(doc, std::sync::Arc::new(typo)).into(), "paper-term", Size::new(500.0, 60.0));
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
        frame
    };
    let whole = Rectangle { x: 0.0, y: 0.0, width: 500.0, height: 60.0 };
    assert!(differing(&sentence(&Settings::default(), "We prove `timed_contract` here."), &sentence(&other, "We prove `timed_contract` here."), 500, whole) > 100);
    assert_eq!(differing(&sentence(&Settings::default(), "We prove it here."), &sentence(&other, "We prove it here."), 500, whole), 0);
}

/// Thicken text draws regular text in Semibold where the family has one and in Regular where it
/// does not: no synthetic bold, and no other family's face
#[test]
fn thicken_text_draws_semibold_where_the_family_has_one_and_regular_otherwise() {
    use cosmic::iced::font::Weight;
    use super::settings::TextFont;
    use super::text_weight::{BOLD, NORMAL, SEMIBOLD, has_semibold, weight_in};
    let _turn = super::snapshot::ui_turn();
    super::load_fonts();
    let family = |name: &'static str| cosmic_text::Family::Name(name);
    // the bundled families: Libertinus Serif has a semibold (and italic), New Computer Modern has none
    assert!(has_semibold(family("Libertinus Serif")), "LibertinusSerif-Semibold.otf is bundled");
    assert!(!has_semibold(family("NewComputerModern")));
    assert_eq!((weight_in(family("Libertinus Serif"), SEMIBOLD), weight_in(family("NewComputerModern"), SEMIBOLD)), (SEMIBOLD, NORMAL));
    assert_eq!((weight_in(family("NewComputerModern"), BOLD), weight_in(family("Libertinus Serif"), BOLD)), (BOLD, BOLD));

    let mut thick = Settings::default();
    thick.change(settings::Change::ThickenText(true));
    let weights = |settings: &Settings| {
        let _published = Published::new(settings);
        let typo = |font: TextFont| {
            let mut s = settings.clone();
            s.display.font = font;
            settings::publish(&s);
            super::view::typography(&cosmic::Theme::light()).body_weight
        };
        (
            TextFont::Libertinus.editor_font().weight,
            TextFont::NewComputerModern.editor_font().weight,
            typo(TextFont::Libertinus),
            typo(TextFont::NewComputerModern),
        )
    };
    assert_eq!(weights(&Settings::default()), (Weight::Normal, Weight::Normal, 400, 400), "off: all Regular");
    assert_eq!(weights(&thick), (Weight::Semibold, Weight::Normal, 600, 400), "on: Semibold where there is one");

    // the code font, by the same rule: Libertinus Serif has a semibold, New Computer Modern has none
    let code = |settings: &Settings, name: &str| {
        let mut s = settings.clone();
        s.change(settings::Change::CodeFont(name.into()));
        let _published = Published::new(&s);
        (super::view::code_font(), super::view::code_font_bold())
    };
    let (regular, bold) = code(&thick, "Libertinus Serif");
    assert_eq!((regular.weight, bold.weight), (Weight::Semibold, Weight::Bold));
    let (regular, bold) = code(&thick, "NewComputerModern");
    assert_eq!((regular.weight, bold.weight), (Weight::Normal, Weight::Bold), "no semibold: Regular, and its own bold stays");
    assert_eq!(regular.family, cosmic::iced::font::Family::Name("NewComputerModern"));
    let (regular, _) = code(&Settings::default(), "Libertinus Serif");
    assert_eq!(regular.weight, Weight::Normal);

    // and on the page: the Libertinus prose is heavier (more ink), the New Computer Modern prose
    // is drawn exactly as before
    let ink = |settings: &Settings, font: TextFont| {
        let mut s = settings.clone();
        s.display.font = font;
        let _published = Published::new(&s);
        cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
        let mut typo = super::view::typography(&cosmic::theme::active());
        typo.size = 20.0;
        let doc = std::sync::Arc::new(prose_render::markdown::parse("Stop deadlocks but lets time pass, and its trace grows only by time steps."));
        let frame = super::snapshot::render(prose_render::rich::rich_text(doc, std::sync::Arc::new(typo)).into(), "paper-thicken", Size::new(700.0, 70.0));
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
        frame
    };
    // (the ink: how far the pixels are from the page's)
    let dark_ink = |frame: &[u8]| frame.chunks(4).map(|p| u32::from(p[0].abs_diff(frame[0]))).sum::<u32>();
    let (plain, heavy) = (ink(&Settings::default(), TextFont::Libertinus), ink(&thick, TextFont::Libertinus));
    assert!(dark_ink(&heavy) as f32 > dark_ink(&plain) as f32 * 1.05, "Semibold Libertinus is not heavier: {} against {}", dark_ink(&heavy), dark_ink(&plain));
    assert_eq!(ink(&Settings::default(), TextFont::NewComputerModern), ink(&thick, TextFont::NewComputerModern), "no semibold: drawn as before");
}


// ----- Paper: the cells' shared width and the gap after the strip

/// The zoom set for the length of a test (under its harness's turn), 100% again after
struct Zoomed;

impl Zoomed {
    fn new(zoom: f32) -> Self {
        super::view::set_zoom(zoom);
        Zoomed
    }
}

impl Drop for Zoomed {
    fn drop(&mut self) {
        super::view::set_zoom(1.0);
    }
}

/// What a cell takes beside its code: the 4 px strip and the room for its border
const CHROME: f32 = 6.0;

/// A theory of three one-line lemmas, the second with a line of `long` characters
fn widths_app(long: usize) -> App {
    // (a first line of exactly n characters: `lemma a: "` is 10 of them, then the closing quote)
    let line = |name: char, n: usize| format!("lemma {name}: \"{}\"", "x".repeat(n - 11));
    let text = format!("theory W imports Main begin\n\n{}\n  by simp\n\n{}\n  by simp\n\n{}\n  by simp\n\nend\n", line('a', 20), line('b', long), line('c', 30));
    let mut app = demo_app(Mode::Literate);
    let doc = serde_json::json!({"text": text, "revision": 1, "prover": super::snapshot::prover_json(&text)});
    app.workspace.as_mut().unwrap().theory = Some(super::editor::Theory::new("/tmp/W.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), Mode::Literate, super::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    app.workspace.as_mut().unwrap().output = None;
    app
}

/// The cells' widths (the card with its strip and border) and the page's column, as laid out in a
/// window this wide
fn cell_widths(app: &App, settings: &Settings, window: f32) -> (Vec<f32>, f32) {
    in_light(Size::new(window, 700.0), |h| {
        let _published = Published::new(settings);
        let _ = h.draw(app, "paper-widths-warm");
        let _ = h.draw(app, "paper-widths");
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let mut all = Bounds(Vec::new());
        h.operate(app, &mut all);
        let widths = t.blocks.iter().enumerate().filter(|(_, b)| b.kind == Kind::Formal)
            .map(|(i, _)| all.0.iter().find(|(x, _)| *x == super::view::test_block_id(i)).expect("the cell is on the page").1.width).collect();
        (widths, super::view::last_column())
    })
}

/// Cells have one shared width, Settings ▸ Code ▸ Cell width in characters (80 by default), however
/// long their lines; Fill is the column's whole width
#[test]
fn cells_share_the_cell_width_whatever_their_lines_and_fill_means_the_column() {
    let near = |a: f32, b: f32| (a - b).abs() < 1.0;
    let settings = Settings::default();
    assert_eq!((settings.code.cell_width, settings.code.size), (80, 14));
    // 80 characters of 14 px code: 8.4 px each, and the half character kept after them, between the
    // gap (0.75 em, 10.5 px) either side, and the strip and border
    let eighty = (80.5 * 8.4) + 2.0 * 10.5 + CHROME;
    let (widths, column) = cell_widths(&widths_app(30), &settings, 1700.0);
    assert!(widths.iter().all(|w| near(*w, eighty)), "every cell is 80 characters wide whatever its lines: {widths:?} (wanted {eighty})");
    assert!(near(column, 960.0), "the column ({column}) keeps its 48 em, wider than the cells, with room for the goal");

    // a line of 100 characters, and one of 300: every cell still the same
    for long in [100, 300] {
        let (widths, _) = cell_widths(&widths_app(long), &settings, 1700.0);
        assert!(widths.iter().all(|w| near(*w, eighty)), "a line of {long} wraps: {widths:?}");
    }

    // another width in characters, the same for every cell
    let hundred = (100.0 + 0.5) * 8.4 + 2.0 * 10.5 + CHROME;
    let mut wide = Settings::default();
    wide.change(settings::Change::CellWidth(100));
    let (widths, _) = cell_widths(&widths_app(30), &wide, 1700.0);
    assert!(widths.iter().all(|w| near(*w, hundred)), "{widths:?}");

    // below the narrowest a window asks for: as set, at any width
    let forty = (40.0 + 0.5) * 8.4 + 2.0 * 10.5 + CHROME;
    let mut narrow = Settings::default();
    narrow.change(settings::Change::CellWidth(40));
    for window in [1700.0, 900.0] {
        let (widths, _) = cell_widths(&widths_app(30), &narrow, window);
        assert!(widths.iter().all(|w| near(*w, forty)), "{window} px: {widths:?}");
    }

    // Fill: every cell the column's whole width, as the column narrows too
    let mut fill = Settings::default();
    fill.change(settings::Change::CellWidth(0));
    for window in [1700.0, 1200.0, 900.0] {
        let (widths, column) = cell_widths(&widths_app(30), &fill, window);
        assert!(widths.iter().all(|w| near(*w, column)), "{window} px: Fill is the column: {widths:?} against {column}");
    }
}

/// The gap between the status strip and the code (and the code and the card's right edge) is
/// 0.75 em of the code's size: 10.5 px at 14 px, and it scales with the zoom. The strip stays 4 px
#[test]
fn the_gap_after_the_strip_is_three_quarters_of_an_em_of_the_code_at_any_zoom() {
    let app = cells_app(None);
    for zoom in [1.0f32, 1.5] {
        let (gap, ink_at, strip) = in_light(SIZE, |h| {
            let _zoomed = Zoomed::new(zoom);
            let gap = super::view::code_padding().left;
            let _ = h.draw(&app, "paper-gap-warm");
            let frame = h.draw(&app, "paper-gap");
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let first = t.blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
            let mut all = Bounds(Vec::new());
            h.operate(&app, &mut all);
            let card = all.0.iter().find(|(x, _)| *x == super::view::test_block_id(first)).unwrap().1;
            // the first character of the first line: the first pixel right of the strip that is
            // not the card's page colour, on the middle of that line
            let page = super::view::page_color(&cosmic::Theme::light());
            let y = card.y + 1.0 + super::view::code_padding().top + super::view::code_line_height() / 2.0;
            let width = SIZE.width as u32;
            let ink = (card.x as u32 + 5..card.x as u32 + 80).find(|&x| (-8..=8).any(|dy| !is(px(&frame, width, x as f32, y + dy as f32), page))).expect("code on the line");
            let strip_end = (card.x as u32..card.x as u32 + 12).find(|&x| is(px(&frame, width, x as f32, y), page)).expect("the strip ends");
            (gap, ink as f32 - strip_end as f32, strip_end as f32 - card.x)
        });
        let code = (14.0 * zoom).round();
        assert!(zoom != 1.0 || (gap - 10.5).abs() < 1e-3, "10.5 px at 14 px code");
        assert!((gap - 0.75 * code).abs() < 1e-3, "zoom {zoom}: the gap is {gap}, not 0.75 em of {code} px");
        assert!((strip - 4.0).abs() < 1.5, "zoom {zoom}: the strip is {strip} px wide");
        // (the first glyph's own side bearing, a pixel or two, comes after the gap)
        // 
        assert!(ink_at >= gap - 1.0 && ink_at <= gap + 3.0, "zoom {zoom}: the code begins {ink_at} px after the strip, the gap being {gap}");
    }
}


// ----- Paper: Ctrl+= / - / 0 zoom the content, not the interface

/// The bounds of chrome (the pop-out button, the tools' bar) and of content (the open cell, the
/// goal's first subgoal) at `zoom`
fn zoomed_bounds(zoom: f32) -> Vec<(&'static str, Rectangle)> {
    let app = goal_app();
    in_light(PANEL, |h| {
        let _zoomed = Zoomed::new(zoom);
        super::view::set_goal_beside(false);
        let _ = h.draw(&app, "paper-zoom-warm");
        let _ = h.draw(&app, "paper-zoom");
        let mut all = Bounds(Vec::new());
        h.operate(&app, &mut all);
        super::view::set_goal_beside(true);
        let get = |id: cosmic::widget::Id| all.0.iter().find(|(x, _)| *x == id).map(|(_, b)| *b);
        [
            ("the pop-out button", get(super::view::pop_out_button_id())),
            ("the tools' bar", get(super::insert::insert_bar_id())),
            ("the open cell", get(super::view::selected_block_id())),
            ("the goal's first subgoal", get(super::view::test_panel_id("subgoal-1"))),
        ]
        .into_iter()
        .map(|(name, b)| (name, b.unwrap_or_else(|| panic!("{name} is on the page"))))
        .collect()
    })
}

/// Zoom changes the size of the content (prose, code in cells, goals) and not of the interface
/// (buttons, the tools' chips and bar): the interface's size is its own setting
#[test]
fn zoom_scales_the_content_and_not_the_interface() {
    let (at_100, at_150) = (zoomed_bounds(1.0), zoomed_bounds(1.5));
    let by_name = |v: &[(&'static str, Rectangle)], n: &str| v.iter().find(|(name, _)| *name == n).unwrap().1;
    for chrome in ["the pop-out button", "the tools' bar"] {
        let (a, b) = (by_name(&at_100, chrome), by_name(&at_150, chrome));
        assert!((a.height - b.height).abs() < 0.5, "{chrome} is {} px tall at 100% and {} px at 150%", a.height, b.height);
        if chrome == "the pop-out button" {
            assert!((a.width - b.width).abs() < 0.5, "{chrome} is {} px wide at 100% and {} px at 150%", a.width, b.width);
        }
    }
    for content in ["the goal's first subgoal"] {
        let (a, b) = (by_name(&at_100, content), by_name(&at_150, content));
        // (its text zooms by half, its padding does not)
        assert!(b.height > a.height * 1.15, "{content} is {} px tall at 100% and {} px at 150%: it did not zoom", a.height, b.height);
    }
    // and the prose above the cell: the cell sits lower the larger the prose and the headings
    let (a, b) = (by_name(&at_100, "the open cell").y, by_name(&at_150, "the open cell").y);
    assert!(b > a * 1.3, "the prose above the cell did not zoom: the cell is at {a} and {b}");
}


// ----- Paper: the theory's title and imports

/// "Theory" is in the interface's bold sans and the theory's name in the code font, bold; the
/// imports are a 13 px sans caption and then each name in the code font; and a code font with no
/// bold sets the name in its regular weight, never in another family
#[test]
fn the_title_sets_theory_in_sans_and_the_name_and_imports_in_the_code_font() {
    use cosmic::iced::font::{Family, Weight};
    let _turn = super::snapshot::ui_turn();
    super::load_fonts();
    let sans = cosmic::font::default();
    let under = |code_font: &str| {
        let mut s = Settings::default();
        s.change(settings::Change::CodeFont(code_font.into()));
        Published::new(&s)
    };

    // a code font with a bold (New Computer Modern, bundled)
    let _published = under("NewComputerModern");
    let title = super::view::title_runs("Timed_Circus_Stop");
    assert_eq!(title.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["Theory ", "Timed_Circus_Stop"]);
    assert_eq!((title[0].1.family, title[0].1.weight), (sans.family, Weight::Bold), "Theory: the interface's sans, bold");
    assert_eq!((title[1].1.family, title[1].1.weight), (Family::Name("NewComputerModern"), Weight::Bold), "the name: the code font, bold");

    let imports = super::view::imports_runs(&["imports", "Timed_Circus_Observations", "Timed_Circus_Time_Step"]);
    let words: Vec<&(String, cosmic::iced::Font)> = imports.iter().filter(|(t, _)| !t.trim().is_empty()).collect();
    assert_eq!(words.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["imports", "Timed_Circus_Observations", "Timed_Circus_Time_Step"]);
    assert_eq!((words[0].1.family, words[0].1.weight), (sans.family, Weight::Normal), "the caption: the interface's sans");
    for (name, font) in &words[1..] {
        assert_eq!((font.family, font.weight), (Family::Name("NewComputerModern"), Weight::Normal), "{name}: the code font");
    }
    assert_eq!((super::view::TITLE_SIZE, super::view::IMPORTS_SIZE), (26.0, 13.0), "the caption is 13 px");

    // a code font with no bold: the name's regular weight, in that family. (An installed family
    // with no bold face, taken from the font database: none is bundled, so a machine whose fonts
    // all have a bold has nothing to try here)
    drop(_published);
    let unbolded = {
        let mut font_system = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
        let mut names: Vec<String> = font_system.raw().db().faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        drop(font_system);
        names.sort();
        names.dedup();
        names.into_iter().find(|n| !super::text_weight::has_weight(cosmic_text::Family::Name(Box::leak(n.clone().into_boxed_str())), super::text_weight::BOLD))
    };
    let Some(unbolded) = unbolded else {
        eprintln!("every installed family has a bold: no family to try the fallback on");
        return;
    };
    let family = Family::Name(Box::leak(unbolded.clone().into_boxed_str()));
    let _published = under(&unbolded);
    let title = super::view::title_runs("Timed_Circus_Stop");
    assert_eq!((title[1].1.family, title[1].1.weight), (family, Weight::Normal), "{unbolded} has no bold: the same family, regular");
    assert_eq!(title[0].1.weight, Weight::Bold, "Theory stays bold");
}

// ----- a wrapped line of code continues at its own indentation and two characters

/// A proof with an indented line too long for a cell, and the line's words at 80 characters
const HANGING_LINE: &str = "    \"alpha_one beta_two gamma_three delta_four epsilon_five zeta_six eta_seven theta_eight iota_nine kappa_ten lambda_eleven mu_twelve nu_thirteen xi_fourteen omicron_fifteen pi_sixteen rho_seventeen\"";

fn hanging_app() -> App {
    let text = format!("theory H imports Main begin\n\nlemma hanging:\n{HANGING_LINE}\n  by simp\n\nend\n");
    let mut app = demo_app(Mode::Literate);
    let doc = serde_json::json!({"text": text, "revision": 1, "prover": super::snapshot::prover_json(&text)});
    app.workspace.as_mut().unwrap().theory = Some(super::editor::Theory::new("/tmp/H.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), Mode::Literate, super::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    app.workspace.as_mut().unwrap().output = None;
    app
}

fn open_hanging(app: &mut App) {
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
    let _ = cosmic::Application::update(app, super::Message::EditBlock(i));
}

/// Each row of `line` in a cell at this text width, as cosmic-text lays it out: where each of its
/// glyphs starts in the line and across the row
fn hanging_rows(line: &str, width: f32) -> Vec<Vec<(usize, f32)>> {
    use cosmic::iced::advanced::graphics::text as graphics;
    let mut font_system = graphics::font_system().write().unwrap();
    let size = super::view::code_size();
    let mut buffer = cosmic_text::Buffer::new_empty(cosmic_text::Metrics::new(size, size));
    buffer.set_size(Some(width), None);
    buffer.set_wrap(graphics::to_wrap(super::wrap::WRAPPING));
    buffer.set_hanging_indent(Some(super::wrap::HANGING));
    buffer.set_text(line, &graphics::to_attributes(super::view::code_font()), cosmic_text::Shaping::Advanced, None);
    buffer.line_layout(font_system.raw(), 0).unwrap().iter().map(|row| row.glyphs.iter().map(|g| (g.start, g.x)).collect()).collect()
}

/// The first pixel of a row of an editor's text that is not the row's background
fn ink_left(frame: &[u8], width: u32, text_x: f32, top: f32, row: usize) -> f32 {
    let lh = super::view::code_line_height();
    let background = px(frame, width, text_x - 4.0, top + (row as f32 + 0.5) * lh);
    let y = |dy: f32| top + row as f32 * lh + dy;
    (text_x as u32 - 2..text_x as u32 + 400)
        .find(|&x| (2..(lh as u32 - 2)).any(|dy| px(frame, width, x as f32, y(dy as f32)) != background))
        .expect("ink on the row") as f32
}

/// What differs on a row of an editor's text between two frames: the leftmost and rightmost pixels
fn row_changes(a: &[u8], b: &[u8], width: u32, editor: Rectangle, row: usize) -> Option<(f32, f32)> {
    let lh = super::view::code_line_height();
    let top = editor.y + super::view::code_padding().top + row as f32 * lh;
    let changed: Vec<u32> = (editor.x as u32..(editor.x + editor.width) as u32)
        // (not the row's first and last pixels: the rows either side reach into them at a fraction)
        .filter(|&x| (2..lh as u32 - 2).any(|dy| px(a, width, x as f32, top + dy as f32) != px(b, width, x as f32, top + dy as f32)))
        .collect();
    Some((*changed.first()? as f32, *changed.last()? as f32))
}

/// A wrapped line continues at its own indentation and two characters, in an open cell and a shown
/// one; its first row stays where it was, and the document's text is untouched
#[test]
fn a_wrapped_line_of_code_continues_at_its_indentation_and_two_characters() {
    let size = Size::new(1400.0, 800.0);
    let mut app = hanging_app();
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
    let source = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
    in_light(size, |h| {
        let pad = super::view::code_padding();
        let advance = super::view::code_size() * super::view::CODE_ADVANCE;
        // shown
        let _ = h.draw(&app, "hanging-shown-warm");
        let shown = h.draw(&app, "hanging-shown");
        let mut all = Bounds(Vec::new());
        h.operate(&app, &mut all);
        let card = part(&all.0, super::view::test_block_id(i));
        // open
        open_hanging(&mut app);
        let _ = h.draw(&app, "hanging-open-warm");
        let open = h.draw(&app, "hanging-open");
        let mut all = Focusable(None);
        h.operate(&app, &mut all);
        let editor = all.0.expect("the open editor");
        let rows = hanging_rows(HANGING_LINE, super::view::block_code_width() - pad.left - pad.right);
        assert!(rows.len() >= 3, "the line wraps twice: {} rows", rows.len());
        for (frame, x, top, name) in [(&shown, card.x + 4.0 + pad.left, card.y + 1.0 + pad.top, "shown"), (&open, editor.x + pad.left, editor.y + pad.top, "open")] {
            // (the line's first row is the cell's second, starting with a quote at its four blanks)
            let first = ink_left(frame, size.width as u32, x, top, 1);
            assert!((first - (x + 4.0 * advance)).abs() < 6.0, "{name}: the first row's ink is at {first}, its indentation at {}", x + 4.0 * advance);
            for row in 1..rows.len() {
                let ink = ink_left(frame, size.width as u32, x, top, 1 + row);
                let want = x + 6.0 * advance;
                assert!(ink >= want - 1.0 && ink <= want + 3.0, "{name}: row {row} of the line starts at {ink}, not at 4 + 2 characters ({want})");
            }
            // the line after it is at its own indentation
            let after = ink_left(frame, size.width as u32, x, top, 1 + rows.len());
            assert!((after - (x + 2.0 * advance)).abs() < 3.0, "{name}: `by simp` starts at {after}");
        }
    });
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.local, source, "the document's text is untouched");
    assert_eq!(t.block_editor.as_ref().unwrap().content.text().lines().nth(1), Some(HANGING_LINE), "and the editor's");
}

/// A click on a continuation row puts the caret on the character drawn there; one in the indent, on
/// the row's first character; and the caret is drawn at that character
#[test]
fn a_click_on_a_continuation_row_puts_the_caret_on_the_character_there() {
    let size = Size::new(1400.0, 800.0);
    let mut app = hanging_app();
    open_hanging(&mut app);
    let mut h = Harness::new(size);
    let pad = super::view::code_padding();
    cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
    h.focus(&app, super::view::block_editor_id());
    let _ = h.draw(&app, "hanging-click-warm");
    let _ = h.draw(&app, "hanging-click");
    let mut all = Focusable(None);
    h.operate(&app, &mut all);
    let editor = all.0.expect("the open editor");
    let rows = hanging_rows(HANGING_LINE, super::view::block_code_width() - pad.left - pad.right);
    let row = &rows[1];
    let text_x = editor.x + pad.left;
    let lh = super::view::code_line_height();
    // (the second row of the line is the cell's third)
    let y = editor.y + pad.top + 2.5 * lh;
    let advance = row[1].1 - row[0].1;
    let mut click = |app: &mut App, x: f32| {
        for m in h.click(app, cosmic::iced::Point::new(x, y)) {
            let _ = cosmic::Application::update(app, m);
        }
        let _ = h.draw(app, "hanging-click");
        app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().unwrap().content.cursor().position
    };
    // the fourth character of the row, on its left half
    let position = click(&mut app, text_x + row[3].1 + advance * 0.25);
    assert_eq!((position.line, position.column), (1, row[3].0), "on the fourth character of the row");
    // past the middle of another (far enough off not to be a double click): the caret after it
    let position = click(&mut app, text_x + row[7].1 + advance * 0.75);
    assert_eq!((position.line, position.column), (1, row[8].0), "after the eighth character of the row");
    // in the indent: the row's first character, not the end of the row
    let position = click(&mut app, text_x + advance);
    assert_eq!((position.line, position.column), (1, row[0].0), "in the indent");

    // the caret as drawn: at two places on the row, what differs is at those places
    let mut at = |app: &mut App, column: usize| {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        super::editor::place(&mut be.content, cosmic::widget::text_editor::Position { line: 1, column });
        let _ = h.draw(app, "hanging-caret-warm");
        h.draw(app, "hanging-caret")
    };
    let (a, b) = (at(&mut app, row[3].0), at(&mut app, row[8].0));
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    let (left, right) = row_changes(&a, &b, size.width as u32, editor, 2).expect("the caret moved");
    assert!((left - (text_x + row[3].1)).abs() <= 2.0, "the caret at the fourth character is drawn at {left}, the glyph at {}", text_x + row[3].1);
    assert!((right - (text_x + row[8].1)).abs() <= 3.0, "the caret at the ninth character is drawn at {right}, the glyph at {}", text_x + row[8].1);
}

/// A selection across the wrap is drawn from the character it starts at, to the end of that row, and
/// on the next row from the indent, not from the left edge, to the character it ends at
#[test]
fn a_selection_across_a_wrap_is_drawn_at_the_characters() {
    let size = Size::new(1400.0, 800.0);
    let mut app = hanging_app();
    open_hanging(&mut app);
    let mut h = Harness::new(size);
    let pad = super::view::code_padding();
    cosmic::theme::set_active(cosmic::theme::ThemeType::Light);
    h.focus(&app, super::view::block_editor_id());
    let _ = h.draw(&app, "hanging-selection-warm");
    let _ = h.draw(&app, "hanging-selection-warm");
    let mut all = Focusable(None);
    h.operate(&app, &mut all);
    let editor = all.0.expect("the open editor");
    let rows = hanging_rows(HANGING_LINE, super::view::block_code_width() - pad.left - pad.right);
    let text_x = editor.x + pad.left;
    let position = cosmic::widget::text_editor::Position { line: 1, column: rows[1][5].0 };
    let anchor = cosmic::widget::text_editor::Position { line: 1, column: rows[0][20].0 };
    let mut with = |app: &mut App, selection: Option<cosmic::widget::text_editor::Position>| {
        let be = app.workspace.as_mut().unwrap().theory.as_mut().unwrap().block_editor.as_mut().unwrap();
        be.content.move_to(cosmic::widget::text_editor::Cursor { position, selection });
        let _ = h.draw(app, "hanging-selection-warm");
        h.draw(app, "hanging-selection")
    };
    let (plain, selected) = (with(&mut app, None), with(&mut app, Some(anchor)));
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    let width = size.width as u32;
    // the cell's second row, the line's first: from the anchor's character on
    let (left, _) = row_changes(&plain, &selected, width, editor, 1).expect("the selection on the first row");
    assert!((left - (text_x + rows[0][20].1)).abs() <= 2.0, "the first row's selection starts at {left}, its character at {}", text_x + rows[0][20].1);
    // the cell's third row, the line's second: the indent to the caret's character
    let (left, right) = row_changes(&plain, &selected, width, editor, 2).expect("the selection on the continuation row");
    assert!((left - (text_x + rows[1][0].1)).abs() <= 2.0, "the continuation's selection starts at {left}, its first character at {}", text_x + rows[1][0].1);
    assert!((right - (text_x + rows[1][5].1)).abs() <= 3.0, "the continuation's selection ends at {right}, the caret's character at {}", text_x + rows[1][5].1);
}

/// Prose being edited, a paragraph on the page and interface text wrap at their left edge, even with
/// indentation in them
#[test]
fn prose_and_interface_text_do_not_hang() {
    // where each of the lines of text in a box starts: leftmost ink, in bands a line tall
    fn starts(frame: &[u8], width: u32, area: Rectangle, line: f32, background: [u8; 3]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut y = area.y;
        while y + line <= area.y + area.height {
            let ink = (area.x as u32..(area.x + area.width) as u32).find(|&x| (0..line as u32).any(|dy| px(frame, width, x as f32, y + dy as f32) != background));
            if let Some(ink) = ink {
                out.push(ink as f32);
            }
            y += line;
        }
        out
    }
    // prose being edited
    let para = "We prove that the reactive design is healthy, and that its refinement follows from the refinement of its pre and postconditions, which is the main theorem of this section and is used throughout the rest of the development without further comment, as the reader will see in the chapters that follow.";
    let text = format!("theory P imports Main begin\n\ntext \\<open>    {para}\\<close>\n\nlemma after: \"True\"\n  by simp\n\nend\n");
    let mut app = demo_app(Mode::Literate);
    let doc = serde_json::json!({"text": text, "revision": 1, "prover": super::snapshot::prover_json(&text)});
    app.workspace.as_mut().unwrap().theory = Some(super::editor::Theory::new("/tmp/P.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), std::sync::Arc::new(std::collections::HashMap::new()), Mode::Literate, super::prover::profile(None)));
    app.show_right = false;
    app.show_sidebar = false;
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Text).unwrap();
    let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
    let size = Size::new(900.0, 800.0);
    let (frame, editor, line) = in_light(size, |h| {
        let _ = h.draw(&app, "prose-wrap-warm");
        let frame = h.draw(&app, "prose-wrap");
        let mut all = Focusable(None);
        h.operate(&app, &mut all);
        let display = super::settings::display();
        (frame, all.0.expect("the prose editor"), super::view::z(display.text_size) * display.line_height)
    });
    let background = px(&frame, size.width as u32, editor.x + editor.width - 2.0, editor.y + 2.0);
    let lines = starts(&frame, size.width as u32, editor, line, background);
    assert!(lines.len() >= 3, "the paragraph wraps: {lines:?}");
    // (after the first, with the paragraph's own blanks, every line starts at the left edge)
    let rest = &lines[1..];
    let (least, most) = rest.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(most - least < 6.0, "the prose's wrapped lines all start at its left edge: {lines:?}");
    assert!(least - editor.x < 6.0, "and that is the editor's: {lines:?}, the editor at {}", editor.x);

    // interface text
    let long = "    Interface text with indentation, long enough to wrap in its box at this width, and to wrap again after that";
    let element: cosmic::Element<'_, super::Message> = cosmic::widget::container(cosmic::widget::text(long).size(14.0)).width(cosmic::iced::Length::Fixed(280.0)).into();
    let size = Size::new(300.0, 120.0);
    let frame = super::snapshot::draw_element(element, "interface-wrap", size);
    let area = Rectangle { x: 0.0, y: 0.0, width: 290.0, height: 110.0 };
    let lines = starts(&frame, size.width as u32, area, 14.0 * 1.4, px(&frame, size.width as u32, 295.0, 115.0));
    assert!(lines.len() >= 3, "the text wraps: {lines:?}");
    let rest = &lines[1..];
    let (least, most) = rest.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(most - least < 6.0 && least < 6.0, "interface text's wrapped lines start at its left edge: {lines:?}");
}

/// A term in the goal panel is a code view: its wrapped lines hang too
#[test]
fn a_wrapped_term_in_the_goal_continues_at_its_indentation_and_two_characters() {
    let _turn = super::snapshot::ui_turn();
    super::load_fonts();
    let term = "  alpha_one beta_two gamma_three delta_four epsilon_five zeta_six eta_seven theta_eight iota_nine";
    let view = super::editor::CodeView::from_message(&serde_json::json!({"text": term}), &super::prover::isabelle::symbols::Table::fallback());
    let size = Size::new(320.0, 200.0);
    let font = super::view::code_font();
    let element: cosmic::Element<'_, super::Message> = cosmic::iced::widget::text_editor(&view.content).font(font).size(14.0).padding(0).line_height(cosmic::iced::widget::text::LineHeight::Relative(1.4)).width(300.0).style(|_: &cosmic::Theme, _| cosmic::iced::widget::text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: cosmic::iced::Border::default(),
        placeholder: Color::TRANSPARENT,
        value: Color::BLACK,
        selection: Color::TRANSPARENT,
    }).into();
    let frame = super::snapshot::render(element, "goal-term-wrap", size);
    let lh = 14.0 * 1.4;
    let background = px(&frame, size.width as u32, 310.0, 190.0);
    let ink = |row: f32| (0..300u32).find(|&x| (2..18u32).any(|dy| px(&frame, size.width as u32, x as f32, row * lh + dy as f32) != background));
    let (first, second) = (ink(0.0).expect("the first row") as f32, ink(1.0).expect("the second row") as f32);
    let advance = 14.0 * super::view::CODE_ADVANCE;
    assert!((first - 2.0 * advance).abs() < 3.0, "the term starts at its two blanks: {first}");
    assert!((second - 4.0 * advance).abs() < 3.0, "its next row at two blanks and two characters: {second}");
}

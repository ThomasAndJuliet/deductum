// SPDX-License-Identifier: GPL-3.0-only

//! Acceptance tests: how Deductum marks what is selected and what is chosen
//! (household-ui RULES.md rule 6, "What is highlighted, and how"; household-ui
//! TESTING.md section 5, step 1). Written from Juliet's words before the
//! change is built, by an agent other than the builder.
//!
//! Her words, and which tests pin them:
//!
//! - 25 Oct, the highlight standard: "use the tint for deductum code".
//!   Rule 6: a document or code is marked with the tint, "the accent at 32 %
//!   over what is beneath", opaque (`household_tones::tint`); "the fill rule
//!   works against the tint as it shows over the page, and greys go to that
//!   page's strongest text colour (a light theme's darkest text, #222222, not
//!   black)" (`household_tones::on_tint`); "a colour is only ever changed for
//!   contrast, so a colour that already reads is left as it is".
//!   Deductum's code is its editors: the source view, the block being edited
//!   (code and prose), the reference windows and the prover's output views
//!   (`CodeView`). Today each one gives its `text_editor::Style` a
//!   translucent `Color { a: 0.32, ..accent }`, which libcosmic draws under
//!   the first pass of the text.
//!   -> `the_source_selection_*`, `the_block_selection_*`,
//!      `the_prose_block_selection_*`, `the_output_view_selection_*`
//! - Stage 2, said by the lead (25 Oct): syntax-highlighted spans inside an
//!   iced `text_editor` selection keep their own colour for now, because
//!   libcosmic can only recolour the editor's default text colour.
//!   -> `a_highlighted_span_keeps_its_colour_on_the_tint`
//! - Rule 6: "text boxes keep the solid default": a text input's selection is
//!   the solid accent. -> `a_text_input_keeps_the_solid_selection`
//! - Rule 6 (15 Oct): "toggles that are on, the chosen part of a segmented
//!   control and the chosen side-panel tab use a muted version of the accent,
//!   so a row of them doesn't catch the eye the way a Primary button does;
//!   enabled text on it keeps at least 4.5:1." The side-panel tabs
//!   (`panel_tabs`), the sidebar's section tabs (`sidebar::tab_style`) and the
//!   Goals/Text switch are chosen parts of a row; today each is the solid
//!   accent (`Suggested`, or `Color::from(c.accent_color())`).
//!   -> `the_chosen_*`
//! - Rule 6, "What is drawn on a fill": text and icons on the fill go to the
//!   fill's on colour (greys all the way). -> the same tests
//! - Rule 7: Primary stays full accent. -> `primary_buttons_stay_full_accent`
//! - Shared code: the expected colours are `household_tones::{tint, on_tint,
//!   muted_accent, on_muted}`, not copies.
//!
//! What fails today, and what is held:
//! - FAIL until built: the chosen tabs and the Goals/Text switch (solid
//!   accent today, the muted accent wanted); the selection over the
//!   construct's band (the translucent fill lets the band show through, the
//!   opaque tint does not); a default text colour that does not read on the
//!   tint (left as it is today, `on_tint` wanted).
//! - PASS today, and must still pass after (guards): the selection's colour
//!   over a plain page, a span's colour, the unselected frame, a text input's
//!   solid selection, Primary's full accent, the vim badge's text, and a
//!   reference muted button (the measurements recognise the right look).
//!   Over a uniform page `Color { a: 0.32, ..accent }` and the opaque tint
//!   are the same pixels, and the theme's own text (#e7e7e7 on dark, #121212
//!   on light) already reads on the tint, so `on_tint` leaves it alone.
//!   Moving the libcosmic pin to a revision with `selection_fill` also moves
//!   the editor's default selected text to the accent's on colour unless the
//!   app sets `selected_text_color`: these guards catch that.
//!
//! The tests draw the real view headless (tiny-skia, as `snapshot::Harness`
//! does) and look at the pixels: what the colours are, not how the code
//! arrives at them. Each selection check compares the frame with the caret
//! with the frame with everything selected, so the text and fill are
//! measured, not assumed.
//!
//! Not pinned here, and why:
//! - the vim mode badge (`vim_badge`): a status badge, neither a chosen
//!   segment, an on-toggle nor a Primary action; rule 6 gives it no colour of
//!   its own, so only its text contrast is held (`the_vim_badge_text_reads`).
//!   Whether it should be the muted accent is for Juliet to rule on.
//! - the header's Literate/Source switch (libcosmic's segmented control) and
//!   the "Queries go to" choice of the LLM section: the first is drawn by the
//!   window's header, which the headless view does not include; the second
//!   shows only with the caret on a `claude`/`codex` line during a request.
//!   A builder that restyles either should add a test for it.
//! - the suggested completion and the suggested offer ("Complete:" bar,
//!   "Insert", "Run", "Go there"): the one main action of a view, Primary.
//! - rendered prose (`prose-render`'s `Typography::selection`): Deductum does
//!   not draw a selection in rendered prose (no `.selection(..)` is passed),
//!   so there is nothing to see. The same translucent value is set there
//!   (view.rs, `typography`) and should follow the standard if it is ever
//!   drawn.
//! - `muted()`'s `selected_fill` (selectable captions): a text widget's
//!   selection, not code.
//!
//! `CT_SNAPSHOTS=<dir> cargo test highlight_ -- --test-threads=1` keeps the
//! frames as PNG files.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic::iced::advanced::widget::Operation;
use cosmic::iced::advanced::widget::operation::{Focusable, TextInput};
use cosmic::iced::{Rectangle, Size};
use cosmic::widget::text_editor::{Action, Content};
use household_tones::{Mode as ToneMode, Role, Srgb, contrast_ratio, muted_accent, on_muted, on_tint, tint};

use super::editor::{HighlightData, HighlightSettings, Hl, Mode, Sem, highlight_format, next_generation};
use super::snapshot::{Harness, demo_app};
use super::theory::Kind;
use super::{App, Panel};

// ---- colours and pixels ----

/// A colour as red, green and blue in 0..255
type V = [f32; 3];

/// How far, in 8-bit units, a pixel may stray from the line between the tint
/// and the colour its ink should be: rounding and anti-aliasing
const OFF_LINE: f32 = 7.0;

/// How close, in 8-bit units per channel, the furthest pixel of an ink must
/// get to the colour it should be (a text of many glyphs has a pixel the
/// stem of some glyph covers entirely)
const REACH: f32 = 5.0;

/// How many pixels in a hundred may stray (the tint's edge cuts through the
/// anti-aliasing of the first and last glyph of a line)
const STRAY_PERCENT: usize = 2;

fn v(c: Srgb) -> V {
    [c.r * 255.0, c.g * 255.0, c.b * 255.0]
}

fn v_of(c: cosmic::iced::Color) -> V {
    [c.r * 255.0, c.g * 255.0, c.b * 255.0]
}

fn srgb_of(c: V) -> Srgb {
    Srgb::new((c[0] / 255.0).clamp(0.0, 1.0), (c[1] / 255.0).clamp(0.0, 1.0), (c[2] / 255.0).clamp(0.0, 1.0))
}

fn hex(c: V) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0].round().clamp(0.0, 255.0) as u8, c[1].round().clamp(0.0, 255.0) as u8, c[2].round().clamp(0.0, 255.0) as u8)
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The distance between two colours
fn dist(a: V, b: V) -> f32 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// The largest difference in one channel
fn off(a: V, b: V) -> f32 {
    let d = sub(a, b);
    d[0].abs().max(d[1].abs()).max(d[2].abs())
}

/// How far `p` is from the straight line between `a` and `b`, within it
fn off_segment(p: V, a: V, b: V) -> f32 {
    let ab = sub(b, a);
    let len2 = dot(ab, ab);
    if len2 < 1e-6 {
        return dist(p, a);
    }
    let k = (dot(sub(p, a), ab) / len2).clamp(0.0, 1.0);
    dist(p, [a[0] + k * ab[0], a[1] + k * ab[1], a[2] + k * ab[2]])
}

/// A drawn frame: RGBA
struct Frame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Frame {
    fn at(&self, x: u32, y: u32) -> V {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i] as f32, self.rgba[i + 1] as f32, self.rgba[i + 2] as f32]
    }

    /// Every pixel of `region` (end exclusive), clipped to the frame
    fn pixels(&self, region: Region) -> impl Iterator<Item = (u32, u32, V)> + '_ {
        let (x0, y0, x1, y1) = region;
        (y0..y1.min(self.height)).flat_map(move |y| (x0..x1.min(self.width)).map(move |x| (x, y))).map(|(x, y)| (x, y, self.at(x, y)))
    }

    /// The colour most pixels of `region` have, for those `keep` lets through
    fn mode(&self, region: Region, keep: impl Fn(V) -> bool) -> Option<V> {
        let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
        for (_, _, p) in self.pixels(region) {
            if keep(p) {
                *counts.entry([p[0] as u8, p[1] as u8, p[2] as u8]).or_default() += 1;
            }
        }
        counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| [c[0] as f32, c[1] as f32, c[2] as f32])
    }
}

/// x0, y0, x1, y1 (end exclusive)
type Region = (u32, u32, u32, u32);

fn region_of(r: Rectangle) -> Region {
    (r.x.floor().max(0.0) as u32, r.y.floor().max(0.0) as u32, (r.x + r.width).ceil() as u32, (r.y + r.height).ceil() as u32)
}

/// The theme the test draws in, set for it (the harness's turn is held) and
/// put back as the other tests expect it
struct Themed {
    dark: bool,
    theme: cosmic::Theme,
}

impl Themed {
    fn set(dark: bool) -> Self {
        cosmic::theme::set_active(if dark { cosmic::theme::ThemeType::Dark } else { crate::tones::light_type_for_tests() });
        Self { dark, theme: cosmic::theme::active() }
    }

    /// The theme with its text colour set to a grey that does not read on the tint
    /// (the real themes' text always does, and is left as it is)
    fn with_text(dark: bool, grey: u8) -> Self {
        let base = if dark { cosmic::cosmic_theme::Theme::dark_default() } else { crate::tones::light_for_tests().cosmic().clone() };
        // (the background's text colour is a private field: set through the theme's serde form)
        let mut value = serde_json::to_value(&base).expect("the theme as JSON");
        assert!(value["background"]["on"].is_string(), "the text colour's form changed: {:?}", value["background"]["on"]);
        value["background"]["on"] = serde_json::json!(format!("#{grey:02X}{grey:02X}{grey:02X}FF"));
        let custom: cosmic::cosmic_theme::Theme = serde_json::from_value(value).expect("the theme from JSON");
        cosmic::theme::set_active(cosmic::theme::ThemeType::Custom(Arc::new(custom)));
        Self { dark, theme: cosmic::theme::active() }
    }

    fn cosmic(&self) -> &cosmic::cosmic_theme::Theme {
        self.theme.cosmic()
    }

    fn name(&self) -> &'static str {
        if self.dark { "dark" } else { "light" }
    }

    /// The accent a selection and a fill are made of
    fn accent(&self) -> Srgb {
        let a = self.cosmic().accent_color();
        Srgb::new(a.red, a.green, a.blue)
    }

    /// The text colour of an editor: the theme's
    fn text(&self) -> V {
        v_of(cosmic::iced::Color::from(self.cosmic().on_bg_color()))
    }

    /// The page's strongest text colour (a light theme's darkest text, not black)
    fn strongest(&self) -> Srgb {
        Role::Darkest.tone(if self.dark { ToneMode::Dark } else { ToneMode::Light }).expect("darkest has a tone")
    }

    fn tint_over(&self, beneath: V) -> V {
        v(tint(self.accent(), srgb_of(beneath)))
    }

    fn on_tint(&self, own: V, beneath: V) -> V {
        v(on_tint(srgb_of(own), self.accent(), srgb_of(beneath), self.strongest()))
    }
}

impl Drop for Themed {
    fn drop(&mut self) {
        cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    }
}

/// Where the widgets are, as laid out: the focusable ones (buttons, editors,
/// inputs) with their ids, the texts, and the text inputs
#[derive(Default)]
struct Probe {
    focusables: Vec<(Option<cosmic::widget::Id>, Rectangle)>,
    texts: Vec<(Rectangle, String)>,
    inputs: Vec<Rectangle>,
}

impl Operation<()> for Probe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }
    fn focusable(&mut self, id: Option<&cosmic::widget::Id>, bounds: Rectangle, _: &mut dyn Focusable) {
        self.focusables.push((id.cloned(), bounds));
    }
    fn text_input(&mut self, _: Option<&cosmic::widget::Id>, bounds: Rectangle, _: &mut dyn TextInput) {
        self.inputs.push(bounds);
    }
    fn text(&mut self, _: Option<&cosmic::widget::Id>, bounds: Rectangle, text: &str) {
        self.texts.push((bounds, text.to_string()));
    }
}

impl Probe {
    fn of(h: &mut Harness, app: &App) -> Probe {
        let mut probe = Probe::default();
        h.operate(app, &mut probe);
        probe
    }

    /// The smallest focusable widget holding the text `label`
    fn button_with(&self, label: &str) -> Option<Rectangle> {
        self.nth_button_with(label, false)
    }

    /// As [`Self::button_with`], for the last such text on the page
    fn last_button_with(&self, label: &str) -> Option<Rectangle> {
        self.nth_button_with(label, true)
    }

    fn nth_button_with(&self, label: &str, last: bool) -> Option<Rectangle> {
        let mut found = self.texts.iter().filter(|(_, t)| t == label);
        let text = if last { found.next_back()? } else { found.next()? }.0;
        let c = text.center();
        self.focusables
            .iter()
            .map(|(_, b)| *b)
            .filter(|b| b.contains(c) && b.height < 60.0)
            .min_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
    }
}

/// A frame of the app as `h` has it (drawn twice, the first not kept, as the
/// window's widgets and glyph cache catch up with the layout)
fn frame(h: &mut Harness, app: &App, name: &str) -> Frame {
    let _ = h.draw(app, name);
    let rgba = h.draw(app, name);
    Frame { width: h.size.width as u32, height: h.size.height as u32, rgba }
}

// ---- the editors ----

/// Many wide glyphs with straight stems, so some pixel of the text is
/// covered whole
const PLAIN: &str = "MMMMMMMM WWWWWWWW HHHHHHHH IIIIIIII llllllll\nMMMMMMMM WWWWWWWW HHHHHHHH IIIIIIII llllllll\nHHHHHHHH IIIIIIII MMMMMMMM WWWWWWWW llllllll\nIIIIIIII HHHHHHHH WWWWWWWW MMMMMMMM llllllll\nMMMMMMMM WWWWWWWW HHHHHHHH IIIIIIII llllllll\nMMMMMMMM WWWWWWWW HHHHHHHH IIIIIIII llllllll\nHHHHHHHH IIIIIIII MMMMMMMM WWWWWWWW llllllll\nIIIIIIII HHHHHHHH WWWWWWWW MMMMMMMM llllllll";

/// The highlighting of `PLAIN`: none, so every glyph is the editor's default
/// colour; or all of it one syntax class, so every glyph is that class's colour
fn highlight_of(text: &str, class: Option<Hl>) -> HighlightSettings {
    let lines = text.split('\n').map(|l| class.map(|c| vec![(0..l.len(), c)]).unwrap_or_default()).collect();
    HighlightSettings(Arc::new(HighlightData { generation: next_generation(), lines }), false)
}

/// Which editor of the window a scene draws
#[derive(Clone, Copy, Debug)]
enum Site {
    /// The source view of a document
    Source,
    /// A block of code being edited in the literate view
    CodeBlock,
    /// A paragraph of prose being edited in the literate view
    ProseBlock,
}

/// One editor, drawn with the caret and with everything selected
struct Scene {
    theme: Themed,
    /// Without a selection
    before: Frame,
    /// With everything selected
    after: Frame,
    /// The area the selection's fill covers (its bounding box), empty if
    /// there is none
    region: Region,
    /// Where the editor is
    editor: Region,
    /// How many pixels the selection changed
    changed: usize,
    /// What is beneath the selection: the colour the unselected frame shows
    /// where the selection came
    beneath: V,
    label: String,
    /// Holds the UI's turn while the scene is judged (the theme is global)
    _turn: Harness,
}

impl Scene {
    fn new(site: Site, dark: bool, class: Option<Hl>, label: &str) -> Self {
        Self::build(site, dark, None, class, label)
    }

    /// As [`Self::new`], with the theme's text a grey of its own
    fn build(site: Site, dark: bool, text_grey: Option<u8>, class: Option<Hl>, label: &str) -> Self {
        let mut app = demo_app(if matches!(site, Site::Source) { Mode::Source } else { Mode::Literate });
        app.show_right = false;
        app.show_sidebar = false;
        let (theme, mut h) = {
            // The harness takes the turn first; only then is the theme set
            let h = Harness::new(Size::new(1400.0, 900.0));
            (text_grey.map_or_else(|| Themed::set(dark), |g| Themed::with_text(dark, g)), h)
        };
        let id = match site {
            Site::Source => super::view::source_editor_id(),
            _ => super::view::block_editor_id(),
        };
        if !matches!(site, Site::Source) {
            let ws = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let kind = if matches!(site, Site::CodeBlock) { Kind::Formal } else { Kind::Text };
            let i = ws.blocks.iter().position(|b| b.kind == kind && b.name != "end").expect("a block of the kind");
            let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        }
        {
            let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
            let text = PLAIN;
            match site {
                Site::Source => {
                    t.source = Content::with_text(text);
                    t.highlight = highlight_of(text, class);
                }
                _ => {
                    let be = t.block_editor.as_mut().expect("an open block");
                    be.content = Content::with_text(text);
                    be.preview = None;
                    t.block_highlight = highlight_of(text, class);
                }
            }
        }
        h.focus(&app, id.clone());
        let name = format!("hl-{label}-{}-{site:?}", theme.name());
        let before = frame(&mut h, &app, &format!("{name}-caret"));
        let probe = Probe::of(&mut h, &app);
        let bounds = probe.focusables.iter().find(|(i, _)| i.as_ref() == Some(&id)).map(|(_, b)| *b).expect("the editor is on the page");
        {
            let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
            match site {
                Site::Source => t.source.perform(Action::SelectAll),
                _ => t.block_editor.as_mut().unwrap().content.perform(Action::SelectAll),
            }
        }
        let after = frame(&mut h, &app, &format!("{name}-all"));
        let region = region_of(bounds);
        // What changed with the selection: its fill and the text on it. What was beneath the
        // fill is what the unselected frame shows there (the editor's background, which is not
        // one colour across its padding)
        let changed: Vec<(u32, u32)> = before.pixels(region).filter(|(x, y, u)| dist(*u, after.at(*x, *y)) > 2.0).map(|(x, y, _)| (x, y)).collect();
        let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
        for (x, y) in &changed {
            let u = before.at(*x, *y);
            *counts.entry([u[0] as u8, u[1] as u8, u[2] as u8]).or_default() += 1;
        }
        let beneath = counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| [c[0] as f32, c[1] as f32, c[2] as f32]).unwrap_or([0.0; 3]);
        let tint = theme.tint_over(beneath);
        let fill_like: Vec<(u32, u32)> = changed.iter().copied().filter(|(x, y)| dist(after.at(*x, *y), tint) <= 2.0).collect();
        let area = (
            fill_like.iter().map(|p| p.0).min().unwrap_or(0),
            fill_like.iter().map(|p| p.1).min().unwrap_or(0),
            fill_like.iter().map(|p| p.0 + 1).max().unwrap_or(0),
            fill_like.iter().map(|p| p.1 + 1).max().unwrap_or(0),
        );
        Scene { theme, before, after, region: area, editor: region, beneath, changed: changed.len(), label: name, _turn: h }
    }

    /// The colour the selection fill shows as: the commonest colour of the
    /// selected frame that is not what was beneath
    fn fill_seen(&self) -> (V, usize) {
        let beneath = self.beneath;
        let fill = self.after.mode(self.editor, |p| dist(p, beneath) > 2.0).unwrap_or(beneath);
        let n = self.after.pixels(self.editor).filter(|(_, _, p)| dist(*p, fill) <= 2.0).count();
        (fill, n)
    }

    /// Judge the ink: every pixel that was ink before the selection must,
    /// after it, be a blend of the tint and the colour `wanted`; the furthest
    /// of them from the tint must be that colour
    fn judge_ink(&self, wanted: V) -> Vec<String> {
        let tint = self.theme.tint_over(self.beneath);
        // An editor can have both its ordinary background and the construct
        // band under the selection. Count only glyph ink, not either broad
        // background, even when the band differs by more than 12 levels.
        let mut colours = HashMap::<[u8; 3], usize>::new();
        for (_, _, pixel) in self.before.pixels(self.region) {
            *colours.entry([pixel[0] as u8, pixel[1] as u8, pixel[2] as u8]).or_default() += 1;
        }
        let area = (self.region.2 - self.region.0) as usize * (self.region.3 - self.region.1) as usize;
        let backgrounds: Vec<V> = colours.into_iter().filter_map(|(c, count)| {
            let colour = [c[0] as f32, c[1] as f32, c[2] as f32];
            (count * 20 >= area && dist(colour, self.beneath) <= 40.0).then_some(colour)
        }).collect();
        let (mut ink, mut strays) = (0usize, 0usize);
        let (mut furthest, mut furthest_at) = (tint, 0.0f32);
        let mut worst = (0.0f32, (0, 0), [0.0; 3]);
        for (x, y, u) in self.before.pixels(self.region) {
            if dist(u, self.beneath) <= 12.0 || backgrounds.iter().any(|background| dist(u, *background) <= 12.0) {
                continue;
            }
            ink += 1;
            let s = self.after.at(x, y);
            let d = off_segment(s, tint, wanted);
            if d > OFF_LINE {
                strays += 1;
                if d > worst.0 {
                    worst = (d, (x, y), s);
                }
            }
            let far = dist(s, tint);
            if far > furthest_at {
                furthest_at = far;
                furthest = s;
            }
        }
        let mut problems = Vec::new();
        if ink < 300 {
            problems.push(format!("the scene has only {ink} ink pixels: the test's set-up is wrong"));
            return problems;
        }
        if strays * 100 > ink * STRAY_PERCENT {
            problems.push(format!(
                "{strays} of {ink} ink pixels are no blend of the tint {} and {}: the worst, at {:?}, is {} ({:.0} off the line)",
                hex(tint),
                hex(wanted),
                worst.1,
                hex(worst.2),
                worst.0
            ));
        }
        if off(furthest, wanted) > REACH {
            problems.push(format!("the ink on the tint {} is drawn in {} at its strongest, not {}", hex(tint), hex(furthest), hex(wanted)));
        }
        if !problems.is_empty() {
            let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
            for (_, _, p) in self.after.pixels(self.region) {
                *counts.entry([p[0] as u8, p[1] as u8, p[2] as u8]).or_default() += 1;
            }
            let mut top: Vec<_> = counts.into_iter().collect();
            top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let top: Vec<String> = top.iter().take(6).map(|(c, n)| format!("{} x{n}", hex([c[0] as f32, c[1] as f32, c[2] as f32]))).collect();
            problems.push(format!("(the selected area's commonest colours: {})", top.join(", ")));
        }
        problems
    }
}

fn assert_clean(scene: &Scene, problems: Vec<String>) {
    assert!(problems.is_empty(), "{}: {}", scene.label, problems.join("; "));
}

/// The fill is the tint, the accent at 32 % over what is beneath, and it
/// covers the selected text opaquely; default-coloured text on it is
/// `on_tint` of the default colour
fn selection_is_the_tint(site: Site, dark: bool) {
    let scene = Scene::new(site, dark, None, "default");
    let tint = scene.theme.tint_over(scene.beneath);
    let (seen, count) = scene.fill_seen();
    assert!(scene.changed > 3000, "{}: the selection changed only {} pixels: no selection drawn", scene.label, scene.changed);
    assert!(count > 3000 && off(seen, tint) <= 2.0, "{}: the selection fill is {} ({count} px), not the tint {} of the accent over {}", scene.label, hex(seen), hex(tint), hex(scene.beneath));
    let wanted = scene.theme.on_tint(scene.theme.text(), scene.beneath);
    assert_clean(&scene, scene.judge_ink(wanted));
}

#[test]
fn the_source_selection_is_the_tint_in_the_dark_theme() {
    selection_is_the_tint(Site::Source, true);
}

#[test]
fn the_source_selection_is_the_tint_in_the_light_theme() {
    selection_is_the_tint(Site::Source, false);
}

#[test]
fn the_block_selection_is_the_tint_in_the_dark_theme() {
    selection_is_the_tint(Site::CodeBlock, true);
}

#[test]
fn the_block_selection_is_the_tint_in_the_light_theme() {
    selection_is_the_tint(Site::CodeBlock, false);
}

#[test]
fn the_prose_block_selection_is_the_tint_in_the_dark_theme() {
    selection_is_the_tint(Site::ProseBlock, true);
}

#[test]
fn the_prose_block_selection_is_the_tint_in_the_light_theme() {
    selection_is_the_tint(Site::ProseBlock, false);
}

/// The fill rule: a default text colour that does not read on the tint (a
/// theme whose text is a mid grey) goes to the page's strongest text colour,
/// `on_tint`; the real themes' own text already reads and is left as it is
/// ("a colour is only ever changed for contrast")
#[test]
fn a_default_text_that_does_not_read_on_the_tint_goes_to_the_strongest_colour() {
    for dark in [true, false] {
        for site in [Site::Source, Site::CodeBlock] {
            let scene = Scene::build(site, dark, Some(0x80), None, "grey-text");
            let own = scene.theme.text();
            let wanted = scene.theme.on_tint(own, scene.beneath);
            assert!(dist(own, wanted) > 40.0, "{}: the test's grey {} should not already read on the tint (it would go to {})", scene.label, hex(own), hex(wanted));
            assert!(scene.changed > 3000, "{}: the selection changed only {} pixels: no selection drawn", scene.label, scene.changed);
            assert_clean(&scene, scene.judge_ink(wanted));
        }
    }
}

/// Stage 1: a syntax-highlighted span keeps its own colour on the tint
/// (libcosmic can only recolour the editor's default text colour)
fn span_keeps_its_colour(site: Site, dark: bool) {
    let class = Hl::Sem(Sem::Keyword2);
    let scene = Scene::new(site, dark, Some(class), "span");
    let own = v_of(highlight_format(&class, &scene.theme.theme).color.expect("a colour"));
    assert!(scene.changed > 3000, "{}: the selection changed only {} pixels: no selection drawn", scene.label, scene.changed);
    assert_clean(&scene, scene.judge_ink(own));
}

#[test]
fn a_highlighted_span_keeps_its_colour_on_the_tint() {
    for dark in [true, false] {
        span_keeps_its_colour(Site::Source, dark);
        span_keeps_its_colour(Site::CodeBlock, dark);
    }
}

/// Guard: with nothing selected an editor is as it was: its text in the
/// theme's text colour, and no tint anywhere
#[test]
fn unselected_editors_are_unchanged() {
    for dark in [true, false] {
        for site in [Site::Source, Site::CodeBlock, Site::ProseBlock] {
            let scene = Scene::new(site, dark, None, "unselected");
            let tint = scene.theme.tint_over(scene.beneath);
            let text = scene.theme.text();
            let (mut furthest, mut far) = (scene.beneath, 0.0f32);
            let mut tinted = 0;
            for (_, _, p) in scene.before.pixels(scene.editor) {
                if dist(p, tint) <= 2.0 {
                    tinted += 1;
                }
                if dist(p, scene.beneath) > far {
                    far = dist(p, scene.beneath);
                    furthest = p;
                }
            }
            assert!(tinted < 20, "{}: {tinted} pixels of the tint {} with nothing selected", scene.label, hex(tint));
            assert!(off(furthest, text) <= REACH, "{}: unselected text is drawn in {}, not the theme's {}", scene.label, hex(furthest), hex(text));
        }
    }
}

// ---- the fill is opaque ----

/// A document with the caret in a proof, so the source view tints the
/// construct the caret is in behind its lines, under a run of lines that
/// have no tint
fn construct_app() -> App {
    let mut text = String::from("theory Pop imports Main begin\n\n");
    for i in 0..14 {
        text.push_str(&format!("(* line {i} of the filler *)\n"));
    }
    text.push_str("\nlemma pop: \"(a::nat) + b + c = c + b + a\"\nproof -\n  have one: \"a + b = b + a\"\n    by simp\n  show ?thesis\n    using one\n    by simp\nqed\n\nend\n");
    let commands: Vec<serde_json::Value> = super::prover::isabelle::blocks::lexical_commands(&text, &HashMap::new())
        .iter()
        .map(|c| serde_json::json!([c.start, c.end, c.name, "finished"]))
        .collect();
    let prover = serde_json::json!({
        "revision": 1,
        "status": {"finished": commands.len(), "unprocessed": 0, "running": 0, "failed": 0, "consolidated": true},
        "commands": commands,
        "messages": [],
    });
    let doc = serde_json::json!({"text": text, "revision": 1, "prover": prover});
    let mut app = demo_app(Mode::Source);
    let theory = super::editor::Theory::new("/tmp/Pop.thy".into(), &doc, &super::prover::isabelle::symbols::Table::fallback(), Arc::new(HashMap::new()), Mode::Source, crate::prover::profile(None));
    app.workspace.as_mut().unwrap().theory = Some(theory);
    app.show_right = false;
    app.show_sidebar = false;
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = text[..text.find("have one").unwrap()].chars().count() + 3;
    app
}

/// The selection is opaque: it is the tint of the accent over the page on
/// every line, also over the tint of the construct the caret is in, which
/// does not show through it (the translucent fill shows the construct's
/// band through, so lines of the construct are a different colour from the
/// others)
#[test]
fn the_source_selection_is_one_tint_on_every_line_also_over_the_construct_band() {
    for dark in [true, false] {
        let app = construct_app();
        let mut h = Harness::new(Size::new(1400.0, 900.0));
        let theme = Themed::set(dark);
        let id = super::view::source_editor_id();
        h.focus(&app, id.clone());
        let name = format!("hl-band-{}", theme.name());
        let before = frame(&mut h, &app, &format!("{name}-caret"));
        let probe = Probe::of(&mut h, &app);
        let bounds = probe.focusables.iter().find(|(i, _)| i.as_ref() == Some(&id)).map(|(_, b)| *b).expect("the source editor");
        let mut app = app;
        app.workspace.as_mut().unwrap().theory.as_mut().unwrap().source.perform(Action::SelectAll);
        let after = frame(&mut h, &app, &format!("{name}-all"));
        let (x0, y0, x1, y1) = region_of(bounds);
        // Each row's commonest colour before (the page, or the construct's band) and the commonest
        // colour of what the selection changed in it (its fill)
        let mut rows: Vec<(u32, V, V)> = Vec::new();
        for y in y0..y1.min(before.height) {
            let changed: Vec<u32> = (x0..x1.min(before.width)).filter(|x| dist(before.at(*x, y), after.at(*x, y)) > 2.0).collect();
            if changed.len() < 40 {
                continue;
            }
            let mode = |f: &dyn Fn(u32) -> V| {
                let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
                for x in &changed {
                    let p = f(*x);
                    *counts.entry([p[0] as u8, p[1] as u8, p[2] as u8]).or_default() += 1;
                }
                counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| [c[0] as f32, c[1] as f32, c[2] as f32]).unwrap()
            };
            rows.push((y, mode(&|x| before.at(x, y)), mode(&|x| after.at(x, y))));
        }
        assert!(rows.len() > 100, "{name}: only {} rows were selected: no selection drawn", rows.len());
        // The page is what most selected rows had beneath them
        let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
        for (_, b, _) in &rows {
            *counts.entry([b[0] as u8, b[1] as u8, b[2] as u8]).or_default() += 1;
        }
        let page = counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| [c[0] as f32, c[1] as f32, c[2] as f32]).unwrap();
        // The construct's band: the commonest colour of the other rows (rows at the edge of a
        // selection or of the band are blends of two, and are left out)
        let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
        for (_, b, _) in rows.iter().filter(|(_, b, _)| dist(*b, page) > 3.0) {
            *counts.entry([b[0] as u8, b[1] as u8, b[2] as u8]).or_default() += 1;
        }
        let band = counts.into_iter().max_by_key(|(c, n)| (*n, *c)).map(|(c, _)| [c[0] as f32, c[1] as f32, c[2] as f32]).expect("a band");
        let under_band: Vec<&(u32, V, V)> = rows.iter().filter(|(_, b, _)| dist(*b, band) <= 1.0).collect();
        let on_page: Vec<&(u32, V, V)> = rows.iter().filter(|(_, b, _)| dist(*b, page) <= 1.0).collect();
        assert!(under_band.len() > 20, "{name}: the construct's band {} covers only {} selected rows: the test's set-up is wrong", hex(band), under_band.len());
        let want = theme.tint_over(page);
        // (a row where two lines' fills meet is partly covered: between the page and the tint)
        // (and the caret's line, where the selection's end leaves it with no text selected, shows
        // COSMIC's grey wash, over the page or the band, or an edge row of it between that and what is
        // beneath: not a selection)
        let wash = super::tones::caret_line(&theme.theme);
        let washed = |b: V| v_of(super::tones::over(wash, cosmic::iced::Color::from_rgb(b[0] / 255.0, b[1] / 255.0, b[2] / 255.0)));
        let judged: Vec<&(u32, V, V)> = under_band.iter().chain(on_page.iter()).copied().filter(|(_, b, f)| off_segment(*f, *b, washed(*b)) > 2.0).collect();
        let right = judged.iter().filter(|(_, _, f)| dist(*f, want) <= 2.0).count();
        let wrong: Vec<&(u32, V, V)> = judged.iter().copied().filter(|(_, _, f)| off_segment(*f, page, want) > 2.0 || right * 100 < judged.len() * 75 && dist(*f, want) > 2.0).collect();
        assert!(
            wrong.is_empty(),
            "{name}: the selection should be the tint {} of the accent over the page {} on every line, also over the construct's band {}; {} of {} rows under the band and {} on the page are another colour, such as {}",
            hex(want),
            hex(page),
            hex(band),
            wrong.iter().filter(|(_, b, _)| dist(*b, band) <= 1.0).count(),
            under_band.len(),
            wrong.iter().filter(|(_, b, _)| dist(*b, page) <= 1.0).count(),
            wrong.iter().step_by((wrong.len() / 4).max(1)).map(|(y, b, f)| format!("y={y} over {} is {}", hex(*b), hex(*f))).collect::<Vec<_>>().join(", ")
        );
    }
}

// ---- text boxes keep the solid selection ----

/// Selects everything in the first text input of the window, and focuses it
struct SelectInput {
    at: Rectangle,
}

impl Operation<()> for SelectInput {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
        operate(self);
    }
    fn focusable(&mut self, _: Option<&cosmic::widget::Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if bounds == self.at {
            state.focus();
        }
    }
    fn text_input(&mut self, _: Option<&cosmic::widget::Id>, bounds: Rectangle, state: &mut dyn TextInput) {
        if bounds == self.at {
            state.select_all();
        }
    }
}

/// Guard (rule 6: "text boxes keep the solid default"): the selection in a
/// text input is the theme's solid accent, with its on colour on it
#[test]
fn a_text_input_keeps_the_solid_selection() {
    for dark in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.show_right = true;
        app.workspace.as_mut().unwrap().panel = Panel::Symbols;
        app.workspace.as_mut().unwrap().symbol_search = "MMMMMMMMMMMMMMMMMMMM".into();
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let _ = frame(&mut h, &app, "hl-input-warm");
        let probe = Probe::of(&mut h, &app);
        // The symbol search of the side panel (the page's other input is the note's reply, left of it)
        let input = probe.inputs.iter().copied().filter(|b| b.x > 900.0).last().expect("the symbol search on the page");
        let before = frame(&mut h, &app, "hl-input-before");
        h.operate(&app, &mut SelectInput { at: input });
        let after = frame(&mut h, &app, "hl-input-after");
        let region = region_of(input);
        let accent = v(theme.accent());
        let beneath = before.mode(region, |_| true).expect("pixels");
        let fill = after.mode(region, |p| dist(p, beneath) > 2.0).expect("a selection");
        assert!(off(fill, accent) <= 2.0, "{}: a text input's selection is {}, not the solid accent {}", theme.name(), hex(fill), hex(accent));
    }
}

// ---- chosen parts of a row, and on-toggles ----

/// The muted accent of the theme and the colour for text on it
fn muted_of(theme: &Themed) -> (V, V) {
    let muted = muted_accent(theme.accent(), theme.dark);
    (v(muted), v(on_muted(muted)))
}

/// The fill of a control, sampled just inside its left edge on its middle line
fn fill_at(frame: &Frame, b: Rectangle) -> V {
    frame.at((b.x + 3.0) as u32, (b.y + b.height / 2.0) as u32)
}

/// Check a chosen control: its fill is the muted accent, and what is drawn on
/// it is the on colour (greys all the way) with at least 4.5:1 against the
/// fill
fn assert_muted_fill(theme: &Themed, frame: &Frame, b: Rectangle, what: &str) {
    let (muted, on) = muted_of(theme);
    let fill = fill_at(frame, b);
    assert!(off(fill, muted) <= 3.0, "{what} ({}): filled {}, not the muted accent {} (the accent is {})", theme.name(), hex(fill), hex(muted), hex(v(theme.accent())));
    // What is drawn on the fill: the pixel furthest from it
    let (mut furthest, mut far) = (fill, 0.0f32);
    for (_, _, p) in frame.pixels(region_of(Rectangle { x: b.x + 4.0, y: b.y + 2.0, width: (b.width - 8.0).max(1.0), height: (b.height - 4.0).max(1.0) })) {
        if dist(p, fill) > far {
            far = dist(p, fill);
            furthest = p;
        }
    }
    assert!(far > 100.0, "{what} ({}): nothing is drawn on the fill (furthest pixel {} from {})", theme.name(), hex(furthest), hex(fill));
    assert!(off(furthest, on) <= 40.0, "{what} ({}): drawn on the fill in {}, not the on colour {}", theme.name(), hex(furthest), hex(on));
    let ratio = contrast_ratio(srgb_of(furthest), srgb_of(fill));
    assert!(ratio >= 4.5, "{what} ({}): {} on {} is {ratio:.2}:1, under 4.5:1", theme.name(), hex(furthest), hex(fill));
}

/// The measurements recognise the right look: a button drawn here with the
/// muted accent and its on colour passes `assert_muted_fill`, so a failure of
/// the app's own tabs is the app's
#[test]
fn a_reference_button_in_the_muted_accent_passes_the_checks() {
    use cosmic::iced::{Color, Length};
    use cosmic::{theme, widget};
    for dark in [true, false] {
        let turn = super::snapshot::ui_turn();
        let theme = Themed::set(dark);
        let (muted, on) = muted_of(&theme);
        let (fill, text) = (Color::from_rgb8(muted[0] as u8, muted[1] as u8, muted[2] as u8), Color::from_rgb8(on[0] as u8, on[1] as u8, on[2] as u8));
        let style = move |_: &cosmic::Theme| {
            let mut s = widget::button::Style::new();
            s.background = Some(fill.into());
            s.text_color = Some(text);
            s.icon_color = Some(text);
            s.border_radius = 13.0.into();
            s
        };
        let button = widget::button::custom(widget::text("Goals").size(13.0).width(Length::Fill).align_x(cosmic::iced::Alignment::Center))
            .width(Length::Fixed(64.0))
            .height(Length::Fixed(26.0))
            .padding(0)
            .class(theme::Button::Custom { active: Box::new(move |_, t| style(t)), disabled: Box::new(style), hovered: Box::new(move |_, t| style(t)), pressed: Box::new(move |_, t| style(t)) });
        let element: cosmic::Element<'_, super::Message> = widget::container(button).padding(20).into();
        let size = Size::new(200.0, 100.0);
        let rgba = super::snapshot::render(element, "hl-reference-muted", size);
        let f = Frame { width: 200, height: 100, rgba };
        assert_muted_fill(&theme, &f, Rectangle { x: 20.0, y: 20.0, width: 64.0, height: 26.0 }, "the reference button");
        drop(turn);
    }
}

/// The chosen tab of the side panel
#[test]
fn the_chosen_side_panel_tab_is_the_muted_accent() {
    for dark in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.show_right = true;
        app.workspace.as_mut().unwrap().panel = Panel::Symbols;
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let f = frame(&mut h, &app, "hl-side-tab");
        let probe = Probe::of(&mut h, &app);
        let tab = probe.button_with("Symbols").expect("the Symbols tab");
        assert_muted_fill(&theme, &f, tab, "the chosen side-panel tab");
    }
}

/// The chosen section tab of the sidebar (icons only): the one filled button
/// of the row is the muted accent
#[test]
fn the_chosen_sidebar_tab_is_the_muted_accent() {
    for dark in [true, false] {
        let app = demo_app(Mode::Literate);
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let f = frame(&mut h, &app, "hl-sidebar-tab");
        let probe = Probe::of(&mut h, &app);
        // The row of section tabs: equal, 32 px tall buttons side by side at the sidebar's top
        let row: Vec<Rectangle> = probe.focusables.iter().map(|(_, b)| *b).filter(|b| b.height == 32.0 && b.x < 400.0 && b.y < 200.0 && b.width > 20.0).collect();
        assert!(row.len() >= 3, "the sidebar's tab row: {} buttons found", row.len());
        let (muted, _) = muted_of(&theme);
        let chosen: Vec<Rectangle> = row.iter().copied().filter(|b| dist(fill_at(&f, *b), v(theme.accent())) < 40.0 || dist(fill_at(&f, *b), muted) < 40.0).collect();
        assert_eq!(chosen.len(), 1, "one chosen tab in the sidebar's row ({} found)", chosen.len());
        assert_muted_fill(&theme, &f, chosen[0], "the chosen sidebar tab");
    }
}

/// The on side of the Goals/Text switch
#[test]
fn the_chosen_goals_text_switch_is_the_muted_accent() {
    for dark in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.show_right = true;
        app.workspace.as_mut().unwrap().panel = Panel::Output;
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let f = frame(&mut h, &app, "hl-goals-text");
        let probe = Probe::of(&mut h, &app);
        let on = probe.last_button_with("Goals").expect("the Goals/Text switch of the side panel");
        assert_muted_fill(&theme, &f, on, "the chosen side of the Goals/Text switch");
    }
}

/// Rule 7: a Primary button stays the full accent (the guard that the muted
/// fill does not leak into it)
#[test]
fn primary_buttons_stay_full_accent() {
    for dark in [true, false] {
        // The welcome page: no session open, its one main action
        let mut app = demo_app(Mode::Literate);
        app.workspace = None;
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let f = frame(&mut h, &app, "hl-primary");
        let probe = Probe::of(&mut h, &app);
        let b = probe.button_with("Open a folder").expect("the Primary button of the welcome page");
        let fill = fill_at(&f, b);
        let accent = v(theme.accent());
        assert!(off(fill, accent) <= 3.0, "the Primary button ({}) is {}, not the accent {}", theme.name(), hex(fill), hex(accent));
    }
}

/// Text on the vim badge keeps at least 4.5:1 against its fill (the badge is
/// neither a chosen part nor a toggle, so only its text is held)
#[test]
fn the_vim_badge_text_reads() {
    for dark in [true, false] {
        let mut app = demo_app(Mode::Literate);
        app.vim_enabled = true;
        app.vim.mode = super::vim::Mode::Normal;
        let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == Kind::Formal).unwrap();
        let _ = cosmic::Application::update(&mut app, super::Message::EditBlock(i));
        let mut h = Harness::new(Size::new(1600.0, 900.0));
        let theme = Themed::set(dark);
        let f = frame(&mut h, &app, "hl-vim-badge");
        let probe = Probe::of(&mut h, &app);
        let label = super::vim::Mode::Normal.label();
        let (text, _) = probe.texts.iter().find(|(_, t)| t == label).map(|(b, t)| (*b, t.clone())).expect("the vim badge");
        let fill = f.at((text.x - 4.0) as u32, (text.y + text.height / 2.0) as u32);
        let (mut furthest, mut far) = (fill, 0.0f32);
        for (_, _, p) in f.pixels(region_of(text)) {
            if dist(p, fill) > far {
                far = dist(p, fill);
                furthest = p;
            }
        }
        let ratio = contrast_ratio(srgb_of(furthest), srgb_of(fill));
        assert!(ratio >= 4.5, "the vim badge ({}): {} on {} is {ratio:.2}:1", theme.name(), hex(furthest), hex(fill));
    }
}

// ---- added by the builder (TESTING.md section 5, step 2): the two chosen parts the headless
// window does not show, the header's Literate/Source switch and the "Queries go to" choice, drawn
// on their own from the code that builds them

/// Where the muted accent fills `frame`: the bounding box of the pixels of that colour, and how
/// many there are
fn muted_area(frame: &Frame, muted: V) -> Option<(Rectangle, usize)> {
    let (mut x0, mut y0, mut x1, mut y1, mut n) = (u32::MAX, u32::MAX, 0, 0, 0usize);
    for (x, y, p) in frame.pixels((0, 0, frame.width, frame.height)) {
        if off(p, muted) <= 3.0 {
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1), n + 1);
        }
    }
    (n > 0).then(|| (Rectangle { x: x0 as f32, y: y0 as f32, width: (x1 - x0) as f32, height: (y1 - y0) as f32 }, n))
}

/// The one chosen part of a control drawn alone in `frame`: filled with the muted accent, what is
/// on it in the on colour at 4.5:1 or more; where it is
fn chosen_part(theme: &Themed, frame: &Frame, what: &str) -> Rectangle {
    let (muted, _) = muted_of(theme);
    let (b, n) = muted_area(frame, muted).unwrap_or_else(|| panic!("{what} ({}): no part filled with the muted accent {}", theme.name(), hex(muted)));
    assert!(n > 300, "{what} ({}): only {n} pixels of the muted accent {}", theme.name(), hex(muted));
    assert_muted_fill(theme, frame, b, what);
    b
}

/// The header's Literate/Source switch (libcosmic's segmented control): its chosen part is the
/// muted accent, and it is the part of the view shown (Literate on the left, Source on the right)
#[test]
fn the_chosen_part_of_the_header_view_switch_is_the_muted_accent() {
    use cosmic::iced::{Alignment, Length};
    use cosmic::widget;
    for dark in [true, false] {
        let mut parts = Vec::new();
        for mode in [Mode::Literate, Mode::Source] {
            let turn = super::snapshot::ui_turn();
            let theme = Themed::set(dark);
            let app = demo_app(mode);
            let row = widget::row::with_children(vec![super::shell::views_bar(&app)]).align_y(Alignment::Center);
            let element: cosmic::Element<'_, super::Message> = widget::container(row).padding(20).width(Length::Fixed(600.0)).into();
            let rgba = super::snapshot::render(element, &format!("hl-header-switch-{}-{mode:?}", theme.name()), Size::new(600.0, 80.0));
            let f = Frame { width: 600, height: 80, rgba };
            parts.push(chosen_part(&theme, &f, &format!("the chosen part of the header's view switch ({mode:?})")));
            drop(theme);
            drop(turn);
        }
        // (the two parts meet, so their fills may share a column of blended pixels)
        assert!(parts[0].x + parts[0].width < parts[1].center_x() && parts[0].center_x() < parts[1].x, "the chosen part should be Literate (left) in the literate view and Source (right) in the source view: {parts:?}");
    }
}

/// Where `claude` and `codex` queries go: the chosen one is the muted accent, and it is the route
/// the session has
#[test]
fn the_chosen_query_route_is_the_muted_accent() {
    use cosmic::iced::Length;
    use cosmic::widget;
    for dark in [true, false] {
        let mut parts = Vec::new();
        for route in ["helper", "fresh"] {
            let turn = super::snapshot::ui_turn();
            let theme = Themed::set(dark);
            let mut app = demo_app(Mode::Literate);
            app.workspace.as_mut().unwrap().activity = Some(serde_json::json!({"route": route}));
            let ws = app.workspace.as_ref().unwrap();
            let element: cosmic::Element<'_, super::Message> = widget::container(super::view::query_route_row(ws)).padding(20).width(Length::Fixed(700.0)).into();
            let rgba = super::snapshot::render(element, &format!("hl-query-route-{}-{route}", theme.name()), Size::new(700.0, 90.0));
            let f = Frame { width: 700, height: 90, rgba };
            parts.push(chosen_part(&theme, &f, &format!("the chosen query route ({route})")));
            drop(theme);
            drop(turn);
        }
        assert!(parts[0].x + parts[0].width < parts[1].center_x() && parts[0].center_x() < parts[1].x, "the chosen route should be \"the helper\", then \"a fresh request\" right of it: {parts:?}");
    }
}

/// The header's toggle for the goal beside the text, a toggle that is on: filled with the muted
/// accent, its icon in the on colour; off, no fill at all (rule 6: "never rely on `.selected()`
/// on an Icon-class button")
#[test]
fn the_goal_beside_toggle_is_the_muted_accent_when_on() {
    use cosmic::iced::{Alignment, Length};
    use cosmic::widget;
    for dark in [true, false] {
        let mut app = demo_app(Mode::Literate);
        assert!(super::view::goal_beside_on(), "the goal beside the text is on by default");
        for on in [true, false] {
            let turn = super::snapshot::ui_turn();
            let theme = Themed::set(dark);
            let row = widget::row::with_children(vec![super::shell::goal_beside_button()]).align_y(Alignment::Center);
            let element: cosmic::Element<'_, super::Message> = widget::container(row).padding(20).width(Length::Fixed(500.0)).into();
            let rgba = super::snapshot::render(element, &format!("hl-goal-beside-{}-{on}", theme.name()), Size::new(500.0, 80.0));
            let f = Frame { width: 500, height: 80, rgba };
            if on {
                let b = chosen_part(&theme, &f, "the goal-beside toggle, on");
                assert!(b.width < 48.0 && b.height < 48.0, "only the toggle is filled: {b:?}");
            } else {
                let (muted, _) = muted_of(&theme);
                assert!(muted_area(&f, muted).is_none_or(|(_, n)| n < 20), "the goal-beside toggle, off ({}): filled with the muted accent", theme.name());
            }
            drop(theme);
            drop(turn);
            let _ = cosmic::Application::update(&mut app, super::Message::ToggleGoalBeside);
        }
        assert!(super::view::goal_beside_on(), "put back on");
    }
}

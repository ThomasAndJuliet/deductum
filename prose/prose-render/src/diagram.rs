// SPDX-License-Identifier: GPL-3.0-only

//! Mermaid diagrams in threads: a `mermaid` code block, laid out and drawn
//! as SVG by merman, a pure-Rust port of Mermaid, as Zed draws them.
//!
//! merman measures labels through a callback, answered here with
//! cosmic-text in the family the SVG names, so labels fit their boxes. Its
//! colours are the thread's, given as a host theme; the few it hard-codes
//! are recoloured after. The labels are then taken out of the SVG: usvg lays
//! them out, and they come back as [`Label`]s for the rich-text widget to
//! shape as cells, so that they select and copy like the rest of the thread.
//! The SVG with its text is kept for the diagram's own window.
//!
//! Diagrams are drawn when the view lays them out, and kept in one cache for
//! every thread, as formulas are, bounded by [`CACHE_BYTES`].

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use cosmic::iced::Color;
use cosmic::iced::advanced::svg;
use cosmic_text::fontdb;

/// The most SVG the cache holds, in bytes
const CACHE_BYTES: usize = 16 * 1024 * 1024;

/// The colours a diagram is drawn in
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub page: Color,
    /// Behind nodes
    pub surface: Color,
    pub text: Color,
    pub muted: Color,
    /// Borders of groups
    pub rule: Color,
    pub accent: Color,
    pub error: Color,
}

impl Palette {
    fn key(&self) -> [[u8; 4]; 7] {
        [self.page, self.surface, self.text, self.muted, self.rule, self.accent, self.error].map(Color::into_rgba8)
    }

    pub fn dark(&self) -> bool {
        self.page.relative_luminance() < 0.25
    }
}

/// A line of a diagram's text, in pixels from the diagram's top left
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
    /// Its left edge
    pub x: f32,
    pub baseline: f32,
}

/// A diagram, drawn
#[derive(Debug)]
pub struct Diagram {
    /// Its shapes, without text
    pub handle: svg::Handle,
    /// Its shapes and text, for its own window
    pub full: svg::Handle,
    pub svg: Arc<str>,
    pub width: f32,
    pub height: f32,
    pub labels: Vec<Label>,
    /// The family its text is set in
    pub family: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    source: String,
    palette: [[u8; 4]; 7],
}

struct Cache {
    entries: HashMap<Key, (Result<Arc<Diagram>, String>, u64, usize)>,
    bytes: usize,
    clock: u64,
}

static CACHE: LazyLock<Mutex<Cache>> =
    LazyLock::new(|| Mutex::new(Cache { entries: HashMap::new(), bytes: 0, clock: 0 }));

/// The text of each diagram last drawn, by its source, for copying and
/// selecting it without drawing it
static LABELS: LazyLock<Mutex<HashMap<u64, Arc<Vec<String>>>>> = LazyLock::new(Default::default);

fn source_key(source: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// The lines of text of `source`'s diagram, if it has been drawn
pub fn labels_of(source: &str) -> Option<Arc<Vec<String>>> {
    LABELS.lock().unwrap().get(&source_key(source)).cloned()
}

/// `source` drawn in `palette`, or why it can't be
pub fn render(source: &str, palette: &Palette) -> Result<Arc<Diagram>, String> {
    let key = Key { source: source.to_string(), palette: palette.key() };
    let mut cache = CACHE.lock().unwrap();
    cache.clock += 1;
    let clock = cache.clock;
    if let Some((result, used, _)) = cache.entries.get_mut(&key) {
        *used = clock;
        return result.clone();
    }
    drop(cache);
    // merman and usvg can panic on input they don't expect; a diagram is
    // not worth a crash
    let result = std::panic::catch_unwind(|| draw(source, palette))
        .unwrap_or_else(|_| Err("the diagram could not be laid out".to_string()));
    if let Ok(diagram) = &result {
        let mut labels = LABELS.lock().unwrap();
        if labels.len() > 256 {
            labels.clear();
        }
        labels.insert(source_key(source), Arc::new(diagram.labels.iter().map(|l| l.text.clone()).collect()));
    }
    let bytes = source.len() + 64 + result.as_ref().map_or(0, |diagram| 2 * diagram.svg.len());
    let mut cache = CACHE.lock().unwrap();
    cache.bytes += bytes;
    cache.entries.insert(key, (result.clone(), clock, bytes));
    if cache.bytes > CACHE_BYTES {
        // Drop the least recently used half
        let mut uses: Vec<u64> = cache.entries.values().map(|(_, used, _)| *used).collect();
        uses.sort_unstable();
        let cutoff = uses[uses.len() / 2];
        let mut freed = 0;
        cache.entries.retain(|_, (_, used, bytes)| {
            let keep = *used > cutoff;
            if !keep {
                freed += *bytes;
            }
            keep
        });
        cache.bytes -= freed;
    }
    result
}

/// The cache's size in bytes and entries, for the memory readout
pub fn cache_usage() -> (usize, usize) {
    let cache = CACHE.lock().unwrap();
    (cache.bytes, cache.entries.len())
}

// ---------------------------------------------------------------------------
// Fonts

/// The fonts diagrams are measured and laid out with: the app's, and a
/// cosmic-text of their own, as merman measures while the view holds the
/// app's
struct Fonts {
    db: Arc<fontdb::Database>,
    family: String,
    measure: Mutex<(cosmic_text::FontSystem, HashMap<(String, u32, bool, bool), f64>)>,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

/// Take the app's fonts, before the first diagram is drawn while the app's
/// font system is held
pub fn prepare(system: &cosmic_text::FontSystem) {
    FONTS.get_or_init(|| fonts_from(system.db(), system.locale()));
}

fn fonts() -> &'static Fonts {
    FONTS.get_or_init(|| {
        let mut system = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
        let system = system.raw();
        fonts_from(system.db(), system.locale())
    })
}

fn fonts_from(db: &fontdb::Database, locale: &str) -> Fonts {
    // The interface's sans-serif, by the name its faces give, or else the
    // first of the usual ones installed
    let family = std::iter::once(db.family_name(&fontdb::Family::SansSerif))
        .chain(["Open Sans", "Noto Sans", "Inter", "Cantarell", "DejaVu Sans", "Liberation Sans"])
        .find(|name| {
            db.query(&fontdb::Query { families: &[fontdb::Family::Name(name)], ..Default::default() }).is_some()
        })
        .or_else(|| db.faces().find_map(|face| face.families.first().map(|(name, _)| name.as_str())))
        .unwrap_or("sans-serif")
        .to_string();
    let system = cosmic_text::FontSystem::new_with_locale_and_db(locale.to_string(), db.clone());
    Fonts { db: Arc::new(db.clone()), family, measure: Mutex::new((system, HashMap::new())) }
}

/// The width of `text` in the diagrams' family
fn measure(text: &str, size: f32, bold: bool, italic: bool) -> f64 {
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight};
    let fonts = fonts();
    let mut guard = fonts.measure.lock().unwrap();
    let (system, widths) = &mut *guard;
    let key = (text.to_string(), size.to_bits(), bold, italic);
    if let Some(width) = widths.get(&key) {
        return *width;
    }
    let mut buffer = Buffer::new(system, Metrics::new(size, size * 1.2));
    buffer.set_size(None, None);
    let attrs = Attrs::new()
        .family(Family::Name(&fonts.family))
        .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if italic { Style::Italic } else { Style::Normal });
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    let width = buffer.layout_runs().map(|run| run.line_w).fold(0.0f32, f32::max) as f64;
    if widths.len() > 16_384 {
        widths.clear();
    }
    widths.insert(key, width);
    width
}

// ---------------------------------------------------------------------------
// Drawing

fn hex(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `a` moved `t` of the way to `b`
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t)
}

/// `color` turned `degrees` round the hue circle, as series colours that
/// go with the accent
fn turned(color: Color, degrees: f32) -> Color {
    let (r, g, b) = (color.r, color.g, color.b);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-4 {
        return color;
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = (h + degrees).rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Color::from_rgb(r + m, g + m, b + m)
}

fn theme(palette: &Palette, family: &str) -> Result<merman::svg::HostTheme, String> {
    use merman::svg::{HostTheme, HostThemeAppearance, ThemeRole};
    let p = palette;
    let roles = [
        (ThemeRole::Canvas, p.page),
        (ThemeRole::Surface, p.surface),
        (ThemeRole::SurfaceAlt, mix(p.surface, p.text, 0.08)),
        (ThemeRole::SurfaceMuted, mix(p.page, p.surface, 0.5)),
        (ThemeRole::Text, p.text),
        (ThemeRole::SubtleText, p.muted),
        (ThemeRole::Border, mix(p.surface, p.accent, 0.8)),
        (ThemeRole::Line, p.muted),
        (ThemeRole::EdgeLabelBackground, p.page),
        (ThemeRole::ClusterBackground, mix(p.page, p.text, 0.035)),
        (ThemeRole::ClusterBorder, p.rule),
        (ThemeRole::NoteBackground, mix(p.page, p.accent, 0.14)),
        (ThemeRole::NoteBorder, mix(p.page, p.accent, 0.55)),
        (ThemeRole::NoteText, p.text),
        (ThemeRole::ActorBackground, p.surface),
        (ThemeRole::ActorBorder, mix(p.surface, p.accent, 0.8)),
        (ThemeRole::ActorText, p.text),
        (ThemeRole::ActivationBackground, mix(p.surface, p.accent, 0.25)),
        (ThemeRole::ActivationBorder, mix(p.surface, p.accent, 0.8)),
        (ThemeRole::Error, p.error),
        (ThemeRole::Warning, turned(p.error, 40.0)),
        (ThemeRole::Success, turned(p.error, 120.0)),
    ];
    let mut theme = HostTheme::new()
        .with_appearance(if p.dark() { HostThemeAppearance::Dark } else { HostThemeAppearance::Light })
        .try_with_font_family(family)
        .map_err(|err| err.to_string())?;
    for (role, color) in roles {
        theme = theme.try_with_role(role, hex(color)).map_err(|err| err.to_string())?;
    }
    // Series colours go round from the accent, toned towards the page
    let series: Vec<String> =
        [0.0, 150.0, 60.0, 210.0, 300.0, 100.0, 255.0, 30.0].map(|d| hex(mix(turned(p.accent, d), p.page, 0.1))).into();
    theme.try_with_series_palette(series).map_err(|err| err.to_string())
}

/// What merman hard-codes, in the palette's colours
fn recolour(svg: &str, palette: &Palette) -> String {
    let tile = hex(mix(palette.page, palette.accent, 0.85));
    let muted = hex(palette.muted);
    let text_fill = format!(r#"style="fill:{}""#, hex(palette.text));
    // C4 sets words as spans that start with their space, which usvg drops
    let mut out = svg.replace(
        r#"<tspan class="text-inner-tspan"> "#,
        r#"<tspan class="text-inner-tspan" xml:space="preserve"> "#,
    );
    for (from, to) in [
        // Architecture icons' tiles
        ("#087ebf", tile.as_str()),
        // C4's descriptions, and its white text on the palette's surface
        ("#444444", muted.as_str()),
        (r#"style="fill:#FFFFFF""#, text_fill.as_str()),
    ] {
        out = out.replace(from, to);
    }
    out
}

/// `svg` without its `<text>` elements, which are drawn as cells
fn without_text(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<text") {
        // Not `<textPath`, or anything else that starts alike
        if !rest[start + 5..].starts_with([' ', '>', '/', '\n', '\t']) {
            out.push_str(&rest[..start + 5]);
            rest = &rest[start + 5..];
            continue;
        }
        out.push_str(&rest[..start]);
        let tag_end = rest[start..].find('>').map_or(rest.len(), |i| start + i + 1);
        let end = if rest[..tag_end].ends_with("/>") {
            tag_end
        } else {
            rest[tag_end..].find("</text>").map_or(rest.len(), |i| tag_end + i + "</text>".len())
        };
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn draw(source: &str, palette: &Palette) -> Result<Arc<Diagram>, String> {
    use merman::svg::{
        CssOverridePolicy, DeterministicTextMeasurer, MeasurementProfileId, Presentation, SvgOutputPolicy,
        SvgPipelinePreset, SvgRenderOptions, TextMeasurementPolicy, TextMeasurementProfile,
        TextMeasurementProfileIdentity,
    };
    use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgEnvironment, SvgRequest};

    let fonts = fonts();
    let resolved = Presentation::new().with_theme(theme(palette, &fonts.family)?).resolve();
    // Labels wrap at 200 pixels, as in Mermaid 10, not merman's 120
    let config = merman::MermaidConfig::from_value(serde_json::json!({"flowchart": {"wrappingWidth": 200}}));
    let renderer =
        Renderer::new().with_engine(resolved.materialize_engine(merman::Engine::new()).with_site_config(config));
    let measurer = DeterministicTextMeasurer::default().with_width_callback(|text, style| {
        let bold = style
            .font_weight
            .as_deref()
            .is_some_and(|w| w == "bold" || w == "bolder" || w.parse::<u16>().is_ok_and(|w| w >= 600));
        let italic = style.font_style.as_deref().is_some_and(|s| s == "italic" || s == "oblique");
        measure(text, style.font_size as f32, bold, italic)
    });
    let identity = MeasurementProfileId::new("cosmic-term.cosmic-text")
        .and_then(|id| TextMeasurementProfileIdentity::new(id, "1"))
        .map_err(|err| err.to_string())?;
    let environment = SvgEnvironment::deterministic()
        .with_text_measurement_policy(TextMeasurementPolicy::uniform(TextMeasurementProfile::new(identity, measurer)));
    let output = SvgOutputPolicy {
        preset: SvgPipelinePreset::ResvgSafe,
        css_override_policy: CssOverridePolicy::StripExistingImportant,
        root_background_color: Some(hex(palette.page)),
        drop_native_duplicate_fallbacks: true,
        ..SvgOutputPolicy::default()
    };
    let request = SvgRequest {
        environment,
        pipeline: Some(output.pipeline()),
        presentation: resolved.render_policy(),
        options: SvgRenderOptions { diagram_id: Some(format!("mermaid-{:x}", source_key(source))), ..Default::default() },
        ..Default::default()
    };
    let svg = match renderer.render(RenderRequest::svg(source, OperationControl::new(), request)) {
        Ok(RenderOutput::Svg(Some(svg))) => recolour(svg.svg(), palette),
        Ok(_) => return Err("this is not a diagram merman knows".into()),
        Err(err) => return Err(err.to_string()),
    };
    let options = usvg::Options {
        fontdb: fonts.db.clone(),
        font_family: fonts.family.clone(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(&svg, &options).map_err(|err| format!("the SVG does not parse: {err}"))?;
    let mut labels = Vec::new();
    collect_labels(tree.root(), &tree, palette.text, &mut labels);
    let size = tree.size();
    Ok(Arc::new(Diagram {
        handle: svg::Handle::from_memory(without_text(&svg).into_bytes()),
        full: svg::Handle::from_memory(svg.clone().into_bytes()),
        svg: svg.into(),
        width: size.width(),
        height: size.height(),
        labels,
        family: fonts.family.clone(),
    }))
}

/// Each line of the tree's text, where usvg lays it out
fn collect_labels(group: &usvg::Group, tree: &usvg::Tree, ink: Color, labels: &mut Vec<Label>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_labels(group, tree, ink, labels),
            usvg::Node::Text(text) => text_labels(text, tree, ink, labels),
            _ => {}
        }
    }
}

fn text_labels(text: &usvg::Text, tree: &usvg::Tree, ink: Color, labels: &mut Vec<Label>) {
    let abs = text.abs_transform();
    let scale = (abs.sx * abs.sx + abs.ky * abs.ky).sqrt();
    let mut lines: Vec<Label> = Vec::new();
    for span in text.layouted() {
        if !span.visible {
            continue;
        }
        let color = match span.fill.as_ref().map(|fill| (fill.paint(), fill.opacity().get())) {
            Some((usvg::Paint::Color(c), opacity)) => Color::from_rgba8(c.red, c.green, c.blue, opacity),
            _ => ink,
        };
        for glyph in &span.positioned_glyphs {
            let ts = glyph.transform();
            let (x, y) = (ts.tx, ts.ty);
            let (x, y) = (abs.sx * x + abs.kx * y + abs.tx, abs.ky * x + abs.sy * y + abs.ty);
            let size = span.font_size.get() * scale;
            let new_line = lines.last().is_none_or(|line| (line.baseline - y).abs() > size * 0.5);
            if new_line {
                let face = tree.fontdb().face(glyph.font);
                lines.push(Label {
                    text: String::new(),
                    size,
                    bold: face.is_some_and(|f| f.weight.0 >= 600),
                    italic: face.is_some_and(|f| f.style != fontdb::Style::Normal),
                    color,
                    x,
                    baseline: y,
                });
            }
            let line = lines.last_mut().unwrap();
            line.x = line.x.min(x);
            line.text.push_str(&glyph.text);
        }
    }
    // The chunks' own text, when there is a line for each, as glyphs that
    // share a cluster each carry its text
    let chunks: Vec<&str> = text.chunks().iter().map(|chunk| chunk.text().trim()).filter(|t| !t.is_empty()).collect();
    if chunks.len() == lines.len() {
        for (line, chunk) in lines.iter_mut().zip(chunks) {
            line.text = chunk.to_string();
        }
    }
    labels.extend(lines.into_iter().filter(|line| !line.text.trim().is_empty()).map(|mut line| {
        line.text = line.text.trim().to_string();
        line
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn dark() -> Palette {
        Palette {
            page: Color::from_rgb8(0x1b, 0x1b, 0x1b),
            surface: Color::from_rgb8(0x2a, 0x2e, 0x33),
            text: Color::from_rgb8(0xe0, 0xe0, 0xe0),
            muted: Color::from_rgb8(0x9a, 0xa0, 0xa6),
            rule: Color::from_rgb8(0x4a, 0x50, 0x56),
            accent: Color::from_rgb8(0x63, 0xd0, 0xdf),
            error: Color::from_rgb8(0xff, 0x6b, 0x6b),
        }
    }

    const FLOWCHART: &str = "flowchart TD\n    A[Parse markdown] --> B{Mermaid fence?}\n    B -->|yes| C[Draw the diagram]\n    B -->|no| D[Code block]\n";

    #[test]
    fn draws_a_flowchart_with_its_labels() {
        let diagram = render(FLOWCHART, &dark()).expect("a diagram");
        assert!(diagram.width > 100.0 && diagram.height > 100.0, "{} × {}", diagram.width, diagram.height);
        let texts: Vec<&str> = diagram.labels.iter().map(|l| l.text.as_str()).collect();
        for expected in ["Parse markdown", "Mermaid fence?", "Draw the diagram", "Code block", "yes", "no"] {
            assert!(texts.contains(&expected), "{texts:?}");
        }
        for label in &diagram.labels {
            assert!(label.x >= 0.0 && label.x < diagram.width, "{label:?}");
            assert!(label.baseline > 0.0 && label.baseline <= diagram.height, "{label:?}");
            assert!(label.size > 8.0, "{label:?}");
        }
        // The thread's SVG leaves the text to the cells; the window's keeps it
        let svg::Data::Bytes(bytes) = diagram.handle.data() else { panic!() };
        let bare = std::str::from_utf8(bytes).unwrap();
        assert!(!bare.contains("<text") && diagram.svg.contains("<text"));
        assert!(diagram.svg.contains(&diagram.family));
        assert_eq!(labels_of(FLOWCHART).unwrap().len(), diagram.labels.len());
    }

    #[test]
    fn draws_architecture_in_the_palettes_colours() {
        let source = "architecture-beta\n    group api(cloud)[API]\n    service db(database)[Database] in api\n    service server(server)[Server] in api\n    db:L -- R:server\n";
        let diagram = render(source, &dark()).expect("a diagram");
        assert!(!diagram.svg.contains("#087ebf"), "the icons keep merman's blue");
        let texts: Vec<&str> = diagram.labels.iter().map(|l| l.text.as_str()).collect();
        assert!(texts.contains(&"Database") && texts.contains(&"Server") && texts.contains(&"API"), "{texts:?}");
    }

    #[test]
    fn draws_a_sequence_diagram() {
        let source = "sequenceDiagram\n    participant T as Terminal\n    participant C as Claude\n    T->>C: prompt\n    C-->>T: markdown\n";
        let diagram = render(source, &dark()).expect("a diagram");
        let texts: Vec<&str> = diagram.labels.iter().map(|l| l.text.as_str()).collect();
        assert!(texts.contains(&"Terminal") && texts.contains(&"prompt") && texts.contains(&"markdown"), "{texts:?}");
    }

    #[test]
    fn errors_are_reported_and_cached() {
        let bad = "flowchart TD\n  A --> \n  B[unclosed\n";
        let err = render(bad, &dark()).err().expect("an error");
        assert!(!err.is_empty());
        let (_, entries) = cache_usage();
        assert!(render(bad, &dark()).is_err());
        assert!(cache_usage().1 >= entries);
        assert!(render("not a diagram at all", &dark()).is_err());
    }

    #[test]
    fn the_cache_keeps_one_drawing_per_source_and_palette() {
        let source = "flowchart LR\n    X --> Y\n";
        let first = render(source, &dark()).unwrap();
        assert!(Arc::ptr_eq(&first, &render(source, &dark()).unwrap()));
        let light = Palette { page: Color::WHITE, text: Color::BLACK, ..dark() };
        let other = render(source, &light).unwrap();
        assert!(!Arc::ptr_eq(&first, &other));
        assert!(other.svg.contains("#ffffff"));
    }

    /// One of each kind of diagram, with their text, for a look
    #[test]
    #[ignore]
    fn dump_every_kind() {
        let Some(dir) = std::env::var_os("CT_SNAPSHOTS") else { return };
        let kinds = [
            ("flowchart", FLOWCHART),
            ("sequence", "sequenceDiagram\n    participant T as Terminal\n    participant C as Claude\n    T->>C: prompt\n    activate C\n    C-->>T: markdown\n    deactivate C\n    Note over T,C: drawn locally\n    loop Every turn\n        T->>C: more\n    end\n"),
            ("class", "classDiagram\n    class Block {\n        +String source\n        +render()\n    }\n    class Diagram\n    Block <|-- Diagram\n"),
            ("state", "stateDiagram-v2\n    [*] --> Streaming\n    Streaming --> Closed: fence ends\n    Closed --> [*]\n"),
            ("er", "erDiagram\n    THREAD ||--o{ ENTRY : holds\n    ENTRY {\n        string id\n        string text\n    }\n"),
            ("architecture", "architecture-beta\n    group api(cloud)[API]\n    service db(database)[Database] in api\n    service disk1(disk)[Storage] in api\n    service server(server)[Server] in api\n    service net(internet)[Internet]\n    db:L -- R:server\n    disk1:T -- B:server\n    net:R -- L:server\n"),
            ("c4", "C4Context\n    title System Context\n    Person(user, \"User\", \"Uses the terminal\")\n    System(term, \"Agent Terminal\", \"Renders threads\")\n    System_Ext(api, \"Claude API\", \"The model\")\n    Rel(user, term, \"Types prompts\")\n    Rel(term, api, \"Sends messages\", \"HTTPS\")\n"),
            ("mindmap", "mindmap\n  root((Threads))\n    Markdown\n      Math\n      Charts\n    Diagrams\n      Mermaid\n"),
            ("timeline", "timeline\n    title Releases\n    2024 : Terminal\n    2025 : Agents : Threads\n    2026 : Diagrams\n"),
            ("gantt", "gantt\n    title Plan\n    dateFormat YYYY-MM-DD\n    section Work\n    Parse :a1, 2026-01-01, 3d\n    Draw  :after a1, 4d\n"),
            ("pie", "pie title Time\n    \"Layout\" : 40\n    \"Drawing\" : 35\n    \"Text\" : 25\n"),
        ];
        let light = Palette {
            page: Color::WHITE,
            surface: Color::from_rgb8(0xf0, 0xf0, 0xf0),
            text: Color::from_rgb8(0x1a, 0x1a, 0x1a),
            muted: Color::from_rgb8(0x60, 0x60, 0x60),
            rule: Color::from_rgb8(0xc8, 0xc8, 0xc8),
            accent: Color::from_rgb8(0x00, 0x52, 0x9c),
            error: Color::from_rgb8(0xb0, 0x22, 0x22),
        };
        for (name, source) in kinds {
            for (tone, palette) in [("dark", dark()), ("light", light)] {
                match render(source, &palette) {
                    Ok(diagram) => std::fs::write(std::path::Path::new(&dir).join(format!("{name}-{tone}.svg")), &*diagram.svg).unwrap(),
                    Err(err) => eprintln!("{name}: {err}"),
                }
            }
        }
    }

    #[test]
    fn text_is_taken_out_whole() {
        let svg = r#"<svg><g><text x="1"><tspan>a</tspan></text><rect/><text/><textPath/></g></svg>"#;
        assert_eq!(without_text(svg), "<svg><g><rect/><textPath/></g></svg>");
    }

    #[test]
    fn hues_turn_and_keep_lightness() {
        let red = Color::from_rgb(1.0, 0.0, 0.0);
        let green = turned(red, 120.0).into_rgba8();
        assert_eq!(green, [0, 255, 0, 255]);
    }
}

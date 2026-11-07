// SPDX-License-Identifier: GPL-3.0-only

//! LaTeX set as KaTeX sets it, by RaTeX, a pure-Rust port of KaTeX, as SVG
//! whose glyphs are paths.
//!
//! RaTeX lays a formula out in well under a millisecond, so formulas are
//! set when the view lays them out, and kept in one cache for every thread,
//! bounded by [`CACHE_BYTES`]. What falls out is set again if it is seen
//! again; iced keeps the rasterised images only while they are drawn.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use cosmic::iced::advanced::svg;

/// The most SVG the cache holds, in bytes
const CACHE_BYTES: usize = 24 * 1024 * 1024;

/// A formula, set
#[derive(Debug)]
pub struct Formula {
    pub handle: svg::Handle,
    pub width: f32,
    pub height: f32,
    /// From the top to the baseline
    pub baseline: f32,
    /// What it was set from, so that it can be set again in another colour
    source: Source,
}

/// What a formula was set from
#[derive(Debug)]
struct Source {
    latex: String,
    display: bool,
    size: f32,
    color: [u8; 3],
}

impl Formula {
    /// The same formula, its ink in `color`: the selected formula's ink is
    /// recoloured as the text beside it is (household rule 6, formulas
    /// count as text). Set from the cache, as any formula is; itself if it
    /// is already in that colour or cannot be set again
    pub fn in_color(self: &Arc<Self>, color: [u8; 3]) -> Arc<Formula> {
        if self.source.color == color {
            return self.clone();
        }
        render(
            &self.source.latex,
            self.source.display,
            self.source.size,
            color,
        )
        .unwrap_or_else(|_| self.clone())
    }

    /// The colour its ink is set in
    pub fn color(&self) -> [u8; 3] {
        self.source.color
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    latex: String,
    display: bool,
    /// Pixels per em, in quarters
    size: u32,
    color: [u8; 3],
}

struct Cache {
    entries: HashMap<Key, (Result<Arc<Formula>, String>, u64, usize)>,
    bytes: usize,
    clock: u64,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| {
    Mutex::new(Cache {
        entries: HashMap::new(),
        bytes: 0,
        clock: 0,
    })
});

/// `latex` set at `size` pixels per em in `color`, or why it can't be
pub fn render(
    latex: &str,
    display: bool,
    size: f32,
    color: [u8; 3],
) -> Result<Arc<Formula>, String> {
    let key = Key {
        latex: latex.to_string(),
        display,
        size: (size * 4.0).round() as u32,
        color,
    };
    let mut cache = CACHE.lock().unwrap();
    cache.clock += 1;
    let clock = cache.clock;
    if let Some((result, used, _)) = cache.entries.get_mut(&key) {
        *used = clock;
        return result.clone();
    }
    drop(cache);
    let result = set(latex, display, size, color);
    let bytes = latex.len() + 64 + result.as_ref().map_or(0, |formula| svg_bytes(formula));
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

/// Whether `latex` can be set at all
pub fn parses(latex: &str) -> bool {
    render(latex, false, 16.0, [0, 0, 0]).is_ok()
}

/// The cache's size in bytes and entries, for the memory readout
pub fn cache_usage() -> (usize, usize) {
    let cache = CACHE.lock().unwrap();
    (cache.bytes, cache.entries.len())
}

fn svg_bytes(formula: &Formula) -> usize {
    match formula.handle.data() {
        svg::Data::Bytes(bytes) => bytes.len(),
        svg::Data::Path(_) => 0,
    }
}

/// LaTeX numbers `align` and its kind, but each formula here stands alone,
/// so every one would be (1); set them unnumbered, as their starred forms
fn unnumbered(latex: &str) -> String {
    let mut out = latex.to_string();
    for env in ["align", "equation", "gather", "multline", "eqnarray"] {
        out = out
            .replace(&format!("\\begin{{{env}}}"), &format!("\\begin{{{env}*}}"))
            .replace(&format!("\\end{{{env}}}"), &format!("\\end{{{env}*}}"));
    }
    // `\tag` lands on the wrong side in RaTeX, and numbers mean little here
    out
}

fn set(latex: &str, display: bool, size: f32, color: [u8; 3]) -> Result<Arc<Formula>, String> {
    use ratex_layout::{LayoutOptions, layout, to_display_list};
    use ratex_svg::{SvgOptions, render_to_svg};
    use ratex_types::{color::Color, math_style::MathStyle};

    let source = unnumbered(latex);
    let ast = ratex_parser::parser::parse(&source).map_err(|err| format!("{err:?}"))?;
    let style = if display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let [r, g, b] = color.map(|c| f32::from(c) / 255.0);
    let options = LayoutOptions::default()
        .with_style(style)
        .with_color(Color::rgb(r, g, b));
    // Layout can panic on input it doesn't expect; a formula is not worth a crash
    let list = std::panic::catch_unwind(|| to_display_list(&layout(&ast, &options)))
        .map_err(|_| "RaTeX could not lay this out".to_string())?;
    let size = f64::from(size);
    let pad = 1.0;
    let svg = render_to_svg(
        &list,
        &SvgOptions {
            font_size: size,
            padding: pad,
            embed_glyphs: true,
            ..Default::default()
        },
    )
    // RaTeX gives its size in points, which would scale it by 4/3
    .replacen("pt\" height=", "\" height=", 1)
    .replacen("pt\">", "\">", 1);
    Ok(Arc::new(Formula {
        handle: svg::Handle::from_memory(svg.into_bytes()),
        width: (list.width * size + 2.0 * pad) as f32,
        height: ((list.height + list.depth) * size + 2.0 * pad) as f32,
        baseline: (list.height * size + pad) as f32,
        source: Source {
            latex: latex.to_string(),
            display,
            size: size as f32,
            color,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_formulas_with_a_baseline_inside_them() {
        let formula = render(r"\int_0^1 x^2\,dx = \frac{1}{3}", true, 20.0, [0, 0, 0]).unwrap();
        assert!(formula.width > 50.0 && formula.height > 20.0);
        assert!(formula.baseline > 0.0 && formula.baseline < formula.height);
        let inline = render("x^2", false, 20.0, [0, 0, 0]).unwrap();
        assert!(inline.height < formula.height);
    }

    /// Set again in another colour, a formula keeps its size and baseline;
    /// in its own colour it is itself
    #[test]
    fn a_formula_in_another_colour_is_the_same_shape() {
        let black = render(r"\frac{a}{b} + x^2", true, 20.0, [0, 0, 0]).unwrap();
        assert!(Arc::ptr_eq(&black.in_color([0, 0, 0]), &black));
        let red = black.in_color([200, 0, 0]);
        assert_eq!(red.color(), [200, 0, 0]);
        assert_eq!(
            (red.width, red.height, red.baseline),
            (black.width, black.height, black.baseline)
        );
        assert_ne!(red.handle.id(), black.handle.id());
    }

    #[test]
    fn numbered_environments_are_set_unnumbered() {
        assert_eq!(
            unnumbered(r"\begin{align}a\end{align}"),
            r"\begin{align*}a\end{align*}"
        );
        assert!(
            render(
                r"\begin{align}a &= b \\ c &= d\end{align}",
                true,
                20.0,
                [0, 0, 0]
            )
            .is_ok()
        );
    }

    #[test]
    fn errors_are_reported_and_cached() {
        assert!(render(r"\frac{1}{", true, 20.0, [0, 0, 0]).is_err());
        assert!(render(r"\frac{1}{", true, 20.0, [0, 0, 0]).is_err());
    }
}

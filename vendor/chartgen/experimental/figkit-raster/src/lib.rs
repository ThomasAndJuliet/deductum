//! PNG output for figkit scenes. The scene is encoded by figkit's own SVG backend and
//! rasterised by resvg, so the pixels show exactly what the SVG does. Fonts are the caller's:
//! this crate reads no files, as figkit's computational crates read none.
//!
//! Experimental (see `experimental/README.md`): the options and the choice of returning a PNG
//! or a pixmap may still change.
use figkit_model::{ErrorKind, FigureError, FigureResult};
use figkit_render::{Backend, render};
use figkit_scene::PreparedScene;
pub use resvg::tiny_skia;
pub use resvg::usvg::fontdb;
use std::sync::Arc;

/// How to rasterise.
#[derive(Clone)]
pub struct RasterOptions {
    /// Pixels per point: 1 gives one pixel per figure unit; 2.4 turns a 1600 × 900 figure into
    /// a 3840 × 2160 wallpaper.
    pub scale: f64,
    /// Fonts for any text in the scene. None: text is left out (a figure without text needs
    /// none).
    pub fonts: Option<Arc<fontdb::Database>>,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self { scale: 1., fonts: None }
    }
}

fn failure(message: impl Into<String>) -> FigureError {
    FigureError::new(ErrorKind::ResourceFailure, message)
}

/// The scene as pixels (premultiplied RGBA), `scale` pixels per point.
pub fn render_pixmap(scene: &PreparedScene, options: &RasterOptions) -> FigureResult<tiny_skia::Pixmap> {
    if !(options.scale.is_finite() && options.scale > 0.) {
        return Err(FigureError::new(ErrorKind::InvalidInput, "raster scale must be positive"));
    }
    let svg = render(scene, Backend::Svg)?;
    let mut usvg_options = resvg::usvg::Options::default();
    if let Some(fonts) = &options.fonts {
        usvg_options.fontdb = fonts.clone();
    }
    let tree = resvg::usvg::Tree::from_str(&svg, &usvg_options).map_err(|e| failure(format!("SVG for rasterising: {e}")))?;
    // The SVG states its size in points; the pixel size is the figure's size × scale.
    let size = &scene.figure().size;
    let (w, h) = ((size.width * options.scale).round() as u32, (size.height * options.scale).round() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| failure("raster too large"))?;
    let (sx, sy) = (w as f32 / tree.size().width(), h as f32 / tree.size().height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(sx, sy), &mut pixmap.as_mut());
    Ok(pixmap)
}

/// The scene as PNG bytes, `scale` pixels per point.
pub fn render_png(scene: &PreparedScene, options: &RasterOptions) -> FigureResult<Vec<u8>> {
    render_pixmap(scene, options)?.encode_png().map_err(|e| failure(format!("PNG encoding: {e}")))
}

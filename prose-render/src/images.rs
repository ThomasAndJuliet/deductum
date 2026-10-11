// SPDX-License-Identifier: GPL-3.0-only

//! Image files: their size without decoding them, and thumbnails, so a page
//! decodes a small image and only the image's own window decodes it whole.

use std::path::{Path, PathBuf};

/// Whether the image at `path` is an SVG drawing, by its name
pub fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz"))
}

/// An image's size in pixels, reading no more of it than that needs: for a
/// drawing, its size in CSS pixels
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    if !is_svg(path) {
        return image::image_dimensions(path).ok();
    }
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let size = tree.size().to_int_size();
    Some((size.width(), size.height()))
}

/// Where a large image's thumbnail is kept: beside it
pub fn thumbnail_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    path.with_file_name(format!("{stem}.thumb.png"))
}

/// Images wider or taller than this get a thumbnail
pub const THUMBNAIL_SIZE: u32 = 960;

/// Write a thumbnail of a large image at [`thumbnail_path`]
pub fn make_thumbnail(path: &Path) {
    let Ok((w, h)) = image::image_dimensions(path) else { return };
    if w <= THUMBNAIL_SIZE && h <= THUMBNAIL_SIZE {
        return;
    }
    let result = image::open(path).and_then(|image| {
        image
            .thumbnail(THUMBNAIL_SIZE, THUMBNAIL_SIZE)
            .save_with_format(thumbnail_path(path), image::ImageFormat::Png)
    });
    if let Err(err) = result {
        log::warn!("no thumbnail for {}: {err}", path.display());
    }
}

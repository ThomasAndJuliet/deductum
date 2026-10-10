// SPDX-License-Identifier: GPL-3.0-only

//! Images in threads live in files, not in memory.
//!
//! The CLI sends images as base64 inside its JSON: screenshots it read,
//! images the user pasted. [`spill`] writes each to a file named by a hash
//! of its bytes, under `$XDG_CACHE_HOME/cosmic-term/thread-images`, and
//! leaves the path in the JSON. It runs where the line is parsed, off the UI
//! thread. The view then shows images from their files, and iced decodes
//! only those on screen and drops them when they leave it.

use std::hash::Hasher;
use std::path::PathBuf;

use serde_json::Value;

/// Files not used for this long are removed when the terminal starts
const KEEP_DAYS: u64 = 14;

/// Whether the image at `path` is an SVG drawing, by its name
pub fn is_svg(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz"))
}

/// An image's size in pixels, reading no more of it than that needs: for a
/// drawing, its size in CSS pixels
pub fn dimensions(path: &std::path::Path) -> Option<(u32, u32)> {
    if !is_svg(path) {
        return image::image_dimensions(path).ok();
    }
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let size = tree.size().to_int_size();
    Some((size.width(), size.height()))
}

pub fn cache_dir() -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".cache")
        });
    cache.join("cosmic-term/thread-images")
}

/// Replace every base64 image source in `value` with `{"path": …}`
pub fn spill(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let is_image = map.get("type").and_then(Value::as_str) == Some("image");
            if is_image
                && let Some(source) = map.get_mut("source")
                && source.get("type").and_then(Value::as_str) == Some("base64")
            {
                let media_type = source.get("media_type").and_then(Value::as_str).unwrap_or("image/png");
                let data = source.get("data").and_then(Value::as_str).unwrap_or_default();
                match write(media_type, data) {
                    Ok(path) => *source = serde_json::json!({"type": "file", "path": path}),
                    Err(err) => log::warn!("image not kept: {err}"),
                }
                return;
            }
            for value in map.values_mut() {
                spill(value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(spill),
        _ => {}
    }
}

fn write(media_type: &str, data: &str) -> Result<PathBuf, String> {
    let bytes = cosmic_term::b64::decode(data)?;
    let extension = match media_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    let mut hasher = std::hash::DefaultHasher::new();
    hasher.write(&bytes);
    let dir = cache_dir();
    let path = dir.join(format!("{:016x}-{}.{extension}", hasher.finish(), bytes.len()));
    if !path.exists() {
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let partial = path.with_extension("part");
        std::fs::write(&partial, &bytes).map_err(|err| err.to_string())?;
        std::fs::rename(&partial, &path).map_err(|err| err.to_string())?;
        make_thumbnail(&path);
    } else {
        // Touched, so that pruning keeps it
        let _ = std::fs::File::options().append(true).open(&path).and_then(|file| file.set_modified(std::time::SystemTime::now()));
    }
    Ok(path)
}

/// Where a large image's thumbnail is kept
pub fn thumbnail_path(path: &std::path::Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    path.with_file_name(format!("{stem}.thumb.png"))
}

/// Images wider or taller than this get a thumbnail for the thread
const THUMBNAIL_SIZE: u32 = 960;

/// Write a thumbnail of a large image, so that the thread decodes a small
/// one; only the image's own window decodes it whole
fn make_thumbnail(path: &std::path::Path) {
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

/// The base64 of an image file and its media type, to send to the agent
pub fn read_for_sending(path: &std::path::Path) -> Result<(String, String), String> {
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let media_type = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::Gif) => "image/gif",
        Ok(image::ImageFormat::WebP) => "image/webp",
        Ok(image::ImageFormat::Png) => "image/png",
        _ => return Err(format!("{}: not a PNG, JPEG, GIF or WebP image", path.display())),
    };
    Ok((media_type.to_string(), cosmic_term::b64::encode(&bytes)))
}

/// Remove images not used for [`KEEP_DAYS`]
pub fn prune() {
    let Ok(entries) = std::fs::read_dir(cache_dir()) else { return };
    let limit = std::time::Duration::from_secs(KEEP_DAYS * 24 * 3600);
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_move_to_files_and_their_data_leaves_memory() {
        let dir = std::env::temp_dir().join(format!("ct-images-{}", std::process::id()));
        // SAFETY: tests in this module only
        unsafe { std::env::set_var("XDG_CACHE_HOME", &dir) };
        // A 1×1 PNG
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let mut value = serde_json::json!({"content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}}]});
        spill(&mut value);
        let path = value.pointer("/content/0/source/path").and_then(Value::as_str).unwrap();
        assert!(std::path::Path::new(path).is_file());
        assert!(!value.to_string().contains(png));
        let (media_type, data) = read_for_sending(std::path::Path::new(path)).unwrap();
        assert_eq!((media_type.as_str(), data.as_str()), ("image/png", png));
        let _ = std::fs::remove_dir_all(dir);
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! Immutable images prepared away from a widget's layout/draw callbacks.
//!
//! The application schedules preparation and publishes a new snapshot when it
//! completes. Missing files stay missing until another snapshot is published;
//! this makes both successful and failed resource invalidation explicit.

use cosmic::iced::advanced::{image, svg};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub enum ImageHandle {
    Raster(image::Handle),
    Svg(svg::Handle),
}

#[derive(Clone)]
pub struct PreparedImage {
    pub width: u32,
    pub height: u32,
    pub handle: ImageHandle,
}

impl PreparedImage {
    /// Read a local image and its thumbnail. Call on a background worker.
    pub fn load(path: &Path) -> Option<Self> {
        let (width, height) = crate::images::dimensions(path)?;
        let handle = if crate::images::is_svg(path) {
            ImageHandle::Svg(svg::Handle::from_memory(std::fs::read(path).ok()?))
        } else {
            let thumbnail = crate::images::thumbnail_path(path);
            let data = std::fs::read(&thumbnail).or_else(|_| std::fs::read(path)).ok()?;
            ImageHandle::Raster(image::Handle::from_bytes(data))
        };
        Some(Self { width, height, handle })
    }
}

/// A resource snapshot. Every new snapshot has a distinct cache generation.
pub struct PreparedImages {
    generation: u64,
    images: HashMap<PathBuf, PreparedImage>,
}

impl PreparedImages {
    pub fn new(images: impl IntoIterator<Item = (PathBuf, PreparedImage)>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self { generation: NEXT.fetch_add(1, Ordering::Relaxed), images: images.into_iter().collect() }
    }

    pub fn get(&self, path: &Path) -> Option<&PreparedImage> {
        self.images.get(path)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Default for PreparedImages {
    fn default() -> Self {
        Self::new([])
    }
}

// SPDX-License-Identifier: GPL-3.0-only

use cosmic::widget::icon;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IconCacheKey {
    name: &'static str,
    size: u16,
}

pub struct IconCache {
    cache: HashMap<IconCacheKey, icon::Handle>,
    /// Source of the bundled icons, for recolouring
    svgs: HashMap<&'static str, &'static [u8]>,
    colored: HashMap<(&'static str, [u8; 3]), icon::Handle>,
}

impl IconCache {
    pub fn new() -> Self {
        let mut cache = HashMap::new();
        let mut svgs = HashMap::new();

        macro_rules! bundle {
            ($name:expr, $size:expr) => {
                let data: &'static [u8] = include_bytes!(concat!("../res/icons/", $name, ".svg"));
                svgs.insert($name, data);
                cache.insert(
                    IconCacheKey {
                        name: $name,
                        size: $size,
                    },
                    icon::from_svg_bytes(data).symbolic(true),
                );
            };
        }

        bundle!("dialog-error-symbolic", 16);
        bundle!("edit-clear-symbolic", 16);
        bundle!("edit-delete-symbolic", 16);
        bundle!("edit-undo-symbolic", 16);
        bundle!("list-add-symbolic", 16);
        bundle!("accessories-text-editor-symbolic", 16);
        bundle!("computer-symbolic", 16);
        bundle!("network-server-symbolic", 16);
        bundle!("object-select-symbolic", 16);
        bundle!("pan-down-symbolic", 16);
        bundle!("utilities-terminal-symbolic", 16);
        bundle!("go-down-symbolic", 16);
        bundle!("go-up-symbolic", 16);
        bundle!("sidebar-show-symbolic", 16);
        bundle!("worktree-new-symbolic", 16);
        bundle!("folder-symbolic", 16);
        bundle!("dialog-password-symbolic", 16);
        bundle!("view-compact-symbolic", 16);
        bundle!("view-expanded-symbolic", 16);
        bundle!("view-more-symbolic", 16);
        bundle!("window-close-symbolic", 16);
        // Threads
        bundle!("weather-clear-night-symbolic", 16);
        bundle!("weather-clear-symbolic", 16);
        bundle!("x-office-document-symbolic", 16);
        bundle!("media-playback-stop-symbolic", 16);
        bundle!("insert-image-symbolic", 16);
        bundle!("zoom-in-symbolic", 16);
        bundle!("zoom-out-symbolic", 16);
        bundle!("zoom-fit-best-symbolic", 16);
        bundle!("zoom-original-symbolic", 16);
        bundle!("edit-copy-symbolic", 16);
        bundle!("dialog-warning-symbolic", 16);
        bundle!("emblem-ok-symbolic", 16);
        bundle!("pan-end-symbolic", 16);
        bundle!("document-open-symbolic", 16);

        Self {
            cache,
            svgs,
            colored: HashMap::new(),
        }
    }

    /// A bundled symbolic icon drawn in `color` instead of the theme's
    /// foreground
    pub fn get_colored(&mut self, name: &'static str, color: [u8; 3], size: u16) -> icon::Icon {
        let svgs = &self.svgs;
        let handle = self
            .colored
            .entry((name, color))
            .or_insert_with(|| {
                let [r, g, b] = color;
                let svg = String::from_utf8_lossy(svgs.get(name).copied().unwrap_or_default())
                    // The Cosmic symbolic icons draw in this grey
                    .replace("#232323", &format!("#{r:02x}{g:02x}{b:02x}"));
                icon::from_svg_bytes(svg.into_bytes())
            })
            .clone();
        icon::icon(handle).size(size)
    }

    pub fn get(&mut self, name: &'static str, size: u16) -> icon::Icon {
        let handle = self
            .cache
            .entry(IconCacheKey { name, size })
            .or_insert_with(|| icon::from_name(name).size(size).handle())
            .clone();
        icon::icon(handle).size(size)
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! Thicken text, as the terminal and Cerea do it (this is the terminal's `text_weight.rs`, with
//! `has_weight` and `weight_in` added for the title's bold): one switch that draws normal text in its font's Semibold
//! (600) rather than Regular (400), in threads and in the terminal; bold
//! text stays Bold (700), and italic follows (semibold italic).
//!
//! Only where the font really has a semibold, in the family the text is
//! drawn in (a family that is not installed is drawn in another that
//! stands in for it). A font without one is left as it is: asked for a
//! weight it lacks, it would be drawn in its nearest face, which for 600
//! is its bold.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use cosmic_text::Family;
use cosmic_text::fontdb::Style;

/// Normal text, unthickened
pub const NORMAL: u16 = 400;
/// Normal text, thickened
pub const SEMIBOLD: u16 = 600;
/// Bold text, either way
pub const BOLD: u16 = 700;

/// Normal and bold text's weights in `family`: thickened if `on` and the
/// font has a semibold
pub fn weights(family: Family, on: bool) -> (u16, u16) {
    (if on && has_semibold(family) { SEMIBOLD } else { NORMAL }, BOLD)
}

/// Whether the font text in `family` is drawn in has a semibold, upright
/// and (if it has italics) italic. Kept once found: fonts are loaded at
/// start
pub fn has_semibold(family: Family) -> bool {
    has_weight(family, SEMIBOLD)
}

/// Whether that font has `weight` (as [`has_semibold`] for 600)
pub fn has_weight(family: Family, weight: u16) -> bool {
    static FOUND: LazyLock<Mutex<HashMap<(String, u16), bool>>> = LazyLock::new(Default::default);
    let key = (format!("{family:?}"), weight);
    if let Some(found) = FOUND.lock().unwrap().get(&key) {
        return *found;
    }
    let found = find_weight(family, weight);
    FOUND.lock().unwrap().insert(key, found);
    found
}

/// The weight to draw `family` in when `wanted` is asked for: `wanted` if the font has it, else
/// Regular, so that a missing bold or semibold never lets another face (or another family) stand
/// in for it
pub fn weight_in(family: Family, wanted: u16) -> u16 {
    if wanted == NORMAL || has_weight(family, wanted) { wanted } else { NORMAL }
}

fn find_weight(family: Family, wanted: u16) -> bool {
    let mut font_system = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
    let font_system = font_system.raw();
    let name = drawn_family(font_system, family);
    let faces: Vec<(cosmic_text::fontdb::ID, u16, bool)> = font_system
        .db()
        .faces()
        .filter(|face| face.families.iter().any(|(face_family, _)| face_family.eq_ignore_ascii_case(&name)))
        .map(|face| (face.id, face.weight.0, face.style != Style::Normal))
        .collect();
    // A face that is semibold, or a variable font whose weight axis
    // reaches it (cosmic-text sets the axis to the weight asked for)
    let mut semibold = |id, weight| {
        weight == wanted
            || font_system.get_font(id, cosmic_text::Weight(weight)).is_some_and(|font| {
                font.as_swash()
                    .variations()
                    .find(|variation| variation.tag() == u32::from_be_bytes(*b"wght"))
                    .is_some_and(|axis| (axis.min_value()..=axis.max_value()).contains(&f32::from(wanted)))
            })
    };
    let mut upright = false;
    let (mut italics, mut italic) = (false, false);
    for (id, weight, is_italic) in faces {
        if is_italic {
            italics = true;
            italic = italic || semibold(id, weight);
        } else {
            upright = upright || semibold(id, weight);
        }
    }
    upright && (italic || !italics)
}

/// The family text in `family` is drawn in: found by setting a letter in
/// it, as a family not installed is drawn in another
fn drawn_family(font_system: &mut cosmic_text::FontSystem, family: Family) -> String {
    let named = font_system.db().family_name(&family).to_string();
    let mut buffer = cosmic_text::Buffer::new(font_system, cosmic_text::Metrics::new(16.0, 16.0));
    buffer.set_text("a", &cosmic_text::Attrs::new().family(family), cosmic_text::Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);
    let font_id = buffer.layout_runs().next().and_then(|run| run.glyphs.first()).map(|glyph| glyph.font_id);
    font_id
        .and_then(|id| font_system.db().face(id))
        .and_then(|face| face.families.first())
        .map_or(named, |(name, _)| name.clone())
}

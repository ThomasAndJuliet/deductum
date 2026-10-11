// SPDX-License-Identifier: GPL-3.0-only

//! Agent threads: a conversation with Claude shown as rich text rather than
//! in a terminal. See AGENTS-PLAN.md, "Threads".

pub mod claude;
pub mod codex;
pub use prose_render::{diagram, markdown, math, rich};

pub mod images;
pub mod model;
pub mod protocol;
pub mod session;
pub mod tab;
pub mod view;
pub mod viewer;

/// The fonts a paper is set in: Typst's default, Libertinus Serif, and
/// LaTeX's, New Computer Modern; see `res/fonts/README`
pub const FONTS: [&[u8]; 8] = [
    include_bytes!("../../res/fonts/LibertinusSerif-Regular.otf"),
    include_bytes!("../../res/fonts/LibertinusSerif-Bold.otf"),
    include_bytes!("../../res/fonts/LibertinusSerif-Italic.otf"),
    include_bytes!("../../res/fonts/LibertinusSerif-BoldItalic.otf"),
    include_bytes!("../../res/fonts/NewCM10-Regular.otf"),
    include_bytes!("../../res/fonts/NewCM10-Bold.otf"),
    include_bytes!("../../res/fonts/NewCM10-Italic.otf"),
    include_bytes!("../../res/fonts/NewCM10-BoldItalic.otf"),
];
pub mod app;
#[cfg(test)]
pub mod snapshot;
#[cfg(debug_assertions)]
pub mod debug;

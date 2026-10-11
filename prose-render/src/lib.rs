// SPDX-License-Identifier: GPL-3.0-only

//! Markdown rendered for COSMIC: a parsed [`markdown::Doc`] laid out and
//! drawn by the selectable [`rich::RichText`] widget, with LaTeX
//! ([`math`]), Mermaid diagrams ([`diagram`]), charts ([`chart`]) and
//! images ([`images`]).
//!
//! Shared by Cerea, Isabellum and Terminal Agents' threads; it lives in
//! Cerea's `prose/` and is a git subtree in the others.

pub mod chart;
pub mod diagram;
pub mod images;
pub mod markdown;
pub mod math;
pub mod rich;

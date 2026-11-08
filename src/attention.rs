// SPDX-License-Identifier: GPL-3.0-only

//! What in the open theory needs attention: the cells that failed and those left unfinished (a
//! `sorry`), counted for the status chip and the status bar's links, and the next of each to go to
//! (Next failure F8, Next unfinished Shift+F8).

use super::editor::Theory;
use super::theory::{Kind, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Failed,
    Unfinished,
}

/// The cells that fail or are unfinished, by block index, in the document's order
pub fn problems(t: &Theory) -> Vec<(usize, Problem)> {
    t.blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.kind == Kind::Formal)
        .filter_map(|(i, b)| {
            if b.status == Status::Failed {
                Some((i, Problem::Failed))
            } else if b.commands.iter().any(|c| t.profile.unfinished().contains(&c.name.as_str())) {
                Some((i, Problem::Unfinished))
            } else {
                None
            }
        })
        .collect()
}

/// How many cells fail, and how many are unfinished
pub fn counts(t: &Theory) -> (usize, usize) {
    let all = problems(t);
    (all.iter().filter(|p| p.1 == Problem::Failed).count(), all.iter().filter(|p| p.1 == Problem::Unfinished).count())
}

/// The words for the counts, "1 failed · 1 unfinished", with a part left out when it is none
pub fn words(failed: usize, unfinished: usize) -> String {
    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if unfinished > 0 {
        parts.push(format!("{unfinished} unfinished"));
    }
    parts.join(" \u{b7} ")
}

/// The source offset of the next cell of this kind after the selected one (or the caret), going
/// round to the first
pub fn next(t: &Theory, kind: Problem) -> Option<usize> {
    let all: Vec<usize> = problems(t).into_iter().filter(|p| p.1 == kind).map(|p| p.0).collect();
    let here = t.selected.or_else(|| t.blocks.iter().position(|b| b.start <= t.cursor && t.cursor < b.end.max(b.start + 1)));
    let pick = match here {
        Some(h) => all.iter().find(|i| **i > h).or(all.first()),
        None => all.first(),
    };
    pick.and_then(|i| t.blocks.get(*i)).map(|b| b.start)
}

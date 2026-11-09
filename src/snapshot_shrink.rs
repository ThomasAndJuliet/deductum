// SPDX-License-Identifier: GPL-3.0-only

//! The goal beside its cell in a gap too short for it: it keeps its height when the gap holds it,
//! shrinks to the gap (its goals scrolling inside) when the gap holds at least its heading,
//! controls and the first subgoal's kept lines, and goes to the far side, past the wider
//! paragraphs, at its height when the gap is shorter than that. `CT_SHEET_THEORY` (a theory with
//! the sheet's lemmas) and `CT_SNAPSHOTS` keep the frames of the real theory.

use cosmic::iced::{Rectangle, Size};

use super::editor::Mode;
use super::snapshot::Harness;
use super::gap::GOAL_SIDE;
use super::snapshot_sheet::{Case, app_for};

/// A theory with the cells and paragraphs the sheet's cases use, of its own words
const THEORY: &str = "theory Gap imports Main begin

section \\<open>A section\\<close>

lemma raw_execution_stop_iff: \"True\" by simp

lemma raw_FE_stop:
  \"P x = Q x\"
proof (rule equalityI)
  show \"P x \\<subseteq> Q x\"
    unfolding P_def by (auto simp: Q_def)
next
  show \"Q x \\<subseteq> P x\"
  proof
    fix t :: \"'e tttrace\"
    assume \"t \\<in> Q x\"
    then obtain Xs where t: \"t \\<in> tockifications Xs\"
      by blast
    obtain a b :: \"('s, 'e) timed_vars\" where
      \"raw_execution timed_stop a b Xs\"
      using intermediate_witness[where Y = \"{}\"]
      by blast
    have \"a = a\" by simp
    have \"b = b\" by simp
    have \"c = c\" by simp
    have \"d = d\" by simp
    with t show \"t \\<in> P x\"
      unfolding P_def by blast
  qed
qed

lemma raw_FR_stop: \"True\" by simp

text \\<open>These are forward equations, not a closure construction. The refusal before a tock is an
  alternative that deterministic tockify leaves out, and the final refusal may add more.\\<close>

end
";

/// What a layout did: the gap, the goal's normal height, its least, the height it took, and where
/// the panel and the cell are drawn
#[derive(Debug, Clone, Copy)]
struct Fit {
    gap: f32,
    normal: f32,
    least: f32,
    taken: f32,
    panel: Rectangle,
    cell: Rectangle,
}

fn fit(text: &str, k: usize, n: usize, draw: Option<&str>) -> Fit {
    fit_in(&mut Harness::new(Size::new(1920.0, 900.0)), text, k, n, draw).0
}

/// [`fit`] in a harness that has drawn before (the goal's side is remembered between frames), and
/// whether the goal was on the far side
fn fit_in(h: &mut Harness, text: &str, k: usize, n: usize, draw: Option<&str>) -> (Fit, bool) {
    let app = app_for(text, Mode::Literate, 1920.0, false, Case::Trim(k, n), 76);
    cosmic::theme::set_active(crate::tones::light_type_for_tests());
    super::view::set_goal_beside(true);
    let _ = h.draw(&app, "shrink-warm");
    let _ = h.draw(&app, draw.unwrap_or("shrink"));
    let far = GOAL_SIDE.lock().unwrap().unwrap_or(false);
    let moved = super::gap::goal_moved();
    let mut all = super::snapshot_cells::Bounds(Vec::new());
    h.operate(&app, &mut all);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    let find = |id: cosmic::widget::Id| all.0.iter().find(|(x, _)| *x == id).map(|(_, r)| *r).expect("on the page");
    let panel = find(super::view::test_panel_id("panel"));
    let (gap, normal, least, taken) = super::gap::GOAL_FIT.lock().unwrap().expect("a goal laid out");
    (Fit { gap, normal, least, taken, panel: Rectangle { x: panel.x + moved.x, y: panel.y + moved.y, ..panel }, cell: find(super::view::selected_block_id()) }, far)
}

/// The three ways a goal meets its gap, and where it goes in each
#[test]
fn a_goal_fits_its_gap_shrinks_to_it_or_goes_to_the_far_side() {
    // fits: a short goal in the cell's own gap, unchanged and next to the cell
    let fits = fit(THEORY, 0, 1, None);
    assert!(fits.gap >= fits.normal, "{fits:?}");
    assert_eq!(fits.taken, fits.normal);
    assert!((fits.panel.x - (fits.cell.x + fits.cell.width + super::view::GOAL_GAP_PUB)).abs() < 1.0, "next to the cell: {fits:?}");
    // fewer lines in the cell, a longer goal: the gaps it goes through
    let mut shrunk = None;
    let mut far = None;
    let mut last = None;
    for k in 0..16 {
        let f = fit(THEORY, k, 3, None);
        if f.gap < f.normal && f.gap >= f.least && shrunk.is_none() {
            shrunk = Some((k, f));
        }
        if f.gap < f.least && far.is_none() {
            far = Some((k, f));
        }
        last = Some(f);
    }
    let (k_shrunk, s) = shrunk.expect("a gap the goal shrinks to");
    assert!((s.taken - s.gap).abs() < 0.5, "as tall as the gap: {s:?}");
    assert!((s.panel.height - s.gap).abs() < 1.5, "drawn as tall as the gap: {s:?}");
    assert!((s.panel.x - (s.cell.x + s.cell.width + super::view::GOAL_GAP_PUB)).abs() < 1.0, "still next to the cell: {s:?}");
    assert!(s.panel.y + 0.5 >= s.cell.y - 300.0 && s.panel.y + s.panel.height <= s.cell.y + s.cell.height + 300.0, "in its gap: {s:?}");
    let (k_far, f) = far.expect("a gap too short for its least");
    assert!(k_far > k_shrunk, "the gap shortens with the lines taken out of the cell");
    assert_eq!(f.taken, f.normal, "its own height: {f:?}");
    assert!(f.panel.x > f.cell.x + f.cell.width + 100.0, "past the wider paragraphs: {f:?}");
    println!("least height at the default text size: {:.1} px; first shrunk gap {:.1} (k {k_shrunk}), first far gap {:.1} (k {k_far}); last {:?}", s.least, s.gap, f.gap, last);
}

/// The least height is the heading and controls and 5 to 10 lines of the first subgoal
#[test]
fn the_kept_lines_of_the_first_subgoal_are_five_to_ten() {
    use super::view::{SUBGOAL_MAX_LINES, SUBGOAL_MIN_LINES, kept_lines};
    assert_eq!((SUBGOAL_MIN_LINES, SUBGOAL_MAX_LINES), (5, 10));
    assert_eq!([kept_lines(1), kept_lines(5), kept_lines(7), kept_lines(10), kept_lines(11), kept_lines(400)], [5, 5, 7, 10, 10, 10]);
}

/// Frames of the real theory for review (`CT_SHEET_THEORY`, `CT_SNAPSHOTS`): the gap just above the
/// least height, just below it, and the goal that fits; the numbers in `CT_SHEET_LABELS`
#[test]
fn goal_shrink_frames() {
    let Some(path) = std::env::var_os("CT_SHEET_THEORY") else { return };
    let text = std::fs::read_to_string(path).expect("the theory");
    let mut fits = Vec::new();
    for k in 0..24 {
        fits.push((k, fit(&text, k, 3, None)));
    }
    let above = fits.iter().filter(|(_, f)| f.gap < f.normal && f.gap >= f.least).min_by(|a, b| (a.1.gap - a.1.least).total_cmp(&(b.1.gap - b.1.least))).expect("a gap just above the least");
    let below = fits.iter().filter(|(_, f)| f.gap < f.least).max_by(|a, b| a.1.gap.total_cmp(&b.1.gap)).expect("a gap just below the least");
    let mut labels = String::new();
    for (name, (k, f), n) in [("shrunk-just-above", *above, 3), ("far-side-just-below", *below, 3)] {
        let again = fit(&text, k, n, Some(&format!("changes-{name}")));
        labels.push_str(&format!("{name}: k {k}: gap {:.1} px, least {:.1} px, normal {:.1} px, taken {:.1} px (cell {:.1} px tall)\n", f.gap, f.least, f.normal, again.taken, again.cell.height));
    }
    let short = fit(&text, 0, 1, Some("changes-fits"));
    labels.push_str(&format!("fits: gap {:.1} px, normal {:.1} px, least {:.1} px\n", short.gap, short.normal, short.least));
    if let Some(file) = std::env::var_os("CT_SHEET_LABELS") {
        std::fs::write(file, labels).expect("the labels");
    }
}

/// The goal on the far side stays there while the gap rises through the margin band, and returns
/// at its least plus the margin; next to the cell it stays while the gap falls through the band;
/// whatever the scroll
#[test]
fn the_side_a_goal_is_on_is_kept_through_the_margin_band() {
    let margin = super::view::GOAL_SIDE_MARGIN_LINES as f32 * super::view::code_line_height();
    // the first k (lines out of the cell) with a gap under the least
    let b = (0..16).find(|&k| { let f = fit(THEORY, k, 3, None); f.gap < f.least }).expect("a far gap");
    // falling: next to the cell until the gap is under the least
    let mut h = Harness::new(Size::new(1920.0, 900.0));
    for k in 0..b {
        let (f, far) = fit_in(&mut h, THEORY, k, 3, None);
        assert!(!far && f.gap >= f.least, "k {k}, falling: next to the cell with the gap {} against {}", f.gap, f.least);
    }
    let (f, far) = fit_in(&mut h, THEORY, b, 3, None);
    assert!(far && f.gap < f.least, "just below the least: the far side ({f:?})");
    // rising again: the far side through the band, back at the least plus the margin
    let mut back = None;
    for k in (0..b).rev() {
        let (f, far) = fit_in(&mut h, THEORY, k, 3, None);
        if f.gap < f.least + margin - 0.01 {
            assert!(far, "k {k}, rising: the gap {} is inside the band (least {} + {margin}) and the goal stays on the far side", f.gap, f.least);
        } else {
            assert!(!far, "k {k}, rising: the gap {} holds the least and the margin: back next to the cell", f.gap);
            back = Some(k);
            break;
        }
    }
    assert!(back.is_some(), "the gap rose through the band");
    drop(h);
    // a cell opened in the band is decided from the least alone: next to the cell
    let (f, far) = fit_in(&mut Harness::new(Size::new(1920.0, 900.0)), THEORY, b - 1, 3, None);
    assert!(!far && f.gap >= f.least && f.gap < f.least + margin, "a fresh cell in the band is next to it ({f:?})");
}

/// Scrolling the page changes nothing about the goal's height or side
#[test]
fn scrolling_does_not_change_the_goals_height_or_side() {
    let app = app_for(THEORY, Mode::Literate, 1920.0, false, Case::Trim(8, 3), 76);
    let mut h = Harness::new(Size::new(1920.0, 500.0));
    super::view::set_goal_beside(true);
    let _ = h.draw(&app, "scroll-warm");
    let _ = h.draw(&app, "scroll-0");
    let before = (*super::gap::GOAL_FIT.lock().unwrap(), *GOAL_SIDE.lock().unwrap());
    for by in [60.0, 140.0, 40.0] {
        h.move_to(&app, cosmic::iced::Point::new(300.0, 250.0));
        h.events(&app, &[cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::WheelScrolled { delta: cosmic::iced::mouse::ScrollDelta::Pixels { x: 0.0, y: -by } })]);
        let _ = h.draw(&app, "scroll-n");
        assert_eq!((*super::gap::GOAL_FIT.lock().unwrap(), *GOAL_SIDE.lock().unwrap()), before, "scrolled by {by}");
    }
}

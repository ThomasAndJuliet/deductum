// SPDX-License-Identifier: GPL-3.0-only

//! The goal beside its cell in a gap too short for it: it keeps its height when the gap holds it,
//! shrinks to the gap (its goals scrolling inside) when the gap holds at least its heading,
//! controls and the first subgoal's kept lines, and goes to the far side, past the wider
//! paragraphs, at its height when the gap is shorter than that. `CT_SHEET_THEORY` (a theory with
//! the sheet's lemmas) and `CT_SNAPSHOTS` keep the frames of the real theory.

use cosmic::iced::{Rectangle, Size};

use super::editor::Mode;
use super::snapshot::Harness;
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
    let app = app_for(text, Mode::Literate, 1920.0, false, Case::Trim(k, n), 76);
    let mut h = Harness::new(Size::new(1920.0, 900.0));
    cosmic::theme::set_active(crate::tones::light_type_for_tests());
    super::view::set_goal_beside(true);
    let _ = h.draw(&app, "shrink-warm");
    let _ = h.draw(&app, draw.unwrap_or("shrink"));
    let moved = super::gap::goal_moved();
    let mut all = super::snapshot_cells::Bounds(Vec::new());
    h.operate(&app, &mut all);
    cosmic::theme::set_active(cosmic::theme::ThemeType::Dark);
    let find = |id: cosmic::widget::Id| all.0.iter().find(|(x, _)| *x == id).map(|(_, r)| *r).expect("on the page");
    let panel = find(super::view::test_panel_id("panel"));
    let (gap, normal, least, taken) = super::gap::GOAL_FIT.lock().unwrap().expect("a goal laid out");
    Fit { gap, normal, least, taken, panel: Rectangle { x: panel.x + moved.x, y: panel.y + moved.y, ..panel }, cell: find(super::view::selected_block_id()) }
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

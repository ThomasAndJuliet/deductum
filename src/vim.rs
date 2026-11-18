// SPDX-License-Identifier: GPL-3.0-only

//! Vim keys for the editors. A small modal machine turns keys into the
//! editor's own actions (moves, selections, edits), so every change still goes
//! through the same path to the session host; `u` and Ctrl+R are Deductum's
//! undo and redo, which are the human's own (design C5).

use cosmic::widget::text_editor::{Action, Edit, Motion};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "VISUAL LINE",
        }
    }
}

/// What a key does to the editor and the app
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Act(Action),
    /// Copy the selection into the register (linewise if true)
    YankSelection(bool),
    /// Copy the current line(s) into the register
    YankLines(usize),
    Paste { after: bool },
    /// A new line below (true) or above, indented, in insert mode
    OpenLine(bool),
    Undo,
    Redo,
    Save,
    Escape,
    /// Go to a symbol of the document (`g s`, as Zed's vim mode has it; Ctrl+Shift+O)
    Symbols,
    /// Back to where a jump came from (Ctrl+O; Alt+Left)
    Back,
    /// The definition of the name at the caret (`g d`; Ctrl+click)
    Definition,
    /// What the prover says about the name at the caret, as the pointer shows it (`g h`)
    Hover,
    /// The caret's menu, as a right click there opens it (`g .`)
    Actions,
    /// The session's theorems searched for the name at the caret (`g S`)
    SearchProject,
    /// The next problem (true) or the one before (`] d`, `[ d`, `g ]`, `g [`)
    Problem(bool),
    /// Comment out these lines from the caret's, or take their comments away (`g c c`)
    ToggleComment(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Vim {
    pub mode: Mode,
    /// Keys of an unfinished command: a count, an operator, `g`, `r`, `:`
    pub pending: String,
    pub register: String,
    pub linewise: bool,
}

fn motion(key: &str) -> Option<Motion> {
    Some(match key {
        "h" => Motion::Left,
        "l" | " " => Motion::Right,
        "j" => Motion::Down,
        "k" => Motion::Up,
        "w" | "W" | "e" | "E" => Motion::WordRight,
        "b" | "B" => Motion::WordLeft,
        "0" | "^" => Motion::Home,
        "$" => Motion::End,
        "G" => Motion::DocumentEnd,
        _ => return None,
    })
}

impl Vim {
    /// The ops for a key typed outside insert mode (`ctrl` for Ctrl+key)
    pub fn key(&mut self, key: &str, ctrl: bool) -> Vec<Op> {
        use Op::*;
        if ctrl {
            self.pending.clear();
            return match key {
                "r" => vec![Redo],
                "d" => vec![Act(Action::Move(Motion::PageDown))],
                "u" => vec![Act(Action::Move(Motion::PageUp))],
                "s" => vec![Save],
                "o" => vec![Back],
                _ => Vec::new(),
            };
        }
        if key == "Escape" {
            self.pending.clear();
            let was_visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
            self.mode = Mode::Normal;
            return if was_visual { vec![Act(Action::ClearSelection)] } else { vec![Escape] };
        }
        // Ex commands
        if self.pending.starts_with(':') {
            if key == "Enter" {
                let cmd = std::mem::take(&mut self.pending);
                return match cmd.as_str() {
                    ":w" | ":write" | ":wq" | ":x" => vec![Save],
                    _ => Vec::new(),
                };
            }
            if key == "Backspace" {
                self.pending.pop();
            } else if key.chars().count() == 1 {
                self.pending.push_str(key);
            }
            return Vec::new();
        }
        // r<char>: replace the character under the caret
        if self.pending.ends_with('r') && !self.pending.ends_with("dr") && key.chars().count() == 1 {
            self.pending.clear();
            return vec![Act(Action::Select(Motion::Right)), Act(Action::Edit(Edit::Paste(std::sync::Arc::new(key.to_string())))), Act(Action::Move(Motion::Left))];
        }
        let digits: String = self.pending.chars().take_while(|c| c.is_ascii_digit()).collect();
        let count: usize = digits.parse().unwrap_or(1).max(1);
        let op_pending: String = self.pending.chars().skip_while(|c| c.is_ascii_digit()).collect();
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        // Counts
        if key.chars().all(|c| c.is_ascii_digit()) && !(key == "0" && self.pending.is_empty()) {
            self.pending.push_str(key);
            return Vec::new();
        }
        let repeat = |ops: Vec<Op>| -> Vec<Op> { (0..count).flat_map(|_| ops.clone()).collect() };
        // gg
        if op_pending.ends_with('g') && key == "g" {
            self.pending.clear();
            let select = visual || !op_pending.trim_end_matches('g').is_empty();
            return vec![Act(if select { Action::Select(Motion::DocumentStart) } else { Action::Move(Motion::DocumentStart) })];
        }
        // g and a key: what Zed's vim mode does with it (normal mode): gs the document's
        // symbols, gd the definition, gh the hover, g. the caret's menu, gS the theorems search,
        // g] and g[ the next and previous problem, gcc a comment
        if op_pending == "g" && !visual {
            let op = match key {
                "s" => Some(Symbols),
                "d" => Some(Definition),
                "h" => Some(Hover),
                "." => Some(Actions),
                "S" => Some(SearchProject),
                "]" => Some(Problem(true)),
                "[" => Some(Problem(false)),
                _ => None,
            };
            if let Some(op) = op {
                self.pending.clear();
                return vec![op];
            }
            if key == "c" {
                self.pending = format!("{digits}gc");
                return Vec::new();
            }
        }
        if op_pending == "gc" {
            self.pending.clear();
            return if key == "c" { vec![ToggleComment(count)] } else { Vec::new() };
        }
        // ]d and [d: the next and previous problem
        if op_pending == "]" || op_pending == "[" {
            self.pending.clear();
            return if key == "d" { vec![Problem(op_pending == "]")] } else { Vec::new() };
        }
        // Operators: d, c, y followed by a motion, the same key (whole lines), or iw/aw
        if let Some(op) = op_pending.chars().next().filter(|c| matches!(c, 'd' | 'c' | 'y')) {
            let rest = &op_pending[1..];
            if rest == "i" || rest == "a" {
                self.pending.clear();
                if key == "w" || key == "W" {
                    return self.apply_operator(op, vec![Act(Action::SelectWord)], false);
                }
                return Vec::new();
            }
            if key == "i" || key == "a" || key == "g" {
                self.pending.push_str(key);
                return Vec::new();
            }
            self.pending.clear();
            if key == op.to_string() {
                // dd, cc, yy: whole lines
                if op == 'y' {
                    return vec![YankLines(count)];
                }
                let mut sel = vec![Act(Action::Move(Motion::Home))];
                sel.extend((0..count).map(|_| Act(Action::Select(Motion::Down))));
                if op == 'c' {
                    // keep the line itself, cleared
                    sel = vec![Act(Action::Move(Motion::Home)), Act(Action::Select(Motion::End))];
                }
                return self.apply_operator(op, sel, op != 'c');
            }
            if let Some(m) = motion(key) {
                let sel: Vec<Op> = (0..count).map(|_| Act(Action::Select(m))).collect();
                let linewise = matches!(key, "j" | "k");
                let sel = if linewise {
                    let mut s = vec![Act(Action::Move(Motion::Home))];
                    s.extend((0..count + 1).map(|_| Act(Action::Select(m))));
                    s
                } else {
                    sel
                };
                return self.apply_operator(op, sel, linewise);
            }
            return Vec::new();
        }
        self.pending.clear();
        if visual {
            if let Some(m) = motion(key) {
                return repeat(vec![Act(Action::Select(m))]);
            }
            let linewise = self.mode == Mode::VisualLine;
            return match key {
                "d" | "x" => {
                    self.mode = Mode::Normal;
                    vec![YankSelection(linewise), Act(Action::Edit(Edit::Delete))]
                }
                "c" | "s" => {
                    self.mode = Mode::Insert;
                    vec![YankSelection(linewise), Act(Action::Edit(Edit::Delete))]
                }
                "y" => {
                    self.mode = Mode::Normal;
                    vec![YankSelection(linewise), Act(Action::ClearSelection)]
                }
                "g" => {
                    self.pending.push('g');
                    Vec::new()
                }
                "v" => {
                    self.mode = Mode::Normal;
                    vec![Act(Action::ClearSelection)]
                }
                _ => Vec::new(),
            };
        }
        if let Some(m) = motion(key) {
            return repeat(vec![Act(Action::Move(m))]);
        }
        match key {
            "i" => {
                self.mode = Mode::Insert;
                Vec::new()
            }
            "a" => {
                self.mode = Mode::Insert;
                vec![Act(Action::Move(Motion::Right))]
            }
            "I" => {
                self.mode = Mode::Insert;
                vec![Act(Action::Move(Motion::Home))]
            }
            "A" => {
                self.mode = Mode::Insert;
                vec![Act(Action::Move(Motion::End))]
            }
            "o" | "O" => {
                self.mode = Mode::Insert;
                vec![OpenLine(key == "o")]
            }
            "x" => {
                let mut ops: Vec<Op> = (0..count).map(|_| Act(Action::Select(Motion::Right))).collect();
                ops.push(YankSelection(false));
                ops.push(Act(Action::Edit(Edit::Delete)));
                ops
            }
            "X" => repeat(vec![Act(Action::Edit(Edit::Backspace))]),
            "D" => vec![Act(Action::Select(Motion::End)), YankSelection(false), Act(Action::Edit(Edit::Delete))],
            "C" => {
                self.mode = Mode::Insert;
                vec![Act(Action::Select(Motion::End)), YankSelection(false), Act(Action::Edit(Edit::Delete))]
            }
            "s" => {
                self.mode = Mode::Insert;
                vec![Act(Action::Select(Motion::Right)), Act(Action::Edit(Edit::Delete))]
            }
            "p" => repeat(vec![Paste { after: true }]),
            "P" => repeat(vec![Paste { after: false }]),
            "u" => repeat(vec![Undo]),
            // (the character under the cursor is selected from the start, as in vim)
            "v" => {
                self.mode = Mode::Visual;
                vec![Act(Action::Select(Motion::Right))]
            }
            "V" => {
                self.mode = Mode::VisualLine;
                vec![Act(Action::Move(Motion::Home)), Act(Action::Select(Motion::Down))]
            }
            "d" | "c" | "y" | "g" | "r" | ":" | "]" | "[" => {
                self.pending = format!("{digits}{key}");
                Vec::new()
            }
            "J" => vec![Act(Action::Move(Motion::End)), Act(Action::Edit(Edit::Delete))],
            _ => Vec::new(),
        }
    }

    fn apply_operator(&mut self, op: char, select: Vec<Op>, linewise: bool) -> Vec<Op> {
        let mut ops = select;
        ops.push(Op::YankSelection(linewise));
        match op {
            'y' => ops.push(Op::Act(Action::ClearSelection)),
            'c' => {
                ops.push(Op::Act(Action::Edit(Edit::Delete)));
                self.mode = Mode::Insert;
            }
            _ => ops.push(Op::Act(Action::Edit(Edit::Delete))),
        }
        ops
    }
}

/// Lines commented out, or their comments taken away if every line that is not blank has one
/// (as vim-commentary and Zed's `g c c`): the comment goes after the indent; blank lines stay
pub fn toggle_comments(lines: &[&str], (open, close): (&str, &str)) -> Vec<String> {
    let (bare_open, bare_close) = (open.trim_end(), close.trim_start());
    let commented = |l: &str| {
        let t = l.trim();
        t.starts_with(bare_open) && t.ends_with(bare_close) && t.len() >= bare_open.len() + bare_close.len()
    };
    let uncomment = lines.iter().any(|l| !l.trim().is_empty()) && lines.iter().filter(|l| !l.trim().is_empty()).all(|l| commented(l));
    lines
        .iter()
        .map(|l| {
            let body = l.trim_start();
            if body.is_empty() {
                return l.to_string();
            }
            let indent = &l[..l.len() - body.len()];
            let body = body.trim_end();
            if uncomment {
                let inner = body.strip_prefix(open).or_else(|| body.strip_prefix(bare_open)).unwrap_or(body);
                let inner = if bare_close.is_empty() { inner } else { inner.strip_suffix(close).or_else(|| inner.strip_suffix(bare_close)).unwrap_or(inner) };
                format!("{indent}{inner}")
            } else {
                format!("{indent}{open}{body}{close}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_counts_and_operators() {
        let mut v = Vim::default();
        assert_eq!(v.key("3", false), vec![]);
        assert_eq!(v.key("j", false).len(), 3);
        assert_eq!(v.key("i", false), vec![]);
        assert_eq!(v.mode, Mode::Insert);
        v.mode = Mode::Normal;
        assert_eq!(v.key("d", false), vec![]);
        let dw = v.key("w", false);
        assert!(dw.contains(&Op::Act(Action::Edit(Edit::Delete))));
        assert_eq!(v.key("u", false), vec![Op::Undo]);
        assert_eq!(v.key("r", true), vec![Op::Redo]);
        v.key("c", false);
        v.key("i", false);
        let ciw = v.key("w", false);
        assert!(ciw.contains(&Op::Act(Action::SelectWord)));
        assert_eq!(v.mode, Mode::Insert);
        v.mode = Mode::Normal;
        v.key(":", false);
        v.key("w", false);
        assert_eq!(v.key("Enter", false), vec![Op::Save]);
        assert_eq!(v.key("y", false), vec![]);
        assert_eq!(v.key("y", false), vec![Op::YankLines(1)]);
        v.key("g", false);
        assert_eq!(v.key("g", false), vec![Op::Act(Action::Move(Motion::DocumentStart))]);
        v.key("g", false);
        assert_eq!(v.key("s", false), vec![Op::Symbols]);
        assert_eq!(v.pending, "");
    }

    #[test]
    fn comments_go_on_after_the_indent_and_come_off_again() {
        let isabelle = ("(* ", " *)");
        assert_eq!(toggle_comments(&["  by simp"], isabelle), ["  (* by simp *)"]);
        assert_eq!(toggle_comments(&["  (* by simp *)"], isabelle), ["  by simp"]);
        assert_eq!(toggle_comments(&["(*by simp*)"], isabelle), ["by simp"]);
        let lean = ("-- ", "");
        assert_eq!(toggle_comments(&["  exact h", "", "  rfl"], lean), ["  -- exact h", "", "  -- rfl"]);
        assert_eq!(toggle_comments(&["  -- exact h", "", "  -- rfl"], lean), ["  exact h", "", "  rfl"]);
        // (some lines commented, some not: all are commented)
        assert_eq!(toggle_comments(&["-- a", "b"], lean), ["-- -- a", "-- b"]);
        assert_eq!(toggle_comments(&["   "], lean), ["   "]);
    }

    #[test]
    fn zed_s_g_keys_jumps_and_problems() {
        let mut v = Vim::default();
        v.mode = Mode::Normal;
        let mut keys = |ks: &[&str]| ks.iter().map(|k| v.key(k, false)).last().unwrap();
        assert_eq!(keys(&["g", "d"]), vec![Op::Definition]);
        assert_eq!(keys(&["g", "h"]), vec![Op::Hover]);
        assert_eq!(keys(&["g", "."]), vec![Op::Actions]);
        assert_eq!(keys(&["g", "S"]), vec![Op::SearchProject]);
        assert_eq!(keys(&["g", "]"]), vec![Op::Problem(true)]);
        assert_eq!(keys(&["g", "["]), vec![Op::Problem(false)]);
        assert_eq!(keys(&["]", "d"]), vec![Op::Problem(true)]);
        assert_eq!(keys(&["[", "d"]), vec![Op::Problem(false)]);
        assert_eq!(keys(&["g", "c", "c"]), vec![Op::ToggleComment(1)]);
        assert_eq!(keys(&["3", "g", "c", "c"]), vec![Op::ToggleComment(3)]);
        // (anything else after ] or gc does nothing, and is not left pending)
        assert_eq!(keys(&["]", "x"]), vec![]);
        assert_eq!(keys(&["g", "c", "x"]), vec![]);
        assert_eq!(v.pending, "");
        assert_eq!(v.key("o", true), vec![Op::Back]);
        // the old ones still work
        v.key("g", false);
        assert_eq!(v.key("g", false), vec![Op::Act(Action::Move(Motion::DocumentStart))]);
        v.key("d", false);
        assert_eq!(v.key("d", false).last(), Some(&Op::Act(Action::Edit(Edit::Delete))));
    }
}

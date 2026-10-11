// SPDX-License-Identifier: GPL-3.0-only

//! Isar as the editors know it: the goal taken apart for completions (`fix`,
//! `assume`, `show` from it), indentation, the statement the caret is in,
//! names to look up, comments and brackets the prover's colours leave out, and
//! what Isabelle's messages mean for a proof tool that is still running.

use crate::editor::{Completion, Goal, Sem};
use crate::theory::Command;

/// The statement around the caret, among a proof's commands: back to the nearest opener
/// (`have`, `show`, `case`, …) and on to the next opener or `qed`, or, if that is long, the
/// command at the caret. As source offsets; the goal goes level with or just after it
pub fn construct_at(text: &str, cmds: &[&Command], at: usize) -> Option<(usize, usize)> {
    const OPENERS: &[&str] = &["have", "show", "hence", "thus", "obtain", "case", "next", "consider"];
    let i = cmds.iter().position(|c| c.start <= at && at < c.end).or_else(|| cmds.iter().rposition(|c| c.start <= at))?;
    let s = (0..=i).rev().find(|&k| OPENERS.contains(&cmds[k].name.as_str())).unwrap_or(0);
    let e = (i + 1..cmds.len()).find(|&k| OPENERS.contains(&cmds[k].name.as_str()) || cmds[k].name == "qed").map_or(cmds.len() - 1, |k| k.saturating_sub(1).max(i));
    let (cs, ce) = (cmds[s].start, cmds[e].end);
    let lines = text.chars().skip(cs).take(ce.saturating_sub(cs)).filter(|c| *c == '\n').count();
    if lines > 12 { Some((cmds[i].start, cmds[i].end)) } else { Some((cs, ce)) }
}

/// Comments `(* … *)`, nested, as source spans
pub fn comments(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '(' && chars[i + 1] == '*' {
            let mut depth = 0;
            let s = i;
            while i + 1 < chars.len() {
                if chars[i] == '(' && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars[i + 1] == ')' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push((s, i));
        } else {
            i += 1;
        }
    }
    out
}

/// What fails while a proof waits on a tool typed in it (the tool leaves the proof unfinished,
/// so what closes it fails)
pub fn waits_on_tool(text: &str) -> bool {
    ["Illegal application of proof command", "Failed to finish proof", "Bad context for command"].iter().any(|e| text.contains(e))
}

/// An error or warning without the goal Isabelle lists after it (the goal view shows that) or
/// its position marker
pub fn message_summary(text: &str) -> String {
    let t = text.replace("\\<^here>", "").replace('\u{2302}', "");
    let cut = ["\ngoal (", "\ngoal:", "\nGoal ("].iter().filter_map(|g| t.find(g)).min().unwrap_or(t.len());
    t[..cut].trim_end().trim_end_matches(':').to_string()
}

/// Isar proof steps the palette offers, and the typed tool each needs (if any)
pub const PROOF_TEMPLATES: &[(&str, &str, &str, Option<&str>)] = &[
    ("have", "have \"", "\"\n  ", None),
    ("show ?thesis", "show ?thesis\n  ", "", None),
    ("proof … qed", "proof -\n  ", "\nqed", None),
    ("using", "using ", "", None),
    ("unfolding", "unfolding ", "", None),
    ("by simp", "by simp", "", None),
    ("by auto", "by auto", "", None),
    ("sledgehammer", "sledgehammer", "", Some("sledgehammer")),
    ("claude", "claude", "", None),
    ("nitpick", "nitpick", "", Some("nitpick")),
    ("quickcheck", "quickcheck", "", Some("quickcheck")),
];

/// Symbols the palette offers in maths or a term before anything is searched for
pub const COMMON_SYMBOLS: &[&str] = &[
    "∧", "∨", "¬", "⟶", "⟷", "∀", "∃", "⟹", "⇒", "→", "↦", "∈", "∉", "⊆", "⊂", "∪", "∩", "∅", "≤", "≥", "≠", "≡", "×", "∘", "λ", "α",
    "β", "γ", "δ", "σ", "τ", "φ", "ψ", "ω", "⦇", "⦈", "⟦", "⟧", "‹", "›", "⇩", "⇧",
];

/// The symbols the insert bar above the goal offers in a term, most wanted first: the logic,
/// then relations and sets, then the brackets (a pair wraps the selection)
pub const QUICK_SYMBOLS: &[(&str, &str, &str)] = &[
    ("∀", "∀", ""),
    ("∃", "∃", ""),
    ("⟶", "⟶", ""),
    ("⟷", "⟷", ""),
    ("∧", "∧", ""),
    ("∨", "∨", ""),
    ("¬", "¬", ""),
    ("⟹", "⟹", ""),
    ("≤", "≤", ""),
    ("≥", "≥", ""),
    ("≠", "≠", ""),
    ("∈", "∈", ""),
    ("‹›", "‹", "›"),
    ("λ", "λ", ""),
    ("⊆", "⊆", ""),
    ("∪", "∪", ""),
    ("∩", "∩", ""),
    ("×", "×", ""),
    ("∘", "∘", ""),
    ("⟦⟧", "⟦", "⟧"),
    ("⦇⦈", "⦇", "⦈"),
];

/// Whether the caret, after `before` (decoded) in a formal block, is inside a term: inside a
/// string (`"…"`) or a cartouche (`‹…›`, which nest). A string inside a cartouche, or a
/// cartouche inside a string, is still a term
pub fn in_term(before: &str) -> bool {
    let (mut quoted, mut depth) = (false, 0usize);
    for c in before.chars() {
        match c {
            '"' if depth == 0 => quoted = !quoted,
            '‹' if !quoted => depth += 1,
            '›' if !quoted => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    quoted || depth > 0
}

/// The first subgoal in a proof state's text (`goal (n subgoals):  1. …`)
pub fn parse_goal(state: &str) -> Option<Goal> {
    let mut lines = state.lines().skip_while(|l| !l.trim_start().starts_with("1. "));
    let first = lines.next()?.trim_start().strip_prefix("1. ")?.to_string();
    let mut text = first;
    for l in lines {
        let t = l.trim_start();
        if t.split_once(". ").is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit())) || t.is_empty() {
            break;
        }
        text.push(' ');
        text.push_str(t);
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut goal = Goal::default();
    let mut body = text.as_str();
    if let Some(rest) = body.strip_prefix('⋀')
        && let Some((vars, rest)) = rest.split_once(". ")
    {
        goal.vars = vars.split_whitespace().map(str::to_string).collect();
        body = rest;
    }
    if let Some(rest) = body.strip_prefix('⟦')
        && let Some((prems, concl)) = rest.split_once("⟧ ⟹ ")
    {
        goal.premises = prems.split("; ").map(|p| p.trim().to_string()).collect();
        goal.conclusion = concl.trim().to_string();
    } else if let Some((prem, concl)) = body.split_once(" ⟹ ") {
        goal.premises = vec![prem.trim().to_string()];
        goal.conclusion = concl.trim().to_string();
    } else {
        goal.conclusion = body.trim().to_string();
    }
    Some(goal)
}

/// Isar steps from the goal, for the word being typed
pub fn goal_completions(prefix: &str, replace: usize, goal: &Goal) -> Vec<Completion> {
    let quote = |t: &str| format!("\"{t}\"");
    let mut steps: Vec<(&str, String)> = Vec::new();
    if !goal.vars.is_empty() {
        steps.push(("fix", format!("fix {}", goal.vars.join(" "))));
    }
    if !goal.premises.is_empty() {
        steps.push(("assume", format!("assume {}", goal.premises.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" "))));
    }
    if !goal.conclusion.is_empty() {
        steps.push(("show", format!("show {}", quote(&goal.conclusion))));
        steps.push(("have", format!("have {}", quote(&goal.conclusion))));
        steps.push(("thus", format!("thus {}", quote(&goal.conclusion))));
    }
    let intro: Vec<&str> = [
        (goal.conclusion.starts_with('∀'), "allI"),
        (goal.conclusion.contains('⟶'), "impI"),
        (goal.conclusion.contains('∧'), "conjI"),
        (goal.conclusion.contains('⟷'), "iffI"),
    ]
    .into_iter()
    .filter(|(b, _)| *b)
    .map(|(_, r)| r)
    .collect();
    steps.push(("proof", "proof -".into()));
    if !intro.is_empty() {
        steps.push(("proof", format!("proof (intro {})", intro.join(" "))));
    }
    steps.push(("using", "using assms".into()));
    steps
        .into_iter()
        .filter(|(kw, _)| prefix.len() >= 2 && kw.starts_with(prefix))
        .map(|(_, text)| Completion { back: 0, replace, label: format!("{text}   (from the goal)"), text })
        .collect()
}

/// The indentation for a new line after `line`, the Isar way: inside a statement or an
/// opened proof one level deeper, after the end of a proof one level out
pub fn next_indent(line: &str) -> String {
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let trimmed = line.trim();
    let first = trimmed.split(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
    const OPEN: &[&str] = &[
        "lemma", "theorem", "corollary", "proposition", "schematic_goal", "have", "show", "hence", "thus",
        "obtain", "fun", "function", "primrec", "definition", "abbreviation", "inductive", "datatype",
        "record", "locale", "instance", "termination", "case", "next", "interpretation", "sublocale", "consider",
    ];
    const CLOSE: &[&str] = &["done", "qed", "sorry", "oops", "by"];
    let opens = OPEN.contains(&first) || trimmed == "proof" || trimmed.starts_with("proof ") || trimmed.ends_with(" proof") || trimmed.ends_with("{");
    if CLOSE.contains(&first) && !OPEN.contains(&first) {
        let n = indent.chars().count().saturating_sub(2);
        return " ".repeat(n);
    }
    if opens && !trimmed.contains(" by ") && !trimmed.ends_with("done") {
        return format!("{indent}  ");
    }
    indent
}

/// The name at a source offset (an identifier, long names with their dots, Greek letters and
/// sub- and superscripts in it), for looking up what it is
pub fn name_at(text: &str, at: usize) -> Option<String> {
    const LETTERS: &[&str] = &[
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "mu", "nu", "xi", "pi", "rho", "sigma",
        "tau", "upsilon", "phi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi", "Pi", "Sigma", "Upsilon", "Phi",
        "Psi", "Omega", "^sub", "^sup", "^isub", "^isup",
    ];
    let chars: Vec<char> = text.chars().collect();
    // The pieces of the line around it: plain characters, and `\<...>` symbols as one piece
    let mut line_start = at.min(chars.len());
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }
    let mut pieces: Vec<(usize, usize, bool)> = Vec::new();
    let mut i = line_start;
    while i < chars.len() && chars[i] != '\n' {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'<') {
            let close = (i + 2..chars.len()).find(|&k| chars[k] == '>' || chars[k] == '\n');
            if let Some(k) = close.filter(|&k| chars[k] == '>') {
                let name: String = chars[i + 2..k].iter().collect();
                pieces.push((i, k + 1, LETTERS.contains(&name.as_str())));
                i = k + 1;
                continue;
            }
        }
        let c = chars[i];
        pieces.push((i, i + 1, c.is_alphanumeric() || c == '_' || c == '\'' || c == '.'));
        i += 1;
    }
    let k = pieces.iter().position(|&(s, e, _)| s <= at && at < e).or_else(|| pieces.iter().rposition(|&(_, e, _)| e == at))?;
    if !pieces[k].2 {
        return None;
    }
    let (mut a, mut b) = (k, k);
    while a > 0 && pieces[a - 1].2 {
        a -= 1;
    }
    while b + 1 < pieces.len() && pieces[b + 1].2 {
        b += 1;
    }
    let name: String = chars[pieces[a].0..pieces[b].1].iter().collect();
    let name = name.trim_matches('.').to_string();
    (!name.is_empty() && !name.chars().all(|c| c.is_ascii_digit())).then_some(name)
}

/// The other half of a bracket the prover colours as part of a literal (Isabelle colours a
/// delimiter with a letter in it, as UTP's `)\<^sub>e` closing `(P \<or> Q)\<^sub>e`, but not
/// the plain `(` that opens it): the matching bracket, coloured alike, so that the pair reads as one
pub fn paired_brackets(text: &str, colours: &[(usize, usize, Sem)]) -> Vec<(usize, usize, Sem)> {
    let chars: Vec<char> = text.chars().collect();
    let pair = |c: char| match c {
        ')' => Some(('(', false)),
        ']' => Some(('[', false)),
        '}' => Some(('{', false)),
        '(' => Some((')', true)),
        '[' => Some((']', true)),
        '{' => Some(('}', true)),
        _ => None,
    };
    let mut out = Vec::new();
    for &(s, e, sem) in colours {
        if e <= s + 1 || e > chars.len() {
            continue;
        }
        // A closing bracket at its start (matched backwards), or an opening one at its end
        let (at, close) = match (pair(chars[s]), pair(chars[e - 1])) {
            (Some((_, false)), _) => (s, chars[s]),
            (_, Some((_, true))) => (e - 1, chars[e - 1]),
            _ => continue,
        };
        let Some((other, forward)) = pair(close) else { continue };
        let mut depth = 0usize;
        let mut k = at;
        let found = loop {
            k = match forward {
                true if k + 1 < chars.len() && k + 1 < at + 4000 => k + 1,
                false if k > 0 && k + 4000 > at => k - 1,
                _ => break None,
            };
            if chars[k] == close {
                depth += 1;
            } else if chars[k] == other {
                if depth == 0 {
                    break Some(k);
                }
                depth -= 1;
            }
        };
        // (unless the prover colours it already)
        if let Some(k) = found.filter(|&k| !colours.iter().any(|&(cs, ce, _)| cs <= k && k < ce)) {
            out.push((k, k + 1, sem));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_steps_from_the_goal() {
        let state = "proof (prove)\ngoal (1 subgoal):\n 1. ⋀x y. ⟦P x; Q y⟧\n    ⟹ R x y\n";
        let g = parse_goal(state).unwrap();
        assert_eq!(g.vars, vec!["x", "y"]);
        assert_eq!(g.premises, vec!["P x", "Q y"]);
        assert_eq!(g.conclusion, "R x y");
        let c = goal_completions("sh", 2, &g);
        assert_eq!(c[0].text, "show \"R x y\"");
        let f = goal_completions("fi", 2, &g);
        assert_eq!(f[0].text, "fix x y");
    }

    #[test]
    fn indents_the_isar_way() {
        assert_eq!(next_indent("lemma foo: \"x = x\""), "  ");
        assert_eq!(next_indent("  apply simp"), "  ");
        assert_eq!(next_indent("  done"), "");
        assert_eq!(next_indent("  proof (induct xs)"), "    ");
        assert_eq!(next_indent("    show ?case by simp"), "    ");
        assert_eq!(next_indent("    show \"P x\""), "      ");
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn names_are_found_with_their_dots_and_greek_letters() {
        let text = "lemma foo: \"List.append xs ys = rev \\<alpha>\\<^sub>1 \\<Rightarrow> x\"";
        let at = |s: &str| text.find(s).unwrap() + 2;
        assert_eq!(name_at(text, at("append")).as_deref(), Some("List.append"));
        assert_eq!(name_at(text, at("rev")).as_deref(), Some("rev"));
        assert_eq!(name_at(text, at("alpha")).as_deref(), Some("\\<alpha>\\<^sub>1"));
        assert_eq!(name_at(text, at("Rightarrow")), None);
        assert_eq!(name_at(text, text.find(" = ").unwrap() + 1), None);
    }
}

#[cfg(test)]
mod bracket_tests {
    use super::*;

    #[test]
    fn an_expression_bracket_is_coloured_at_both_ends() {
        let text = "raw_execution (P \\<or> (Q)) (R)\\<^sub>e a b";
        let close = text.find(")\\<^sub>e").unwrap();
        let sem = Sem::parse("keyword1").unwrap();
        let open = text.find("(R)").unwrap();
        let close_c = text[..close].chars().count();
        let extra = paired_brackets(text, &[(close_c, close_c + 9, sem)]);
        assert_eq!(extra, vec![(text[..open].chars().count(), text[..open].chars().count() + 1, sem)]);
        // nested: the outer one
        let text = "f ((a) + b)\\<^sub>e";
        let c = text.find(")\\<^sub>e").unwrap();
        assert_eq!(paired_brackets(text, &[(c, c + 9, sem)]), vec![(2, 3, sem)]);
    }
}

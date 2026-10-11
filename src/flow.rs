// SPDX-License-Identifier: GPL-3.0-only

//! The flow of a proof tool typed in a proof, from typing it to an inserted proof.
//!
//! The human types `sledgehammer`, `nitpick`, `exact?` or a `claude` line where a proof is
//! wanted and keeps their hands on the keyboard: the panel beside (or under) the goal says what
//! the tool is doing, then offers what it found. This module reads what the prover said into
//! that: a [`Report`] with an [`Outcome`] said plainly and a list of [`Offer`]s, ranked so that
//! the first is the one to take. The view draws it ([`super::view`]) and the keyboard acts on it:
//! Alt+Enter takes the highlighted offer (the first, unless Alt+Up or Alt+Down picked another),
//! and Esc stops a tool that is running.
//!
//! Isabelle's tools print a line for every step of their search (`e found a proof...`,
//! `cvc5: Duplicate proof`, `Sledgehammering...`). These are left out: the proofs they lead to
//! are listed with who found them and how long their check took, and the lines in between say
//! nothing the human acts on. A counterexample is read into its variables and values, so that it
//! can be shown as a table. A tool that found nothing says so in a sentence and offers what to
//! do next: more time for a search that may need it, Sledgehammer after a quicker tool, the
//! tactic a Lean error suggests.

use serde_json::Value;

use super::theory::Status;
use super::{Message, Workspace};

/// What a tool's run has come to, as the panel says it
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Not running yet: the prover has yet to reach the line (or to see what was typed)
    Waiting(String),
    /// Running, with nothing found so far; what it is doing, if it says
    Running(String),
    /// It found proofs (they are the offers); it may still be running
    Proofs,
    /// A counterexample: the tool's headline, then its sections of variables and values
    Counterexample { headline: String, sections: Vec<Section> },
    /// A counterexample search that found none (which proves nothing)
    NoCounterexample(String),
    /// A search for proofs that found none, or ran out of time
    NothingFound(String),
    /// The tool failed (a Lean tactic's error)
    Failed(String),
    /// Stopped by the human (Isabelle reads it as its time running out)
    Stopped,
    /// Done, with nothing to offer (a Lean `decide` that closed its goal)
    Done(String),
}

/// One part of a counterexample: "Free variables", "Evaluated terms", each a name and a value
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// What an offer does when taken
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Insert a proof the tool found, in place of the tool
    Insert(String),
    /// Insert an LLM's proof in place of its hole
    Fill(String),
    /// Insert the LLM's facts with Sledgehammer after them
    Facts,
    /// Put this in place of the tool's text, which runs it (more time, another tool)
    Replace(String),
    /// Put this keyword line in place of the tool and ask it at once (`claude`)
    Ask(String),
}

/// Something the panel offers to take, with Insert (or Run, or Ask)
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    /// What is inserted or run, as source text
    pub code: String,
    /// Who found it and how long it took to check ("e · 5 ms"), or why it is offered
    pub detail: String,
    /// Whether the prover checked it: a proof that worked, one that failed, or not known
    pub ok: Option<bool>,
    pub act: Act,
}

impl Offer {
    /// The button's word
    pub fn button(&self) -> &'static str {
        match (&self.act, self.ok) {
            (Act::Insert(_) | Act::Fill(_), Some(false)) => "Insert anyway",
            (Act::Insert(_) | Act::Fill(_) | Act::Facts, _) => "Insert",
            (Act::Replace(_), _) => "Run",
            (Act::Ask(_), _) => "Ask",
        }
    }

    /// What Alt+Enter does with it, for the hint
    pub fn verb(&self) -> &'static str {
        match self.act {
            Act::Insert(_) | Act::Fill(_) | Act::Facts => "inserts",
            Act::Replace(_) => "runs",
            Act::Ask(_) => "asks",
        }
    }

    pub fn message(&self) -> Message {
        match &self.act {
            Act::Insert(p) => Message::InsertOutput(p.clone()),
            Act::Fill(p) => Message::InsertFill(p.clone()),
            Act::Facts => Message::InsertUsing,
            Act::Replace(t) => Message::ReplaceTool(t.clone()),
            Act::Ask(k) => Message::AskInstead(k.clone()),
        }
    }
}

/// A typed tool's run at the caret, as the panel shows it
#[derive(Debug, Clone)]
pub struct Report {
    /// The tool as typed (`sledgehammer`), and the whole of its text (`sledgehammer [timeout = 60]`)
    pub tool: String,
    pub running: bool,
    /// Whether Stop (and Esc) can stop it on its own
    pub stoppable: bool,
    pub outcome: Outcome,
    pub offers: Vec<Offer>,
    /// What else it said, to show as it is (a warning, a remark)
    pub notes: Vec<Value>,
}

/// Lines of Isabelle's tools that report their progress, not a result
fn chatter(text: &str) -> bool {
    let t = text.trim();
    t.ends_with("found a proof...")
        || t.ends_with(": Duplicate proof")
        || t.starts_with("Sledgehammering")
        || t.starts_with("Nitpicking")
        || t.starts_with("Testing conjecture")
        || t.starts_with("Trying \"")
        || t == "Done"
}

/// Who found a proof and how long its check took, from the line offering it:
/// `e: Try this: using add.commute by blast (5 ms)` is e, 5 ms; `Found proof: by simp (1 ms)` is
/// 1 ms; `Try0 found a proof: by simp (2 ms)` is try0, 2 ms. False when the check failed or timed
/// out (`(> 1 s, timed out)`, `(failed)`)
pub fn credit(text: &str) -> (Option<String>, Option<String>, bool) {
    let first = text.lines().next().unwrap_or("");
    let who = if let Some((p, _)) = first.split_once(": Try this:") {
        Some(p.trim().to_string())
    } else if let Some((p, _)) = first.split_once(" found a proof:") {
        Some(p.trim().to_lowercase())
    } else {
        None
    }
    .filter(|p| !p.is_empty() && !p.contains(' '));
    let paren = first.trim_end().strip_suffix(')').and_then(|s| s.rsplit_once('(')).map(|(_, t)| t.trim().to_string());
    let works = !paren.as_deref().is_some_and(|p| p.contains("timed out") || p.contains("failed"));
    let time = paren.filter(|p| p.ends_with('s') && p.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '>' || c == '<')).map(|p| p.trim_end_matches(", timed out").to_string());
    (who, time, works)
}

/// The proofs offered by the tool's lines, each once, with everyone who found it: a summary's
/// "Try this" (try0's choice, Lean's suggestion) first, then in the order found, and those whose
/// check failed last
pub fn proofs(messages: &[Value], tactic: &str) -> Vec<Offer> {
    struct Found {
        code: String,
        who: Vec<String>,
        time: Option<String>,
        works: bool,
        choice: bool,
        order: usize,
    }
    let mut found: Vec<Found> = Vec::new();
    for m in messages {
        let text = m["text"].as_str().unwrap_or("");
        let (who, time, works) = credit(text);
        // (a bare "Try this" is a summary's pick: try0's, among the proofs it lists, or Lean's)
        let choice = text.trim_start().starts_with("Try this:");
        for sb in m["sendbacks"].as_array().into_iter().flatten().filter_map(|s| s.as_str()) {
            match found.iter_mut().find(|f| f.code == sb) {
                Some(f) => {
                    f.who.extend(who.clone().filter(|w| !f.who.contains(w)));
                    f.time = f.time.take().or(time.clone());
                    f.choice |= choice;
                    f.works |= works;
                }
                None => {
                    let order = found.len();
                    found.push(Found { code: sb.to_string(), who: who.clone().into_iter().collect(), time: time.clone(), works, choice, order });
                }
            }
        }
    }
    found.sort_by_key(|f| (!f.works, !f.choice, f.order));
    found
        .into_iter()
        .map(|f| {
            let mut parts = Vec::new();
            if !f.who.is_empty() {
                parts.push(f.who.join(", "));
            } else if !tactic.is_empty() && f.choice && f.time.is_none() {
                parts.push(tactic.to_string());
            }
            if let Some(t) = f.time {
                parts.push(t);
            }
            if !f.works {
                parts.push("its check failed".into());
            }
            Offer { detail: parts.join(" · "), ok: Some(f.works), act: Act::Insert(f.code.clone()), code: f.code }
        })
        .collect()
}

/// A counterexample read into its sections: `Nitpick found a counterexample:` and then
/// `Free variables:` with `xs = [4]` under it, or Quickcheck's values and then `Evaluated
/// terms:`. A value that runs over several lines is joined to its row. None when the text is not
/// a counterexample
pub fn counterexample(text: &str) -> Option<(String, Vec<Section>)> {
    let mut lines = text.lines();
    let headline = lines.next()?.trim();
    if !headline.contains("found a") || !headline.contains("counterexample") {
        return None;
    }
    let mut sections: Vec<Section> = Vec::new();
    for line in lines {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(title) = l.strip_suffix(':').filter(|t| !t.contains(" = ")) {
            sections.push(Section { title: title.to_string(), rows: Vec::new() });
            continue;
        }
        if sections.is_empty() {
            sections.push(Section { title: "Variables".into(), rows: Vec::new() });
        }
        let section = sections.last_mut().unwrap();
        match l.split_once(" = ") {
            Some((name, value)) if !name.is_empty() => section.rows.push((name.trim().to_string(), value.trim().to_string())),
            _ => match section.rows.last_mut() {
                Some((_, value)) => {
                    value.push(' ');
                    value.push_str(l);
                }
                None => section.rows.push((String::new(), l.to_string())),
            },
        }
    }
    sections.retain(|s| !s.rows.is_empty());
    Some((headline.trim_end_matches(':').to_string(), sections))
}

/// The facts `solve_direct` names (`Groups.ab_semigroup_add_class.add.commute: ?a + ?b = …`)
fn direct_facts(text: &str) -> Vec<String> {
    if !text.contains("can be solved directly with") {
        return Vec::new();
    }
    text.lines()
        .skip(1)
        .filter(|l| l.starts_with("  ") && !l.starts_with("    "))
        .filter_map(|l| l.trim().split_once(": ").map(|(n, _)| n.to_string()))
        .filter(|n| !n.is_empty() && !n.contains(' '))
        .collect()
}

/// The timeout typed with a tool (`sledgehammer [timeout = 4]`), if any
pub fn typed_timeout(text: &str) -> Option<u32> {
    let at = text.find("timeout")?;
    let rest = text[at + 7..].trim_start().strip_prefix('=')?.trim_start();
    rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

/// The tool's text again with more time: twice what it had, and at least 30 s more than the
/// default would give a search that stopped early
pub fn with_more_time(text: &str, current: u32) -> (String, u32) {
    let more = (current * 2).max(60);
    let name = text.split_whitespace().next().unwrap_or(text);
    // (other options typed with it are kept, the timeout replaced)
    let options: Vec<String> = text
        .find('[')
        .and_then(|b| text[b + 1..].split_once(']').map(|(inside, _)| inside))
        .map(|inside| inside.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty() && !o.starts_with("timeout")).collect())
        .unwrap_or_default();
    let mut all = options;
    all.push(format!("timeout = {more}"));
    (format!("{name} [{}]", all.join(", ")), more)
}

/// A tactic a Lean error suggests trying instead ("Try `apply?` to see partial suggestions")
fn suggested_tactic(text: &str) -> Option<String> {
    let at = text.find("Try `")?;
    let rest = &text[at + 5..];
    rest.split_once('`').map(|(t, _)| t.to_string()).filter(|t| !t.is_empty() && !t.contains('\n'))
}

/// The tool's run at the caret, if the caret is on a typed tool
pub fn report(ws: &Workspace) -> Option<Report> {
    let out = ws.output.as_ref()?;
    let cmd = &out["command"];
    let tool = ws.caps.typed_tool(cmd["name"].as_str()?)?;
    let t = ws.theory.as_ref()?;
    let name = tool.typed.clone();
    let status = t.caret_signature().2;
    let running = status == Status::Running;
    let messages: Vec<Value> = out["messages"].as_array().into_iter().flatten().filter(|m| m["kind"] != "state").cloned().collect();
    let stopped = !running && messages.iter().any(|m| m["kind"] == "error" && m["text"].as_str().is_some_and(|x| ws.profile.interrupted(x)));
    let mut report = Report { tool: name.clone(), running, stoppable: tool.stop, outcome: Outcome::Running(String::new()), offers: Vec::new(), notes: Vec::new() };
    // The tool's text replaced (a proof inserted for it): what it said is history
    let (cs, ce, _) = t.caret_command()?;
    if !t.source_text(cs, ce).trim_start().starts_with(name.as_str()) {
        return None;
    }
    // What was typed there has not been checked yet: what the prover said is about what was
    // there before (or the tool's last run, with other options)
    if t.unchecked_from().is_some_and(|from| from <= t.cursor) {
        report.outcome = Outcome::Waiting("Checking…".into());
        return Some(report);
    }
    if status == Status::Unprocessed {
        // (the note's "sledgehammer starts when …" is said by the section's heading)
        let note = super::view::waiting_note(ws).unwrap_or_default();
        let note = note.split_once(" starts when the prover reaches this line. ").map_or(note.as_str(), |(_, rest)| rest);
        let note = if note.is_empty() || note.starts_with("Waiting for the prover to reach this line") { "Starts when the prover reaches this line." } else { note };
        report.outcome = Outcome::Waiting(note.to_string());
        return Some(report);
    }
    if stopped {
        report.outcome = Outcome::Stopped;
        return Some(report);
    }
    // The text of the tool's command, as typed (with any options)
    let typed = {
        let (cs, ce, _, _) = t.caret_signature();
        let s = t.source_text(cs, ce);
        if s.trim().is_empty() { name.clone() } else { s.trim().to_string() }
    };
    report.offers = proofs(&messages, &name);
    let mut used = vec![false; messages.len()];
    for (i, m) in messages.iter().enumerate() {
        let text = m["text"].as_str().unwrap_or("");
        if chatter(text) || m["sendbacks"].as_array().is_some_and(|s| !s.is_empty()) {
            used[i] = true;
        }
    }
    let texts: Vec<&str> = messages.iter().map(|m| m["text"].as_str().unwrap_or("")).collect();
    let find = |want: &dyn Fn(&str) -> bool| texts.iter().position(|x| want(x));
    let more_time = |report: &mut Report| {
        let current = typed_timeout(&typed).unwrap_or_else(|| tool.timeout(&ws.tool_settings) as u32);
        let (again, secs) = with_more_time(&typed, current);
        report.offers.push(Offer { code: again.clone(), detail: format!("again, with {secs} s"), ok: None, act: Act::Replace(again) });
    };
    let next_tool = |report: &mut Report, what: &str, why: &str| {
        if ws.caps.typed_tool(what).is_some() {
            report.offers.push(Offer { code: what.to_string(), detail: why.to_string(), ok: None, act: Act::Replace(what.to_string()) });
        }
    };
    if !report.offers.is_empty() {
        report.outcome = Outcome::Proofs;
    } else if let Some(i) = find(&|x| counterexample(x).is_some()) {
        used[i] = true;
        let (headline, sections) = counterexample(texts[i]).unwrap();
        report.outcome = Outcome::Counterexample { headline, sections };
    } else if let Some(i) = find(&|x| x.contains("found no counterexample")) {
        used[i] = true;
        report.outcome = Outcome::NoCounterexample(texts[i].trim().trim_end_matches('.').to_string());
        next_tool(&mut report, "sledgehammer", "no counterexample: look for a proof");
    } else if let Some(i) = find(&|x| !direct_facts(x).is_empty()) {
        used[i] = true;
        report.outcome = Outcome::Proofs;
        for fact in direct_facts(texts[i]) {
            let code = format!("by (rule {fact})");
            report.offers.push(Offer { detail: "the goal is an instance of this fact".into(), ok: None, act: Act::Insert(code.clone()), code });
        }
    } else if let Some(i) = find(&|x| x.contains("ran out of time") || x.trim() == "Timed out" || x.trim_end().ends_with(": Timed out")) {
        used[i] = true;
        report.outcome = Outcome::NothingFound(format!("{} ran out of time.", name));
        more_time(&mut report);
    } else if let Some(i) = find(&|x| x.trim() == "No proof found" || x.trim() == "Tried in vain") {
        used[i] = true;
        for (j, x) in texts.iter().enumerate() {
            if x.trim() == "Tried in vain" || x.trim() == "No proof found" {
                used[j] = true;
            }
        }
        report.outcome = Outcome::NothingFound(format!("{name} found no proof."));
        if name == "sledgehammer" {
            more_time(&mut report);
            if ws.profile.keyword_line("claude") {
                report.offers.push(Offer { code: "claude".into(), detail: "ask Claude for a proof".into(), ok: None, act: Act::Ask("claude".into()) });
            }
        } else {
            next_tool(&mut report, "sledgehammer", "a longer search, with external provers");
        }
    } else if let Some(i) = messages.iter().position(|m| m["kind"] == "error") {
        used[i] = true;
        report.outcome = Outcome::Failed(ws.profile.message_summary(texts[i]));
        if let Some(tactic) = suggested_tactic(texts[i]) {
            report.offers.push(Offer { detail: "as the error suggests".into(), ok: None, act: Act::Replace(tactic.clone()), code: tactic });
        }
    } else if running {
        // (who has found something, while their proofs are being checked)
        let finders: Vec<&str> = texts.iter().filter_map(|x| x.trim().strip_suffix(" found a proof...")).collect();
        let mut seen: Vec<&str> = Vec::new();
        for f in finders {
            if !seen.contains(&f) {
                seen.push(f);
            }
        }
        report.outcome = Outcome::Running(match seen.len() {
            0 => String::new(),
            _ => format!("{} found {}; checking…", seen.join(", "), if seen.len() == 1 { "a proof" } else { "proofs" }),
        });
    } else {
        report.outcome = Outcome::Done(String::new());
    }
    report.notes = messages.iter().zip(used).filter(|(_, u)| !u).map(|(m, _)| m.clone()).collect();
    if let Outcome::Done(said) = &mut report.outcome
        && report.notes.is_empty()
    {
        // (a Lean tactic that closed its goal says nothing: that is what it did)
        *said = format!("{name} closes the goal here; nothing to insert.");
    }
    Some(report)
}

/// An LLM's proofs for its hole (a `claude` or `codex` line), the checked ones first, and the
/// facts it suggests for Sledgehammer last
pub fn fill_offers(ws: &Workspace) -> Vec<Offer> {
    if !super::view::fill_here(ws) {
        return Vec::new();
    }
    let mut offers: Vec<Offer> = ws
        .fill
        .candidates
        .iter()
        .map(|c| {
            let code = c["text"].as_str().unwrap_or("").to_string();
            let ok = c["ok"].as_bool();
            let detail = match ok {
                Some(true) => "checked: it proves the goal".to_string(),
                Some(false) => c["message"].as_str().map(|m| ws.profile.message_summary(m)).filter(|m| !m.is_empty()).unwrap_or_else(|| "it does not check".into()),
                None => "being checked…".into(),
            };
            Offer { act: Act::Fill(code.clone()), code, detail, ok }
        })
        .collect();
    offers.sort_by_key(|o| match o.ok {
        Some(true) => 0,
        None => 1,
        Some(false) => 2,
    });
    if !ws.fill.using.is_empty() {
        let table = ws.profile.symbols(None);
        let decoded: Vec<String> = ws.fill.using.iter().map(|u| table.decode(u).text).collect();
        let code = ws.profile.facts_line(&decoded, "").lines().next().unwrap_or("").to_string();
        offers.push(Offer { code, detail: format!("facts to guide {}, inserted with it after them", ws.profile.facts_tool()), ok: None, act: Act::Facts });
    }
    offers
}

/// What the panel offers at the caret: the typed tool's findings, else an LLM's proofs
pub fn offers_here(ws: &Workspace) -> Vec<Offer> {
    match report(ws) {
        Some(r) if !r.offers.is_empty() => r.offers,
        _ => fill_offers(ws),
    }
}

/// The offer Alt+Enter takes: the one picked with Alt+Up and Alt+Down, else the first
pub fn picked(ws: &Workspace, offers: &[Offer]) -> usize {
    ws.pick.as_ref().and_then(|p| offers.iter().position(|o| &o.code == p)).unwrap_or(0)
}

/// What the footer says the keyboard does here, while a tool runs or has something to offer
pub fn key_hint(ws: &Workspace) -> Option<String> {
    let offers = offers_here(ws);
    if let Some(o) = offers.get(picked(ws, &offers)) {
        let others = if offers.len() > 1 { " · Alt+Up/Down another" } else { "" };
        return Some(format!("Alt+Enter {} {}{others}", o.verb(), short(&o.code, 36)));
    }
    if super::view::fill_here(ws) && matches!(ws.fill.status.as_str(), "starting" | "asking" | "checking") {
        return Some(format!("Esc stops {}", if ws.fill.agent == 1 { "codex" } else { "claude" }));
    }
    let r = report(ws)?;
    if r.running && r.stoppable {
        return Some(format!("Esc stops {}", r.tool));
    }
    None
}

/// Text cut to `n` characters, with an ellipsis
pub fn short(text: &str, n: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n { one } else { format!("{}…", one.chars().take(n - 1).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_tools::*;

    #[test]
    fn sledgehammer_proofs_are_listed_once_with_who_found_them() {
        let offers = proofs(&sledgehammer_done(), "sledgehammer");
        let codes: Vec<&str> = offers.iter().map(|o| o.code.as_str()).collect();
        assert_eq!(codes, ["using add.commute by blast", "by presburger"], "in the order found, each once");
        assert_eq!(offers[0].detail, "e · 5 ms");
        assert_eq!(offers[1].detail, "zipperposition · 10 ms");
        assert!(offers.iter().all(|o| o.ok == Some(true)));
    }

    #[test]
    fn try0s_choice_comes_first() {
        let offers = proofs(&try0_found(), "try0");
        assert_eq!(offers[0].code, "by simp", "try0's own Try this first");
        assert_eq!(offers[0].detail, "1 ms");
        assert_eq!(offers.len(), 7, "each method once");
    }

    #[test]
    fn credit_reads_the_prover_and_the_time() {
        assert_eq!(credit("e: Try this: using add.commute by blast (5 ms)"), (Some("e".into()), Some("5 ms".into()), true));
        assert_eq!(credit("Try0 found a proof: by simp (2 ms)"), (Some("try0".into()), Some("2 ms".into()), true));
        assert_eq!(credit("verit: Try this: using le_square by presburger (0.9 ms)").1.as_deref(), Some("0.9 ms"));
        assert!(!credit("vampire: Try this: by (metis foo) (> 1.0 s, timed out)").2, "a timed-out check does not work");
        assert_eq!(credit("Try this: exact Nat.add_comm a b"), (None, None, true));
    }

    #[test]
    fn counterexamples_read_as_tables() {
        let (head, s) = counterexample("Nitpick found a counterexample:\n  Free variables:\n    xs = [4]\n    ys = [3]").unwrap();
        assert_eq!(head, "Nitpick found a counterexample");
        assert_eq!(s, vec![Section { title: "Free variables".into(), rows: vec![("xs".into(), "[4]".into()), ("ys".into(), "[3]".into())] }]);
        let (_, s) = counterexample("Quickcheck found a counterexample:\n  xs = [1]\n  ys = [0]\nEvaluated terms:\n  rev (xs @ ys) = [0, 1]\n  rev xs @ rev ys = [1, 0]").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "Variables");
        assert_eq!(s[1].rows[0], ("rev (xs @ ys)".into(), "[0, 1]".into()));
        assert!(counterexample("Nitpick found no counterexample").is_none());
    }

    #[test]
    fn more_time_doubles_the_timeout_and_keeps_other_options() {
        assert_eq!(with_more_time("sledgehammer", 30), ("sledgehammer [timeout = 60]".into(), 60));
        assert_eq!(with_more_time("sledgehammer [provers = e, timeout = 60]", 60), ("sledgehammer [provers = e, timeout = 120]".into(), 120));
        assert_eq!(typed_timeout("nitpick [timeout = 4]"), Some(4));
        assert_eq!(typed_timeout("nitpick"), None);
    }

    #[test]
    fn solve_direct_and_lean_errors_offer_what_to_do() {
        assert_eq!(direct_facts(&solve_direct_found()[0]["text"].as_str().unwrap()), ["Groups.ab_semigroup_add_class.add.commute", "Groups.ab_semigroup_add_class.add_ac(2)"]);
        assert_eq!(suggested_tactic("`exact?` could not close the goal. Try `apply?` to see partial suggestions.").as_deref(), Some("apply?"));
    }
}

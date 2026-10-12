// SPDX-License-Identifier: GPL-3.0-only

//! The interactive proof tools through the real window, headless (see `snapshot.rs`): a tool
//! typed in a proof (Isabelle's sledgehammer, nitpick, quickcheck, try0, try, solve_direct;
//! Lean's `exact?`, `apply?`, `rw?`, `simp?`, `decide`, `omega`; a `claude` line), with what a
//! real host sent for it, in each state of its panel: waiting, running with progress, findings,
//! a counterexample, a failure or timeout, stopped. Dark and light, the goal beside and under,
//! 800 and 1400 px. The host messages are the ones recorded from Isabelle2025 on au-laptop and
//! Lean v4.19.0 on neo (2026-10-12). Set `CT_SNAPSHOTS` to keep the frames.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic::iced::Size;
use serde_json::{Value, json};

use super::editor::{Mode, Theory};
use super::prover::Profile;
use super::snapshot::{Harness, caret_in_block, demo_app};
use super::{App, Message, QueryState};

/// The scratch theory the live probes ran on; `TOOL` is where the tool is typed
pub const ISA_FLOW: &str = "theory Flow\n  imports Main\nbegin\n\nlemma comm: \"(a::nat) + b = b + a\"\n  TOOL\n\nlemma wrong: \"rev (xs @ ys) = rev xs @ rev (ys::nat list)\"\n  sorry\n\nend\n";

pub const LEAN_FLOW: &str = "theorem add_swap (a b : Nat) : a + b = b + a := by\n  TOOL\n\ntheorem le_succ' (n : Nat) : n ≤ n + 1 := by\n  sorry\n\ntheorem bad_lt (n : Nat) : n < n + 0 := by\n  sorry\n";

/// A message as the host sends it
pub fn msg(kind: &str, text: &str, sendbacks: &[&str]) -> Value {
    json!({"kind": kind, "text": text, "raw": text, "spans": [], "sendbacks": sendbacks})
}

/// Commands as the prover reports them, one per line here: its first word is its name; the
/// theory header is one command. `status` is the tool's; other lines are finished (a `sorry`
/// warned)
fn commands(text: &str, tool: &str, tool_name: &str, status: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut offset = 0;
    let header_end = text.find("begin").map(|b| text[..b + 5].chars().count());
    if let Some(end) = header_end {
        out.push(json!([0, end, "theory", "finished"]));
    }
    for line in text.split('\n') {
        let n = line.chars().count();
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let start = offset + indent;
        offset += n + 1;
        if line.trim().is_empty() || header_end.is_some_and(|e| start < e) {
            continue;
        }
        let mut name = line.trim().split_whitespace().next().unwrap_or("").to_string();
        let st = if !tool.is_empty() && line.trim().starts_with(tool) {
            name = tool_name.to_string();
            status
        } else if name == "sorry" {
            "warned"
        } else {
            "finished"
        };
        // (a keyword line is a `sorry` padded to its length, and the prover's command is the
        // `sorry`, as recorded: the rest of the line is in no command)
        let len = if name == "sorry" && tool_name == "sorry" && tool.len() > 5 && line.trim().starts_with(tool) { 5 } else { n - indent };
        out.push(json!([start, start + len, name, st]));
    }
    out
}

/// What a tool's run looks like in the window: the tool's text typed in the first proof, the
/// prover's report for it, and the goal there
pub struct ToolScene {
    pub lean: bool,
    /// What is typed in place of `TOOL`
    pub typed: String,
    /// The tool's command status (`unprocessed`, `running`, `finished`, `failed`)
    pub status: &'static str,
    /// What the host's `state` reply said at the tool
    pub messages: Vec<Value>,
    /// Seconds since the tool started running
    pub secs: u64,
    /// An LLM's job for the line (a `claude` line): its status, its candidates and facts
    pub fill: Option<(&'static str, Vec<Value>, Vec<String>)>,
}

impl ToolScene {
    pub fn isabelle(typed: &str, status: &'static str, messages: Vec<Value>) -> Self {
        Self { lean: false, typed: typed.into(), status, messages, secs: 2, fill: None }
    }

    pub fn lean(typed: &str, status: &'static str, messages: Vec<Value>) -> Self {
        Self { lean: true, typed: typed.into(), status, messages, secs: 0, fill: None }
    }

    pub fn text(&self) -> String {
        (if self.lean { LEAN_FLOW } else { ISA_FLOW }).replace("TOOL", &self.typed)
    }

    /// The window with the first proof open, the caret on the tool
    pub fn app(&self, width: f32) -> App {
        let text = self.text();
        // (a keyword line is a `sorry` to the prover)
        let keyword = self.typed.starts_with("claude") || self.typed.starts_with("codex");
        let name = if keyword { "sorry".to_string() } else { self.typed.split_whitespace().next().unwrap_or("").to_string() };
        let prover = json!({
            "revision": 1,
            "status": {"consolidated": self.status != "running", "running": usize::from(self.status == "running")},
            "commands": commands(&text, &self.typed, &name, self.status),
            "messages": [],
        });
        let doc = json!({"text": text, "revision": 1, "prover": prover, "dirty": false, "can_undo": true});
        let mut app = if self.lean { super::snapshot_lean::lean_app(Mode::Literate) } else { demo_app(Mode::Literate) };
        let profile: &'static dyn Profile = if self.lean { &super::prover::LEAN } else { crate::prover::profile(None) };
        let table = profile.symbols(None);
        let path = if self.lean { "/home/me/Flow/Flow/Basic.lean" } else { "/home/me/Flow/Flow.thy" };
        let mut theory = Theory::new(path.into(), &doc, &*table, Arc::new(HashMap::new()), Mode::Literate, profile);
        theory.typed_tools = profile.default_capabilities().typed_tools();
        app.show_right = false;
        app.show_sidebar = false;
        app.window_width = width;
        let ws = app.workspace.as_mut().unwrap();
        ws.theory = Some(theory);
        ws.notes = Vec::new();
        ws.history = Vec::new();
        ws.activity = None;
        ws.fill = Default::default();
        ws.goals = QueryState {
            operation: "deductum_goals",
            status: "finished".into(),
            messages: if self.lean {
                super::prover::lean::demo::goal_parts(&[("", &["a b : Nat"], "a + b = b + a")])
            } else {
                ["#deductum:count 1", "#deductum:param 1\na b :: nat", "#deductum:concl 1\na + b = b + a"].iter().map(|r| json!({"kind": "writeln", "raw": r, "text": r, "spans": []})).collect()
            },
            ..Default::default()
        };
        ws.goals_prev = QueryState { operation: "deductum_goals", ..Default::default() };
        let i = ws.theory.as_ref().unwrap().blocks.iter().position(|b| b.name == "lemma" || b.name == "theorem").expect("a proof");
        let _ = cosmic::Application::update(&mut app, Message::EditBlock(i));
        caret_in_block(&mut app, &self.typed, 2);
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        t.sync_block_cursor();
        let (cs, ce, _, _) = t.caret_signature();
        let ws = app.workspace.as_mut().unwrap();
        ws.output = Some(json!({"command": {"start": cs, "end": ce, "name": name, "status": self.status}, "messages": self.messages, "revision": 1}));
        ws.tool_started = (self.status == "running").then(|| std::time::Instant::now() - std::time::Duration::from_secs(self.secs));
        if let Some((status, candidates, using)) = &self.fill {
            ws.fill = super::FillState {
                job: Some("j1".into()),
                model: Some("opus".into()),
                status: status.to_string(),
                candidates: candidates.clone(),
                using: using.clone(),
                replace: Some((cs, ce)),
                revision: 1,
                started: Some(std::time::Instant::now() - std::time::Duration::from_secs(self.secs)),
                ..Default::default()
            };
        }
        ws.rebuild_views(&*table);
        app
    }
}

/// Recorded on au-laptop: sledgehammer on `comm`, part way and done
pub fn sledgehammer_running() -> Vec<Value> {
    vec![
        msg("writeln", "Sledgehammering...", &[]),
        msg("writeln", "spass found a proof...", &[]),
        msg("writeln", "e found a proof...", &[]),
        msg("writeln", "e: Try this: using add.commute by blast (5 ms)", &["using add.commute by blast"]),
        msg("writeln", "cvc5 found a proof...", &[]),
        msg("writeln", "zipperposition found a proof...", &[]),
        msg("writeln", "spass: Duplicate proof", &[]),
        msg("writeln", "zipperposition: Try this: by presburger (10 ms)", &["by presburger"]),
    ]
}

pub fn sledgehammer_done() -> Vec<Value> {
    let mut m = sledgehammer_running();
    for t in ["cvc5: Duplicate proof", "cvc5 found a proof...", "vampire found a proof...", "zipperposition found a proof...", "cvc5: Duplicate proof", "zipperposition: Duplicate proof", "vampire: Duplicate proof", "verit found a proof...", "verit: Duplicate proof", "Done"] {
        m.push(msg("writeln", t, &[]));
    }
    m
}

pub fn nitpick_counterexample() -> Vec<Value> {
    vec![msg("writeln", "Nitpicking formula...", &[]), msg("writeln", "Nitpick found a counterexample:\n  Free variables:\n    xs = [4]\n    ys = [3]", &[])]
}

pub fn quickcheck_counterexample() -> Vec<Value> {
    vec![
        msg("writeln", "Testing conjecture with Quickcheck-exhaustive...", &[]),
        msg("writeln", "Quickcheck found a counterexample:\n  xs = [1]\n  ys = [0]\nEvaluated terms:\n  rev (xs @ ys) = [0, 1]\n  rev xs @ rev ys = [1, 0]", &[]),
    ]
}

pub fn try0_found() -> Vec<Value> {
    let mut m = vec![msg("writeln", "Trying \"simp\", \"auto\", \"blast\", \"metis\", \"argo\", \"linarith\", \"presburger\", \"algebra\", \"fast\", \"fastforce\", \"force\", \"meson\", \"satx\", and \"order\"...", &[])];
    for (p, t) in [("presburger", 1), ("simp", 1), ("auto", 2), ("fastforce", 1), ("force", 2), ("algebra", 4), ("linarith", 7)] {
        let proof = format!("by {p}");
        m.push(msg("writeln", &format!("Found proof: {proof} ({t} ms)"), &[proof.as_str()]));
    }
    m.push(msg("writeln", "Try this: by simp\n(simp, presburger, fastforce: 1 ms; auto, force: 2 ms; algebra: 4 ms; linarith: 7 ms)", &["by simp"]));
    m
}

pub fn solve_direct_found() -> Vec<Value> {
    vec![msg("writeln", "solve_direct: the current goal can be solved directly with\n  Groups.ab_semigroup_add_class.add.commute: ?a + ?b = ?b + ?a\n  Groups.ab_semigroup_add_class.add_ac(2): ?a + ?b = ?b + ?a", &[])]
}

/// Recorded on neo with Lean v4.19.0
pub fn lean_exact() -> Vec<Value> {
    vec![msg("information", "Try this: exact Nat.add_comm a b", &["exact Nat.add_comm a b"]), msg("state", "No goals", &[])]
}

pub fn lean_rw() -> Vec<Value> {
    vec![msg("information", "Try this: rw [List.length_append]\n-- \"no goals\"", &["rw [List.length_append]"]), msg("state", "No goals", &[])]
}

pub fn lean_exact_failed() -> Vec<Value> {
    vec![msg("error", "`exact?` could not close the goal. Try `apply?` to see partial suggestions.", &[]), msg("state", "n : Nat\n⊢ n < n + 0", &[])]
}

pub fn lean_omega_failed() -> Vec<Value> {
    vec![msg("error", "omega could not prove the goal:\na possible counterexample may satisfy the constraints\n  a ≥ 0\nwhere\n a := ↑n", &[]), msg("state", "n : Nat\n⊢ n < n + 0", &[])]
}

/// A `claude` line's proofs as the host sends them: checked, one failing, one passing
pub fn claude_candidates() -> Vec<Value> {
    vec![
        json!({"text": "by auto", "ok": false, "message": "Failed to finish proof\ngoal (1 subgoal):\n 1. a + b = b + a"}),
        json!({"text": "by (simp add: add.commute)", "ok": true, "message": ""}),
    ]
}

/// Each state of a tool's panel, as a scene
pub fn scenes() -> Vec<(&'static str, ToolScene)> {
    let mut running = ToolScene::isabelle("sledgehammer", "running", vec![msg("writeln", "Sledgehammering...", &[]), msg("writeln", "spass found a proof...", &[]), msg("writeln", "e found a proof...", &[])]);
    running.secs = 1;
    let mut asking = ToolScene::isabelle("claude", "warned", vec![]);
    asking.fill = Some(("asking", vec![], vec![]));
    asking.secs = 6;
    let mut answered = ToolScene::isabelle("claude", "warned", vec![]);
    answered.fill = Some(("done", claude_candidates(), vec!["add.commute".into()]));
    vec![
        ("sledgehammer-waiting", ToolScene::isabelle("sledgehammer", "unprocessed", vec![])),
        ("sledgehammer-running", running),
        ("sledgehammer-found", ToolScene::isabelle("sledgehammer", "running", sledgehammer_running())),
        ("sledgehammer-done", ToolScene::isabelle("sledgehammer", "finished", sledgehammer_done())),
        ("sledgehammer-nothing", ToolScene::isabelle("sledgehammer", "finished", vec![msg("writeln", "Sledgehammering...", &[]), msg("writeln", "No proof found", &[])])),
        // (as recorded: a cancelled sledgehammer fails without an error)
        ("sledgehammer-stopped", ToolScene::isabelle("sledgehammer", "failed", vec![msg("writeln", "Sledgehammering...", &[])])),
        ("nitpick-counterexample", ToolScene::isabelle("nitpick", "finished", nitpick_counterexample())),
        ("nitpick-timeout", ToolScene::isabelle("nitpick", "finished", vec![msg("writeln", "Nitpicking formula...", &[]), msg("writeln", "Nitpick ran out of time after checking 7 of 10 scopes", &[])])),
        ("quickcheck-counterexample", ToolScene::isabelle("quickcheck", "finished", quickcheck_counterexample())),
        ("quickcheck-none", ToolScene::isabelle("quickcheck", "finished", vec![msg("writeln", "Testing conjecture with Quickcheck-exhaustive...", &[]), msg("writeln", "Quickcheck found no counterexample.", &[])])),
        ("try0-found", ToolScene::isabelle("try0", "finished", try0_found())),
        ("try-found", ToolScene::isabelle("try", "finished", vec![msg("writeln", "Trying \"solve_direct\", \"quickcheck\", \"try0\", \"sledgehammer\", and \"nitpick\"...", &[]), msg("writeln", "Try0 found a proof: by simp (2 ms)", &["by simp"])])),
        ("solve-direct-found", ToolScene::isabelle("solve_direct", "finished", solve_direct_found())),
        ("claude-asking", asking),
        ("claude-answered", answered),
        ("lean-exact-found", ToolScene::lean("exact?", "finished", lean_exact())),
        ("lean-rw-found", ToolScene::lean("rw?", "finished", lean_rw())),
        ("lean-exact-failed", ToolScene::lean("exact?", "failed", lean_exact_failed())),
        ("lean-omega-failed", ToolScene::lean("omega", "failed", lean_omega_failed())),
    ]
}

/// Every panel state, dark and light, the goal beside the text (1400 px), under it (the
/// toggle off at 1400 px, and an 800 px window): the tool's section is drawn above the goal, and
/// says what the tool is doing or what it came to
#[test]
fn the_tool_panel_in_each_state() {
    use super::flow::Outcome;
    for (name, scene) in scenes() {
        for dark in [true, false] {
            for (w, beside, layout) in [(1400.0, true, "1400-beside"), (1400.0, false, "1400-under"), (800.0, true, "800")] {
                let mut h = Harness::new(Size::new(w, 900.0));
                cosmic::theme::set_active(if dark { cosmic::theme::ThemeType::Dark } else { cosmic::theme::ThemeType::Light });
                super::view::set_goal_beside(beside);
                let app = scene.app(w);
                h.focus(&app, super::view::block_editor_id());
                h.draw(&app, &format!("tool-{name}-{}-{layout}", if dark { "dark" } else { "light" }));
            }
        }
        // What the panel makes of it
        let app = scene.app(1400.0);
        let ws = app.workspace.as_ref().unwrap();
        let report = super::flow::report(ws);
        let offers = super::flow::offers_here(ws);
        match name {
            "sledgehammer-waiting" => assert!(matches!(report.unwrap().outcome, Outcome::Waiting(_))),
            "sledgehammer-running" => assert!(matches!(report.unwrap().outcome, Outcome::Running(ref s) if s.contains("spass, e found proofs"))),
            "sledgehammer-found" | "sledgehammer-done" => assert_eq!(offers[0].code, "using add.commute by blast"),
            "sledgehammer-nothing" => assert_eq!(offers.iter().map(|o| o.code.as_str()).collect::<Vec<_>>(), ["sledgehammer [timeout = 60]", "claude"]),
            "sledgehammer-stopped" => assert_eq!(report.unwrap().outcome, Outcome::Stopped),
            "nitpick-counterexample" | "quickcheck-counterexample" => assert!(matches!(report.unwrap().outcome, Outcome::Counterexample { .. })),
            "nitpick-timeout" => assert_eq!(offers[0].code, "nitpick [timeout = 60]"),
            "quickcheck-none" => assert_eq!(offers[0].code, "sledgehammer"),
            "try0-found" | "try-found" => assert_eq!(offers[0].code, "by simp"),
            "claude-answered" => assert_eq!(offers[0].code, "by (simp add: add.commute)", "the checked proof first"),
            "lean-exact-found" => assert_eq!(offers[0].code, "exact Nat.add_comm a b"),
            "lean-rw-found" => assert_eq!(offers[0].code, "rw [List.length_append]", "without Lean's comment"),
            "lean-exact-failed" => assert_eq!(offers[0].code, "apply?"),
            _ => {}
        }
    }
}

fn update(app: &mut App, h: &mut Harness, m: Message) {
    let task = cosmic::Application::update(app, m);
    h.run_task(app, task);
}

fn alt(h: &mut Harness, app: &App, key: cosmic::iced::keyboard::key::Named) -> Vec<Message> {
    h.key_with(app, cosmic::iced::keyboard::Key::Named(key), None, cosmic::iced::keyboard::Modifiers::ALT)
}

fn theory(app: &App) -> &Theory {
    app.workspace.as_ref().unwrap().theory.as_ref().unwrap()
}

/// Alt+Enter at the tool's line inserts the first proof that worked in place of exactly the
/// tool's text, as one edit, with the caret just after the proof and the editor still taking
/// the typing; Alt+Down first picks the next one
#[test]
fn alt_enter_inserts_the_highlighted_proof_where_the_tool_was() {
    for (pick_next, want) in [(false, "using add.commute by blast"), (true, "by presburger")] {
        let mut h = Harness::new(Size::new(1400.0, 900.0));
        super::view::set_goal_beside(true);
        let scene = ToolScene::isabelle("sledgehammer", "finished", sledgehammer_done());
        let mut app = scene.app(1400.0);
        h.focus(&app, super::view::block_editor_id());
        h.draw(&app, "keys-0");
        let (s, e, _) = theory(&app).caret_command().unwrap();
        if pick_next {
            let msgs = alt(&mut h, &app, cosmic::iced::keyboard::key::Named::ArrowDown);
            assert!(msgs.iter().any(|m| matches!(m, Message::PickOffer(true))), "{msgs:?}");
            for m in msgs {
                update(&mut app, &mut h, m);
            }
            h.draw(&app, "keys-1-picked");
        }
        let msgs = alt(&mut h, &app, cosmic::iced::keyboard::key::Named::Enter);
        assert!(msgs.iter().any(|m| matches!(m, Message::AcceptOffer)), "{msgs:?}");
        for m in msgs {
            update(&mut app, &mut h, m);
        }
        h.draw(&app, if pick_next { "keys-2-inserted-picked" } else { "keys-2-inserted" });
        let t = theory(&app);
        assert!(t.local.contains(&format!("\n  {want}\n")) && !t.local.contains("sledgehammer"), "{:?}", t.local);
        assert_eq!(t.cursor, s + want.chars().count(), "the caret just after the proof");
        let be = t.block_editor.as_ref().expect("the block stays open");
        assert_eq!(be.start + be.decoded.to_source(be.caret()), s + want.chars().count(), "the block's caret too");
        // (one edit, sent: exactly the tool's text replaced)
        assert!(!t.settled(), "the edit went to the host");
        assert_eq!(t.local, super::ot::apply(&t.confirmed, &vec![(s, e, want.to_string())]));
        // typing goes on where the proof ends
        for m in h.type_text(&app, "!") {
            update(&mut app, &mut h, m);
        }
        assert!(theory(&app).local.contains(&format!("{want}!")), "typed after the proof: {:?}", theory(&app).local);
    }
}

/// Clicking Insert does the same: the click takes the focus, which goes back to the editor
#[test]
fn clicking_insert_gives_the_editor_its_focus_back() {
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    let mut app = ToolScene::isabelle("sledgehammer", "finished", sledgehammer_done()).app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    h.draw(&app, "click-0");
    let mut clicked = None;
    'scan: for y in (150..600).step_by(6) {
        for x in (1000..1220).step_by(10) {
            let msgs = h.click(&app, cosmic::iced::Point::new(x as f32, y as f32));
            if let Some(m) = msgs.iter().find(|m| matches!(m, Message::InsertOutput(p) if p == "by presburger")) {
                clicked = Some(m.clone());
                break 'scan;
            }
        }
    }
    let m = clicked.expect("the second proof's Insert button");
    update(&mut app, &mut h, m);
    for m in h.type_text(&app, "!") {
        update(&mut app, &mut h, m);
    }
    assert!(theory(&app).local.contains("by presburger!"), "{:?}", theory(&app).local);
}

/// Esc while the tool runs stops it and leaves the block open, the caret where it was; the next
/// Esc (nothing running) finishes the block
#[test]
fn esc_stops_a_running_tool_before_it_closes_the_block() {
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    let mut app = ToolScene::isabelle("sledgehammer", "running", sledgehammer_running()).app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    let msgs = h.key(&app, cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::Escape), None);
    assert!(msgs.iter().any(|m| matches!(m, Message::Escape)), "{msgs:?}");
    for m in msgs {
        update(&mut app, &mut h, m);
    }
    assert!(app.workspace.as_ref().unwrap().tools_stopping, "Stop was asked for");
    assert!(theory(&app).block_editor.is_some(), "the block stays open");
    h.draw(&app, "esc-stopping");
    // stopped: the next Esc finishes the block
    let mut app = ToolScene::isabelle("sledgehammer", "failed", vec![msg("error", "Interrupt", &[])]).app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    for m in h.key(&app, cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::Escape), None) {
        update(&mut app, &mut h, m);
    }
    assert!(theory(&app).block_editor.is_none(), "the block is finished");
}

/// Lean's Try this and a `claude` line's proofs are taken the same way
#[test]
fn alt_enter_takes_lean_suggestions_and_llm_proofs() {
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    let mut app = ToolScene::lean("rw?", "finished", lean_rw()).app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    for m in alt(&mut h, &app, cosmic::iced::keyboard::key::Named::Enter) {
        update(&mut app, &mut h, m);
    }
    assert!(theory(&app).local.contains("  rw [List.length_append]\n") && !theory(&app).local.contains("rw?"), "{:?}", theory(&app).local);
    let mut answered = ToolScene::isabelle("claude", "warned", vec![]);
    answered.fill = Some(("done", claude_candidates(), vec![]));
    let mut app = answered.app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    for m in alt(&mut h, &app, cosmic::iced::keyboard::key::Named::Enter) {
        update(&mut app, &mut h, m);
    }
    assert!(theory(&app).local.contains("  by (simp add: add.commute)\n") && !theory(&app).local.contains("claude"), "the checked proof: {:?}", theory(&app).local);
    for m in h.type_text(&app, "!") {
        update(&mut app, &mut h, m);
    }
    assert!(theory(&app).local.contains("add.commute)!"), "{:?}", theory(&app).local);
}

/// A `claude[haiku]` line's proofs show with the caret where typing left it, at the line's end,
/// past the `sorry` the prover sees there
#[test]
fn llm_proofs_show_with_the_caret_at_the_end_of_the_keyword() {
    let mut scene = ToolScene::isabelle("claude[haiku]", "warned", vec![]);
    scene.fill = Some(("done", claude_candidates(), vec![]));
    let mut app = scene.app(1400.0);
    caret_in_block(&mut app, "claude[haiku]", "claude[haiku]".len());
    app.workspace.as_mut().unwrap().theory.as_mut().unwrap().sync_block_cursor();
    let ws = app.workspace.as_ref().unwrap();
    assert!(super::view::fill_here(ws), "the job's proofs belong here");
    assert_eq!(super::flow::offers_here(ws)[0].code, "by (simp add: add.commute)");
}

/// More time is the tool's line again with twice its timeout, the caret on it (so its new run
/// shows); a counterexample search that found none offers Sledgehammer the same way
#[test]
fn again_with_more_time_rewrites_the_tool_line() {
    let mut h = Harness::new(Size::new(1400.0, 900.0));
    super::view::set_goal_beside(true);
    let mut app = ToolScene::isabelle("sledgehammer", "finished", vec![msg("writeln", "No proof found", &[])]).app(1400.0);
    h.focus(&app, super::view::block_editor_id());
    for m in alt(&mut h, &app, cosmic::iced::keyboard::key::Named::Enter) {
        update(&mut app, &mut h, m);
    }
    let t = theory(&app);
    assert!(t.local.contains("  sledgehammer [timeout = 60]\n"), "{:?}", t.local);
    let (s, e, _) = t.caret_command().expect("the caret on the tool's line");
    assert_eq!(t.source_text(s, e), "sledgehammer [timeout = 60]");
    h.draw(&app, "again-checking");
    assert!(matches!(super::flow::report(app.workspace.as_ref().unwrap()).unwrap().outcome, super::flow::Outcome::Waiting(_)), "until the prover has seen the new line, the panel waits");
}



#[test]
fn zz_debug_focus() {
    for height in [950.0, 260.0, 200.0] {
        let mut h = Harness::new(Size::new(1400.0, height));
        super::view::set_goal_beside(true);
        let scene = ToolScene::isabelle("sorry", "warned", vec![]);
        let mut app = scene.app(1400.0);
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let be = t.block_editor.as_mut().unwrap();
        let text = be.content.text();
        let li = super::editor::LineIndex::new(&text);
        super::editor::place(&mut be.content, li.position(&text, text.chars().count()));
        t.sync_block_cursor();
        h.focus(&app, super::view::block_editor_id());
        h.draw(&app, &format!("dbg-{height}-0"));
        for i in 0..3 {
            h.frame(&app);
            let msgs = h.key(&app, cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::Backspace), None);
            println!("{height} {i}: {} completions {msgs:?}", theory(&app).completions.len());
            for m in msgs {
                update(&mut app, &mut h, m);
            }
            h.draw(&app, &format!("dbg-{height}-{}", i + 1));
        }
    }
}


/// The prover's command for a keyword line ends with the `sorry` it stands for; the cell takes
/// the whole line, with no stray "e[haiku]" after it
#[test]
fn a_keyword_line_stays_in_its_cell() {
    let scene = ToolScene::isabelle("claude[haiku]", "warned", vec![]);
    let text = scene.text();
    let prover = json!({"revision": 1, "status": {}, "commands": commands(&text, "claude[haiku]", "sorry", "warned"), "messages": []});
    let doc = json!({"text": text, "revision": 1, "prover": prover});
    let t = Theory::new("/x/Flow.thy".into(), &doc, &*crate::prover::profile(None).symbols(None), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None));
    let shown: Vec<String> = t.blocks.iter().map(|b| t.local.chars().skip(b.start).take(b.end - b.start).collect()).collect();
    assert!(shown.iter().any(|b| b.starts_with("lemma comm") && b.ends_with("claude[haiku]")), "{shown:?}");
    assert!(t.blocks.iter().all(|b| b.kind != super::theory::Kind::Comment), "{shown:?}");
}

/// While a tool is the last step of a proof, Isabelle says "Bad context" of the lemma after it:
/// that lemma reads as waiting (unfinished), not as failing
#[test]
fn the_lemma_after_a_running_tool_is_not_shown_failing() {
    let scene = ToolScene::isabelle("sledgehammer", "running", vec![]);
    let text = scene.text();
    let mut cmds = commands(&text, "sledgehammer", "sledgehammer", "running");
    let wrong = text[..text.find("lemma wrong").unwrap()].chars().count();
    for c in cmds.iter_mut().filter(|c| c[0] == json!(wrong)) {
        c[3] = json!("failed");
    }
    let prover = json!({"revision": 1, "status": {}, "commands": cmds, "messages": [{"start": wrong, "end": wrong + 5, "kind": "error", "text": "Bad context for command \"lemma\" -- using reset state"}]});
    let doc = json!({"text": text, "revision": 1, "prover": prover});
    let mut t = Theory::new("/x/Flow.thy".into(), &doc, &*crate::prover::profile(None).symbols(None), Arc::new(HashMap::new()), Mode::Literate, crate::prover::profile(None));
    t.typed_tools = crate::prover::profile(None).default_capabilities().typed_tools();
    let b = t.blocks.iter().find(|b| b.start == wrong).expect("the next lemma");
    assert_eq!(b.status, super::theory::Status::Failed);
    assert_eq!(t.shown_status(b.start, b.end, b.status), super::theory::Status::Warned);
    assert!(t.messages_in(b.start, b.end).iter().all(|m| t.waits_on_previous(m)));
}

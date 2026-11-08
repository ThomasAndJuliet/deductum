// SPDX-License-Identifier: GPL-3.0-only

//! The proof tools' flow against real provers, through the window as the human uses it: type a
//! tool in a proof (or click it on the insert bar), watch its section while it runs, take what
//! it found with Alt+Enter (or Alt+Down first), and see the proof replace the tool's line and be
//! checked; undo it in one step; stop a tool with Esc; run it again with more time.
//!
//! - Isabelle: `DEDUCTUM_LIVE_TOOLS_ISABELLE=1 cargo test --bin deductum live_tools_isabelle --
//!   --ignored --nocapture`, with a host of its own on this computer (`DEDUCTUM_LOCAL_ROOT`, its
//!   own state) and the Isabelle2025 it finds (`~/Documents/Software/Isabelle*`, or
//!   `DEDUCTUM_ISABELLE=/path/bin/isabelle`); or on another
//!   machine with `DEDUCTUM_LIVE=machine DEDUCTUM_REMOTE_ROOT=/tmp/…`. A scratch theory in /tmp;
//!   the user's own host is not touched, and Isabelle's component list (where the test host
//!   registers its backend in place of others) is put back as it was.
//! - Lean: `DEDUCTUM_LIVE_TOOLS_LEAN=1 cargo test --bin deductum live_tools_lean -- --ignored
//!   --nocapture`. A host of its own on this computer (`DEDUCTUM_LOCAL_ROOT`) and a scratch Lake
//!   project for toolchain v4.19.0.
//!
//! `DEDUCTUM_LIVE_LLM=1` adds a `claude[haiku]` line to each. Frames go to `CT_SNAPSHOTS` as
//! `live-tools-*.png`; the timings are printed.

use std::time::{Duration, Instant};

use cosmic::Application;
use cosmic::iced::Size;
use cosmic::iced::futures::{FutureExt, StreamExt};
use cosmic::iced::keyboard::key::Named;
use serde_json::json;

use super::editor::Mode;
use super::flow::Outcome;
use super::snapshot::Harness;
use super::theory::Status;
use super::{App, Message, Pending, link};

/// The window, its host link, and the tasks its updates start (timers, the state's refresh), run
/// as the app's runtime runs them
struct Live<S> {
    app: App,
    h: Harness,
    stream: std::pin::Pin<Box<S>>,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    frames: usize,
    prefix: &'static str,
}

impl<S: cosmic::iced::futures::Stream<Item = (String, link::Event)>> Live<S> {
    fn new(app: App, stream: S, prefix: &'static str) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self { app, h: Harness::new(Size::new(1400.0, 950.0)), stream: Box::pin(stream), tx, rx, frames: 0, prefix }
    }

    /// An update, and what its task does: widget operations at once (a focus), the rest (timers,
    /// the host) as they finish
    fn apply(&mut self, m: Message) {
        let task = self.app.update(m);
        let Some(mut stream) = cosmic::iced::runtime::task::into_stream(task) else { return };
        loop {
            match stream.next().now_or_never() {
                Some(Some(cosmic::iced::runtime::Action::Widget(mut op))) => self.h.operate(&self.app, op.as_mut()),
                Some(Some(cosmic::iced::runtime::Action::Output(cosmic::Action::App(m)))) => {
                    let _ = self.tx.send(m);
                }
                Some(Some(_)) => {}
                Some(None) => return,
                None => break,
            }
        }
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Some(action) = stream.next().await {
                if let cosmic::iced::runtime::Action::Output(cosmic::Action::App(m)) = action {
                    let _ = tx.send(m);
                }
            }
        });
    }

    fn apply_all(&mut self, ms: Vec<Message>) {
        for m in ms {
            self.apply(m);
        }
    }

    /// The host's events and the tasks' messages, until `done` holds
    async fn pump(&mut self, secs: u64, mut done: impl FnMut(&App) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if done(&self.app) {
                return true;
            }
            tokio::select! {
                Some((key, event)) = self.stream.next() => self.apply(Message::Link(key, event)),
                Some(m) = self.rx.recv() => self.apply(m),
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
        done(&self.app)
    }

    fn draw(&mut self, name: &str) {
        self.frames += 1;
        let name = format!("live-tools-{}-{:02}-{name}", self.prefix, self.frames);
        self.h.draw(&self.app, &name);
    }

    fn theory(&self) -> &super::editor::Theory {
        self.app.workspace.as_ref().unwrap().theory.as_ref().unwrap()
    }

    fn local(&self) -> String {
        self.theory().local.clone()
    }

    fn report(&self) -> Option<super::flow::Report> {
        super::flow::report(self.app.workspace.as_ref().unwrap())
    }

    fn key(&mut self, named: Named) {
        self.h.frame(&self.app);
        let msgs = self.h.key(&self.app, cosmic::iced::keyboard::Key::Named(named), None);
        self.apply_all(msgs);
    }

    fn alt(&mut self, named: Named) {
        self.h.frame(&self.app);
        let msgs = self.h.key_with(&self.app, cosmic::iced::keyboard::Key::Named(named), None, cosmic::iced::keyboard::Modifiers::ALT);
        assert!(!msgs.is_empty(), "Alt+{named:?} reaches the editor");
        self.apply_all(msgs);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.h.frame(&self.app);
            let s = c.to_string();
            let msgs = self.h.key(&self.app, cosmic::iced::keyboard::Key::Character(s.clone().into()), Some(&s));
            self.apply_all(msgs);
        }
    }

    /// Open the block holding `needle` (in the source) with the caret `plus` characters after it,
    /// as a click there does, and focus its editor
    fn open_at(&mut self, needle: &str, plus: usize) {
        let _ = self.app.update(Message::CloseBlock);
        let local = self.local();
        let at = local[..local.find(needle).unwrap_or_else(|| panic!("{needle:?} in the theory"))].chars().count() + plus;
        let i = self.theory().blocks.iter().position(|b| b.start <= at && at <= b.end).expect("a block there");
        self.apply(Message::EditBlock(i));
        let t = self.app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let be = t.block_editor.as_mut().expect("the block opened");
        let shown = be.shown();
        let li = super::editor::LineIndex::new(&shown);
        super::editor::place(&mut be.content, li.position(&shown, be.decoded.to_decoded(at - be.start)));
        t.sync_block_cursor();
        self.h.focus(&self.app, super::view::block_editor_id());
        self.apply(Message::StateTick(self.app.state_gen));
    }

    /// In the lemma named `lemma`, the `sorry` deleted and `tool` typed in its place, a key at a
    /// time; when the last key was typed
    fn type_tool(&mut self, lemma: &str, tool: &str) -> Instant {
        let local = self.local();
        let from = local.find(lemma).unwrap_or_else(|| panic!("{lemma}"));
        let sorry = from + local[from..].find("sorry").expect("its sorry");
        let needle = &local[from..sorry + 5];
        self.open_at(needle, needle.chars().count());
        for _ in 0..5 {
            self.key(Named::Backspace);
        }
        self.type_text(tool);
        let be = self.theory().block_editor.as_ref().map(|be| be.content.text());
        assert!(self.local().contains(&format!("\n  {tool}\n")), "{tool} typed in {lemma}: the open block is {be:?}");
        Instant::now()
    }

    /// What the window makes of the caret's place, for a failed assertion
    fn why(&mut self) -> String {
        self.draw("failed");
        let ws = self.app.workspace.as_ref().unwrap();
        let t = self.theory();
        format!(
            "report {:?}; caret command {:?} at {}; output command {}; settled {}; fill {:?} {:?} {:?} here {}",
            self.report().map(|r| r.outcome),
            t.caret_command(),
            t.cursor,
            ws.output.as_ref().map_or(json!(null), |o| o["command"].clone()),
            t.settled(),
            ws.fill.job,
            ws.fill.status,
            ws.fill.replace,
            super::view::fill_here(ws)
        )
    }

    /// Until what was typed has reached the host and the prover has checked it (the human's
    /// next move takes longer than that; a test's does not)
    async fn settle(&mut self) {
        self.pump(90, |a| a.workspace.as_ref().unwrap().theory.as_ref().is_some_and(|t| t.settled() && t.unchecked_from().is_none() && t.prover.commands.iter().all(|c| c.status != Status::Unprocessed))).await;
    }

    /// Until the block holding `needle` is checked and proves what it states
    async fn wait_proved(&mut self, needle: &str, secs: u64) -> bool {
        let needle = needle.to_string();
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if self.proved(&needle) {
                return true;
            }
            self.pump(1, |_| false).await;
        }
        self.proved(&needle)
    }

    /// The block holding `needle` is checked and proves what it states
    fn proved(&self, needle: &str) -> bool {
        let t = self.theory();
        let Some(at) = t.local.find(needle).map(|b| t.local[..b].chars().count()) else { return false };
        // (checked without an error and with no sorry left; a warning, such as simp's about a
        // rule it already has, still proves it)
        t.settled()
            && t.blocks.iter().find(|b| b.start <= at && at <= b.end).is_some_and(|b| matches!(b.status, Status::Finished | Status::Warned) && !t.source_text(b.start, b.end).contains("sorry"))
    }
}

fn secs(t: Instant) -> String {
    format!("{:.1} s", t.elapsed().as_secs_f32())
}

/// Until the tool at the caret offers something; when that was
async fn offers<S: cosmic::iced::futures::Stream<Item = (String, link::Event)>>(live: &mut Live<S>, secs_max: u64) -> bool {
    live.pump(secs_max, |a| !super::flow::offers_here(a.workspace.as_ref().unwrap()).is_empty()).await
}

const ISA: &str = "theory Flow\n  imports Main\nbegin\n\nlemma comm: \"(a::nat) + b = b + a\"\n  sorry\n\nlemma hard: \"\\<forall>n::nat. n * n \\<ge> n\"\n  sorry\n\nlemma direct: \"(a::nat) * b = b * a\"\n  sorry\n\nlemma comm2: \"(x::nat) + y + z = z + y + x\"\n  sorry\n\nlemma wrong: \"rev (xs @ ys) = rev xs @ rev (ys::nat list)\"\n  sorry\n\nlemma fermat: \"\\<lbrakk>n > 2; (x::nat) > 0; y > 0; z > 0\\<rbrakk> \\<Longrightarrow> x^n + y^n \\<noteq> z^n\"\n  sorry\n\nlemma ask: \"length (xs @ ys) = length ys + length (xs::nat list)\"\n  sorry\n\nend\n";

#[test]
#[ignore]
fn live_tools_isabelle() {
    // On this computer (`DEDUCTUM_LIVE_TOOLS_ISABELLE=1`, a host of its own under
    // `DEDUCTUM_LOCAL_ROOT`), or on another over ssh (`DEDUCTUM_LIVE`, `DEDUCTUM_REMOTE_ROOT`)
    let local = std::env::var("DEDUCTUM_LIVE_TOOLS_ISABELLE").ok().as_deref() == Some("1");
    let (dest, root) = if local {
        let root = format!("/tmp/ddi-{}", std::process::id());
        // (the backend's jar from the user's install when it was built from these sources: building
        // it again takes minutes; the install unpacks the sources around it)
        let _ = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!(
                "b=$HOME/.local/share/deductum/backend/Isabelle2025; s={}/backend/Isabelle2025; diff -rq $b/src $s/src >/dev/null && diff -q $b/etc/build.props $s/etc/build.props >/dev/null && mkdir -p {root}/host/backend/Isabelle2025/lib && cp $b/lib/deductum_backend.jar {root}/host/backend/Isabelle2025/lib/",
                env!("CARGO_MANIFEST_DIR")
            ))
            .status();
        // SAFETY: set before any thread of this test reads it
        unsafe { std::env::set_var("DEDUCTUM_LOCAL_ROOT", format!("{root}/host")) };
        (None, format!("{root}/host"))
    } else {
        let (Ok(dest), Ok(root)) = (std::env::var("DEDUCTUM_LIVE"), std::env::var("DEDUCTUM_REMOTE_ROOT")) else { return };
        (Some(dest), root)
    };
    let sh = |script: &str| match &dest {
        Some(d) => super::live::ssh(d, script),
        None => {
            let out = std::process::Command::new("bash").arg("-c").arg(script).output().expect("bash");
            String::from_utf8_lossy(&out.stdout).to_string()
        }
    };
    let folder = format!("{}/live-{}", if local { root.trim_end_matches("/host").to_string() } else { "/tmp/dd-flow-ui".to_string() }, std::process::id());
    sh(&format!("mkdir -p {folder} && cat > {folder}/Flow.thy <<'THY'\n{ISA}THY\n"));
    // (the test host registers its backend with Isabelle in place of any other: what was
    // registered is put back as it was afterwards)
    let components = sh("cat ~/.isabelle/Isabelle2025/etc/components 2>/dev/null");
    // The test host goes, and Isabelle's component list is as it was, however the test ends
    struct Teardown(Box<dyn Fn()>);
    impl Drop for Teardown {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    let _teardown = {
        let (dest, root, folder, components) = (dest.clone(), root.clone(), folder.clone(), components.clone());
        Teardown(Box::new(move || {
            let script = format!(
                "DEDUCTUM_STATE_DIR={root}/state python3 {root}/host/deductum_host.py stop-host; rm -rf {folder}{}; cat > ~/.isabelle/Isabelle2025/etc/components <<'COMPONENTS'\n{components}COMPONENTS\n",
                if local { format!(" {}", root.trim_end_matches("/host")) } else { String::new() }
            );
            match &dest {
                Some(d) => {
                    super::live::ssh(d, &script);
                }
                None => {
                    let _ = std::process::Command::new("bash").arg("-c").arg(&script).status();
                }
            }
        }))
    };
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: dest.clone() };
        let mut app = super::live::app(target.clone());
        app.show_right = false;
        app.show_sidebar = false;
        let mut live = Live::new(app, link::run(&target), "isabelle");
        assert!(live.pump(120, |a| a.links[0].sender.is_some()).await, "connected");
        let t0 = Instant::now();
        // (the runtime by name where another Isabelle is installed beside it: `DEDUCTUM_ISABELLE`)
        let mut create = json!({"folder": folder, "logic": "HOL", "new": true, "start": true});
        if let Ok(runtime) = std::env::var("DEDUCTUM_ISABELLE") {
            create["toolchain"] = json!(runtime);
        }
        live.app.send("live", "session_create", create, Pending::Create);
        let checked = |a: &App| {
            a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.prover.commands.iter().filter(|c| c.name == "sorry").count() == 7 && t.prover.commands.iter().all(|c| !matches!(c.status, Status::Unprocessed | Status::Running)))
        };
        assert!(live.pump(600, checked).await, "the theory is checked");
        println!("session and theory checked in {}", secs(t0));
        let _ = live.app.update(Message::SetMode(Mode::Literate));
        super::view::set_goal_beside(true);

        // (`DEDUCTUM_LIVE_FROM=n` starts at the n-th flow, when looking into one)
        let from: usize = std::env::var("DEDUCTUM_LIVE_FROM").ok().and_then(|n| n.parse().ok()).unwrap_or(1);
        // 1. sledgehammer typed where the sorry was: its section while it runs, its proofs, the
        // first taken with Alt+Enter, checked; one undo brings the tool back
        if from <= 1 {
            live.settle().await;
            let typed = live.type_tool("lemma comm", "sledgehammer");
            // (running, or already done: a fast machine finds this one before the state comes)
            let ok = live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_some_and(|r| r.running || !r.offers.is_empty())).await;
            assert!(ok, "sledgehammer runs: {}", live.why());
            let running = typed.elapsed();
            live.draw("sledgehammer-running");
            let ok = offers(&mut live, 90).await;
            assert!(ok, "sledgehammer finds proofs: {}", live.why());
            let found = typed.elapsed();
            live.draw("sledgehammer-found");
            let first = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code.clone();
            assert!(live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_some_and(|r| !r.running)).await, "sledgehammer finishes");
            let done = typed.elapsed();
            live.draw("sledgehammer-done");
            let first_now = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code.clone();
            assert_eq!(first, first_now, "the first proof stays first as more come");
            let pressed = Instant::now();
            live.alt(Named::Enter);
            let t = live.theory();
            let s = t.cursor - first.chars().count();
            assert!(t.local.contains(&format!("\n  {first}\n")) && !t.local.contains("sledgehammer"), "the proof replaced the tool: {}", t.local);
            assert_eq!(t.source_text(s, t.cursor), first, "the caret is just after the proof");
            assert!(live.wait_proved("lemma comm", 120).await, "the inserted proof is checked");
            println!("sledgehammer: running {:.1} s after typing, first proof at {:.1} s, done at {:.1} s; {first:?} inserted and checked {} after Alt+Enter", running.as_secs_f32(), found.as_secs_f32(), done.as_secs_f32(), secs(pressed));
            live.draw("sledgehammer-inserted");
            live.apply(Message::Undo);
            assert!(live.pump(30, |a| { let t = a.workspace.as_ref().unwrap().theory.as_ref().unwrap(); t.settled() && t.local.contains("\n  sledgehammer\n") }).await, "one undo brings sledgehammer back: {}", live.local());
            live.apply(Message::Redo);
            assert!(live.pump(30, |a| { let t = a.workspace.as_ref().unwrap().theory.as_ref().unwrap(); t.settled() && !t.local.contains("sledgehammer") }).await, "and redo the proof");
            // (the reply's Goto, which puts the caret where the redo acted, comes after the text)
            live.pump(1, |_| false).await;
            println!("undo: the insertion was one step");
            // (the caret after the proof again, as redo leaves it: typing goes on there)
            live.open_at(&format!("  {first}"), 2 + first.chars().count());
            live.type_text(" ");
            assert!(live.local().contains(&format!("{first} \n")), "typing goes on after the proof");
            live.key(Named::Backspace);
            assert!(live.pump(60, |a| a.workspace.as_ref().unwrap().theory.as_ref().is_some_and(|t| t.prover.commands.iter().all(|c| c.status != Status::Running))).await);
        }

        // 2. sledgehammer from the insert bar's palette, the second proof picked with Alt+Down
        if from <= 2 {
            live.settle().await;
            let local = live.local();
            let from = local.find("lemma hard").unwrap();
            let sorry = from + local[from..].find("sorry").unwrap();
            let needle = local[from..sorry + 5].to_string();
            live.open_at(&needle, needle.chars().count());
            for _ in 0..5 {
                live.key(Named::Backspace);
            }
            live.apply(Message::InsertBar(true));
            live.draw("bar-open");
            let mut clicked = false;
            'scan: for y in (60..900).step_by(5) {
                for x in (700..1300).step_by(8) {
                    let msgs = live.h.click(&live.app, cosmic::iced::Point::new(x as f32, y as f32));
                    if msgs.iter().any(|m| matches!(m, Message::Wrap(b, _) if b == "sledgehammer")) {
                        live.apply_all(msgs);
                        clicked = true;
                        break 'scan;
                    }
                }
            }
            assert!(clicked, "a sledgehammer chip in the palette");
            let typed = Instant::now();
            assert!(live.local().contains("\n  sledgehammer\n"), "the chip put sledgehammer where the sorry was: {}", live.local());
            let ok = offers(&mut live, 90).await;
            live.draw("bar-sledgehammer-wait");
            assert!(ok, "sledgehammer from the bar finds proofs: {:?} {:?} {:?}", live.report().map(|r| r.outcome), live.theory().caret_command(), live.theory().prover.commands.iter().map(|c| format!("{}:{:?}", c.name, c.status)).collect::<Vec<_>>());
            assert!(live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_some_and(|r| !r.running)).await);
            let offered = super::flow::offers_here(live.app.workspace.as_ref().unwrap());
            live.draw("bar-sledgehammer-found");
            let want = offered.get(1).unwrap_or(&offered[0]).code.clone();
            live.alt(Named::ArrowDown);
            live.draw("bar-picked-second");
            live.alt(Named::Enter);
            assert!(live.local().contains(&format!("\n  {want}\n")), "the picked proof went in: {}", live.local());
            assert!(live.wait_proved("lemma hard", 120).await, "and is checked");
            println!("insert bar: {} proofs in {}, the second ({want:?}) taken with Alt+Down, Alt+Enter and checked", offered.len(), secs(typed));
        }

        // 3. solve_direct: the fact it names, as a proof, checks
        if from <= 3 {
            live.settle().await;
            let typed = live.type_tool("lemma direct", "solve_direct");
            let ok = offers(&mut live, 60).await;
            assert!(ok, "solve_direct names a fact: {}", live.why());
            live.draw("solve-direct");
            let offer = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code.clone();
            live.alt(Named::Enter);
            assert!(live.wait_proved("lemma direct", 120).await, "{offer:?} checks");
            println!("solve_direct: {offer:?} offered {} after typing, inserted and checked", secs(typed));
        }

        // 4. try0 on comm2: its choice first, checked
        if from <= 4 {
            live.settle().await;
            let typed = live.type_tool("lemma comm2", "try0");
            let ok = offers(&mut live, 60).await;
            assert!(ok, "try0 finds a proof: {}", live.why());
            let took = secs(typed);
            live.draw("try0");
            let offer = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code.clone();
            live.alt(Named::Enter);
            assert!(live.wait_proved("lemma comm2", 120).await, "{offer:?} checks");
            println!("try0: {offer:?} in {took}, inserted and checked");
        }

        // 5. nitpick, then quickcheck, on a false statement: a counterexample as a table
        if from <= 5 {
            live.settle().await;
            let typed = live.type_tool("lemma wrong", "nitpick");
            assert!(live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Counterexample { .. }))).await, "nitpick's counterexample");
            let took = secs(typed);
            live.draw("nitpick-counterexample");
            if let Some(Outcome::Counterexample { sections, .. }) = live.report().map(|r| r.outcome) {
                let names: Vec<&str> = sections.iter().flat_map(|s| s.rows.iter().map(|(n, _)| n.as_str())).collect();
                assert!(names.contains(&"xs") && names.contains(&"ys"), "{sections:?}");
                println!("nitpick: counterexample in {took}: {:?}", sections[0].rows);
            }
            for _ in 0.."nitpick".len() {
                live.key(Named::Backspace);
            }
            live.type_text("quickcheck");
            let typed = Instant::now();
            assert!(live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Counterexample { ref sections, .. }) if sections.len() == 2)).await, "quickcheck's counterexample with its evaluated terms");
            println!("quickcheck: counterexample with evaluated terms in {}", secs(typed));
            live.draw("quickcheck-counterexample");
            for _ in 0.."quickcheck".len() {
                live.key(Named::Backspace);
            }
            live.type_text("sorry");
        }

        // 6. sledgehammer that finds nothing; Alt+Enter runs it again with more time; Esc stops it
        if from <= 6 {
            live.settle().await;
            let typed = live.type_tool("lemma fermat", "sledgehammer [timeout = 3]");
            let ok = live.pump(90, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::NothingFound(_)))).await;
            live.draw("nothing-found-wait");
            assert!(ok, "nothing found: {:?} {:?} {:?}", live.report(), live.theory().caret_command(), live.local().lines().filter(|l| l.contains("sledge")).collect::<Vec<_>>());
            println!("sledgehammer [timeout = 3]: nothing found in {}", secs(typed));
            live.draw("nothing-found");
            live.alt(Named::Enter);
            assert!(live.local().contains("sledgehammer [timeout = 60]"), "again with more time: {}", live.local());
            let ok = live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_some_and(|r| r.running)).await;
            assert!(ok, "running again: {}", live.why());
            live.draw("again-running");
            let pressed = Instant::now();
            live.key(Named::Escape);
            assert!(live.theory().block_editor.is_some(), "Esc stopped the tool and left the block open");
            let ok = live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Stopped))).await;
            live.draw("stopped-wait");
            assert!(ok, "stopped: {:?} {:?} {:?}", live.report(), live.app.workspace.as_ref().unwrap().output, live.theory().prover.commands.iter().map(|c| format!("{}:{:?}", c.name, c.status)).collect::<Vec<_>>());
            println!("Esc stopped it in {}", secs(pressed));
            live.draw("stopped");
            assert!(live.local().contains("sledgehammer [timeout = 60]"), "the line stays");
            // (a tool on its own is not a proof: the lemma after it would be in a bad context)
            live.apply(Message::ReplaceTool("sorry".into()));
        }

        // 7. a claude line, its checked proof taken with Alt+Enter
        if from <= 7 && std::env::var("DEDUCTUM_LIVE_LLM").ok().as_deref() == Some("1") {
            live.settle().await;
            // From a sledgehammer that found nothing: Alt+Down to "claude", Alt+Enter asks it at
            // once in the tool's place; Esc stops it
            live.type_tool("lemma fermat", "sledgehammer [timeout = 3]");
            assert!(live.pump(90, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::NothingFound(_)))).await, "nothing found again");
            live.alt(Named::ArrowDown);
            live.draw("ask-claude-picked");
            let pressed = Instant::now();
            live.alt(Named::Enter);
            assert!(live.local().contains("\n  claude\n"), "claude in the tool's place: {}", live.local());
            let ok = live.pump(60, |a| { let ws = a.workspace.as_ref().unwrap(); super::view::fill_here(ws) && matches!(ws.fill.status.as_str(), "asking" | "checking") }).await;
            assert!(ok, "claude asked at once: {}", live.why());
            println!("Alt+Down, Alt+Enter after nothing found: claude asked {} later", secs(pressed));
            live.draw("ask-claude-asking");
            live.key(Named::Escape);
            assert!(live.app.workspace.as_ref().unwrap().fill.job.is_none() && live.theory().block_editor.is_some(), "Esc stopped claude and left the block open");
            live.apply(Message::ReplaceTool("sorry".into()));
            live.settle().await;
            let typed = live.type_tool("lemma ask", "claude[haiku]");
            let ok = live.pump(240, |a| a.workspace.as_ref().unwrap().fill.candidates.iter().any(|c| c["ok"] == json!(true))).await;
            live.draw("claude");
            assert!(ok, "claude's checked proof: {:?}", live.app.workspace.as_ref().unwrap().fill.status);
            let offers = super::flow::offers_here(live.app.workspace.as_ref().unwrap());
            assert!(!offers.is_empty(), "claude's proofs offered: {}", live.why());
            let offer = offers[0].code.clone();
            println!("claude[haiku]: a checked proof {offer:?} in {}", secs(typed));
            live.alt(Named::Enter);
            assert!(live.wait_proved("lemma ask", 120).await, "{offer:?} checks");
            live.draw("claude-inserted");
        }

        let sid = live.app.workspace.as_ref().unwrap().sid.clone();
        live.app.send("live", "discard", json!({"session": sid, "force": true}), Pending::Plain);
        live.pump(5, |_| false).await;
    });
}

const LEAN: &str = "theorem add_swap (a b : Nat) : a + b = b + a := by\n  sorry\n\ntheorem len_app (xs ys : List Nat) : (xs ++ ys).length = xs.length + ys.length := by\n  sorry\n\ntheorem len_app' (xs ys : List Nat) : (xs ++ ys).length = xs.length + ys.length := by\n  sorry\n\ntheorem le_succ' (n : Nat) : n ≤ n + 1 := by\n  sorry\n\ntheorem bad_lt (n : Nat) : n < n + 0 := by\n  sorry\n\ntheorem two_lt_five : 2 < 5 := by\n  sorry\n\ntheorem ask_me (xs : List Nat) : xs.reverse.reverse = xs := by\n  sorry\n";

/// The test's own host and Lean servers go when it ends, however it ends
struct Cleanup(String);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let root = &self.0;
        let pattern = root.replacen("/tmp/", "/tm[p]/", 1);
        let _ = std::process::Command::new("sh").arg("-c").arg(format!("pkill -f '{pattern}/host/' ; sleep 1; rm -rf {root}")).status();
    }
}

#[test]
#[ignore]
fn live_tools_lean() {
    if std::env::var("DEDUCTUM_LIVE_TOOLS_LEAN").ok().as_deref() != Some("1") {
        return;
    }
    // (a short root: the host's socket path must fit)
    let root = format!("/tmp/ddt-{}", std::process::id());
    let project = format!("{root}/proj");
    let _cleanup = Cleanup(root.clone());
    let lakefile = "name = \"flow\"\ndefaultTargets = [\"Flow\"]\n\n[[lean_lib]]\nname = \"Flow\"\n";
    std::fs::create_dir_all(format!("{project}/Flow")).unwrap();
    std::fs::write(format!("{project}/lakefile.toml"), lakefile).unwrap();
    std::fs::write(format!("{project}/lean-toolchain"), "leanprover/lean4:v4.19.0\n").unwrap();
    std::fs::write(format!("{project}/Flow.lean"), "import Flow.Basic\n").unwrap();
    std::fs::write(format!("{project}/Flow/Basic.lean"), LEAN).unwrap();
    let built = std::process::Command::new("sh").arg("-c").arg(format!("cd {project} && lake build >/dev/null 2>&1")).status().unwrap();
    assert!(built.success(), "the scratch project builds");
    // SAFETY: set before any thread of this test reads it
    unsafe { std::env::set_var("DEDUCTUM_LOCAL_ROOT", format!("{root}/host")) };
    let file = format!("{project}/Flow/Basic.lean");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: None };
        let mut app = super::live::app(target.clone());
        app.show_right = false;
        app.show_sidebar = false;
        let mut live = Live::new(app, link::run(&target), "lean");
        assert!(live.pump(90, |a| a.links[0].sender.is_some()).await, "connects to its own host");
        let t0 = Instant::now();
        live.app.send("live", "session_create", json!({"folder": project, "backend": "lean", "new": true, "start": true}), Pending::Create);
        assert!(live.pump(90, |a| a.workspace.is_some()).await, "a session");
        live.apply(Message::OpenTheory(file.clone()));
        let checked = |a: &App| a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.prover.commands.len() >= 7 && t.prover.commands.iter().all(|c| !matches!(c.status, Status::Unprocessed | Status::Running)));
        assert!(live.pump(300, checked).await, "Basic.lean checked");
        println!("Lean session and file checked in {}", secs(t0));
        let _ = live.app.update(Message::SetMode(Mode::Literate));
        super::view::set_goal_beside(true);

        // Each suggestion tactic: typed, its Try this taken with Alt+Enter, checked
        for (theorem, tool) in [("add_swap", "exact?"), ("len_app ", "rw?"), ("len_app'", "simp?"), ("le_succ'", "apply?")] {
            let typed = live.type_tool(&format!("theorem {theorem}"), tool);
            let ok = offers(&mut live, 90).await;
            assert!(ok, "{tool} suggests something: {}", live.why());
            let took = secs(typed);
            let offer = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].clone();
            live.draw(&format!("{}-found", tool.trim_end_matches('?')));
            assert!(!offer.code.contains("--"), "Lean's comment is not part of it: {:?}", offer.code);
            let pressed = Instant::now();
            live.alt(Named::Enter);
            assert!(live.local().contains(&format!("\n  {}\n", offer.code)) && !live.local().contains(&format!("  {tool}\n")), "{tool} replaced: {}", live.local());
            assert!(live.pump(60, |a| a.workspace.as_ref().unwrap().theory.as_ref().unwrap().settled()).await);
            let name = format!("theorem {theorem}");
            assert!(live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_none() && {
                let t = a.workspace.as_ref().unwrap().theory.as_ref().unwrap();
                let at = t.local.find(&name).map(|b| t.local[..b].chars().count()).unwrap();
                t.blocks.iter().find(|b| b.start <= at && at <= b.end).is_some_and(|b| b.status == Status::Finished)
            }).await, "{:?} checks", offer.code);
            println!("{tool}: {:?} {took} after typing; inserted and checked {} after Alt+Enter", offer.code, secs(pressed));
        }
        live.draw("all-inserted");

        // exact? that cannot close its goal: the error, and apply? (as it suggests) run with Alt+Enter
        let typed = live.type_tool("theorem bad_lt", "exact?");
        assert!(live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Failed(_)))).await, "exact? fails");
        println!("exact? failed in {}", secs(typed));
        live.draw("exact-failed");
        assert_eq!(super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code, "apply?");
        live.alt(Named::Enter);
        assert!(live.local().contains("\n  apply?\n"), "apply? in its place");
        assert!(live.pump(60, |a| super::flow::report(a.workspace.as_ref().unwrap()).is_some_and(|r| !matches!(r.outcome, Outcome::Waiting(_)) && !r.running)).await, "apply? ran");
        println!("apply? after it: {:?}", live.report().map(|r| r.outcome));
        live.draw("apply-after-exact");

        // omega that cannot prove it: said plainly
        for _ in 0.."apply?".len() {
            live.key(Named::Backspace);
        }
        live.type_text("omega");
        assert!(live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Failed(_)))).await, "omega fails");
        live.draw("omega-failed");
        for _ in 0.."omega".len() {
            live.key(Named::Backspace);
        }
        live.type_text("sorry");

        // decide that closes its goal: nothing to insert, and said so
        live.type_tool("theorem two_lt_five", "decide");
        assert!(live.pump(60, |a| matches!(super::flow::report(a.workspace.as_ref().unwrap()).map(|r| r.outcome), Some(Outcome::Done(_)))).await, "decide closes it: {:?}", live.report().map(|r| r.outcome));
        live.draw("decide-done");

        if std::env::var("DEDUCTUM_LIVE_LLM").ok().as_deref() == Some("1") {
            let typed = live.type_tool("theorem ask_me", "claude[haiku]");
            let ok = live.pump(240, |a| a.workspace.as_ref().unwrap().fill.candidates.iter().any(|c| c["ok"] == json!(true))).await;
            live.draw("claude");
            assert!(ok, "claude's checked proof: {:?}", live.app.workspace.as_ref().unwrap().fill.status);
            let offer = super::flow::offers_here(live.app.workspace.as_ref().unwrap())[0].code.clone();
            println!("claude[haiku]: a checked proof {offer:?} in {}", secs(typed));
            live.alt(Named::Enter);
            assert!(live.local().contains(&offer) && !live.local().contains("claude"), "{}", live.local());
            live.draw("claude-inserted");
        }
        let sid = live.app.workspace.as_ref().unwrap().sid.clone();
        live.app.send("live", "stop", json!({"session": sid}), Pending::Plain);
        live.pump(3, |_| false).await;
    });
}

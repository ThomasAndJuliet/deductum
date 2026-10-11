// SPDX-License-Identifier: GPL-3.0-only

//! The app against a real session host: `ISABELLUM_LIVE=d59500 cargo test
//! --bin isabellum live -- --ignored --nocapture` drives the window's state
//! machine with the messages a real host over ssh sends, typing as the human
//! while an agent edits the same theory.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmic::Application;
use cosmic::iced::futures::StreamExt;
use cosmic::widget::text_editor::{Action, Edit};
use serde_json::json;

use super::editor::Mode;
use super::symbols::Table;
use super::{App, Link, Message, Pending, link};

const THY: &str = "theory Live\n  imports Main\nbegin\n\nlemma one: \"(a::nat) + b = b + a\"\n  sorry\n\nlemma two: \"\\<forall>x::nat. x + 0 = x\"\n  apply simp\n  done\n\nend\n";

fn app(target: link::Target) -> App {
    App {
        core: cosmic::app::Core::default(),
        links: vec![Link { target, name: "live".into(), sender: None, status: String::new(), hello: None, client_id: None, symbols: Arc::new(Table::fallback()) }],
        ctrl: false,
        alt: false,
        back: Vec::new(),
        references: Vec::new(),
        entity: None,
        hover_gen: 0,
        keyword_gen: 0,
        prose_pointer: None,
        vim_block: false,
        goal_placement_override: None,
        sessions: BTreeMap::new(),
        next_id: 0,
        pending: HashMap::new(),
        workspace: None,
        new_session: None,
        show_sidebar: true,
        status: None,
        state_gen: 0,
        request: None,
        kinds: Arc::new(HashMap::new()),
        popout: None,
        want_theory: None,
        left: super::LeftMode::Sessions,
        show_right: true,
        left_width: 290.0,
        right_width: 420.0,
        dragging: None,
        window_width: 1400.0,
        vim: super::vim::Vim::default(),
        vim_enabled: false,
        mode_model: cosmic::widget::segmented_button::SingleSelectModel::default(),
        mode_entities: [cosmic::widget::segmented_button::Entity::default(); 2],
    }
}

/// The test sessions off the host again (stopped, then discarded once stopped), and their folders
fn cleanup(dest: &str) {
    ssh(dest, r#"cd ~/.local/share/isabellum/host && python3 - <<'PY'
import time, isabellum_host as H
c = H.SyncClient.local(); c.request("hello", client="cli")
mine = [s for s in c.request("sessions")["sessions"] if s["folder"].startswith("/tmp/isabellum-live")]
for s in mine:
    if s["lifecycle"] not in ("Stopped", "Failed"):
        c.request("stop", session=s["id"])
for _ in range(60):
    if all(x["lifecycle"] in ("Stopped", "Failed") for x in c.request("sessions")["sessions"] if x["folder"].startswith("/tmp/isabellum-live")):
        break
    time.sleep(0.5)
for s in mine:
    try:
        c.request("discard", session=s["id"])
    except H.HostError:
        pass
PY
rm -rf /tmp/isabellum-live-*
"#);
}

fn ssh(dest: &str, script: &str) -> String {
    let out = std::process::Command::new("ssh").args(["-o", "BatchMode=yes", dest, "/bin/bash", "-s"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut c| {
        use std::io::Write;
        c.stdin.take().unwrap().write_all(script.as_bytes())?;
        c.wait_with_output()
    });
    String::from_utf8_lossy(&out.expect("ssh").stdout).to_string()
}

#[test]
#[ignore]
fn live_session() {
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let folder = format!("/tmp/isabellum-live-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && cat > {folder}/Live.thy <<'THY'\n{THY}THY\n"));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        let mut stream = Box::pin(link::run(&target));
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut step = 0;
        let mut typed = false;
        let mut agent_done = false;
        let mut state_asked = false;
        let mut hammer = false;
        let path = format!("{folder}/Live.thy");
        while Instant::now() < deadline {
            let Ok(Some((key, event))) = tokio::time::timeout(Duration::from_millis(500), stream.next()).await else {
                // nothing arrived: carry on with the script
                if step == 0 && app.links[0].sender.is_some() {
                    app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
                    step = 1;
                }
                continue;
            };
            let _ = app.update(Message::Link(key, event));
            let ws_theory = app.workspace.as_ref().and_then(|w| w.theory.as_ref());
            if step == 1 && ws_theory.is_some() {
                println!("theory open, revision {}", ws_theory.unwrap().confirmed_rev);
                step = 2;
            }
            if step == 2 {
                let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
                if !t.prover.commands.is_empty() && !typed {
                    println!("prover sent {} commands; switching to source and typing", t.prover.commands.len());
                    let _ = app.update(Message::SetMode(Mode::Source));
                    // Put the caret at the end of line 5 and type a comment
                    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
                    let line5_end = t.decoded.text.split('\n').take(5).map(|l| l.chars().count() + 1).sum::<usize>() - 1;
                    let pos = t.lines.position(&t.decoded.text.clone(), line5_end);
                    super::editor::place(&mut t.source, pos);
                    for c in " (* typed *)".chars() {
                        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert(c))));
                    }
                    typed = true;
                    // Meanwhile an agent with rights replaces the sorry
                    let sid = app.workspace.as_ref().unwrap().sid.clone();
                    let out = ssh(&dest, &format!(r#"cd ~/.local/share/isabellum/host && python3 - <<'PY'
import isabellum_host as H
app = H.SyncClient.local(); app.request("hello", client="app")
app.request("grant", session="{sid}", agent="claude:live", label="Claude (live)", scope="folder")
a = H.SyncClient.local(); a.request("hello", client="agent", label="Claude (live)", identity={{"provider":"claude","conversation":"live"}})
d = a.request("read", session="{sid}", path="{path}")["doc"]
t = d["text"]; i = t.index("sorry")
r = a.request("edit", session="{sid}", path="{path}", revision=0, edits=[{{"start": i, "end": i+5, "text": "by (simp add: add.commute)"}}])
print("agent edit at revision", r["revision"])
PY"#));
                    println!("{}", out.trim());
                    agent_done = true;
                }
                let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
                if agent_done && t.inflight.is_none() && t.local == t.confirmed && t.local.contains("(* typed *)") && t.local.contains("add.commute") && !state_asked {
                    println!("replica converged at revision {}:\n{}", t.confirmed_rev, t.local);
                    // Ask for the goal in lemma two, after apply simp
                    let off = t.local.find("apply simp").unwrap() + 2;
                    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
                    t.cursor = off;
                    let generation = app.state_gen + 1;
                    app.state_gen = generation;
                    let _ = app.update(Message::StateTick(generation));
                    state_asked = true;
                }
                if state_asked && !hammer && app.workspace.as_ref().unwrap().output.is_some() {
                    println!("output: {}", app.workspace.as_ref().unwrap().output.as_ref().unwrap());
                    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
                    let off = t.local.find("done").unwrap();
                    let _ = app.update(Message::SledgehammerAt(off));
                    hammer = true;
                }
                if hammer {
                    let q = &app.workspace.as_ref().unwrap().sledgehammer;
                    if q.status == "finished" || q.status == "failed" {
                        println!("sledgehammer {}: {} messages, error {:?}", q.status, q.messages.len(), q.error);
                        break;
                    }
                }
            }
        }
        let t = app.workspace.as_ref().and_then(|w| w.theory.as_ref()).expect("a theory");
        assert!(t.local.contains("(* typed *)") && t.local.contains("add.commute"));
        assert_eq!(t.local, t.confirmed);
        let out = app.workspace.as_ref().unwrap().output.clone().expect("a proof state");
        assert!(out.to_string().contains("goal") || out.to_string().contains("subgoal"), "{out}");
        assert!(hammer);
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        app.send("live", "stop", json!({"session": sid}), Pending::Plain);
        tokio::time::sleep(Duration::from_secs(1)).await;
        // Autosave wrote both edits
        let disk = ssh(&dest, &format!("sleep 2; cat {path}"));
        assert!(disk.contains("(* typed *)") && disk.contains("add.commute"), "{disk}");
    });
}

/// The Types box: the parts of the state at a goal, without and then with types
#[test]
#[ignore]
fn live_types() {
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let folder = format!("/tmp/isabellum-live-types-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && cat > {folder}/Live.thy <<'THY'\n{THY}THY\n"));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        let mut stream = Box::pin(link::run(&target));
        let deadline = Instant::now() + Duration::from_secs(400);
        let mut step = 0;
        let mut seen: Vec<String> = Vec::new();
        while Instant::now() < deadline && seen.len() < 2 {
            let Ok(Some((key, event))) = tokio::time::timeout(Duration::from_millis(500), stream.next()).await else {
                if step == 0 && app.links[0].sender.is_some() {
                    app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
                    step = 1;
                }
                continue;
            };
            let _ = app.update(Message::Link(key, event));
            let Some(ws) = app.workspace.as_ref() else { continue };
            let Some(t) = ws.theory.as_ref() else { continue };
            let checked = t.prover.commands.iter().any(|c| c.name == "sorry") && t.prover.commands.iter().all(|c| c.status != super::theory::Status::Unprocessed && c.status != super::theory::Status::Running);
            if step == 1 && checked {
                let off = t.local.find("sorry").unwrap() + 1;
                let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
                t.cursor = off;
                app.state_gen += 1;
                let _ = app.update(Message::StateTick(app.state_gen));
                step = 2;
            }
            let ws = app.workspace.as_ref().unwrap();
            if (step == 2 || step == 3) && ws.goals.status == "finished" && ws.goals.id.is_some() {
                let parts: Vec<String> = super::goals::Goals::parse(&ws.goals.messages).parts().iter().map(|p| p["text"].as_str().unwrap_or("").to_string()).collect();
                println!("types={} parts={parts:?}", ws.show_types);
                seen.push(parts.join(" | "));
                if step == 2 {
                    let _ = app.update(Message::ShowTypes(true));
                    let ws = app.workspace.as_mut().unwrap();
                    ws.goals.status = String::new();
                    app.state_gen += 1;
                    let _ = app.update(Message::StateTick(app.state_gen));
                    step = 3;
                }
            }
        }
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        let _ = sid;
        cleanup(&dest);
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_ne!(seen[0], seen[1]);
        assert!(seen[1].contains("::nat"), "{}", seen[1]);
    });
}

/// Pump the host's events into the app until `done` holds
async fn pump(
    app: &mut App,
    stream: &mut std::pin::Pin<Box<impl cosmic::iced::futures::Stream<Item = (String, link::Event)>>>,
    secs: u64,
    mut done: impl FnMut(&App) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if done(app) {
            return true;
        }
        if let Ok(Some((key, event))) = tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            let _ = app.update(Message::Link(key, event));
        }
    }
    done(app)
}

fn settled(app: &App) -> bool {
    app.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.settled() && t.queued.is_empty())
}

/// The literate view end to end against a real host: add a lemma and type its statement, see
/// its goal in parts, add text, undo a word, delete a block and undo that
#[test]
#[ignore]
fn live_literate() {
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let folder = format!("/tmp/isabellum-live-lit-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && cat > {folder}/Live.thy <<'THY'\n{THY}THY\n"));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 60, |a| a.links[0].sender.is_some()).await, "connects");
        app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
        let checked = |a: &App| {
            a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| {
                t.prover.commands.iter().any(|c| c.name == "sorry")
                    && t.prover.commands.iter().all(|c| !matches!(c.status, super::theory::Status::Unprocessed | super::theory::Status::Running))
            })
        };
        assert!(pump(&mut app, &mut stream, 300, checked).await, "theory checked");
        let _ = app.update(Message::SetMode(Mode::Literate));

        // A new lemma at the end, its statement typed between the quotes
        let _ = app.update(Message::InsertBlock(None, "lemma"));
        for c in "(n::nat) + 0 = n".chars() {
            let _ = app.update(Message::BlockAction(Action::Edit(Edit::Insert(c))));
        }
        let _ = app.update(Message::CloseBlock);
        assert!(pump(&mut app, &mut stream, 30, settled).await, "edits reach the host");
        let local = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
        assert!(local.contains("lemma \"(n::nat) + 0 = n\"\n  sorry"), "{local}");
        println!("lemma added");

        // Its goal, in parts, at its sorry (once the prover has the new lemma)
        let rechecked = |a: &App| {
            checked(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().prover.commands.iter().filter(|c| c.name == "sorry").count() == 2
        };
        assert!(pump(&mut app, &mut stream, 120, rechecked).await, "rechecked");
        let off = local.rfind("sorry").unwrap() + 1;
        app.workspace.as_mut().unwrap().theory.as_mut().unwrap().cursor = off;
        app.state_gen += 1;
        let _ = app.update(Message::StateTick(app.state_gen));
        assert!(pump(&mut app, &mut stream, 60, |a| a.workspace.as_ref().unwrap().goals.status == "finished").await, "goals");
        let g = super::goals::Goals::parse(&app.workspace.as_ref().unwrap().goals.messages);
        let concl = g.subgoals.first().and_then(|s| s.concl.as_ref()).and_then(|c| c["text"].as_str()).unwrap_or("").to_string();
        assert_eq!(concl, "n + 0 = n", "{g:?}");
        println!("goal: {concl}");

        // Text, typed as two words; one undo takes back the last word only
        let _ = app.update(Message::InsertBlock(None, "text"));
        for c in "hello world".chars() {
            let _ = app.update(Message::BlockAction(Action::Edit(Edit::Insert(c))));
        }
        let _ = app.update(Message::CloseBlock);
        assert!(pump(&mut app, &mut stream, 30, settled).await, "text reaches the host");
        let _ = app.update(Message::Undo);
        let undone = |a: &App| settled(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("\\<open>hello \\<close>");
        let ok = pump(&mut app, &mut stream, 30, undone).await;
        let local = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.clone();
        assert!(ok, "one undo takes back one word: {local}");
        println!("undo took back one word ({:?})", app.status);

        // Delete that block, then bring it back
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().position(|b| b.kind == super::theory::Kind::Text && t.source_text(b.start, b.end).contains("hello")).expect("the text block");
        let _ = app.update(Message::DeleteBlock(i));
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && !a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("hello")).await, "deleted");
        let _ = app.update(Message::Undo);
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("hello")).await, "undo restores it");
        println!("block deleted and restored");

        // The file on disk has it all (autosave)
        tokio::time::sleep(Duration::from_secs(3)).await;
        let disk = ssh(&dest, &format!("cat {folder}/Live.thy"));
        assert!(disk.contains("(n::nat) + 0 = n") && disk.contains("hello"), "{disk}");
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        let _ = sid;
        cleanup(&dest);
    });
}

/// The window itself against a real host: the real view drawn, and the mouse and keyboard going
/// through its widgets as they do in the app. Click a lemma (it opens with the caret at the
/// click), type into it, close it, hover a name (the prover says what it is), and vim's cursor
#[test]
#[ignore]
fn live_ui() {
    use super::snapshot::Harness;
    use cosmic::iced::keyboard::{Key, key::Named};
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let folder = format!("/tmp/isabellum-live-ui-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && cat > {folder}/Live.thy <<'THY'\n{THY}THY\n"));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        app.show_right = false;
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 60, |a| a.links[0].sender.is_some()).await);
        app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
        let checked = |a: &App| {
            a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| {
                t.prover.commands.iter().any(|c| c.name == "sorry")
                    && t.prover.commands.iter().all(|c| !matches!(c.status, super::theory::Status::Unprocessed | super::theory::Status::Running))
            })
        };
        assert!(pump(&mut app, &mut stream, 300, checked).await, "checked");
        let _ = app.update(Message::SetMode(Mode::Literate));
        let mut h = Harness::new(cosmic::iced::Size::new(1400.0, 900.0));
        h.draw(&app, "ui-1-theory");

        // The pointer down the page until it is over a shown lemma
        let mut found = None;
        for y in (60..880).step_by(12) {
            let p = cosmic::iced::Point::new(470.0, y as f32);
            let msgs = h.move_to(&app, p);
            for m in &msgs {
                let _ = app.update(m.clone());
            }
            if msgs.iter().any(|m| matches!(m, Message::HoverMove(super::HoverTarget::View(_), _))) {
                found = Some(p);
                break;
            }
        }
        let p = found.expect("a lemma on the page");
        let msgs = h.click(&app, p);
        assert!(msgs.iter().any(|m| matches!(m, Message::ViewPress(_))), "{msgs:?}");
        for m in msgs {
            let _ = app.update(m);
        }
        h.focus(&app, super::view::block_editor_id());
        h.draw(&app, "ui-2-lemma-open");
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let be = t.block_editor.as_ref().expect("the lemma opened");
        println!("opened at {:?} of {:?}", be.content.cursor().position, be.content.text().lines().next());

        // Typing through the editor widget
        let _ = h.key(&app, Key::Named(Named::End), None).into_iter().map(|m| app.update(m)).count();
        for m in h.type_text(&app, " (* typed in the window *)") {
            let _ = app.update(m);
        }
        h.draw(&app, "ui-3-typed");
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("(* typed in the window *)")).await, "typing reached the host");
        println!("typing reached the host");

        // Click outside: the block closes
        for m in h.click(&app, cosmic::iced::Point::new(330.0, 880.0)) {
            let _ = app.update(m);
        }
        assert!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.is_none(), "closed by a click outside");

        // Hover over the shown lemma: the prover's tooltip
        assert!(pump(&mut app, &mut stream, 120, checked).await, "rechecked");
        h.draw(&app, "ui-4-closed");
        let mut hovered = false;
        for dx in (0..400).step_by(16) {
            let q = cosmic::iced::Point::new(p.x - 100.0 + dx as f32, p.y);
            for m in h.move_to(&app, q) {
                let _ = app.update(m);
            }
            let generation = app.hover_gen;
            let _ = app.update(Message::HoverTick(generation));
            let got = pump(&mut app, &mut stream, 5, |a| a.workspace.as_ref().unwrap().hover.as_ref().is_some_and(|h| h.info.is_some())).await;
            let items = app.workspace.as_ref().unwrap().hover.as_ref().and_then(|h| h.info.as_ref()).map(|i| i["tooltip"]["items"].as_array().map_or(0, |a| a.len())).unwrap_or(0);
            if got && items > 0 {
                h.draw(&app, "ui-5-hover");
                println!("hover: {}", app.workspace.as_ref().unwrap().hover.as_ref().unwrap().info.as_ref().unwrap()["tooltip"]["items"]);
                hovered = true;
                break;
            }
        }
        assert!(hovered, "a tooltip somewhere along the lemma");

        // Vim keys: the lemma opened in normal mode, a block cursor
        let _ = app.update(Message::ToggleVim);
        // (the pointer leaves the name first: its tooltip goes)
        for m in h.move_to(&app, cosmic::iced::Point::new(330.0, 880.0)) {
            let _ = app.update(m);
        }
        let msgs = h.click(&app, p);
        assert!(msgs.iter().any(|m| matches!(m, Message::ViewPress(_))), "{msgs:?}");
        for m in msgs {
            let _ = app.update(m);
        }
        h.focus(&app, super::view::block_editor_id());
        // (vim starts in normal mode; Escape there would leave the block)
        for m in h.type_text(&app, "0l") {
            let _ = app.update(m);
        }
        h.draw(&app, "ui-6-vim-block-cursor");
        let be = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().block_editor.as_ref().expect("open");
        let c = be.content.cursor();
        println!("vim cursor {c:?}");
        assert!(c.selection.is_some_and(|s| s.line == c.position.line && s.column + 1 == c.position.column), "a one-character block cursor");

        let sid = app.workspace.as_ref().unwrap().sid.clone();
        let _ = sid;
        cleanup(&dest);
    });
}

/// The real development, in the window: a copy of tock-Circus on UTP-Reactive (as the human's
/// session runs it). Open a theory, click into a lemma and type, undo a word, hover a constant,
/// Ctrl+click through to its definition and back, type sledgehammer in a proof and stop it
#[test]
#[ignore]
fn live_tock() {
    use super::snapshot::Harness;
    use cosmic::iced::keyboard::{Key, key::Named};
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let home = std::env::var("HOME").unwrap();
    let folder = format!("/home/au759518/.cache/cosmic-term/mounts/neo{home}/tmp-isabellum-tock");
    let thy = format!("{folder}/Timed_Circus_Assignment.thy");
    // A fresh copy of the human's project each run (its theories and ROOT; theirs stay untouched)
    let src = format!("{home}/worktrees/timed-circus-utp2-handover-20261002/project/UTP-Tock-Circus");
    let copy = format!("{home}/tmp-isabellum-tock");
    let _ = std::process::Command::new("sh").arg("-c").arg(format!("rm -rf {copy} && mkdir -p {copy} && cp {src}/*.thy {src}/ROOT {copy}/")).status();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        app.show_right = false;
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 60, |a| a.links[0].sender.is_some()).await);
        let t0 = Instant::now();
        app.send("live", "session_create", json!({"folder": folder, "logic": "UTP-Reactive", "dirs": [folder], "include_sessions": ["UTP-Tock-Circus"], "new": true, "start": true}), Pending::Create);
        assert!(pump(&mut app, &mut stream, 60, |a| a.workspace.is_some()).await, "session");
        let _ = app.update(Message::OpenTheory(thy.clone()));
        let checked = |a: &App| {
            a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| {
                t.prover.commands.len() > 20
                    && t.prover.commands.iter().all(|c| !matches!(c.status, super::theory::Status::Unprocessed | super::theory::Status::Running))
            })
        };
        assert!(pump(&mut app, &mut stream, 1200, checked).await, "checked");
        println!("theory checked in {} s ({} commands)", t0.elapsed().as_secs(), app.workspace.as_ref().unwrap().theory.as_ref().unwrap().prover.commands.len());
        let _ = app.update(Message::SetMode(Mode::Literate));
        let mut h = Harness::new(cosmic::iced::Size::new(1600.0, 1000.0));
        h.draw(&app, "tock-1-literate");

        // A lemma under the pointer, clicked, typed into, a word undone
        let mut found = None;
        for y in (80..980).step_by(10) {
            let p = cosmic::iced::Point::new(560.0, y as f32);
            let msgs = h.move_to(&app, p);
            for m in &msgs {
                let _ = app.update(m.clone());
            }
            if let Some(Message::HoverMove(super::HoverTarget::View(i), _)) = msgs.iter().find(|m| matches!(m, Message::HoverMove(super::HoverTarget::View(_), _))) {
                let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
                if t.blocks[*i].name == "lemma" {
                    found = Some(p);
                    break;
                }
            }
        }
        let p = found.expect("a lemma on the page");
        for m in h.click(&app, p) {
            let _ = app.update(m);
        }
        h.focus(&app, super::view::block_editor_id());
        let _ = h.key(&app, Key::Named(Named::End), None).into_iter().map(|m| app.update(m)).count();
        for m in h.type_text(&app, " (* tried *)") {
            let _ = app.update(m);
        }
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("(* tried *)")).await, "typed");
        h.draw(&app, "tock-2-typed");
        let _ = app.update(Message::Undo);
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("(* tried ")).await, "undo kept the first word");
        let _ = app.update(Message::Undo);
        let _ = app.update(Message::Undo);
        assert!(pump(&mut app, &mut stream, 30, |a| settled(a) && !a.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("(* tried")).await, "undo took it all back");
        println!("typed and undone ({:?})", app.status);

        // Hover a constant in the open lemma; Ctrl+click it to its definition, then Back
        assert!(pump(&mut app, &mut stream, 300, checked).await, "rechecked");
        let mut defined = None;
        'scan: for row in 0..8 {
        for dx in (0..520).step_by(16) {
            let q = cosmic::iced::Point::new(p.x - 180.0 + dx as f32, p.y + row as f32 * 17.0);
            for m in h.move_to(&app, q) {
                let _ = app.update(m);
            }
            let generation = app.hover_gen;
            let _ = app.update(Message::HoverTick(generation));
            if pump(&mut app, &mut stream, 4, |a| a.workspace.as_ref().unwrap().hover.as_ref().is_some_and(|h| h.info.is_some())).await {
                let info = app.workspace.as_ref().unwrap().hover.as_ref().unwrap().info.clone().unwrap();
                if std::env::var_os("TOCK_DEBUG").is_some() {
                    println!("  at {q:?} {:?}: {}", app.workspace.as_ref().unwrap().hover.as_ref().map(|h| (h.target, h.offset)), info.to_string().chars().take(200).collect::<String>());
                }
                if info["definition"]["file"].as_str().is_some_and(|f| f.ends_with(".thy")) && info["definition"]["kind"] == "constant" {
                    h.draw(&app, "tock-3-hover");
                    println!("hover: {} -> {}", info["tooltip"]["items"][0]["text"], info["definition"]["name"]);
                    defined = Some((q, info["definition"].clone()));
                    break 'scan;
                }
            }
        }
        }
        let (q, def) = defined.expect("a constant with a definition");
        app.ctrl = true;
        for m in h.click(&app, q) {
            let _ = app.update(m);
        }
        app.ctrl = false;
        let want = def["file"].as_str().unwrap().to_string();
        assert!(pump(&mut app, &mut stream, 60, |a| a.workspace.as_ref().unwrap().theory.as_ref().is_some_and(|t| t.path.ends_with(want.rsplit('/').next().unwrap()))).await, "went to {want}");
        h.draw(&app, "tock-4-definition");
        println!("went to the definition in {}", want.rsplit('/').next().unwrap());
        let _ = app.update(Message::Back);
        assert!(pump(&mut app, &mut stream, 60, |a| a.workspace.as_ref().unwrap().theory.as_ref().is_some_and(|t| t.path == thy)).await, "back");
        println!("and back");

        // sledgehammer typed in a proof: its progress beside the goal, then Stop
        let _ = app.update(Message::SetMode(Mode::Source));
        let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
        let at = t.local.find("  by ").expect("a by step");
        let line = t.decoded.text[..t.decoded.to_decoded(at)].matches('\n').count();
        super::editor::place(&mut t.source, cosmic::widget::text_editor::Position { line, column: 0 });
        t.cursor = at;
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Paste(Arc::new("  sledgehammer\n".into())))));
        let running = |a: &App| a.workspace.as_ref().unwrap().theory.as_ref().unwrap().prover.commands.iter().any(|c| c.name == "sledgehammer" && c.status == super::theory::Status::Running);
        assert!(pump(&mut app, &mut stream, 300, running).await, "sledgehammer running");
        app.state_gen += 1;
        let _ = app.update(Message::StateTick(app.state_gen));
        let _ = pump(&mut app, &mut stream, 5, |_| false).await;
        h.draw(&app, "tock-5-sledgehammer");
        let _ = app.update(Message::StopTools);
        let stopped = |a: &App| a.workspace.as_ref().unwrap().theory.as_ref().unwrap().prover.commands.iter().any(|c| c.name == "sledgehammer" && c.status != super::theory::Status::Running);
        assert!(pump(&mut app, &mut stream, 60, stopped).await, "stopped");
        for _ in 0..4 {
            let _ = pump(&mut app, &mut stream, 3, |_| false).await;
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let b = t.blocks.iter().find(|b| b.commands.iter().any(|c| c.name == "sledgehammer")).unwrap();
            let cmds: Vec<String> = t.prover.commands.iter().filter(|c| c.start >= b.start && c.end <= b.end).map(|c| format!("{}:{:?}", c.name, c.status)).collect();
            println!("sledgehammer stopped; its proof shows {:?} {cmds:?}", t.shown_status(b.start, b.end, b.status));
        }
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let b = t.blocks.iter().find(|b| b.commands.iter().any(|c| c.name == "sledgehammer")).unwrap();
        assert_eq!(t.shown_status(b.start, b.end, b.status), super::theory::Status::Warned, "a stopped sledgehammer leaves the proof unfinished, not failed");
        h.draw(&app, "tock-6-stopped");
        cleanup_folder(&dest, "tmp-isabellum-tock");
    });
}

/// A test session on a folder named so, off the host
fn cleanup_folder(dest: &str, needle: &str) {
    ssh(dest, &format!(r#"cd ~/.local/share/isabellum/host && python3 - <<'PY'
import time, isabellum_host as H
c = H.SyncClient.local(); c.request("hello", client="cli")
mine = [s for s in c.request("sessions")["sessions"] if "{needle}" in s["folder"]]
for s in mine:
    if s["lifecycle"] not in ("Stopped", "Failed"):
        c.request("stop", session=s["id"])
for _ in range(60):
    if all(x["lifecycle"] in ("Stopped", "Failed") for x in c.request("sessions")["sessions"] if "{needle}" in x["folder"]):
        break
    time.sleep(0.5)
for s in mine:
    try:
        c.request("discard", session=s["id"], force=True)
    except H.HostError:
        pass
PY
"#));
}

/// Where in the window a click produces a message `want` picks out (scanning a grid)
fn find_click(h: &mut super::snapshot::Harness, app: &App, want: impl Fn(&Message) -> bool) -> Option<(cosmic::iced::Point, Message)> {
    for y in (40..h.size.height as i32 - 20).step_by(10) {
        for x in (300..h.size.width as i32 - 20).step_by(24) {
            let p = cosmic::iced::Point::new(x as f32, y as f32);
            if let Some(m) = h.click(app, p).into_iter().find(|m| want(m)) {
                return Some((p, m));
            }
        }
    }
    None
}

/// An agent working with the human, in the window: it points the human at a lemma (a banner
/// with Go there) and leaves a note on another (a card under it); the human goes there, replies
/// to the note and resolves it, and the agent hears back
#[test]
#[ignore]
fn live_collaboration() {
    use super::snapshot::Harness;
    let Ok(dest) = std::env::var("ISABELLUM_LIVE") else { return };
    let folder = format!("/tmp/isabellum-live-collab-{}", std::process::id());
    let path = format!("{folder}/Live.thy");
    ssh(&dest, &format!("mkdir -p {folder} && cat > {path} <<'THY'\n{THY}THY\n"));
    let agent = |code: &str| {
        ssh(&dest, &format!(r#"cd ~/.local/share/isabellum/host && python3 - <<'PY'
import json, isabellum_host as H
a = H.SyncClient.local(); a.request("hello", client="agent", label="Claude (collab test)", identity={{"provider": "claude", "conversation": "collab-test"}})
{code}
PY
"#))
    };
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        app.show_right = false;
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 60, |a| a.links[0].sender.is_some()).await);
        app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
        assert!(pump(&mut app, &mut stream, 300, |a| a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.prover.commands.iter().any(|c| c.name == "sorry"))).await, "open");
        let _ = app.update(Message::SetMode(Mode::Literate));
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        let mut h = Harness::new(cosmic::iced::Size::new(1400.0, 900.0));
        h.draw(&app, "collab-0");

        // The agent points at lemma two and leaves a note on lemma one
        let out = agent(&format!(r#"t = a.request("read", session="{sid}", path="{path}")["doc"]["text"]
two = t.index("lemma two"); one = t.index("lemma one")
a.request("show", session="{sid}", path="{path}", start=two, end=two + 9, message="this one is already done")
print(json.dumps(a.request("add_note", session="{sid}", path="{path}", start=one, end=one + 9, text="add.commute proves this")["note"]["id"]))"#));
        let note: String = serde_json::from_str(out.trim().lines().last().unwrap_or("\"\"")).unwrap_or_default();
        assert!(pump(&mut app, &mut stream, 20, |a| a.workspace.as_ref().unwrap().show_offer.is_some() && !a.workspace.as_ref().unwrap().notes.is_empty()).await, "banner and note arrive");
        h.draw(&app, "collab-1-banner-and-note");

        // Go there: the lemma it pointed at is selected
        let (_, go) = find_click(&mut h, &app, |m| matches!(m, Message::ShowOffer(true))).expect("a Go there button");
        let _ = app.update(go);
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let sel = t.selected.map(|i| t.source_text(t.blocks[i].start, t.blocks[i].end)).unwrap_or_default();
        assert!(sel.starts_with("lemma two"), "went to {sel:?}");
        assert!(app.workspace.as_ref().unwrap().show_offer.is_none());
        println!("went where the agent pointed");

        // Reply to the note, then resolve it, by its buttons
        let _ = app.update(Message::NoteReplyInput(note.clone(), "thanks, will do".into()));
        let (_, reply) = find_click(&mut h, &app, |m| matches!(m, Message::NoteReply(_, false))).expect("a Reply button");
        let _ = app.update(reply);
        assert!(pump(&mut app, &mut stream, 10, |a| a.workspace.as_ref().unwrap().notes.iter().any(|n| n["replies"].as_array().is_some_and(|r| !r.is_empty()))).await, "the reply shows on the note");
        h.draw(&app, "collab-2-replied");
        let (_, resolve) = find_click(&mut h, &app, |m| matches!(m, Message::NoteReply(_, true))).expect("a Resolve button");
        let _ = app.update(resolve);
        assert!(pump(&mut app, &mut stream, 10, |a| a.workspace.as_ref().unwrap().notes.is_empty()).await, "resolved: the card goes");
        let heard = agent(r#"print(json.dumps([n["kind"] + ": " + (n.get("reply") or "") for n in a.request("notices")["notices"]]))"#);
        println!("the agent heard: {}", heard.trim());
        assert!(heard.contains("note_reply: thanks, will do") && heard.contains("note_resolved"), "{heard}");
        cleanup(&dest);
    });
}

/// Reference windows against a real host of their own (`ISABELLUM_REMOTE_ROOT`, so the user's
/// host and sessions are not touched): Alt+click `rev` opens HOL's List.thy at its definition,
/// as a library theory; hovering a name in it shows what it is; Ctrl+click moves the window and
/// Alt+click opens another
#[test]
#[ignore]
fn live_reference() {
    let (Ok(dest), Ok(root)) = (std::env::var("ISABELLUM_LIVE"), std::env::var("ISABELLUM_REMOTE_ROOT")) else { return };
    let folder = format!("/tmp/isabellum-ref-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && printf 'theory Ref imports Main begin\\n\\nlemma \"rev (rev xs) = xs\"\\n  by simp\\n\\nend\\n' > {folder}/Ref.thy"));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let target = link::Target { key: "live".into(), destination: Some(dest.clone()) };
        let mut app = app(target.clone());
        let mut stream = Box::pin(link::run(&target));
        assert!(pump(&mut app, &mut stream, 120, |a| a.links[0].sender.is_some()).await, "connected");
        app.send("live", "session_create", json!({"folder": folder, "logic": "HOL", "new": true}), Pending::Create);
        assert!(pump(&mut app, &mut stream, 300, |a| a.workspace.as_ref().and_then(|w| w.theory.as_ref()).is_some_and(|t| t.prover.commands.len() >= 3)).await, "theory checked");
        // Alt+click on rev, once the prover's markup is there
        let mut opened = false;
        for _ in 0..40 {
            let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
            t.cursor = t.local.find("rev (rev").unwrap() + 1;
            app.alt = true;
            let _ = app.goto_definition();
            app.alt = false;
            if pump(&mut app, &mut stream, 5, |a| !a.references.is_empty()).await {
                opened = true;
                break;
            }
        }
        assert!(opened, "a reference window opened");
        let (id, r) = (app.references[0].0, &app.references[0].1);
        println!("opened {} ({} lines, library {})", r.title, r.theory.local.lines().count(), r.library);
        assert!(r.library && r.path.ends_with("HOL/List.thy") && r.title.starts_with("List.rev"));
        assert!(r.theory.local.split('\n').nth(r.line).unwrap().contains("rev"));
        // Hover a name in it
        let at = r.theory.local.find("monoid append Nil").unwrap() + 9;
        app.hover_gen += 1;
        let generation = app.hover_gen;
        app.workspace.as_mut().unwrap().hover = Some(super::Hover { target: super::HoverTarget::Ref(id), point: Default::default(), offset: at, generation, info: None });
        let _ = app.update(Message::HoverTick(generation));
        assert!(pump(&mut app, &mut stream, 60, |a| a.workspace.as_ref().unwrap().hover.as_ref().is_some_and(|h| h.info.is_some())).await, "a tooltip");
        let info = app.workspace.as_ref().unwrap().hover.as_ref().unwrap().info.clone().unwrap();
        println!("tooltip: {}", info["tooltip"]["items"][0]["raw"]);
        assert!(info["tooltip"]["items"][0]["raw"].as_str().unwrap().contains("append ::"));
        // Ctrl+click on it: this window moves to append's definition
        let _ = app.ref_lookup(id, at, super::RefUse::Goto(false));
        assert!(pump(&mut app, &mut stream, 60, |a| a.references[0].1.title.starts_with("List.append")).await, "moved to append");
        assert_eq!((app.references.len(), app.references[0].1.back.len()), (1, 1));
        println!("moved to {}", app.references[0].1.title);
        // Alt+click on a type: another window
        let r = &app.references[0].1;
        let at = r.theory.local.find("'a list").unwrap() + 4;
        let _ = app.ref_lookup(id, at, super::RefUse::Goto(true));
        assert!(pump(&mut app, &mut stream, 60, |a| a.references.len() == 2).await, "a second window");
        println!("second window: {}", app.references[1].1.title);
        // Back
        let _ = app.update(Message::RefBack(id));
        assert!(pump(&mut app, &mut stream, 60, |a| a.references[0].1.title.starts_with("List.rev")).await, "back to rev");
        let sid = app.workspace.as_ref().unwrap().sid.clone();
        app.send("live", "discard", json!({"session": sid, "force": true}), Pending::Plain);
        pump(&mut app, &mut stream, 5, |_| false).await;
    });
    // (and the backend component that host registered with Isabelle)
    ssh(&dest, &format!("ISABELLUM_STATE_DIR={root}/state python3 {root}/host/isabellum_host.py stop-host; rm -rf {folder} {root}; sed -i '\\|^{root}/|d' ~/.isabelle/*/etc/components"));
}

/// `ISABELLUM_LINK_PROBE=1`: the link to each machine Terminal Agents knows, as the app makes it,
/// its events printed for a while (run inside the Flatpak to see what the app sees)
#[test]
#[ignore]
fn live_link_probe() {
    if std::env::var("ISABELLUM_LINK_PROBE").is_err() {
        return;
    }
    let targets = link::targets();
    println!("targets: {:?}", targets.iter().map(|(n, t)| (n.clone(), t.destination.clone())).collect::<Vec<_>>());
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        for (name, target) in targets.into_iter().filter(|(_, t)| t.destination.is_some()) {
            let mut stream = Box::pin(link::run(&target));
            let deadline = Instant::now() + Duration::from_secs(40);
            while Instant::now() < deadline {
                match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                    Ok(Some((_, ev))) => {
                        let s = format!("{ev:?}");
                        println!("{name}: {}", &s[..s.len().min(160)]);
                        if matches!(ev, link::Event::Connected(_)) {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => println!("{name}: (nothing for 5 s)"),
                }
            }
        }
    });
}

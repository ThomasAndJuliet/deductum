// SPDX-License-Identifier: GPL-3.0-only

//! Two backends in one window, against real hosts:
//! `DEDUCTUM_LIVE_MIXED=1 DEDUCTUM_LIVE=d59500 cargo test --bin deductum live_mixed -- --ignored
//! --nocapture`.
//!
//! An Isabelle session on another machine (`DEDUCTUM_LIVE`: its host is the one installed under
//! `~/.local/share/deductum`, as `live.rs` uses it) and a Lean session on this computer's own host,
//! a temporary one (`DEDUCTUM_LOCAL_ROOT`, with its own state) on a fresh copy of DSRVLean. The
//! Lean session is created through the new-session page, as a human does: machine, folder, the
//! cards the host's detection shows, the Lean card, Start. Then the window switches between the
//! two sessions' documents and the profile and capabilities follow (Isabelle's Sledgehammer and its
//! goal labels, Lean's Search and the infoview's goals), the Lean Project tab runs `build`, and the
//! sidebar lists both sessions with their backend marks. Frames go to `CT_SNAPSHOTS` as
//! `mixed-*.png`, at two widths and in both themes.

use std::time::Duration;

use cosmic::Application;
use cosmic::iced::Size;
use cosmic::iced::futures::StreamExt;
use serde_json::json;

use super::live::{app as new_app, cleanup, pump, ssh};
use super::shell::Msg;
use super::snapshot::Harness;
use super::theory::Status;
use super::{App, LeftMode, Link, Message, Pending, link};

const THY: &str = "theory Mixed\n  imports Main\nbegin\n\nlemma open_goal: \"\\<And>x::nat. x > 0 \\<Longrightarrow> x = x\"\n  sorry\n\nlemma two: \"\\<forall>x::nat. x + 0 = x\"\n  by simp\n\nend\n";

fn ws(a: &App) -> &super::Workspace {
    a.workspace.as_ref().unwrap()
}

fn theory(a: &App) -> &super::editor::Theory {
    ws(a).theory.as_ref().unwrap()
}

fn backend(a: &App) -> &str {
    a.workspace.as_ref().map_or("", |w| w.profile.backend())
}

fn checked(a: &App) -> bool {
    a.workspace
        .as_ref()
        .and_then(|w| w.theory.as_ref())
        .is_some_and(|t| t.prover.commands.len() > 2 && t.prover.commands.iter().all(|c| !matches!(c.status, Status::Unprocessed | Status::Running)))
}

fn light(on: bool) {
    cosmic::theme::set_active(if on { cosmic::theme::ThemeType::Light } else { cosmic::theme::ThemeType::Dark });
}

/// A picture at both widths and in both themes
fn frames(h: &mut Harness, app: &App, tag: &str) {
    for (w, hgt) in [(1100.0, 800.0), (1600.0, 900.0)] {
        for dark in [true, false] {
            h.size = Size::new(w, hgt);
            light(!dark);
            h.draw(app, &format!("mixed-{tag}-{}-{}", w as u32, if dark { "dark" } else { "light" }));
        }
    }
    light(false);
    h.size = Size::new(1600.0, 900.0);
}

async fn ask_goals(app: &mut App, stream: &mut std::pin::Pin<Box<impl cosmic::iced::futures::Stream<Item = (String, link::Event)>>>, needle: &str) -> super::goals::Goals {
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    t.cursor = t.local.find(needle).map(|b| t.local[..b].chars().count()).expect("the place") + 1;
    app.workspace.as_mut().unwrap().goals.status = String::new();
    app.state_gen += 1;
    let generation = app.state_gen;
    let _ = app.update(Message::StateTick(generation));
    assert!(pump(app, stream, 120, |a| ws(a).goals.status == "finished").await, "goals at {needle}");
    super::goals::Goals::parse(&ws(app).goals.messages)
}

#[test]
#[ignore]
fn live_mixed() {
    if std::env::var("DEDUCTUM_LIVE_MIXED").ok().as_deref() != Some("1") {
        return;
    }
    let Ok(dest) = std::env::var("DEDUCTUM_LIVE") else { panic!("DEDUCTUM_LIVE names the machine of the Isabelle session (d59500)") };
    let home = std::env::var("HOME").unwrap();
    let source = std::env::var("DEDUCTUM_LEAN_PROJECT").unwrap_or_else(|_| format!("{home}/tmp-deductum-lean/DSRVLean"));
    let root = format!("/tmp/deductum-live-mixed-{}", std::process::id());
    let project = format!("{root}/DSRVLean");
    let status = std::process::Command::new("sh").arg("-c").arg(format!("rm -rf {root} && mkdir -p {root} && cp -r {source} {project}")).status().unwrap();
    assert!(status.success(), "a copy of DSRVLean from {source}");
    // This computer's host is a temporary one of its own; the other machine's is the real one
    // SAFETY: set before any thread of this test reads it
    unsafe { std::env::set_var("DEDUCTUM_LOCAL_ROOT", format!("{root}/host")) };
    let folder = format!("/tmp/deductum-live-mixed-isa-{}", std::process::id());
    ssh(&dest, &format!("mkdir -p {folder} && cat > {folder}/Mixed.thy <<'THY'\n{THY}THY\n"));
    struct Cleanup(String, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let pattern = self.0.replacen("deductum", "[d]eductum", 1);
            let _ = std::process::Command::new("sh").arg("-c").arg(format!("pkill -f '{pattern}/host/' ; sleep 1; rm -rf {}", self.0)).status();
            cleanup(&self.1);
        }
    }
    let _cleanup = Cleanup(root.clone(), dest.clone());
    let lean_file = format!("{project}/DSRV/CausalCorrectness.lean");
    let thy_file = format!("{folder}/Mixed.thy");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let au = link::Target { key: "au".into(), destination: Some(dest.clone()) };
        let neo = link::Target { key: "local".into(), destination: None };
        let mut app = new_app(au.clone());
        app.links[0].name = "au-laptop".into();
        app.links.push(Link { target: neo.clone(), name: "neo".into(), sender: None, status: String::new(), hello: None, client_id: None, symbols: None });
        let mut stream = Box::pin(cosmic::iced::futures::stream::select(Box::pin(link::run(&au)), Box::pin(link::run(&neo))));
        assert!(pump(&mut app, &mut stream, 120, |a| a.links.iter().all(|l| l.sender.is_some())).await, "both hosts connect");
        let mut h = Harness::new(Size::new(1600.0, 900.0));

        // The Isabelle session, on the other machine, with its theory
        app.send("au", "session_create", json!({"folder": folder, "logic": "HOL", "new": true, "start": true}), Pending::Create);
        assert!(pump(&mut app, &mut stream, 90, |a| a.workspace.as_ref().is_some_and(|w| w.theory.is_some())).await, "the Isabelle theory opens");
        assert_eq!(backend(&app), "isabelle");
        let isa_sid = ws(&app).sid.clone();
        assert!(pump(&mut app, &mut stream, 600, checked).await, "the theory is checked");
        let _ = app.update(Message::SetMode(super::editor::Mode::Literate));
        println!("isabelle session {isa_sid} checked");

        // The Lean session, through the new-session page
        app.status = None; // (an older daemon on the other machine does not know `client_settings`)
        let _ = app.update(Message::NewSession(true));
        let _ = app.update(Message::NewMachine(1));
        assert!(pump(&mut app, &mut stream, 60, |a| a.new_session.as_ref().is_some_and(|ns| ns.listing.is_some())).await, "the host lists its folder");
        // (the first look at a folder on a fresh host asks Isabelle for its sessions, which takes a while)
        assert!(pump(&mut app, &mut stream, 400, |a| a.new_session.as_ref().is_some_and(|ns| ns.detections.is_some())).await, "the first detection, of the home folder");
        let _ = app.update(Message::NewBrowse(project.clone()));
        assert!(pump(&mut app, &mut stream, 120, |a| a.new_session.as_ref().is_some_and(|ns| ns.folder.value() == project && ns.detections.is_some())).await, "the host detects what is in the folder");
        let ns = app.new_session.as_ref().unwrap();
        let found: Vec<String> = ns.detections.iter().flatten().filter_map(|d| d["backend"].as_str().map(str::to_string)).collect();
        println!("detections: {found:?}");
        assert!(found.contains(&"lean".to_string()), "the Lean card: {found:?}");
        h.draw(&app, "mixed-new-session-cards");
        let _ = app.update(Message::Shell(Msg::NewBackend("lean".into())));
        frames(&mut h, &app, "new-session");
        let params = app.new_session.as_ref().unwrap().create_params();
        assert_eq!(params["backend"], "lean");
        let _ = app.update(Message::NewCreate);
        assert!(pump(&mut app, &mut stream, 90, |a| backend(a) == "lean" && a.new_session.is_none()).await, "the Lean session opens");
        let lean_sid = ws(&app).sid.clone();
        assert_eq!(ws(&app).machine, "local");
        assert!(ws(&app).caps.search.is_some() && ws(&app).caps.tool("sledgehammer").is_none(), "Lean's capabilities, not Isabelle's");
        // (both sessions are in the sidebar's list, each with its backend)
        let backends: Vec<String> = app.sessions.values().map(|s| s["backend"].as_str().unwrap_or("isabelle").to_string()).collect();
        assert!(backends.contains(&"lean".to_string()) && backends.contains(&"isabelle".to_string()), "{backends:?}");
        println!("lean session {lean_sid}");

        // A Lean document: open, checked, a goal in the infoview's form
        let _ = app.update(Message::OpenTheory(lean_file.clone()));
        assert!(pump(&mut app, &mut stream, 900, checked).await, "CausalCorrectness.lean checked");
        let _ = app.update(Message::SetMode(super::editor::Mode::Literate));
        app.show_right = true;
        let goals = ask_goals(&mut app, &mut stream, "induction fuel generalizing").await;
        let sg = goals.subgoals.first().expect("a Lean goal");
        let lean_shown = ws(&app).profile.present_goal(sg);
        assert_eq!(lean_shown.sections.last().and_then(|s| s.label), Some("⊢"));
        assert!(lean_shown.sections.iter().any(|s| s.label.is_none()), "unlabelled hypotheses");
        println!("lean goal labels: {:?}", lean_shown.sections.iter().map(|s| s.label).collect::<Vec<_>>());
        let _ = app.update(Message::Panel(super::Panel::Output));
        frames(&mut h, &app, "lean-document");

        // The Project tab and its build action
        let _ = app.update(Message::LeftMode(LeftMode::Project));
        let sid = ws(&app).sid.clone();
        assert!(ws(&app).caps.project_actions.iter().any(|a| a.name == "build"), "Lean offers lake build");
        let _ = app.update(Message::Shell(Msg::ProjectAction("build".into())));
        h.draw(&app, "mixed-project-building");
        let finished = |a: &App| a.shell.runs.get(&(sid.clone(), "build".to_string())).is_some_and(|r| !r.running);
        assert!(pump(&mut app, &mut stream, 600, finished).await, "the build finishes");
        let run = app.shell.runs.get(&(sid.clone(), "build".to_string())).unwrap().clone();
        println!("build ok={:?} output: {:?}", run.ok, run.output.lines().last());
        assert!(run.ok == Some(true) && !run.output.trim().is_empty(), "lake build ran and said so: {run:?}");
        frames(&mut h, &app, "lean-project-build");

        // Switch to the Isabelle session: its theory, profile and capabilities
        let _ = app.update(Message::LeftMode(LeftMode::Sessions));
        app.want_theory = Some(thy_file.clone());
        let _ = app.update(Message::OpenSession("au".into(), isa_sid.clone()));
        assert!(pump(&mut app, &mut stream, 90, |a| backend(a) == "isabelle" && a.workspace.as_ref().is_some_and(|w| w.theory.as_ref().is_some_and(|t| t.path == thy_file))).await, "back in Isabelle's theory");
        assert!(ws(&app).caps.tool("sledgehammer").is_some() && ws(&app).caps.search.as_ref().is_some_and(|s| s.operation == "find_theorems"));
        assert!(pump(&mut app, &mut stream, 300, checked).await, "rechecked");
        let goals = ask_goals(&mut app, &mut stream, "sorry").await;
        let sg = goals.subgoals.first().expect("an Isabelle goal");
        let isa_shown = ws(&app).profile.present_goal(sg);
        let labels: Vec<Option<&str>> = isa_shown.sections.iter().map(|s| s.label).collect();
        println!("isabelle goal labels: {labels:?}");
        assert_eq!(labels, vec![Some("fix"), Some("assume"), Some("show")]);
        let _ = app.update(Message::Panel(super::Panel::Tool));
        frames(&mut h, &app, "isabelle-document");

        // And back to Lean: the profile follows again, its document is where it was
        let _ = app.update(Message::OpenSession("local".into(), lean_sid.clone()));
        assert!(pump(&mut app, &mut stream, 90, |a| backend(a) == "lean").await);
        assert!(ws(&app).caps.tool("sledgehammer").is_none() && ws(&app).caps.search.is_some());
        let _ = app.update(Message::Panel(super::Panel::Search));
        frames(&mut h, &app, "lean-search-panel");
        println!("switched both ways");

        app.send("au", "stop", json!({"session": isa_sid}), Pending::Plain);
        app.send("local", "stop", json!({"session": lean_sid}), Pending::Plain);
        let _ = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;
    });
}

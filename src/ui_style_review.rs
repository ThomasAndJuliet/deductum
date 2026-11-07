// SPDX-License-Identifier: GPL-3.0-only
use super::{editor::Mode, new_session::NewSession, Message};
use cosmic::Application;
use serde_json::json;

#[test]
fn editing_folder_invalidates_detected_project() {
    let mut app = super::snapshot_lean::lean_app(Mode::Source);
    let mut ns = NewSession { folder: "/a".into(), listing: Some(json!({"path":"/a"})), backend: Some("lean".into()), ..Default::default() };
    ns.suggested(&json!({"detections":[{"backend":"lean", "project":{"root":"/a"}}]}));
    app.new_session = Some(ns);
    let _ = app.update(Message::NewFolder("/b".into()));
    let ns = app.new_session.as_ref().unwrap();
    assert!(ns.listing.is_none() && ns.backend.is_none() && ns.detections.is_none());
}

#[test]
fn old_folder_replies_and_errors_cannot_replace_current_selection() {
    use super::Pending;
    let mut app = super::snapshot_lean::lean_app(Mode::Source);
    app.new_session = Some(NewSession::default());
    let _ = app.update(Message::NewBrowse("/a".into()));
    let old = app.new_session.as_ref().unwrap().generation;
    let _ = app.on_reply("other-machine".into(), Pending::ListDir(old, "/a".into()), json!({"ok":true,"path":"/other"}));
    assert_eq!(app.new_session.as_ref().unwrap().folder, "/a");
    let _ = app.update(Message::NewBrowse("/b".into()));
    let current = app.new_session.as_ref().unwrap().generation;
    let _ = app.on_reply("local".into(), Pending::ListDir(old, "/a".into()), json!({"ok":true,"path":"/a"}));
    let _ = app.on_reply("local".into(), Pending::Suggest(old, "/a".into()), json!({"ok":false,"error":"old error"}));
    let ns = app.new_session.as_ref().unwrap();
    assert_eq!(ns.folder, "/b");
    assert!(ns.listing.is_none() && ns.error.is_none() && !ns.ready());
    let _ = app.on_reply("local".into(), Pending::ListDir(current, "/b".into()), json!({"ok":true,"path":"/b"}));
    let _ = app.on_reply("local".into(), Pending::Suggest(current, "/b".into()), json!({"ok":true,"detections":[{"backend":"lean","project":{"root":"/b"}}]}));
    assert!(app.new_session.as_ref().unwrap().ready());
    let _ = app.update(Message::NewSession(false));
    let _ = app.update(Message::NewSession(true));
    let _ = app.on_reply("local".into(), Pending::ListDir(current, "/b".into()), json!({"ok":true,"path":"/b"}));
    assert_ne!(app.new_session.as_ref().unwrap().folder, "/b");
}

#[test]
fn machine_change_invalidates_old_replies_and_loading_form_cannot_start() {
    use super::Pending;
    let mut app = super::snapshot_lean::lean_app(Mode::Source);
    app.new_session = Some(NewSession::default());
    let _ = app.update(Message::NewBrowse("/a".into()));
    let old = app.new_session.as_ref().unwrap().generation;
    let _ = app.update(Message::NewMachine(0));
    let _ = app.on_reply("local".into(), Pending::ListDir(old, "/a".into()), json!({"ok":true,"path":"/a"}));
    let ns = app.new_session.as_ref().unwrap();
    assert_eq!(ns.folder, "");
    assert!(!ns.ready());
    let _ = app.update(Message::NewCreate);
    assert!(app.new_session.is_some());
}

#[derive(Default, Debug)]
struct Texts(Vec<String>);
impl cosmic::iced::advanced::widget::Operation<()> for Texts {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn cosmic::iced::advanced::widget::Operation<()>)) { operate(self); }
    fn text(&mut self, _: Option<&cosmic::widget::Id>, _: cosmic::iced::Rectangle, text: &str) { self.0.push(text.into()); }
}

#[test]
fn pending_proof_does_not_show_insertion_palette() {
    let mut app = super::snapshot_lean::lean_app(Mode::Literate);
    app.show_sidebar = false;
    app.show_right = false;
    let ws = app.workspace.as_mut().unwrap();
    ws.output = None;
    let t = ws.theory.as_ref().unwrap();
    let i = t.blocks.iter().position(|b| b.name == "theorem").unwrap();
    let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(1600.0, 900.0));
    let task = app.update(Message::EditBlock(i));
    h.run_task(&app, task);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.block_editor.as_ref().unwrap().start, t.blocks[i].start, "fixture must open the theorem");
    h.frame(&app);
    let mut texts = Texts::default();
    h.operate(&app, &mut texts);
    assert!(!texts.0.iter().any(|s| s.starts_with("Insert in the proof") || s == "Insert a symbol"), "pending formal block showed insert palette: {:?}", texts.0);
    assert!(texts.0.iter().any(|s| s.contains("Checking") || s.contains("Waiting")), "missing pending state: {:?}", texts.0);
    app.workspace.as_mut().unwrap().output = Some(json!({"messages":[]}));
    h.frame(&app);
    let mut texts = Texts::default();
    h.operate(&app, &mut texts);
    assert!(texts.0.iter().any(|s| s == "Goals"), "goal panel did not appear after reply: {:?}", texts.0);
}

#[test]
fn prose_never_displays_a_stale_proof_state() {
    for mode in [Mode::Literate, Mode::Source] {
        for width in [800.0, 1600.0] {
            let mut app = super::snapshot_lean::lean_app(mode);
            app.show_sidebar = false;
            app.show_right = false;
            let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
            let i = t.blocks.iter().position(|b| matches!(b.kind, super::theory::Kind::Text | super::theory::Kind::Heading(_))).unwrap();
            let at = t.blocks[i].start + 1;
            let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(width, 900.0));
            let msg = if mode == Mode::Literate { Message::EditBlock(i) } else { Message::Goto(at) };
            let task = app.update(msg);
            h.run_task(&app, task);
            app.workspace.as_mut().unwrap().output = Some(json!({"messages":[{"kind":"state", "text":"STALE_GOAL_MARKER"}]}));
            h.frame(&app);
            let mut texts = Texts::default();
            h.operate(&app, &mut texts);
            assert!(!texts.0.iter().any(|s| s.contains("STALE_GOAL_MARKER") || s == "Goals"), "stale goal in {mode:?} at {width}: {:?}", texts.0);
        }
    }
}

#[test]
fn source_formal_waits_for_goal_at_both_widths() {
    for width in [800.0, 1600.0] {
        let mut app = super::snapshot_lean::lean_app(Mode::Source);
        app.show_sidebar = false;
        app.show_right = false;
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().position(|b| b.name == "theorem").unwrap();
        let at = t.blocks[i].start + 1;
        let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(width, 900.0));
        let task = app.update(Message::Goto(at));
        h.run_task(&app, task);
        app.workspace.as_mut().unwrap().output = None;
        h.frame(&app);
        let mut texts = Texts::default();
        h.operate(&app, &mut texts);
        assert!(texts.0.iter().any(|s| s.contains("Checking") || s.contains("Waiting")), "no waiting goal at {width}: {:?}", texts.0);
        assert!(!texts.0.iter().any(|s| s == "Insert a symbol" || s.starts_with("Insert in the proof")));
    }
}

#[test]
fn lean_import_keeps_its_insertion_palette_while_waiting() {
    let mut app = super::snapshot_lean::lean_app(Mode::Literate);
    app.show_sidebar = false;
    app.show_right = false;
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let i = t.blocks.iter().position(|b| b.name == "import").unwrap();
    let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(1600.0, 900.0));
    let task = app.update(Message::EditBlock(i));
    h.run_task(&app, task);
    app.workspace.as_mut().unwrap().output = None;
    h.frame(&app);
    let mut texts = Texts::default();
    h.operate(&app, &mut texts);
    assert!(texts.0.iter().any(|s| s == "Insert a symbol"));
    assert!(!texts.0.iter().any(|s| s.contains("Checking") || s.contains("Waiting")));
}

#[test]
fn lean_definition_tactic_body_keeps_goal_while_typing() {
    for text in ["def value : Nat := by\n  ", "def value : Nat := by\n  exact 1", "def value : Nat := 1"] {
        let mut app = super::snapshot_lean::lean_app_with(Mode::Literate, text);
        app.show_sidebar = false;
        app.show_right = false;
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().position(|b| b.name == "def").unwrap();
        let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(1600.0, 900.0));
        let task = app.update(Message::EditBlock(i));
        h.run_task(&app, task);
        app.workspace.as_mut().unwrap().output = None;
        h.frame(&app);
        let mut texts = Texts::default();
        h.operate(&app, &mut texts);
        assert!(texts.0.iter().any(|s| s.contains("Checking") || s.contains("Waiting")), "proof did not show pending goal: {:?}", texts.0);
        assert!(!texts.0.iter().any(|s| s == "Insert a symbol"));
    }
}

#[test]
fn lean_by_inside_comments_and_strings_does_not_create_a_goal() {
    for text in ["#check \"by\"", "#check Nat -- by"] {
        let mut app = super::snapshot_lean::lean_app_with(Mode::Literate, text);
        app.show_sidebar = false;
        app.show_right = false;
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        let i = t.blocks.iter().position(|b| b.kind == super::theory::Kind::Formal).unwrap();
        let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(1600.0, 900.0));
        let task = app.update(Message::EditBlock(i));
        h.run_task(&app, task);
        app.workspace.as_mut().unwrap().output = None;
        h.frame(&app);
        let mut texts = Texts::default();
        h.operate(&app, &mut texts);
        assert!(texts.0.iter().any(|s| s == "Insert a symbol"), "non-proof did not show insert palette: {:?}", texts.0);
        assert!(!texts.0.iter().any(|s| s.contains("Checking") || s.contains("Waiting")));
    }
}

#[test]
fn maths_boundary_right_and_selection_preserve_source_offsets() {
    use cosmic::widget::text_editor::{Action, Motion};
    use super::{editor::{self, Theory}, prover::isabelle::symbols::Table};
    use std::{collections::HashMap, sync::Arc};
    let source = "theory T imports Main begin\n\ntext \\<open>café naïve αβ and $x + 0 = x$. After.\\<close>\n\nend\n";
    let table = Table::fallback();
    let doc = json!({"text": source, "revision": 1, "prover": {}});
    let mut app = super::snapshot::demo_app(Mode::Literate);
    app.show_sidebar = false;
    app.show_right = false;
    app.workspace.as_mut().unwrap().theory = Some(Theory::new("/test.thy".into(), &doc, &table, Arc::new(HashMap::new()), Mode::Literate, super::prover::profile(None)));
    let i = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().blocks.iter().position(|b| b.kind == super::theory::Kind::Text).unwrap();
    let mut h = super::snapshot::Harness::new(cosmic::iced::Size::new(1400.0, 900.0));
    let task = app.update(Message::EditBlock(i)); h.run_task_with_updates(&mut app, task); h.frame(&app);
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let be = t.block_editor.as_mut().unwrap();
    let close = be.decoded.text.chars().position(|c| c == '$').unwrap() + "x + 0 = x".chars().count() + 1;
    be.reshow(super::prover::profile(None), Some(close), &table);
    let pos = editor::LineIndex::new(&be.shown_text).position(&be.shown_text, close);
    editor::place(&mut be.content, pos);
    t.sync_block_cursor();
    let start = t.cursor;
    for n in 1..=3 {
        let task = app.update(Message::BlockAction(Action::Move(Motion::Right))); h.run_task_with_updates(&mut app, task); h.frame(&app);
        assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().cursor, start+n, "right step {n}");
    }
    for n in 1..=3 {
        let task = app.update(Message::BlockAction(Action::Move(Motion::Left))); h.run_task_with_updates(&mut app, task); h.frame(&app);
        assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().cursor, start+3-n, "left step {n}");
    }
    let t = app.workspace.as_mut().unwrap().theory.as_mut().unwrap();
    let be = t.block_editor.as_mut().unwrap();
    be.reshow(super::prover::profile(None), Some(close-1), &table);
    let pos = editor::LineIndex::new(&be.shown_text).position(&be.shown_text, close-1);
    editor::place(&mut be.content, pos); t.sync_block_cursor();
    for _ in 0..3 { let task = app.update(Message::BlockAction(Action::Select(Motion::Right))); h.run_task_with_updates(&mut app, task); h.frame(&app); }
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.local, source);
    let be = t.block_editor.as_ref().unwrap();
    let cursor = be.content.cursor();
    let index = editor::LineIndex::new(&be.shown_text);
    assert_eq!(index.offset(&be.shown_text, cursor.position), close+2);
    assert_eq!(cursor.selection.map(|p| index.offset(&be.shown_text, p)), Some(close-1), "selection anchor changed across markup");
}

#[test]
fn switching_documents_does_not_lose_an_unsent_second_edit() {
    use super::{link, Pending};
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender);
    app.links[0].client_id = Some("me".into());
    let old_path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('B'))));
    let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(first["op"], "edit");
    assert!(commands.try_recv().is_err());
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    assert!(commands.try_recv().is_err(), "must not close or read while the first edit is in flight");
    assert!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("AB"));
    // Input arriving after the switch request must not alter the old replica.
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('C'))));
    assert!(!app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local.contains("ABC"));
    let ack = |request: &serde_json::Value, revision: u64| {
        let edits: Vec<_> = request["edits"].as_array().unwrap().iter().map(|e| json!([e["start"], e["end"], e["text"]])).collect();
        json!({"event":"doc_changed","session":"s1","path":old_path,"revision":revision,"client":"me","request":request["id"],"entry":{"change":edits,"author":{"kind":"human"},"kind":"edit"}})
    };
    let _ = app.on_link("local".into(), link::Event::Line(ack(&first, 2)));
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":first["id"],"ok":true})));
    let second: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(second["op"], "edit");
    assert!(second["edits"].to_string().contains('B'));
    assert!(commands.try_recv().is_err(), "must wait for the second edit too");
    let _ = app.on_link("local".into(), link::Event::Line(ack(&second, 3)));
    assert!(commands.try_recv().is_err(), "broadcast alone does not prove durable acknowledgement");
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":second["id"],"ok":true})));
    let close: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let read: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(close["op"], "close"); assert_eq!(read["op"], "read");
    let _ = app.on_reply("local".into(), Pending::Read("s1".into(), "/tmp/other.thy".into(), None), json!({"ok":true,"doc":{"path":"/tmp/other.thy","text":"theory Other imports Main begin\nend\n","revision":1}}));
    assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path, "/tmp/other.thy");
}

#[test]
fn session_switch_and_close_wait_for_unconfirmed_edits() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    for close in [false, true] {
        let mut app = super::snapshot::demo_app(Mode::Source);
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        app.links[0].sender = Some(sender); app.links[0].client_id = Some("me".into());
        let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
        let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
        let _ = app.update(if close { Message::CloseWorkspace } else { Message::OpenSession("local".into(), "s2".into()) });
        assert!(commands.try_recv().is_err());
        assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path, path);
        let event = json!({"event":"doc_changed","session":"s1","path":path,"revision":2,"client":"me","request":first["id"],"entry":{"change":[[0,0,"A"]],"author":{"kind":"human"},"kind":"edit"}});
        let _ = app.on_link("local".into(), link::Event::Line(event));
        assert!(commands.try_recv().is_err());
        let _ = app.on_link("local".into(), link::Event::Line(json!({"id":first["id"],"ok":true})));
        let detach: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
        assert_eq!(detach["op"], "detach");
        if close { assert!(app.workspace.is_none()); } else { assert_eq!(app.workspace.as_ref().unwrap().sid, "s2"); }
    }
}

#[test]
fn disconnected_switch_keeps_the_unsent_replica() {
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert!(t.local.starts_with('A'));
    assert!(t.inflight.is_none(), "an unsent edit must not be marked in flight");
    assert!(app.deferred_leave.is_some());
}

#[test]
fn closed_sender_keeps_edits_and_queued_undo_for_reconnect() {
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    drop(receiver); app.links[0].sender = Some(sender);
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let _ = app.update(Message::Undo);
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert!(t.local.starts_with('A') && t.inflight.is_none());
    assert_eq!(t.queued.front(), Some(&"undo"));
    assert!(app.deferred_leave.is_some());
}

#[test]
fn rejected_undo_releases_a_waiting_switch() {
    use super::link;
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender);
    let _ = app.update(Message::Undo);
    let undo: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(undo["op"], "undo");
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    assert!(commands.try_recv().is_err());
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":undo["id"],"ok":false,"error":"undo failed"})));
    let close: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let read: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(close["op"], "close"); assert_eq!(read["op"], "read");
    assert_eq!(app.status.as_deref(), Some("undo failed"));
}

#[test]
fn failed_opening_restores_the_old_document_subscription() {
    use super::Pending;
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender);
    let old = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let _ = commands.try_recv().unwrap(); let _ = commands.try_recv().unwrap();
    let _ = app.on_reply("local".into(), Pending::Read("s1".into(), "/tmp/other.thy".into(), None), json!({"ok":false,"error":"missing"}));
    let read: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(read["op"], "read"); assert_eq!(read["path"], old);
    assert!(app.workspace.as_ref().unwrap().opening.is_none());
    assert!(app.nav_target_visible(super::EditorTarget::Source));
}

#[test]
fn resync_of_old_document_keeps_a_newer_opening_pending() {
    use super::Pending;
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, _commands) = tokio::sync::mpsc::unbounded_channel(); app.links[0].sender = Some(sender);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let old = t.path.clone(); let text = t.local.clone();
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let _ = app.on_reply("local".into(), Pending::Read("s1".into(), old.clone(), None), json!({"ok":true,"doc":{"path":old,"text":text,"revision":1}}));
    assert_eq!(app.workspace.as_ref().unwrap().opening.as_deref(), Some("/tmp/other.thy"));
    assert!(!app.nav_target_visible(super::EditorTarget::Source));
}

#[test]
fn reconnect_after_lost_ack_preserves_unsent_edit_before_switch() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let original = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().confirmed.clone();
    let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender);
    app.links[0].client_id = Some("me".into());
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let _first = commands.try_recv().unwrap();
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('B'))));
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let _ = app.on_link("local".into(), link::Event::Disconnected("lost acknowledgement".into()));
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    let _ = app.on_link("local".into(), link::Event::Connected(sender));
    let mut read = None;
    while let Ok(line) = commands.try_recv() {
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["op"] == "read" { read = Some(value.clone()); }
        assert_ne!(value["op"], "close");
    }
    let read = read.unwrap();
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":read["id"],"ok":true,"doc":{"path":path,"text":format!("A{original}"),"revision":2}})));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.local, format!("AB{original}"), "reconnect must not duplicate the acknowledged first edit or lose the second");
    assert!(app.deferred_leave.is_some());
    let edits: Vec<serde_json::Value> = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str(&line).unwrap()).collect();
    assert!(!edits.iter().any(|value| value["op"] == "edit" || value["op"] == "close"));
    assert!(app.status.as_deref().unwrap().contains("outcome is unknown"));
}

#[test]
fn provenance_recovers_lost_ack_and_preserves_identical_foreign_insert() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    for accepted in [false, true] {
        let mut app = super::snapshot::demo_app(Mode::Source);
        let original = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().confirmed.clone();
        let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        app.links[0].sender = Some(sender);
        app.links[0].hello = Some(json!({"edit_identity":true}));
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
        let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('B'))));
        let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
        let _ = app.on_link("local".into(), link::Event::Disconnected("lost".into()));
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let _ = app.on_link("local".into(), link::Event::Connected(sender));
        let read = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str::<serde_json::Value>(&line).unwrap()).find(|v| v["op"] == "read").unwrap();
        let identity = if accepted { first["edit_id"].clone() } else { json!("foreign") };
        let _ = app.on_link("local".into(), link::Event::Line(json!({"id":read["id"],"ok":true,"doc":{"path":path,"text":format!("A{original}"),"revision":2},"edit_history":{"complete":true,"entries":[{"rev":2,"change":[[0,0,"A"]],"edit_id":identity}]}})));
        let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
        assert_eq!(t.local, format!("{}{original}", if accepted { "AB" } else { "AAB" }));
        let edit: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
        assert_eq!(edit["op"], "edit");
        if accepted { assert!(edit["edits"].to_string().contains('B')); }
        else { assert_eq!(edit["edit_id"], first["edit_id"]); assert_eq!(edit["revision"], first["revision"]); assert_eq!(edit["edits"], first["edits"]); }
        assert!(app.deferred_leave.is_some());
    }
}

#[test]
fn late_duplicate_reply_cannot_interrupt_the_next_edit() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender); app.links[0].client_id = Some("me".into());
    let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('B'))));
    let _ = app.on_link("local".into(), link::Event::Line(json!({"event":"doc_changed","session":"s1","path":path,"revision":2,"client":"me","request":first["id"],"entry":{"change":[[0,0,"A"]],"author":{"kind":"human"},"kind":"edit","edit_id":first["edit_id"]}})));
    let second: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":first["id"],"ok":true,"duplicate":true})));
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(t.inflight, second["id"].as_u64());
    assert_eq!(t.inflight_edit.as_ref().unwrap().identity, second["edit_id"].as_str().unwrap());
    assert!(commands.try_recv().is_err());
}

#[test]
fn recovery_replays_remote_edits_around_our_acknowledged_edit() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let original = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().confirmed.clone();
    let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let len = original.chars().count();
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender); app.links[0].hello = Some(json!({"edit_identity":true}));
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('B'))));
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    let _ = app.on_link("local".into(), link::Event::Disconnected("lost".into()));
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    let _ = app.on_link("local".into(), link::Event::Connected(sender));
    let read = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str::<serde_json::Value>(&line).unwrap()).find(|v| v["op"] == "read").unwrap();
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":read["id"],"ok":true,"doc":{"path":path,"text":format!("A{original}XY"),"revision":4},"edit_history":{"complete":true,"entries":[{"rev":2,"change":[[len,len,"X"]],"edit_id":"other1"},{"rev":3,"change":[[0,0,"A"]],"edit_id":first["edit_id"]},{"rev":4,"change":[[len+2,len+2,"Y"]],"edit_id":"other2"}]}})));
    assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local, format!("AB{original}XY"));
    let edit: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    assert_eq!(edit["op"], "edit"); assert!(edit["edits"].to_string().contains('B'));
}

#[test]
fn uncertain_or_malformed_history_keeps_the_original_replica() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    for kind in ["pruned", "range", "duplicate"] {
        let mut app = super::snapshot::demo_app(Mode::Source);
        let original = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().confirmed.clone();
        let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        app.links[0].sender = Some(sender); app.links[0].hello = Some(json!({"edit_identity":true}));
        let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
        let first: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
        let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
        let _ = app.on_link("local".into(), link::Event::Disconnected("lost".into()));
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let _ = app.on_link("local".into(), link::Event::Connected(sender));
        let read = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str::<serde_json::Value>(&line).unwrap()).find(|v| v["op"] == "read").unwrap();
        let history = match kind {
            "pruned" => json!({"complete":false,"entries":[]}),
            "range" => json!({"complete":true,"entries":[{"rev":2,"change":[[1000000,1000000,"A"]],"edit_id":first["edit_id"]}]}),
            _ => json!({"complete":true,"entries":[{"rev":2,"change":[[0,0,"A"]],"edit_id":first["edit_id"]},{"rev":3,"change":[[0,0,"A"]],"edit_id":first["edit_id"]}]}),
        };
        let _ = app.on_link("local".into(), link::Event::Line(json!({"id":read["id"],"ok":true,"doc":{"path":path,"text":format!("A{original}"),"revision":2},"edit_history":history})));
        assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local, format!("A{original}"));
        assert!(app.deferred_leave.is_some()); assert!(commands.try_recv().is_err());
        let _ = app.update(Message::OpenTheory(path));
        assert!(app.deferred_leave.is_none());
        assert!(app.status.as_deref().unwrap().contains("unknown"));
    }
}

#[test]
fn reconnect_after_broadcast_waits_for_a_durable_receipt() {
    use super::link;
    use cosmic::widget::text_editor::{Action, Edit};
    let mut app = super::snapshot::demo_app(Mode::Source);
    let original = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().confirmed.clone();
    let path = app.workspace.as_ref().unwrap().theory.as_ref().unwrap().path.clone();
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    app.links[0].sender = Some(sender); app.links[0].hello = Some(json!({"edit_identity":true})); app.links[0].client_id = Some("me".into());
    let _ = app.update(Message::SourceAction(Action::Edit(Edit::Insert('A'))));
    let edit: serde_json::Value = serde_json::from_str(&commands.try_recv().unwrap()).unwrap();
    let _ = app.on_link("local".into(), link::Event::Line(json!({"event":"doc_changed","session":"s1","path":path,"revision":2,"client":"me","request":edit["id"],"entry":{"change":[[0,0,"A"]],"author":{"kind":"human"},"kind":"edit","edit_id":edit["edit_id"]}})));
    let _ = app.update(Message::OpenTheory("/tmp/other.thy".into()));
    assert!(commands.try_recv().is_err());
    let _ = app.on_link("local".into(), link::Event::Disconnected("lost reply".into()));
    let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
    let _ = app.on_link("local".into(), link::Event::Connected(sender));
    let read = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str::<serde_json::Value>(&line).unwrap()).find(|v| v["op"] == "read").unwrap();
    assert_eq!(read["receipt_ids"], json!([edit["edit_id"]]));
    assert!(app.deferred_leave.is_some());
    let _ = app.on_link("local".into(), link::Event::Line(json!({"id":read["id"],"ok":true,"doc":{"path":path,"text":format!("A{original}"),"revision":2},"edit_receipts":[edit["edit_id"]]})));
    let sent: Vec<serde_json::Value> = std::iter::from_fn(|| commands.try_recv().ok()).map(|line| serde_json::from_str(&line).unwrap()).collect();
    assert!(sent.iter().any(|v| v["op"] == "close"));
    assert!(sent.iter().any(|v| v["op"] == "read" && v["path"] == "/tmp/other.thy"));
}

#[test]
fn other_machine_cannot_remove_the_current_session() {
    let mut app = super::snapshot::demo_app(Mode::Source);
    let _ = app.on_event("other-machine".into(), json!({"event":"session_removed","session":"s1"}));
    assert!(app.workspace.is_some());
}

#[test]
fn stale_same_path_read_from_other_workspace_is_ignored() {
    use super::Pending;
    let mut app = super::snapshot::demo_app(Mode::Source);
    let t = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let path = t.path.clone(); let original = t.local.clone();
    for (machine, sid) in [("other", "s1"), ("local", "other-session")] {
        let _ = app.on_reply(machine.into(), Pending::Read(sid.into(), path.clone(), None), json!({"ok":true,"doc":{"path":path,"text":"foreign","revision":9}}));
        assert_eq!(app.workspace.as_ref().unwrap().theory.as_ref().unwrap().local, original);
        let _ = app.on_reply(machine.into(), Pending::Read(sid.into(), path.clone(), None), json!({"ok":false,"error":"foreign error"}));
        assert!(app.status.as_deref() != Some("foreign error"));
    }
}

#[test]
fn older_same_document_read_does_not_replace_a_newer_event() {
    use super::Pending;
    let mut app = super::snapshot::demo_app(Mode::Source);
    let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    let path = theory.path.clone();
    let original = theory.confirmed.clone();
    let _ = app.on_event("local".into(), json!({"event":"doc_changed","session":"s1","path":path,"revision":2,"entry":{"change":[[0,0,"X"]],"author":{"kind":"human"},"kind":"edit"}}));
    let _ = app.on_reply("local".into(), Pending::Read("s1".into(), path.clone(), None), json!({"ok":true,"doc":{"path":path,"text":original,"revision":1}}));
    let theory = app.workspace.as_ref().unwrap().theory.as_ref().unwrap();
    assert_eq!(theory.confirmed_rev, 2);
    assert_eq!(theory.local, format!("X{original}"));
}

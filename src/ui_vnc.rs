// SPDX-License-Identifier: GPL-3.0-only
//! Opt-in, disconnected graphical fixture for isolated VNC testing.

use super::{App, Message, editor::Mode};
use cosmic::app::{Core, Settings, Task};
use cosmic::iced::{Limits, Size, Subscription};
use cosmic::prelude::*;
use cosmic::{Application, executor};

struct Fixture(App);

impl Application for Fixture {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = super::APP_ID;

    fn core(&self) -> &Core {
        self.0.core()
    }
    fn core_mut(&mut self) -> &mut Core {
        self.0.core_mut()
    }
    fn init(mut core: Core, _: ()) -> (Self, Task<Message>) {
        core.window.content_container = false;
        super::prover::register_highlighters();
        super::load_fonts();
        let mode = if std::env::var_os("DEDUCTUM_VNC_SOURCE").is_some() {
            Mode::Source
        } else {
            Mode::Literate
        };
        let mut app = if std::env::var_os("DEDUCTUM_VNC_INCOMPLETE_PROOF").is_some() {
            super::snapshot_lean::lean_app_with(mode, "def value : Nat := by\n  ")
        } else if std::env::var_os("DEDUCTUM_VNC_LEAN").is_some() {
            super::snapshot_lean::lean_app(mode)
        } else {
            super::snapshot::demo_app(mode)
        };
        app.core = core;
        app.shell.settings.display = super::settings::Settings::load().display;
        super::settings::publish(&app.shell.settings);
        if std::env::var_os("DEDUCTUM_VNC_SCRIPTS").is_some() {
            let text = "theory Scripts imports Main begin\n\ntext \\<open>Scripts: x\\<^sup>2, y\\<^sub>v, z\\<^sup>\\<alpha>, Q\\<^bsup>n+1\\<^esup>. Maths: $x\\<^sup>2 + y\\<^sub>\\<alpha>$ and $$z\\<^bsup>n+1\\<^esup>$$. Antiquotation @{term \"x\\<^sup>2\"}.\\<close>\n\nlemma sample: \"x\\<^sup>2 = x\\<^sup>2\" by simp\n\nend\n";
            let doc = serde_json::json!({"text": text, "revision": 1});
            let table = super::prover::isabelle::symbols::Table::fallback();
            if let Some(ws) = &mut app.workspace {
                ws.theory = Some(super::editor::Theory::new("/tmp/Scripts.thy".into(), &doc, &table, std::sync::Arc::new(std::collections::HashMap::new()), mode, super::prover::profile(None)));
                ws.output = Some(serde_json::json!({"messages":[{"kind":"state","text":"x\\<^sup>2 = y\\<^sub>\\<alpha>"}]}));
                ws.notes.clear();
            }
            app.show_sidebar = false;
            app.show_right = false;
        }
        if std::env::var_os("DEDUCTUM_VNC_LONG").is_some() {
            let mut text = String::from("theory Long imports Main begin\n\n");
            for i in 0..80 {
                if i % 10 == 0 {
                    text.push_str(&format!("section \\<open>Heading {i}\\<close>\n\ntext \\<open>\\<close>\n\n"));
                }
                text.push_str(&format!("text \\<open>Paragraph {i}: café naïve αβ repeated repeated; see [link](https://example.test) and $x + 0 = x$. This deliberately long paragraph wraps across multiple lines and continues with repeated repeated words for navigation.\\<close>\n\nlemma item_{i}: \"(a::nat) + 0 = a\"\n  apply simp\n  done\n\n"));
            }
            text.push_str("end\n");
            let doc = serde_json::json!({"text": text, "revision": 1});
            let table = super::prover::isabelle::symbols::Table::fallback();
            if let Some(ws) = &mut app.workspace {
                ws.theory = Some(super::editor::Theory::new("/tmp/Long.thy".into(), &doc, &table, std::sync::Arc::new(std::collections::HashMap::new()), mode, super::prover::profile(None)));
                ws.output = None;
                ws.notes.clear();
            }
            app.show_sidebar = false;
            app.show_right = false;
        }
        if std::env::var_os("DEDUCTUM_VNC_PENDING_PROOF").is_some() {
            app.show_sidebar = false;
            app.show_right = false;
            if let Some(ws) = &mut app.workspace {
                ws.output = None;
                if let Some(t) = &ws.theory {
                    let name = if std::env::var_os("DEDUCTUM_VNC_INCOMPLETE_PROOF").is_some() { "def" } else { "theorem" };
                    if let Some(i) = t.blocks.iter().position(|b| b.name == name) {
                        let message = if mode == Mode::Source {
                            Message::Goto(t.blocks[i].start + 1)
                        } else {
                            Message::EditBlock(i)
                        };
                        let _ = app.update(message);
                    }
                }
            }
        }
        if std::env::var_os("DEDUCTUM_VNC_NEW_SESSION").is_some() {
            let mut ns = super::NewSession { folder: household_widgets::ShortTextBox::starting_with("/fixture/a"), listing: Some(serde_json::json!({"path":"/fixture/a", "entries":[]})), ..Default::default() };
            ns.suggested(&serde_json::json!({"detections":[{"backend":"lean", "project":{"root":"/fixture/a"}}]}));
            app.new_session = Some(ns);
        }
        if let Some(machine) = app.workspace.as_ref().map(|ws| ws.machine.clone()) {
            let table = app.table(&machine);
            if let Some(ws) = &mut app.workspace { ws.rebuild_views(&*table); }
        }
        if std::env::var_os("DEDUCTUM_VNC_NEW_SESSION").is_none() {
            app.links.clear();
        }
        let fixture = Self(app);
        fixture.record();
        (fixture, Task::none())
    }
    fn header_start(&self) -> Vec<Element<'_, Message>> {
        self.0.header_start()
    }
    fn header_center(&self) -> Vec<Element<'_, Message>> {
        self.0.header_center()
    }
    fn header_end(&self) -> Vec<Element<'_, Message>> {
        self.0.header_end()
    }
    fn view(&self) -> Element<'_, Message> {
        self.0.view()
    }
    fn view_window(&self, id: cosmic::iced::window::Id) -> Element<'_, Message> {
        self.0.view_window(id)
    }
    fn on_close_requested(&self, id: cosmic::iced::window::Id) -> Option<Message> {
        self.0.on_close_requested(id)
    }
    fn subscription(&self) -> Subscription<Message> {
        let keys = cosmic::iced::event::listen_with(|event, _, _| {
            if let cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/deductum-vnc-keys.log") {
                    let _ = writeln!(file, "{key:?} {modifiers:?}");
                }
            }
            None::<Message>
        });
        Subscription::batch([self.0.subscription(), keys])
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if matches!(&message, Message::RawVim(..) | Message::ReadOnlyKey(..) | Message::WindowKey(..) | Message::LineMove(..) | Message::NavLayoutReady(..)) {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/deductum-vnc-input-actions.log") {
                let _ = writeln!(file, "{message:?}");
            }
        }
        if let Message::ViewAction(key, action) = &message {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/deductum-vnc-view-actions.log") {
                let _ = writeln!(file, "{key}: {action:?}");
            }
        }
        let task = self.0.update(message);
        // Simulate a late prover response on the next UI action, only in this disconnected fixture.
        if std::fs::remove_file("/tmp/deductum-vnc-goal").is_ok() {
            if let Some(ws) = &mut self.0.workspace {
                ws.output = Some(serde_json::json!({"messages":[]}));
            }
        }
        self.record();
        task
    }
}

impl Fixture {
    fn record(&self) {
        if let Some(ns) = &self.0.new_session {
            let value = serde_json::json!({"folder": ns.folder.value(), "backend": ns.backend, "listing": ns.listing.is_some(), "ready": ns.ready()});
            let _ = std::fs::write("/tmp/deductum-vnc-session.json", value.to_string());
        }
        let theory = self.0.workspace.as_ref().and_then(|w| w.theory.as_ref());
        let value = theory.map(|t| {
            let block = t.block_editor.as_ref().map(|b| serde_json::json!({
                "start": b.start, "end": b.end, "text": b.content.text(),
                "index": t.blocks.iter().position(|item| item.start <= b.start && b.start <= item.end),
                "wrapped": b.wrap.is_some(),
                "cursor": format!("{:?}", b.content.cursor()), "selection": b.content.selection(),
            }));
            serde_json::json!({"mode": format!("{:?}", t.mode), "cursor": t.cursor,
                "source": t.local, "source_cursor": format!("{:?}", t.source.cursor()),
                "source_selection": t.source.selection(), "block": block,
                "vim_enabled": self.0.vim_enabled, "vim_mode": format!("{:?}", self.0.vim.mode)})
        });
        if let Ok(bytes) = serde_json::to_vec_pretty(&value) {
            let _ = std::fs::write("/tmp/deductum-vnc-state.json", bytes);
        }
    }
}

/// Called from the test-only main-thread launcher, never from libtest's worker thread.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::default()
        .size(Size::new(1400.0, 900.0))
        .size_limits(Limits::NONE.min_width(640.0).min_height(400.0));
    cosmic::app::run::<Fixture>(settings, ())?;
    Ok(())
}

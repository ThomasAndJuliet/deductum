// SPDX-License-Identifier: GPL-3.0-only

//! A scripted driver for checking threads by eye, in debug builds only.
//!
//! `COSMIC_TERM_DEBUG_STEPS` holds steps separated by `;`, run in order once
//! the first tab is open, and `COSMIC_TERM_DEBUG_SHOTS` the directory the
//! window's own screenshots are written to, as `<name>.png`:
//!
//! `wait 2; shot plain; paper on; shot paper; scroll 0.5; selectall; copy;
//! dark; shot dark; image; wait 1; quit`
//!
//! `drop DESCRIPTOR` receives another window's tab of that session,
//! `dragout` carries this window's tab to another, `tabs` counts them.
//! `details on|off` shows or hides thinking and tool calls; `activity`
//! opens or closes the current activity. `mode NAME` sets the permission mode; `choose ANSWER` answers the first
//! open question the agent asked. `split`, `find TEXT`, `findnext` and `zoomin` act as their menu items;
//! `diagram SOURCE` opens a Mermaid diagram's window (`\n` for new lines).
//! `type TEXT` types keys no widget took, and `composer` prints the
//! composer's text; `page up` and `page down` press those keys; `paneclick` sends the click a pane reports for any click inside it, and
//! `focused` prints whether the thread's composer has the keys

use std::time::Duration;

use cosmic::Application;
use cosmic::app::Task;
use cosmic::iced::window;

use crate::{App, Message};

pub fn steps() -> Vec<String> {
    std::env::var("COSMIC_TERM_DEBUG_STEPS")
        .map(|steps| steps.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

impl App {
    pub(crate) fn debug_step(&mut self, index: usize) -> Task<Message> {
        let steps = steps();
        let Some(step) = steps.get(index) else { return Task::none() };
        eprintln!("debug step {index}: {step}");
        let next = move || Message::DebugStep(index + 1);
        let (verb, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let then = |task: Task<Message>| Task::batch([task, cosmic::task::message(next())]);
        match verb {
            "wait" => {
                let secs: f64 = rest.parse().unwrap_or(1.0);
                cosmic::task::future(async move {
                    tokio::time::sleep(Duration::from_secs_f64(secs)).await;
                    next()
                })
            }
            "shot" => {
                let Some(id) = self.image_windows.keys().next().copied().filter(|_| rest.starts_with("image"))
                    .or(self.core.main_window_id())
                else {
                    return cosmic::task::message(next());
                };
                let name = rest.to_string();
                window::screenshot(id).map(move |shot| cosmic::Action::App(Message::DebugShot(name.clone(), shot, index + 1)))
            }
            "paper" => then(self.update(Message::ThreadPaper(rest == "on"))),
            "dark" => then(self.update(Message::AppTheme(crate::config::AppTheme::Dark))),
            "light" => then(self.update(Message::AppTheme(crate::config::AppTheme::Light))),
            "tone" => {
                let mut threads = self.config.threads.clone();
                threads.paper_tone = match rest {
                    "white" => crate::config::PaperTone::White,
                    "sepia" => crate::config::PaperTone::Sepia,
                    _ => crate::config::PaperTone::Theme,
                };
                self.save_threads_config(threads);
                cosmic::task::message(next())
            }
            "font" => {
                let mut threads = self.config.threads.clone();
                threads.paper_font = match rest {
                    "cm" => crate::config::PaperFont::NewComputerModern,
                    "serif" => crate::config::PaperFont::Serif,
                    "sans" => crate::config::PaperFont::Sans,
                    _ => crate::config::PaperFont::Libertinus,
                };
                self.save_threads_config(threads);
                cosmic::task::message(next())
            }
            "selectall" => then(self.update(Message::ThreadSelectAll)),
            "copy" => {
                let text = self.active_thread().and_then(|t| t.selected_text()).unwrap_or_default();
                eprintln!("selected text ({} bytes):\n{text}", text.len());
                cosmic::task::message(next())
            }
            "expand" => {
                // Open every thinking and tool entry
                if let Some(id) = self.active_thread().map(|t| t.id)
                    && let Some(tab) = self.thread_mut(id)
                    && let Some(entries) = &tab.entries
                {
                    let ids: Vec<u64> = entries.items.iter().map(|i| i.id).collect();
                    tab.expanded.extend(ids);
                }
                cosmic::task::message(next())
            }
            "scroll" => {
                let Some(tab) = self.active_thread() else { return cosmic::task::message(next()) };
                let y: f32 = rest.parse().unwrap_or(0.0);
                then(cosmic::iced::widget::scrollable::snap_to(
                    tab.scroll_id.clone(),
                    cosmic::iced::widget::scrollable::RelativeOffset { x: None, y: Some(y) },
                ))
            }
            "send" => {
                let Some(id) = self.active_thread().map(|t| t.id) else { return cosmic::task::message(next()) };
                if let Some(tab) = self.thread_mut(id) {
                    tab.composer = cosmic::widget::text_editor::Content::with_text(rest);
                }
                then(self.thread_message(id, super::tab::Msg::Send))
            }
            "allow" => {
                let Some(tab) = self.active_thread() else { return cosmic::task::message(next()) };
                let id = tab.id;
                let request = tab.entries.as_ref().and_then(|e| e.permissions.first()).map(|r| r.request_id.clone());
                match request {
                    Some(request) => then(self.thread_message(
                        id,
                        super::tab::Msg::Answer(request, super::claude::Decision::Allow),
                    )),
                    None => cosmic::task::message(next()),
                }
            }
            "image" => {
                let path = self.active_thread().and_then(|t| {
                    t.entries.as_ref()?.items.iter().find_map(|i| match &i.kind {
                        super::model::ItemKind::Tool(tool) => tool.images.first().map(|i| i.path.clone()),
                        super::model::ItemKind::User { images, .. } => images.first().map(|i| i.path.clone()),
                        _ => None,
                    })
                });
                match path {
                    Some(path) => then(self.open_image(path)),
                    None => cosmic::task::message(next()),
                }
            }
            "zoom" => {
                let Some(id) = self.image_windows.keys().next().copied() else {
                    return cosmic::task::message(next());
                };
                let zoom = match rest {
                    "in" => super::viewer::Zoom::In,
                    "out" => super::viewer::Zoom::Out,
                    "actual" => super::viewer::Zoom::Actual,
                    _ => super::viewer::Zoom::Fit,
                };
                then(self.update(Message::ImageZoom(id, zoom)))
            }
            "page" => {
                let named = if rest == "up" {
                    cosmic::iced::keyboard::key::Named::PageUp
                } else {
                    cosmic::iced::keyboard::key::Named::PageDown
                };
                then(self.update(Message::Key(
                    cosmic::iced::keyboard::Modifiers::empty(),
                    cosmic::iced::keyboard::key::Physical::Unidentified(
                        cosmic::iced::keyboard::key::NativeCode::Unidentified,
                    ),
                    cosmic::iced::keyboard::Key::Named(named),
                )))
            }
            "type" => {
                // As keys no widget took, one by one
                let tasks: Vec<Task<Message>> = rest
                    .chars()
                    .map(|c| {
                        self.update(Message::KeyUnclaimed(
                            cosmic::iced::keyboard::Modifiers::empty(),
                            cosmic::iced::keyboard::key::Physical::Unidentified(
                                cosmic::iced::keyboard::key::NativeCode::Unidentified,
                            ),
                            cosmic::iced::keyboard::Key::Character(c.to_string().into()),
                            Some(c.to_string()),
                        ))
                    })
                    .collect();
                then(Task::batch(tasks))
            }
            "composer" => {
                let text = self.active_thread().map(|t| t.composer.text()).unwrap_or_default();
                eprintln!("composer: {:?}", text.trim_end());
                cosmic::task::message(next())
            }
            "mode" => {
                let Some(id) = self.active_thread().map(|t| t.id) else { return cosmic::task::message(next()) };
                match super::tab::MODES.iter().position(|(value, _)| *value == rest) {
                    Some(index) => then(self.thread_message(id, super::tab::Msg::SetMode(index))),
                    None => cosmic::task::message(next()),
                }
            }
            "choose" => {
                // The first open question of the first prompt that asks some
                let found = self.active_thread().and_then(|t| {
                    let request = t.entries.as_ref()?.permissions.iter().find(|r| !r.questions.is_empty())?;
                    let question = request.questions.iter().find(|q| !t.answers.contains_key(&q.key))?;
                    Some((t.id, request.request_id.clone(), question.key.clone()))
                });
                eprintln!("choose {rest:?}: {}", if found.is_some() { "asked" } else { "no question" });
                match found {
                    Some((id, request, key)) => then(self.thread_message(id, super::tab::Msg::Choose(request, key, rest.to_string()))),
                    None => cosmic::task::message(next()),
                }
            }
            "details" => {
                let on = rest == "on";
                then(Task::batch([
                    self.update(Message::ThreadShowThinking(Some(on))),
                    self.update(Message::ThreadShowTools(Some(on))),
                ]))
            }
            "activity" => match self.active_thread().map(|t| t.id) {
                Some(id) => then(self.thread_message(id, super::tab::Msg::ToggleActivity)),
                None => cosmic::task::message(next()),
            },
            // A tab of another window dropped here, carrying this session
            "drop" => match crate::agent::Session::from_descriptor(rest) {
                Some(session) => then(self.update(Message::TabDropped(
                    self.pane_model.focused(),
                    Some(crate::dnd::DndDrop { paths: Vec::new(), session: Some(session) }),
                ))),
                None => cosmic::task::message(next()),
            },
            // This window's thread tab carried to another, which took it
            "dragout" => {
                let pane = self.pane_model.focused();
                let Some(entity) = self.pane_model.active().map(|t| t.active()) else { return cosmic::task::message(next()) };
                let relay = self.active_thread().and_then(|t| t.relay.as_ref().map(|r| r.descriptor()));
                eprintln!("dragging out {relay:?}");
                self.tab_drag = Some((pane, entity, std::time::Instant::now() - Duration::from_secs(1)));
                self.tab_drag_left_window = true;
                then(self.update(Message::TabDragEnded(true)))
            }
            "tabs" => {
                let count = self.pane_model.panes.iter().map(|(_, t)| t.iter().count()).sum::<usize>();
                eprintln!("tabs: {count}, threads: {}", self.thread_ids().len());
                cosmic::task::message(next())
            }
            "split" => then(self.update(Message::PaneSplit(cosmic::widget::pane_grid::Axis::Vertical))),
            "find" => then(Task::batch([
                self.update(Message::Find(true)),
                self.update(Message::FindSearchValueChanged(rest.to_string())),
            ])),
            "findnext" => then(self.update(Message::FindNext)),
            "zoomin" => then(self.update(Message::ZoomIn)),
            "diagram" => then(self.open_diagram(rest.replace("\\n", "\n"))),
            "save" => {
                self.save_tabs();
                cosmic::task::message(next())
            }
            "paneclick" => then(self.update(Message::PaneClicked(self.pane_model.focused()))),
            "focus" => match self.active_thread().map(|t| t.composer_id.clone()) {
                Some(id) => then(cosmic::widget::text_input::focus(id)),
                None => cosmic::task::message(next()),
            },
            "focused" => {
                let Some(id) = self.active_thread().map(|t| t.composer_id.clone()) else {
                    return cosmic::task::message(next());
                };
                cosmic::iced::runtime::widget::operation::is_focused(id).map(move |focused| {
                    eprintln!("composer focused: {focused}");
                    cosmic::Action::App(next())
                })
            }
            "memory" => {
                let (loaded, agents, bytes) = self.thread_memory();
                eprintln!("threads loaded {loaded}, agents {agents}, about {bytes} bytes");
                cosmic::task::message(next())
            }
            "quit" => cosmic::iced::exit(),
            _ => {
                log::warn!("unknown debug step {step:?}");
                cosmic::task::message(next())
            }
        }
    }

    pub(crate) fn debug_shot(&mut self, name: String, shot: window::Screenshot, next: usize) -> Task<Message> {
        let dir = std::env::var("COSMIC_TERM_DEBUG_SHOTS").unwrap_or_else(|_| ".".into());
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        match image::RgbaImage::from_raw(shot.size.width, shot.size.height, shot.rgba.to_vec()) {
            Some(image) => match image.save(&path) {
                Ok(()) => log::info!("debug screenshot {}", path.display()),
                Err(err) => log::warn!("debug screenshot {}: {err}", path.display()),
            },
            None => log::warn!("debug screenshot {name}: bad size"),
        }
        self.debug_step(next)
    }
}

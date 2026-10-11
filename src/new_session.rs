// SPDX-License-Identifier: GPL-3.0-only

//! Starting a session (design: provers §6.4). Pick the machine, browse to a folder; the host's
//! detection (`suggest` → `detections`) then shows one card per backend that recognised the
//! folder, with its evidence. Picking a card shows that backend's project form:
//!
//! - Isabelle: the runtime (from `hello`'s `backends[].toolchains`), the base logic and the session
//!   directories;
//! - Lean: the Lake root and toolchain, read-only since they come from the project's files, with
//!   the host's note when the toolchain or a cache is missing.
//!
//! A folder with no evidence offers every backend the host has. Start session sends
//! `session_create {folder, backend, toolchain, project}`; Isabelle's logic and directories also
//! go at the top level, as older hosts read them.
//!
//! A document clicked in the folder browser, or named on the command line, opens in a session of
//! its own backend: the session on that machine that holds it, else a new one for its folder.

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::Application;
use cosmic::app::Task;
use cosmic::prelude::*;
use cosmic::widget::{self, button, settings};
use cosmic::{iced, theme};
use serde_json::{Value, json};

use super::shell::{self, Msg, backend_mark, sp};
use super::{App, Message};

/// The new-session page's state
#[derive(Default)]
pub struct NewSession {
    /// The machine, as an index of the app's links
    pub machine: usize,
    pub folder: String,
    pub listing: Option<Value>,
    /// What each backend found in the folder, as the host's `suggest` says; none yet while asked
    pub detections: Option<Vec<Value>>,
    /// The backend picked (a card), and its toolchain picked (an id)
    pub backend: Option<String>,
    pub toolchain: Option<String>,
    /// Isabelle's base logic and session directories, as typed
    pub logic: String,
    pub dirs: String,
    /// The host's older suggestion (Isabelle's), for hosts without detections
    pub suggestion: Option<Value>,
    pub error: Option<String>,
    /// Why the page opened, when a document of a backend without a session was opened
    pub reason: Option<String>,
}

impl NewSession {
    /// The detection of a backend, if it recognised the folder
    pub fn detection(&self, backend: &str) -> Option<&Value> {
        self.detections.as_ref()?.iter().find(|d| d["backend"].as_str() == Some(backend))
    }

    /// The suggest reply, taken in: the cards, and the form's starting values
    pub fn suggested(&mut self, v: &Value) {
        self.suggestion = Some(v["suggestion"].clone());
        let detections: Vec<Value> = v["detections"].as_array().cloned().unwrap_or_default();
        // (one backend found: its card is picked; a backend asked for before stays picked)
        if self.backend.as_ref().is_none_or(|b| !detections.iter().any(|d| d["backend"].as_str() == Some(b))) {
            self.backend = match detections.as_slice() {
                [one] => one["backend"].as_str().map(str::to_string),
                _ => self.backend.take().filter(|_| detections.is_empty()),
            };
        }
        self.detections = Some(detections);
        let isabelle = self.detection("isabelle").map(|d| d["project"].clone());
        let project = isabelle.as_ref().unwrap_or(&v["suggestion"]);
        self.logic = project["logic"].as_str().unwrap_or("HOL").to_string();
        self.dirs = project["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
        self.toolchain = None;
    }

    /// `session_create`'s parameters for the backend picked (Isabelle when the host detected nothing
    /// and nothing was picked)
    pub fn create_params(&self) -> Value {
        let backend = self.backend.clone().unwrap_or_else(|| "isabelle".into());
        let mut params = json!({"folder": self.folder, "start": true, "resume": true, "backend": backend});
        let detected = self.detection(&backend);
        match backend.as_str() {
            "isabelle" => {
                let logic = self.logic.trim().to_string();
                let dirs: Vec<String> = self.dirs.split([',', ' ']).map(str::trim).filter(|d| !d.is_empty()).map(str::to_string).collect();
                let mut project = json!({"dirs": dirs});
                if !logic.is_empty() {
                    project["logic"] = json!(logic);
                }
                // (the sessions a detected logic includes, while that logic is the one asked for)
                let suggested = detected.map(|d| &d["project"]).or(self.suggestion.as_ref());
                if let Some(s) = suggested
                    && s["logic"].as_str() == Some(logic.as_str())
                {
                    project["include_sessions"] = s["include_sessions"].clone();
                }
                for key in ["logic", "dirs", "include_sessions"] {
                    if !project[key].is_null() {
                        params[key] = project[key].clone();
                    }
                }
                params["project"] = project;
                if let Some(t) = self.toolchain.clone().or_else(|| detected.and_then(|d| d["toolchain"].as_str()).map(str::to_string)) {
                    params["toolchain"] = json!(t);
                }
            }
            _ => {
                // The project as detected; the toolchain is the project's own, which the host
                // resolves (and has elan fetch when it is missing)
                if let Some(d) = detected {
                    params["project"] = d["project"].clone();
                }
                if let Some(t) = &self.toolchain {
                    params["toolchain"] = json!(t);
                }
            }
        }
        params
    }
}

impl App {
    /// The backends a machine's host has, with their toolchains and documents (`hello`); a host
    /// older than backends ran Isabelle only
    pub fn backends(&self, machine: usize) -> Vec<Value> {
        let hello = self.links.get(machine).and_then(|l| l.hello.as_ref());
        match hello.and_then(|h| h["backends"].as_array()).filter(|b| !b.is_empty()) {
            Some(b) => b.clone(),
            None => vec![json!({"backend": "isabelle", "label": "Isabelle", "documents": [".thy"],
                                "toolchains": hello.and_then(|h| h["runtimes"].as_array()).cloned().unwrap_or_default()
                                    .into_iter().map(|r| json!({"id": r["path"], "label": r["path"].as_str().and_then(|p| p.rsplit('/').next()), "ok": r["supported"]})).collect::<Vec<_>>()})],
        }
    }

    /// A backend's toolchains on a machine
    pub fn toolchains(&self, machine: usize, backend: &str) -> Vec<Value> {
        self.backends(machine).into_iter().find(|b| b["backend"].as_str() == Some(backend)).and_then(|b| b["toolchains"].as_array().cloned()).unwrap_or_default()
    }

    pub fn new_session_backend(&mut self, backend: String) -> Task<Message> {
        if let Some(ns) = &mut self.new_session {
            if ns.backend.as_deref() != Some(backend.as_str()) {
                ns.toolchain = None;
            }
            ns.backend = Some(backend);
        }
        Task::none()
    }

    /// The backend whose documents a path is, by the extensions a machine's host lists
    pub fn backend_of(&self, machine: usize, path: &str) -> Option<String> {
        self.backends(machine).iter().find_map(|b| {
            let docs = b["documents"].as_array()?;
            docs.iter().filter_map(Value::as_str).any(|ext| path.ends_with(ext)).then(|| b["backend"].as_str().map(str::to_string)).flatten()
        })
    }

    /// The session on a machine that holds a document of this backend: one whose folder holds it
    /// (the innermost folder wins)
    pub fn session_for(&self, machine: &str, backend: &str, path: &str) -> Option<String> {
        self.sessions
            .iter()
            .filter(|((m, _), s)| m == machine && s["backend"].as_str().unwrap_or("isabelle") == backend)
            .filter_map(|((_, id), s)| {
                let folder = s["folder"].as_str()?.trim_end_matches('/');
                path.strip_prefix(folder).filter(|rest| rest.starts_with('/')).map(|_| (folder.len(), id.clone()))
            })
            .max()
            .map(|(_, id)| id)
    }

    /// A document clicked in the folder browser: in the session of its backend that holds it, else
    /// the new-session form for its folder with its backend picked
    pub fn open_document(&mut self, path: String) -> Task<Message> {
        let Some(ns) = &self.new_session else { return Task::none() };
        let index = ns.machine;
        let Some(machine) = self.links.get(index).map(|l| l.target.key.clone()) else { return Task::none() };
        let backend = self.backend_of(index, &path);
        if let Some(b) = &backend
            && let Some(sid) = self.session_for(&machine, b, &path)
        {
            self.new_session = None;
            self.want_theory = Some(path);
            return self.update(Message::OpenSession(machine, sid));
        }
        let folder = std::path::Path::new(&path).parent().map(|p| p.display().to_string()).unwrap_or_default();
        if let Some(ns) = &mut self.new_session {
            ns.backend = backend.clone();
            ns.reason = backend.map(|b| format!("{} is a {} document, and no session holds it: start one for its folder", super::file_name(&path), shell::backend_label(&b)));
        }
        self.update(Message::NewBrowse(folder))
    }
}

pub fn page<'a>(app: &'a App, ns: &'a NewSession) -> Element<'a, Message> {
    let s = sp();
    let names: Vec<String> = app.links.iter().map(|l| l.name.clone()).collect();
    let mut col = widget::column::with_capacity(16).spacing(s.space_m).padding(s.space_l).max_width(820);
    col = col.push(widget::text::title3("New session"));
    col = col.push(widget::text::body(
        "A session is one shared prover for a folder: an Isabelle session for its theories, a Lean server for its Lake package. It keeps running when you close the window, and agents can attach to it.",
    ));
    if let Some(reason) = &ns.reason {
        col = col.push(note(reason.clone(), false));
    }
    // Where
    let mut place = settings::section().title("Folder").add(settings::item("Machine", widget::dropdown(names, Some(ns.machine), Message::NewMachine)));
    place = place.add(settings::item(
        "Folder",
        widget::text_input("/path/to/project", &ns.folder).on_input(Message::NewFolder).on_submit(Message::NewBrowse).width(Length::Fixed(420.0)),
    ));
    col = col.push(place);
    if let Some(listing) = &ns.listing {
        col = col.push(browser(listing));
    }
    // What the host found there: a card per backend
    if !ns.folder.is_empty() && ns.listing.is_some() {
        col = col.push(widget::text::heading("Backend"));
        match &ns.detections {
            None => col = col.push(widget::row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(shell::spinner(14.0)).push(widget::text::caption("Looking at the folder…"))),
            Some(found) if found.is_empty() => {
                col = col.push(widget::text::caption("Nothing here says which prover it is for (no ROOT, no lakefile, no documents): pick one."));
                for b in app.backends(ns.machine) {
                    let backend = b["backend"].as_str().unwrap_or("isabelle").to_string();
                    let documents: Vec<&str> = b["documents"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    col = col.push(card(&backend, format!("No {} project found", shell::backend_label(&backend)), vec![format!("Documents: {}", documents.join(", "))], ns.backend.as_deref() == Some(backend.as_str())));
                }
            }
            Some(found) => {
                if found.len() > 1 && ns.backend.is_none() {
                    col = col.push(widget::text::caption("More than one prover recognises this folder: pick the one to start. A folder can have a session of each."));
                }
                for d in found {
                    let backend = d["backend"].as_str().unwrap_or("isabelle").to_string();
                    let evidence: Vec<String> = d["evidence"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect();
                    col = col.push(card(&backend, summary(d), evidence, ns.backend.as_deref() == Some(backend.as_str())));
                }
            }
        }
        if let Some(backend) = &ns.backend {
            col = col.push(form(app, ns, backend));
        }
    }
    if let Some(err) = &ns.error {
        col = col.push(note(err.clone(), true));
    }
    let ready = !ns.folder.is_empty() && (ns.backend.is_some() || ns.detections.as_ref().is_some_and(|d| d.is_empty()));
    col = col.push(
        widget::row::with_capacity(3)
            .spacing(s.space_s)
            .push(widget::Space::new().width(Length::Fill))
            .push(widget::button::standard("Cancel").on_press(Message::NewSession(false)))
            .push(widget::button::suggested("Start session").on_press_maybe(ready.then_some(Message::NewCreate))),
    );
    widget::scrollable(widget::container(col).center_x(Length::Fill)).height(Length::Fill).into()
}

/// A detection in a line: "Lake package, Mathlib, toolchain v4.19.0", "ROOT session Foo, 12 .thy files"
pub fn summary(d: &Value) -> String {
    let p = &d["project"];
    match d["backend"].as_str() {
        Some("lean") => {
            let mut parts = vec!["Lake package".to_string()];
            if p["mathlib"].as_bool() == Some(true) {
                parts.push("Mathlib".into());
            }
            if let Some(tc) = p["toolchain"].as_str().or(d["toolchain"].as_str()) {
                parts.push(format!("toolchain {}", tc.rsplit(':').next().unwrap_or(tc)));
            }
            parts.join(", ")
        }
        _ => {
            let mut parts = Vec::new();
            let evidence: Vec<&str> = d["evidence"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if let Some(session) = p["session"].as_str() {
                parts.push(format!("ROOT session {session}"));
            } else if evidence.iter().any(|e| e.starts_with("ROOT")) {
                parts.push("ROOT file".into());
            }
            if let Some(n) = evidence.iter().find(|e| e.ends_with(".thy files") || e.ends_with(".thy file")) {
                parts.push(n.to_string());
            }
            if let Some(logic) = p["logic"].as_str() {
                parts.push(format!("base logic {logic}"));
            }
            if parts.is_empty() { "Isabelle theories".into() } else { parts.join(", ") }
        }
    }
}

/// A backend's card: its mark and name, what was found, and the evidence; picked, it is outlined
fn card<'a>(backend: &str, summary: String, evidence: Vec<String>, picked: bool) -> Element<'a, Message> {
    let s = sp();
    let mut col = widget::column::with_capacity(4).spacing(4);
    col = col.push(
        widget::row::with_capacity(4)
            .spacing(s.space_xs)
            .align_y(Alignment::Center)
            .push(backend_mark(backend))
            .push(widget::text::body(shell::backend_label(backend)).font(cosmic::font::semibold()))
            .push(widget::text::body(summary).width(Length::Fill))
            .push(if picked { shell::tinted_icon("object-select-symbolic", 16, shell::accent) } else { widget::Space::new().width(Length::Fixed(16.0)).into() }),
    );
    for e in evidence {
        col = col.push(widget::container(widget::text::caption(e).wrapping(iced::widget::text::Wrapping::WordOrGlyph)).padding(Padding { left: 26.0, ..Padding::ZERO }));
    }
    button::custom(col)
        .width(Length::Fill)
        .padding([10, 14])
        .class(card_style(picked))
        .on_press(Message::Shell(Msg::NewBackend(backend.to_string())))
        .into()
}

fn card_style(picked: bool) -> theme::Button {
    fn style(theme: &cosmic::Theme, picked: bool, hover: bool) -> button::Style {
        let c = theme.cosmic();
        let text: Color = c.on_bg_color().into();
        let accent: Color = c.accent_color().into();
        let mut s = button::Style::new();
        s.border_radius = 10.0.into();
        s.background = Some(shell::blend(Color::from(c.bg_component_color()), if picked { accent } else { text }, if picked { 0.10 } else if hover { 0.05 } else { 0.0 }).into());
        s.border_width = if picked { 2.0 } else { 1.0 };
        s.border_color = if picked { accent } else { Color { a: 0.12, ..text } };
        s.text_color = Some(text);
        s.icon_color = Some(text);
        s
    }
    theme::Button::Custom {
        active: Box::new(move |_, t| style(t, picked, false)),
        disabled: Box::new(move |t| style(t, picked, false)),
        hovered: Box::new(move |_, t| style(t, picked, true)),
        pressed: Box::new(move |_, t| style(t, picked, true)),
    }
}

/// The picked backend's project form
fn form<'a>(app: &'a App, ns: &'a NewSession, backend: &str) -> Element<'a, Message> {
    let detected = ns.detection(backend);
    let mut section = settings::section().title(format!("{} project", shell::backend_label(backend)));
    match backend {
        "isabelle" => {
            let toolchains: Vec<Value> = app.toolchains(ns.machine, "isabelle");
            let labels: Vec<String> = toolchains
                .iter()
                .map(|t| {
                    let label = t["label"].as_str().unwrap_or("?").to_string();
                    if t["ok"] == json!(false) { format!("{label} (not usable)") } else { label }
                })
                .collect();
            let wanted = ns.toolchain.clone().or_else(|| detected.and_then(|d| d["toolchain"].as_str()).map(str::to_string));
            let selected = wanted
                .and_then(|w| toolchains.iter().position(|t| t["id"].as_str() == Some(w.as_str())))
                .or_else(|| toolchains.iter().position(|t| t["ok"] != json!(false)));
            if toolchains.is_empty() {
                section = section.add(settings::item::builder("Runtime").description("The host found no Isabelle installation with a Deductum backend").control(widget::Space::new()));
            } else {
                section = section.add(settings::item::builder("Runtime").description("The Isabelle installation the prover runs in").control(widget::dropdown(labels, selected, |i| Message::Shell(Msg::NewToolchain(i)))));
            }
            if let Some(problem) = selected.and_then(|i| toolchains[i]["problem"].as_str()) {
                section = section.add(settings::item::builder("").description(problem.to_string()).control(widget::Space::new()));
            }
            section = section
                .add(settings::item::builder("Base logic").description("The heap the theories build on; its parent session when the folder is one").control(widget::text_input("HOL", &ns.logic).on_input(Message::NewLogic).width(Length::Fixed(240.0))))
                .add(settings::item::builder("Session directories").description("Other folders whose ROOT sessions the theories import, separated by commas").control(widget::text_input("none", &ns.dirs).on_input(|d| Message::Shell(Msg::NewDirs(d))).width(Length::Fixed(240.0))));
        }
        "lean" => {
            let p = detected.map(|d| &d["project"]);
            let root = p.and_then(|p| p["lake_root"].as_str()).unwrap_or("(the folder's Lake package)").to_string();
            let tc = p.and_then(|p| p["toolchain"].as_str()).or(detected.and_then(|d| d["toolchain"].as_str())).unwrap_or("the newest installed").to_string();
            section = section
                .add(settings::item::builder("Lake root").description("From the lakefile: the session runs lake serve here").control(widget::text::body(root).wrapping(iced::widget::text::Wrapping::WordOrGlyph)))
                .add(settings::item::builder("Toolchain").description("From lean-toolchain, through elan").control(widget::text::body(tc)));
            let packages: Vec<&str> = p.and_then(|p| p["packages"].as_array()).into_iter().flatten().filter_map(Value::as_str).collect();
            if !packages.is_empty() {
                section = section.add(settings::item::builder("Packages").control(widget::text::body(packages.join(", "))));
            }
        }
        _ => {}
    }
    let mut col = widget::column::with_capacity(2).spacing(sp().space_s).push(section);
    if let Some(n) = detected.and_then(|d| d["note"].as_str()).filter(|n| !n.is_empty()) {
        col = col.push(note(n.to_string(), false));
    }
    col.into()
}

/// A note in a tinted box (an error in the destructive colour)
fn note<'a>(text: String, error: bool) -> Element<'a, Message> {
    let icon = if error { shell::tinted_icon("dialog-error-symbolic", 16, shell::destructive) } else { shell::tinted_icon("dialog-information-symbolic", 16, shell::accent) };
    widget::container(widget::row::with_capacity(2).spacing(sp().space_xs).align_y(Alignment::Center).push(icon).push(widget::text::body(text).width(Length::Fill)))
        .padding([8, 12])
        .width(Length::Fill)
        .class(theme::Container::custom(move |t| {
            let c: Color = if error { t.cosmic().destructive_color().into() } else { t.cosmic().accent_color().into() };
            iced::widget::container::Style {
                background: Some(Color { a: 0.10, ..c }.into()),
                border: Border { radius: 8.0.into(), width: 1.0, color: Color { a: 0.35, ..c } },
                ..Default::default()
            }
        }))
        .into()
}

/// The folder's entries: folders to browse into, documents to open in their backend's session
fn browser<'a>(listing: &'a Value) -> Element<'a, Message> {
    let mut list = widget::column::with_capacity(40).spacing(1);
    let row = |icon: &'static str, label: String| {
        widget::row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(widget::icon::from_name(icon).size(16).icon()).push(widget::text::body(label).wrapping(iced::widget::text::Wrapping::None))
    };
    if let Some(parent) = listing["parent"].as_str() {
        list = list.push(button::custom(row("go-up-symbolic", "..".into())).class(super::sidebar::nav_style(false)).padding([4, 8]).width(Length::Fill).on_press(Message::NewBrowse(parent.to_string())));
    }
    let base = listing["path"].as_str().unwrap_or("");
    for e in listing["entries"].as_array().into_iter().flatten() {
        let name = e["name"].as_str().unwrap_or("");
        let dir = e["dir"].as_bool() == Some(true);
        let full = format!("{}/{}", base.trim_end_matches('/'), name);
        let b = button::custom(row(if dir { "folder-symbolic" } else { "text-x-generic-symbolic" }, if dir { format!("{name}/") } else { name.to_string() }))
            .class(super::sidebar::nav_style(false))
            .padding([4, 8])
            .width(Length::Fill);
        list = list.push(if dir { b.on_press(Message::NewBrowse(full)) } else { b.on_press(Message::Shell(Msg::OpenDocument(full))) });
    }
    widget::container(widget::scrollable(list)).height(Length::Fixed(240.0)).padding(4).class(theme::Container::Card).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both() -> Value {
        json!({"suggestion": {"logic": "HOL", "dirs": [], "include_sessions": []}, "detections": [
            {"backend": "isabelle", "evidence": ["ROOT in /p", "4 .thy files"], "project": {"logic": "HOL-Library", "dirs": ["../lib"], "include_sessions": ["Foo"], "session": "Demo"}, "toolchain": "/opt/Isabelle2025"},
            {"backend": "lean", "evidence": ["lakefile.toml in /p", "lean-toolchain: leanprover/lean4:v4.19.0"], "project": {"lake_root": "/p", "toolchain": "leanprover/lean4:v4.19.0", "packages": [], "mathlib": false}, "toolchain": "leanprover/lean4:v4.19.0", "note": null}]})
    }

    #[test]
    fn a_folder_of_two_backends_waits_for_a_card_and_each_card_starts_its_own() {
        let mut ns = NewSession { folder: "/p".into(), ..Default::default() };
        ns.suggested(&both());
        assert_eq!(ns.backend, None, "two backends: none picked for the human");
        assert_eq!(ns.logic, "HOL-Library");
        assert_eq!(ns.dirs, "../lib");
        ns.backend = Some("isabelle".into());
        let p = ns.create_params();
        assert_eq!(p["backend"], "isabelle");
        assert_eq!(p["toolchain"], "/opt/Isabelle2025");
        assert_eq!(p["project"]["logic"], "HOL-Library");
        assert_eq!(p["project"]["include_sessions"], json!(["Foo"]));
        assert_eq!(p["logic"], "HOL-Library", "also at the top level, for older hosts");
        ns.backend = Some("lean".into());
        let p = ns.create_params();
        assert_eq!(p["backend"], "lean");
        assert_eq!(p["project"]["lake_root"], "/p");
        assert!(p.get("logic").is_none() && p.get("toolchain").is_none(), "{p}");
        assert_eq!(summary(ns.detection("lean").unwrap()), "Lake package, toolchain v4.19.0");
        assert_eq!(summary(ns.detection("isabelle").unwrap()), "ROOT session Demo, 4 .thy files, base logic HOL-Library");
    }

    #[test]
    fn one_backend_is_picked_and_a_changed_logic_drops_its_sessions() {
        let mut ns = NewSession { folder: "/p".into(), ..Default::default() };
        let mut v = both();
        v["detections"].as_array_mut().unwrap().pop();
        ns.suggested(&v);
        assert_eq!(ns.backend.as_deref(), Some("isabelle"));
        ns.logic = "HOL".into();
        ns.dirs = "a, b".into();
        let p = ns.create_params();
        assert_eq!(p["project"]["dirs"], json!(["a", "b"]));
        assert!(p["project"].get("include_sessions").is_none());
    }
}

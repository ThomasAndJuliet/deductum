// SPDX-License-Identifier: GPL-3.0-only

//! Starting a session (design: provers §6.4). Pick the machine, type or browse to a folder; the host's
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
//! A document named on the command line opens in a session of its own backend: the session on that
//! machine that holds it, else a new one for its folder.
//!
//! Browse picks the folder in a dialog. For this computer that is the desktop's own file chooser
//! (the portal, which on COSMIC is COSMIC Files' dialog; on Windows the system folder dialog, both
//! through libcosmic's `dialog`). A remote machine has no such mount on every platform, so its
//! folders are listed by the host (`list_dir`) in `Picker`, Deductum's own dialog.

use cosmic::iced::{Alignment, Border, Color, Length, Padding};
use cosmic::Application;
use cosmic::app::Task;
use cosmic::prelude::*;
use cosmic::widget::{self, button};
use cosmic::{iced, theme};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

use household_widgets::ShortTextBox;

use super::fields::Field;
use super::shell::{self, Msg, backend_mark, sp};
use super::{App, Message};

/// The new-session page's state
#[derive(Default)]
pub struct NewSession {
    /// The machine, as an index of the app's links
    pub machine: usize,
    pub folder: ShortTextBox,
    pub listing: Option<Value>,
    /// What each backend found in the folder, as the host's `suggest` says; none yet while asked
    pub detections: Option<Vec<Value>>,
    /// The backend picked (a card), and its toolchain picked (an id)
    pub backend: Option<String>,
    pub toolchain: Option<String>,
    /// Isabelle's base logic and session directories, as typed
    pub logic: ShortTextBox,
    pub dirs: ShortTextBox,
    /// The host's older suggestion (Isabelle's), for hosts without detections
    pub suggestion: Option<Value>,
    pub error: Option<String>,
    /// Why the page opened, when a document of a backend without a session was opened
    pub reason: Option<String>,
    /// Changes on every folder edit or navigation, including across form instances.
    pub generation: u64,
    /// The prover's other choices are shown (Change was pressed)
    pub changing: bool,
    /// Deductum's folder dialog for a remote machine, while it is open
    pub picker: Option<Picker>,
}

/// The remote folder dialog: the folder it is looking at and what the host listed there
#[derive(Default)]
pub struct Picker {
    pub path: String,
    pub listing: Option<Value>,
    pub error: Option<String>,
    /// Changes on every request, so that a late reply to an earlier one is dropped
    pub seq: u64,
}

impl Picker {
    /// The folders of a listing, by name (the host leaves out hidden ones)
    pub fn folders(&self) -> Vec<String> {
        self.listing.as_ref().and_then(|l| l["entries"].as_array()).into_iter().flatten().filter(|e| e["dir"].as_bool() == Some(true)).filter_map(|e| e["name"].as_str().map(str::to_string)).collect()
    }
    /// The folder a name in this listing is
    pub fn child(&self, name: &str) -> String {
        format!("{}/{}", self.path.trim_end_matches('/'), name)
    }
    pub fn parent(&self) -> Option<String> {
        self.listing.as_ref()?["parent"].as_str().filter(|p| *p != self.path).map(str::to_string)
    }
}

impl NewSession {
    pub fn invalidate_folder(&mut self) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        self.generation = NEXT.fetch_add(1, Ordering::Relaxed);
        self.listing = None;
        self.detections = None;
        self.backend = None;
        self.toolchain = None;
        self.suggestion = None;
        self.logic = ShortTextBox::new();
        self.dirs = ShortTextBox::new();
        self.error = None;
        self.changing = false;
    }

    pub fn ready(&self) -> bool {
        !self.folder.is_empty() && self.listing.is_some() && self.detections.is_some() && self.backend.is_some()
    }
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
        // (where the fields start, not a change to them)
        self.logic = ShortTextBox::starting_with(project["logic"].as_str().unwrap_or("HOL"));
        self.dirs = ShortTextBox::starting_with(&project["dirs"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", "));
        self.toolchain = None;
    }

    /// `session_create`'s parameters for the backend picked (Isabelle when the host detected nothing
    /// and nothing was picked)
    pub fn create_params(&self) -> Value {
        let backend = self.backend.clone().unwrap_or_else(|| "isabelle".into());
        let mut params = json!({"folder": self.folder.value(), "start": true, "resume": true, "backend": backend});
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
            ns.changing = false;
        }
        Task::none()
    }

    /// Browse: this computer's own folder dialog, or Deductum's over the remote link
    pub fn new_session_browse(&mut self) -> Task<Message> {
        let Some(ns) = &mut self.new_session else { return Task::none() };
        let Some(link) = self.links.get(ns.machine) else { return Task::none() };
        if link.target.destination.is_none() {
            return cosmic::task::future(async {
                use cosmic::dialog::file_chooser::{Error, open::Dialog};
                match Dialog::new().title("Choose a folder").open_folder().await {
                    Ok(response) => match response.url().to_file_path() {
                        Ok(path) => Message::Shell(Msg::NewBrowseTo(path.display().to_string())),
                        Err(()) => Message::Shell(Msg::NewBrowseFailed("the folder is not on this computer".into())),
                    },
                    Err(Error::Cancelled) => Message::Noop,
                    Err(why) => Message::Shell(Msg::NewBrowseFailed(why.to_string())),
                }
            });
        }
        // (the dialog opens in the folder typed, else the machine's home)
        ns.picker = Some(Picker { path: ns.folder.to_string(), ..Default::default() });
        self.picker_go(None)
    }

    /// The remote dialog to a folder (none: the one it is in, or the machine's home)
    pub fn picker_go(&mut self, path: Option<String>) -> Task<Message> {
        let Some(ns) = &mut self.new_session else { return Task::none() };
        let Some(picker) = &mut ns.picker else { return Task::none() };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        picker.seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let seq = picker.seq;
        let path = path.unwrap_or_else(|| picker.path.clone());
        picker.error = None;
        let params = if path.is_empty() { json!({}) } else { json!({"path": path}) };
        let key = self.links.get(ns.machine).map(|l| l.target.key.clone()).unwrap_or_default();
        self.send(&key, "list_dir", params, super::Pending::PickDir(seq));
        Task::none()
    }

    /// The host's listing for the remote dialog
    pub fn picker_listed(&mut self, seq: u64, v: Value) {
        if let Some(picker) = self.new_session.as_mut().and_then(|ns| ns.picker.as_mut()).filter(|p| p.seq == seq) {
            picker.path = v["path"].as_str().unwrap_or("").to_string();
            picker.listing = Some(v);
        }
    }

    /// The host could not list a folder for the remote dialog
    pub fn picker_failed(&mut self, error: String) {
        if let Some(picker) = self.new_session.as_mut().and_then(|ns| ns.picker.as_mut()) {
            picker.error = Some(error);
        }
    }

    /// The remote dialog's Choose: the folder it is in goes to the form
    pub fn picker_choose(&mut self) -> Task<Message> {
        let Some(path) = self.new_session.as_mut().and_then(|ns| ns.picker.take()).map(|p| p.path) else { return Task::none() };
        self.update(Message::NewBrowse(path))
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
        let task = self.update(Message::NewBrowse(folder));
        if let Some(ns) = &mut self.new_session {
            ns.backend = backend.clone();
            ns.reason = backend.map(|b| format!("{} is a {} document, and no session holds it: start one for its folder", super::file_name(&path), shell::backend_label(&b)));
        }
        task
    }
}

/// The width of the labels' column (rule 24b: labels left of their fields)
const LABEL: f32 = 110.0;
/// More machines than this make the Machine row a dropdown
const BAR_MAX: usize = 5;

pub fn page<'a>(app: &'a App, ns: &'a NewSession) -> Element<'a, Message> {
    let s = sp();
    let mut col = widget::column::with_capacity(12).spacing(s.space_m).padding(s.space_l).max_width(760);
    col = col.push(widget::text::title3("New session"));
    col = col.push(widget::text::body("One shared prover for a folder. It keeps running when you close the window, and agents can attach to it."));
    if let Some(reason) = &ns.reason {
        col = col.push(note(reason.clone(), false));
    }
    // Machine
    let name_of = |l: &super::Link| if l.target.destination.is_none() { "This computer".to_string() } else { l.name.clone() };
    let names: Vec<String> = app.links.iter().map(name_of).collect();
    let machine: Element<'a, Message> = if names.len() <= BAR_MAX {
        bar(names, ns.machine)
    } else {
        widget::dropdown(names, Some(ns.machine), Message::NewMachine).into()
    };
    col = col.push(field("Machine", machine));
    // Folder: the field, Browse, and what the host said about it
    let on = app.links.get(ns.machine).map(|l| if l.target.destination.is_none() { "this computer".to_string() } else { format!("{}", l.name) }).unwrap_or_default();
    let input = super::fields::text(&ns.folder, Field::NewFolder, "/path/to/project", |f| f.on_submit(Message::NewBrowse(ns.folder.to_string())).width(Length::Fixed(340.0)));
    let folder_row = widget::row::with_capacity(2).spacing(s.space_s).align_y(Alignment::Center).push(input).push(super::view::secondary("Browse", Some(Message::Shell(Msg::NewBrowseOpen))));
    let mut folder = widget::column::with_capacity(3).spacing(4).push(folder_row);
    let looked_at = ns.listing.is_some();
    match &ns.error {
        Some(err) if !looked_at => folder = folder.push(error_line(err.clone())),
        _ if ns.folder.is_empty() => folder = folder.push(widget::text::caption(format!("A folder on {on}."))),
        _ if !looked_at => folder = folder.push(widget::text::caption("Press Enter to look at the folder.")),
        _ => folder = folder.push(widget::text::caption(format!("On {on}. A folder can have one session for each prover."))),
    }
    col = col.push(field("Folder", folder.into()));
    // Prover: what the host found, as one line
    if !ns.folder.is_empty() && looked_at {
        match &ns.detections {
            None => col = col.push(field("Prover", widget::row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(shell::spinner(14.0)).push(widget::text::caption("Looking at the folder\u{2026}")).into())),
            Some(found) if found.is_empty() => {
                let mut cards = widget::column::with_capacity(3).spacing(s.space_xs).push(widget::text::caption("Nothing here says which prover it is for (no ROOT, no lakefile, no documents): pick one."));
                for b in app.backends(ns.machine) {
                    let backend = b["backend"].as_str().unwrap_or("isabelle").to_string();
                    let documents: Vec<&str> = b["documents"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    cards = cards.push(card(&backend, format!("No {} project found", shell::backend_label(&backend)), vec![format!("Documents: {}", documents.join(", "))], ns.backend.as_deref() == Some(backend.as_str())));
                }
                col = col.push(field("Prover", cards.into()));
            }
            Some(found) => col = col.push(field("Prover", prover(app, ns, found, &on))),
        }
        if let Some(backend) = &ns.backend {
            for row in form(app, ns, backend) {
                col = col.push(row);
            }
        }
    }
    if let Some(err) = ns.error.as_ref().filter(|_| looked_at) {
        col = col.push(note(err.clone(), true));
    }
    // (a backend picked: by the human, or as the only one found)
    let ready = ns.ready();
    col = col.push(
        widget::row::with_capacity(3)
            .spacing(s.space_s)
            .push(widget::Space::new().width(Length::Fixed(LABEL)))
            .push(widget::button::suggested("Start session").on_press_maybe(ready.then_some(Message::NewCreate)))
            .push(super::view::secondary("Cancel", Some(Message::NewSession(false)))),
    );
    let page = widget::scrollable(widget::container(col).center_x(Length::Fill)).height(Length::Fill);
    match &ns.picker {
        Some(picker) => iced::widget::stack![page, picker_dialog(app, ns, picker)].into(),
        None => page.into(),
    }
}

/// A row of the form: its label at the left, its field and the help under it
fn field<'a>(label: &'static str, control: Element<'a, Message>) -> Element<'a, Message> {
    widget::row::with_capacity(2)
        .spacing(sp().space_s)
        .push(widget::container(widget::text::body(label)).width(Length::Fixed(LABEL)).padding(Padding { top: 6.0, ..Padding::ZERO }))
        .push(widget::container(control).width(Length::Fill))
        .into()
}

/// The machines as a bar sized to its parts, the chosen one filled with the muted accent
fn bar<'a>(names: Vec<String>, selected: usize) -> Element<'a, Message> {
    let mut row = widget::row::with_capacity(names.len()).spacing(2).align_y(Alignment::Center);
    for (i, name) in names.into_iter().enumerate() {
        let class = if i == selected { super::tones::muted_button(theme::Button::Text) } else { theme::Button::Text };
        row = row.push(widget::button::text(name).class(class).on_press(Message::NewMachine(i)));
    }
    widget::container(row)
        .padding(2)
        .class(theme::Container::custom(|t| iced::widget::container::Style {
            background: Some(Color { a: 0.12, ..Color::from(t.cosmic().on_bg_color()) }.into()),
            border: Border { radius: t.cosmic().radius_s().into(), ..Default::default() },
            ..Default::default()
        }))
        .into()
}

/// A line of error under the field that caused it
fn error_line<'a>(text: String) -> Element<'a, Message> {
    widget::row::with_capacity(2)
        .spacing(sp().space_xxs)
        .align_y(Alignment::Center)
        .push(shell::tinted_icon("dialog-error-symbolic", 14, shell::destructive))
        .push(widget::text::caption(text).class(theme::Text::Custom(|t| iced::widget::text::Style { color: Some(shell::destructive(t)), ..Default::default() })))
        .into()
}

/// A detection for the prover's line: what `summary` says without the base logic (the form has
/// its own field for it), its theories counted in words: "ROOT session Timed_Circus, 14 theories"
pub fn prover_line(d: &Value) -> String {
    let line = summary(d);
    let line = line.split(", base logic").next().unwrap_or(&line);
    line.replace(" .thy files", " theories").replace(" .thy file", " theory")
}

/// Where the prover was found from, in words: "the ROOT file"
pub fn found_from(d: &Value) -> String {
    let evidence: Vec<&str> = d["evidence"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let has = |prefix: &str| evidence.iter().any(|e| e.starts_with(prefix));
    match d["backend"].as_str() {
        Some("lean") if has("lakefile") => "the lakefile".into(),
        Some("lean") => "the Lean package".into(),
        _ if has("ROOT") => "the ROOT file".into(),
        _ => "its .thy files".into(),
    }
}

/// The prover as one line, with Change when more than one fits, and cards while changing or
/// while none is picked
fn prover<'a>(app: &'a App, ns: &'a NewSession, found: &'a [Value], on: &str) -> Element<'a, Message> {
    let s = sp();
    let mut col = widget::column::with_capacity(4).spacing(4).width(Length::Fill);
    let picked = ns.backend.as_deref().and_then(|b| found.iter().find(|d| d["backend"].as_str() == Some(b)));
    if let Some(d) = picked {
        let backend = d["backend"].as_str().unwrap_or("isabelle");
        let mut line = widget::row::with_capacity(4)
            .spacing(s.space_xs)
            .align_y(Alignment::Center)
            .push(backend_mark(backend))
            .push(widget::text::body(shell::backend_label(backend)).font(cosmic::font::semibold()))
            .push(widget::text::body(format!("\u{b7} {}", prover_line(d))).width(Length::Fill));
        if found.len() > 1 {
            line = line.push(super::view::tertiary("Change", Message::Shell(Msg::NewChange)));
        }
        col = col.push(line);
        let toolchain = d["toolchain"].as_str().and_then(|t| {
            app.toolchains(ns.machine, backend).iter().find(|c| c["id"].as_str() == Some(t)).and_then(|c| c["label"].as_str().map(str::to_string)).or_else(|| Some(t.rsplit(['/', ':']).next().unwrap_or(t).to_string()))
        });
        let help = match toolchain {
            Some(t) => format!("Found from {}. {t} on {on}.", found_from(d)),
            None => format!("Found from {}.", found_from(d)),
        };
        col = col.push(widget::text::caption(help));
    } else {
        col = col.push(widget::text::caption("More than one prover recognises this folder: pick the one to start. A folder can have a session of each."));
    }
    if picked.is_none() || ns.changing {
        for d in found {
            let backend = d["backend"].as_str().unwrap_or("isabelle").to_string();
            let evidence: Vec<String> = d["evidence"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect();
            col = col.push(card(&backend, summary(d), evidence, ns.backend.as_deref() == Some(backend.as_str())));
        }
    }
    col.into()
}

/// The remote folder dialog over the page: COSMIC Files' look of a title, the location with its
/// way up, the folders, and Cancel with Choose
fn picker_dialog<'a>(app: &'a App, ns: &'a NewSession, picker: &'a Picker) -> Element<'a, Message> {
    let s = sp();
    let machine = app.links.get(ns.machine).map(|l| l.name.clone()).unwrap_or_default();
    let mut list = widget::column::with_capacity(40).spacing(1);
    let row = |icon: &'static str, label: String| {
        widget::row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(widget::icon::from_name(icon).size(16).icon()).push(widget::text::body(label).wrapping(iced::widget::text::Wrapping::None))
    };
    for name in picker.folders() {
        let full = picker.child(&name);
        list = list.push(button::custom(row("folder-symbolic", name)).class(super::sidebar::nav_style(false)).padding([6, 8]).width(Length::Fill).on_press(Message::Shell(Msg::PickerGo(full))));
    }
    let body: Element<'a, Message> = match (&picker.error, &picker.listing) {
        (Some(err), _) => error_line(err.clone()),
        (None, None) => widget::row::with_capacity(2).spacing(8).align_y(Alignment::Center).push(shell::spinner(14.0)).push(widget::text::caption("Listing the folder\u{2026}")).into(),
        (None, Some(_)) if picker.folders().is_empty() => widget::text::caption("No folders here.").into(),
        _ => widget::scrollable(list).height(Length::Fixed(280.0)).into(),
    };
    let up = widget::button::icon(widget::icon::from_name("go-up-symbolic").size(16)).on_press_maybe(picker.parent().map(|p| Message::Shell(Msg::PickerGo(p))));
    let location = widget::row::with_capacity(2)
        .spacing(s.space_xs)
        .align_y(Alignment::Center)
        .push(up)
        .push(widget::text::body(picker.path.clone()).font(cosmic::font::mono()).wrapping(iced::widget::text::Wrapping::WordOrGlyph).width(Length::Fill));
    let card = widget::column::with_capacity(5)
        .spacing(s.space_s)
        .push(widget::text::title4(format!("Choose a folder on {machine}")))
        .push(location)
        .push(widget::container(body).padding(4).width(Length::Fill).class(theme::Container::Card))
        .push(
            widget::row::with_capacity(3)
                .spacing(s.space_s)
                .push(widget::Space::new().width(Length::Fill))
                .push(super::view::secondary("Cancel", Some(Message::Shell(Msg::PickerClose))))
                .push(widget::button::suggested("Choose").on_press_maybe(picker.listing.is_some().then_some(Message::Shell(Msg::PickerChoose)))),
        );
    let dialog = widget::container(card).padding(s.space_m).width(Length::Fixed(520.0)).class(theme::Container::custom(|t| {
        let c = t.cosmic();
        iced::widget::container::Style {
            background: Some(Color::from(c.bg_component_color()).into()),
            border: Border { radius: c.corner_radii.radius_m.into(), width: 1.0, color: Color::from(c.bg_divider()) },
            text_color: Some(c.on_bg_color().into()),
            ..Default::default()
        }
    }));
    // (the scrim takes the clicks, so the form under the dialog is not reachable)
    iced::widget::opaque(
        widget::container(dialog)
            .center(Length::Fill)
            .class(theme::Container::custom(|_| iced::widget::container::Style { background: Some(Color { a: 0.45, ..Color::BLACK }.into()), ..Default::default() })),
    )
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

/// The picked backend's rows of the form, after the prover line: Isabelle's base logic and what
/// goes with it, or the Lean project's notes
fn form<'a>(app: &'a App, ns: &'a NewSession, backend: &str) -> Vec<Element<'a, Message>> {
    let detected = ns.detection(backend);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let optional = |input: Element<'a, Message>| -> Element<'a, Message> {
        widget::row::with_capacity(2).spacing(sp().space_xs).align_y(Alignment::Center).push(input).push(widget::text::caption("(optional)")).into()
    };
    match backend {
        "isabelle" => {
            let toolchains: Vec<Value> = app.toolchains(ns.machine, "isabelle");
            // (one Isabelle installation is what the prover line already says)
            if toolchains.len() > 1 {
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
                let mut control = widget::column::with_capacity(2).spacing(4).push(widget::dropdown(labels, selected, |i| Message::Shell(Msg::NewToolchain(i))));
                if let Some(problem) = selected.and_then(|i| toolchains[i]["problem"].as_str()) {
                    control = control.push(widget::text::caption(problem.to_string()));
                }
                rows.push(field("Runtime", control.into()));
            } else if toolchains.is_empty() {
                rows.push(field("Runtime", widget::text::caption("The host found no Isabelle installation with a Deductum backend").into()));
            }
            let logic = widget::column::with_capacity(2)
                .spacing(4)
                .push(optional(super::fields::text(&ns.logic, Field::NewLogic, "HOL", |f| f.width(Length::Fixed(200.0)))))
                .push(widget::text::caption("The heap the theories build on; its parent session when the folder is one"));
            rows.push(field("Base logic", logic.into()));
            let dirs = widget::column::with_capacity(2)
                .spacing(4)
                .push(optional(super::fields::text(&ns.dirs, Field::NewDirs, "none", |f| f.width(Length::Fixed(200.0)))))
                .push(widget::text::caption("Other folders whose ROOT sessions the theories import, separated by commas"));
            rows.push(field("Session directories", dirs.into()));
        }
        "lean" => {
            let p = detected.map(|d| &d["project"]);
            let root = p.and_then(|p| p["lake_root"].as_str()).unwrap_or("the folder's Lake package").to_string();
            let packages: Vec<&str> = p.and_then(|p| p["packages"].as_array()).into_iter().flatten().filter_map(Value::as_str).collect();
            let mut help = format!("The session runs lake serve in {root}, with the toolchain of lean-toolchain through elan.");
            if !packages.is_empty() {
                help.push_str(&format!(" Packages: {}.", packages.join(", ")));
            }
            rows.push(field("", widget::text::caption(help).into()));
        }
        _ => {}
    }
    if let Some(n) = detected.and_then(|d| d["note"].as_str()).filter(|n| !n.is_empty()) {
        rows.push(note(n.to_string(), false));
    }
    rows
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
        let mut ns = NewSession { folder: ShortTextBox::starting_with("/p"), ..Default::default() };
        ns.suggested(&both());
        assert_eq!(ns.backend, None, "two backends: none picked for the human");
        assert_eq!(ns.logic.value(), "HOL-Library");
        assert_eq!(ns.dirs.value(), "../lib");
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
        assert_eq!(prover_line(ns.detection("isabelle").unwrap()), "ROOT session Demo, 4 theories", "the line leaves the logic to its own field");
        assert_eq!(prover_line(ns.detection("lean").unwrap()), "Lake package, toolchain v4.19.0");
    }

    #[test]
    fn one_backend_is_picked_and_a_changed_logic_drops_its_sessions() {
        let mut ns = NewSession { folder: ShortTextBox::starting_with("/p"), ..Default::default() };
        let mut v = both();
        v["detections"].as_array_mut().unwrap().pop();
        ns.suggested(&v);
        assert_eq!(ns.backend.as_deref(), Some("isabelle"));
        ns.logic = ShortTextBox::starting_with("HOL");
        ns.dirs = ShortTextBox::starting_with("a, b");
        let p = ns.create_params();
        assert_eq!(p["project"]["dirs"], json!(["a", "b"]));
        assert!(p["project"].get("include_sessions").is_none());
    }
}

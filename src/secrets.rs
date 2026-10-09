// SPDX-License-Identifier: GPL-3.0-only

//! The window's side of the secrets store and sudo approvals (see
//! `AGENTS-PLAN.md`, "Secrets and sudo approvals"): the connection to
//! `ct-sudo-agent`, the bar a sudo request shows on the tab that asked, and
//! the Secrets pane on the right — the store's lock, the waiting requests,
//! a section per machine with its sudo approvals and saved passwords, the
//! other saved secrets, and the policy.

use cosmic::{
    Element,
    iced::{Alignment, Length},
    style, theme,
    widget::{self},
};
use cosmic_term::sudo_control::{
    Approval, ApprovalRule, EntryEdit, EntryInfo, FromAgent, Policy, Request, Scope, State, ToAgent,
};
use std::{
    collections::BTreeMap,
    time::Instant,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::{Message, fl, icon_cache_get};

/// How long a click-approval waits after its bar appears before it works,
/// so that it cannot be fired the instant a request arrives

/// The fingers fprintd knows, for enrolling
pub const FINGERS: [&str; 10] = [
    "right-index-finger",
    "right-middle-finger",
    "right-thumb",
    "right-ring-finger",
    "right-little-finger",
    "left-index-finger",
    "left-middle-finger",
    "left-thumb",
    "left-ring-finger",
    "left-little-finger",
];

/// Idle times offered, in minutes
const LOCK_AFTER: [u64; 4] = [5, 15, 30, 60];

pub struct Pending {
    pub request: Request,
    pub password: String,
    pub error: Option<String>,
    /// A fingerprint check was started for it
    pub armed: bool,
    /// A finger is on the reader
    pub reading: bool,
    /// The password went to the agent, which is checking it
    pub checking: bool,
    pub input: widget::Id,
}

/// Adding or editing a saved secret
#[derive(Clone, Default)]
pub struct Form {
    pub id: Option<u64>,
    pub edit: EntryEdit,
}

/// An entry waiting for the store password before it can be pasted
pub struct Asking {
    pub id: u64,
    pub password: String,
}

pub struct Secrets {
    pub agent: Option<UnboundedSender<ToAgent>>,
    pub pending: BTreeMap<u64, Pending>,
    pub state: State,
    /// When `state` came, for the time left before it locks
    pub state_at: Option<Instant>,
    /// The store password being typed, to create or unlock it
    pub password: String,
    pub again: String,
    pub error: Option<String>,
    pub form: Option<Form>,
    pub asking: Option<Asking>,
    /// The machine whose new sudo key is awaited, to set it up
    pub setting_up: Option<(String, String)>,
    pub finger: usize,
    pub enrolment: Option<String>,
    pub password_id: widget::Id,
    pub again_id: widget::Id,
    pub ask_id: widget::Id,
    pub form_ids: [widget::Id; 3],
}

impl Default for Secrets {
    fn default() -> Self {
        Self {
            agent: None,
            pending: BTreeMap::new(),
            state: State::default(),
            state_at: None,
            password: String::new(),
            again: String::new(),
            error: None,
            form: None,
            asking: None,
            setting_up: None,
            finger: 0,
            enrolment: None,
            password_id: crate::field_id("secrets-password"),
            again_id: crate::field_id("secrets-again"),
            ask_id: crate::field_id("secrets-ask"),
            form_ids: [crate::field_id("secrets-label"), crate::field_id("secrets-secret"), crate::field_id("secrets-note")],
        }
    }
}

impl Secrets {
    pub fn send(&self, message: ToAgent) -> bool {
        self.agent.as_ref().is_some_and(|agent| agent.send(message).is_ok())
    }

    pub fn policy(&self) -> Policy {
        self.state.policy.unwrap_or_default()
    }

    /// Take in what the agent says; returns the request that is new, if any
    pub fn receive(&mut self, event: &FromAgent) -> Option<u64> {
        match event {
            FromAgent::Request(request) => {
                let id = request.id;
                let new = !self.pending.contains_key(&id);
                self.pending
                    .entry(id)
                    .and_modify(|p| p.request = request.clone())
                    .or_insert_with(|| Pending {
                        request: request.clone(),
                        password: String::new(),
                        error: None,
                        armed: false,
                        reading: false,
                        checking: false,
                        input: crate::field_id("sudo-password"),
                    });
                return new.then_some(id);
            }
            FromAgent::Done { id, .. } => {
                self.pending.remove(id);
            }
            FromAgent::FingerReading { id } => {
                if let Some(p) = self.pending.get_mut(id) {
                    p.reading = true;
                    p.error = None;
                }
            }
            FromAgent::Retry { id, error } | FromAgent::FingerprintFailed { id, error } => {
                if let Some(p) = self.pending.get_mut(id) {
                    p.reading = false;
                    p.checking = false;
                    p.error = Some(error.clone());
                    p.password.clear();
                    p.armed = false;
                }
            }
            FromAgent::State(state) => {
                if state.unlocked && !self.state.unlocked {
                    self.password.clear();
                    self.again.clear();
                    self.error = None;
                }
                self.state = state.clone();
                self.state_at = Some(Instant::now());
            }
            FromAgent::Enrolment { status, done } => {
                self.enrolment = Some(if *done { format!("{status} (done)") } else { status.clone() });
            }
            FromAgent::Error { message } => {
                if let Some(asking) = &mut self.asking {
                    asking.password.clear();
                }
                self.error = Some(message.clone());
            }
            FromAgent::Generated(_) | FromAgent::Secret { .. } | FromAgent::AutoApproved(_) => {}
        }
        None
    }

    /// The requests from a session
    pub fn for_session(&self, session: &str) -> impl Iterator<Item = &Pending> {
        self.pending
            .values()
            .filter(move |p| p.request.session.as_deref() == Some(session))
    }

    fn locks_in(&self) -> u64 {
        let since = self.state_at.map_or(0, |at| at.elapsed().as_secs());
        self.state.locks_in.saturating_sub(since)
    }
}

fn caption<'a>(text: String) -> Element<'a, Message> {
    widget::text::caption(text).into()
}

/// A bar across a tab or the window: its text on a line of its own, so it
/// never squeezes to nothing in a narrow pane, then its controls at the end
pub fn banner<'a>(icon: Option<&'static str>, text: String, controls: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    banner_of(icon, widget::text::body(text).width(Length::Fill).into(), controls)
}

/// A bar as [`banner`] makes, with what it says given as a widget
fn banner_of<'a>(
    icon: Option<&'static str>,
    content: Element<'a, Message>,
    controls: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let mut line = widget::row::with_capacity(2)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center);
    if let Some(icon) = icon {
        line = line.push(icon_cache_get(icon, 16));
    }
    line = line.push(content);
    let mut row = widget::row::with_capacity(controls.len() + 1)
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center)
        .push(widget::space::horizontal());
    for control in controls {
        row = row.push(control);
    }
    widget::layer_container(
        widget::column::with_children(vec![line.into(), row.into()])
            .spacing(spacing.space_xxs)
            .padding(spacing.space_xxs),
    )
    .layer(cosmic::cosmic_theme::Layer::Primary)
    .width(Length::Fill)
    .into()
}

/// A request's bar. `in_front` says whether a click can approve it now: its
/// tab is in front in a focused window
pub fn bar<'a>(pending: &'a Pending) -> Element<'a, Message> {
    let request = &pending.request;
    let spacing = theme::active().cosmic().spacing;
    // The command as a terminal shows it
    let mut content = widget::column::with_capacity(2).spacing(spacing.space_xxxs).push(
        widget::row::with_children(vec![
            widget::text::body(fl!("sudo-on", host = request.host.as_str())).into(),
            command_text(request),
        ])
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center),
    );
    if let Some(error) = &pending.error {
        content = content.push(caption(error.clone()));
    }
    banner_of(
        Some("dialog-password-symbolic"),
        content.width(Length::Fill).into(),
        approval_controls(pending, true),
    )
}

/// A request whose session has no tab in this window, as a popup.
/// `session` is the tab's title elsewhere, or the session's name
pub fn popup<'a>(pending: &'a Pending, session: Option<String>) -> Element<'a, Message> {
    let request = &pending.request;
    let id = request.id;
    // The command first, then where and for whom, as controls, which a
    // dialog shows after its body
    let place = match session {
        Some(session) => fl!("sudo-popup-from", cwd = request.cwd.as_str(), session = session.as_str()),
        None => fl!("sudo-popup-unknown", cwd = request.cwd.as_str()),
    };
    let mut controls = approval_controls(pending, false);
    // Approve is the last control, unless the password is being checked
    let approve = (!pending.checking).then(|| controls.pop()).flatten();
    let mut dialog = widget::dialog()
        .title(fl!("sudo-popup-title", host = request.host.as_str()))
        .icon(icon_cache_get("dialog-password-symbolic", 32))
        .control(command_text(request))
        .control(widget::text::body(place))
        .secondary_action(widget::button::standard(fl!("sudo-deny")).on_press(Message::SudoDeny(id)));
    if let Some(error) = &pending.error {
        dialog = dialog.control(caption(error.clone()));
    }
    for control in controls {
        dialog = dialog.control(control);
    }
    if let Some(approve) = approve {
        dialog = dialog.primary_action(approve);
    }
    dialog.into()
}

/// A request's command, in the terminal's monospace face
fn command_text<'a>(request: &Request) -> Element<'a, Message> {
    widget::text::body(request.command.join(" "))
        .font(cosmic::font::mono())
        .width(Length::Fill)
        .into()
}

/// The password field, if it asks for one, and Approve and Deny; with
/// `deny` false, Deny is left to the caller
pub fn approval_controls<'a>(pending: &'a Pending, deny: bool) -> Vec<Element<'a, Message>> {
    let request = &pending.request;
    let id = request.id;
    let mut controls: Vec<Element<_>> = Vec::with_capacity(3);
    if pending.checking {
        controls.push(caption(fl!("sudo-checking")));
        if deny {
            controls.push(widget::button::standard(fl!("sudo-deny")).on_press(Message::SudoDeny(id)).into());
        }
        return controls;
    }
    // The store password is always a way to approve: needed while it is
    // locked, and beside the reader when the reader is offered
    let field = widget::secure_input(fl!("secrets-store-password"), &pending.password, None, true)
        .id(pending.input.clone())
        .width(Length::Fill)
        .on_focus(Message::SecretsFocus(pending.input.clone()))
        .on_unfocus(Message::SecretsUnfocus(pending.input.clone()))
        .on_input(move |text| Message::SudoPassword(id, text))
        .on_submit(move |_| Message::SudoApprove(id));
    match request.approval {
        Approval::Password => controls.push(field.into()),
        Approval::Fingerprint => {
            controls.push(caption(if pending.reading {
                fl!("sudo-reading")
            } else {
                fl!("sudo-touch-or-type")
            }));
            controls.push(field.into());
        }
        Approval::Click => {}
    }
    let approve = widget::button::suggested(fl!("sudo-approve")).on_press(Message::SudoApprove(id));
    // Why the reader is not used, when it would be
    controls.push(match &request.note {
        Some(note) => widget::tooltip(
            approve,
            widget::text::body(fl!("sudo-no-reader", why = note.as_str())),
            widget::tooltip::Position::Bottom,
        )
        .into(),
        None => approve.into(),
    });
    if deny {
        controls.push(widget::button::standard(fl!("sudo-deny")).on_press(Message::SudoDeny(id)).into());
    }
    controls
}

fn heading<'a>(text: String) -> Element<'a, Message> {
    widget::text::heading(text).into()
}

fn icon_button<'a>(icon: &'static str, tip: String, message: Message) -> Element<'a, Message> {
    widget::tooltip(
        widget::button::custom(icon_cache_get(icon, 16))
            .on_press(message)
            .padding(4)
            .class(style::Button::Icon),
        widget::text::caption(tip),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

/// One saved secret's row
fn entry_row<'a>(secrets: &'a Secrets, entry: &'a EntryInfo) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let mut text = entry.label.clone();
    if !entry.note.is_empty() {
        text = format!("{text} · {}", entry.note);
    }
    let mut column = widget::column::with_capacity(2).push(
        widget::row::with_children(vec![
            widget::text::body(text).width(Length::Fill).into(),
            widget::button::standard(fl!("secrets-paste"))
                .on_press(Message::SecretsPaste(entry.id))
                .into(),
            icon_button("accessories-text-editor-symbolic", fl!("secrets-edit"), Message::SecretsEdit(Some(entry.id))),
            icon_button("edit-delete-symbolic", fl!("secrets-delete"), Message::SecretsDelete(entry.id)),
        ])
        .spacing(spacing.space_xxs)
        .align_y(Alignment::Center),
    );
    if let Some(asking) = secrets.asking.as_ref().filter(|a| a.id == entry.id) {
        column = column.push(
            widget::secure_input(fl!("secrets-store-password"), &asking.password, None, true)
                .id(secrets.ask_id.clone())
                .on_focus(Message::SecretsFocus(secrets.ask_id.clone())).on_unfocus(Message::SecretsUnfocus(secrets.ask_id.clone())).on_input(Message::SecretsAskPassword)
                .on_submit(|_| Message::SecretsAskSubmit),
        );
    }
    column.into()
}

/// The form for adding or editing a secret
fn form_view<'a>(secrets: &'a Secrets, form: &'a Form) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let label = if form.edit.host.is_some() {
        fl!("secrets-account")
    } else {
        fl!("secrets-name")
    };
    widget::column::with_children(vec![
        widget::text_input(label, &form.edit.label)
            .id(secrets.form_ids[0].clone())
            .on_focus(Message::SecretsFocus(secrets.form_ids[0].clone())).on_unfocus(Message::SecretsUnfocus(secrets.form_ids[0].clone())).on_input(Message::SecretsFormLabel)
            .into(),
        widget::secure_input(
            if form.id.is_some() { fl!("secrets-secret-unchanged") } else { fl!("secrets-secret") },
            &form.edit.secret,
            None,
            true,
        )
        .id(secrets.form_ids[1].clone())
        .on_focus(Message::SecretsFocus(secrets.form_ids[1].clone())).on_unfocus(Message::SecretsUnfocus(secrets.form_ids[1].clone())).on_input(Message::SecretsFormSecret)
        .into(),
        widget::text_input(fl!("secrets-note"), &form.edit.note)
            .id(secrets.form_ids[2].clone())
            .on_focus(Message::SecretsFocus(secrets.form_ids[2].clone())).on_unfocus(Message::SecretsUnfocus(secrets.form_ids[2].clone())).on_input(Message::SecretsFormNote)
            .into(),
        widget::checkbox(form.edit.always_ask).label(fl!("secrets-always-ask"))
            .on_toggle(Message::SecretsFormAlwaysAsk)
            .into(),
        widget::row::with_children(vec![
            widget::button::standard(fl!("cancel"))
                .on_press(Message::SecretsEdit(None))
                .into(),
            widget::button::suggested(fl!("secrets-save"))
                .on_press_maybe((!form.edit.label.trim().is_empty()).then_some(Message::SecretsSave))
                .into(),
        ])
        .spacing(spacing.space_xxs)
        .into(),
    ])
    .spacing(spacing.space_xxs)
    .into()
}

/// The Secrets pane. `machines` are (label, ssh destination or empty, host
/// name), this computer first
pub fn pane<'a>(
    secrets: &'a Secrets,
    machines: Vec<(String, String, String)>,
) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let mut column = widget::column::with_capacity(16).spacing(spacing.space_s);
    let state = &secrets.state;

    // The lock
    if secrets.agent.is_none() {
        column = column.push(caption(fl!("sudo-no-agent")));
    } else if !state.exists {
        let ready = !secrets.password.is_empty() && secrets.password == secrets.again;
        column = column
            .push(heading(fl!("secrets-create-title")))
            .push(caption(fl!("secrets-create-body")))
            .push(
                widget::secure_input(fl!("secrets-store-password"), &secrets.password, None, true)
                    .id(secrets.password_id.clone())
                    .on_focus(Message::SecretsFocus(secrets.password_id.clone())).on_unfocus(Message::SecretsUnfocus(secrets.password_id.clone())).on_input(Message::SecretsPassword),
            )
            .push(
                widget::secure_input(fl!("sudo-setup-again"), &secrets.again, None, true)
                    .id(secrets.again_id.clone())
                    .on_focus(Message::SecretsFocus(secrets.again_id.clone())).on_unfocus(Message::SecretsUnfocus(secrets.again_id.clone())).on_input(Message::SecretsAgain)
                    .on_submit(|_| Message::SecretsCreate),
            )
            .push(
                widget::button::suggested(fl!("secrets-create"))
                    .on_press_maybe(ready.then_some(Message::SecretsCreate)),
            );
    } else if !state.unlocked {
        column = column.push(
            widget::row::with_children(vec![
                icon_cache_get("dialog-password-symbolic", 16).into(),
                widget::secure_input(fl!("secrets-store-password"), &secrets.password, None, true)
                    .id(secrets.password_id.clone())
                    .on_focus(Message::SecretsFocus(secrets.password_id.clone())).on_unfocus(Message::SecretsUnfocus(secrets.password_id.clone())).on_input(Message::SecretsPassword)
                    .on_submit(|_| Message::SecretsUnlock)
                    .width(Length::Fill)
                    .into(),
                widget::button::suggested(fl!("secrets-unlock"))
                    .on_press(Message::SecretsUnlock)
                    .into(),
            ])
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center),
        );
    } else {
        let minutes = secrets.locks_in().div_ceil(60);
        column = column.push(
            widget::row::with_children(vec![
                widget::text::body(fl!("secrets-unlocked", minutes = minutes))
                    .width(Length::Fill)
                    .into(),
                widget::button::standard(fl!("secrets-lock"))
                    .on_press(Message::SecretsLock)
                    .into(),
            ])
            .align_y(Alignment::Center),
        );
    }
    if let Some(error) = &secrets.error {
        column = column.push(caption(error.clone()));
    }


    let unlocked = state.unlocked;
    let form_for = |host: Option<&str>| {
        secrets
            .form
            .as_ref()
            .filter(|f| f.edit.host.as_deref() == host)
    };

    // A section per machine
    for (label, dest, machine) in machines {
        let mut section = widget::column::with_capacity(8)
            .spacing(spacing.space_xxs)
            .push(heading(label));
        let set_up = state.machines.contains(&machine);
        let sudo_row: Element<_> = if set_up {
            widget::row::with_children(vec![
                widget::text::body(fl!("secrets-sudo-set-up")).width(Length::Fill).into(),
                widget::button::standard(fl!("sudo-set-up-again"))
                    .on_press_maybe(unlocked.then(|| Message::SudoSetup(dest.clone(), machine.clone())))
                    .into(),
            ])
            .align_y(Alignment::Center)
            .into()
        } else {
            widget::row::with_children(vec![
                widget::text::body(fl!("secrets-sudo-not-set-up")).width(Length::Fill).into(),
                widget::button::standard(fl!("sudo-set-up"))
                    .on_press_maybe(
                        (unlocked && !machine.is_empty())
                            .then(|| Message::SudoSetup(dest.clone(), machine.clone())),
                    )
                    .into(),
            ])
            .align_y(Alignment::Center)
            .into()
        };
        section = section.push(sudo_row);
        for entry in state.entries.iter().filter(|e| e.host.as_deref() == Some(machine.as_str())) {
            section = section.push(entry_row(secrets, entry));
        }
        match form_for(Some(machine.as_str())) {
            Some(form) => section = section.push(form_view(secrets, form)),
            None if unlocked => {
                section = section.push(
                    widget::button::text(fl!("secrets-add-password"))
                        .on_press(Message::SecretsAdd(Some(machine.clone()))),
                );
            }
            None => {}
        }
        column = column.push(section);
    }

    // Other secrets
    let mut other = widget::column::with_capacity(8)
        .spacing(spacing.space_xxs)
        .push(heading(fl!("secrets-other")));
    for entry in state.entries.iter().filter(|e| e.host.is_none()) {
        other = other.push(entry_row(secrets, entry));
    }
    match form_for(None) {
        Some(form) => other = other.push(form_view(secrets, form)),
        None if unlocked => {
            other = other.push(
                widget::row::with_children(vec![
                    widget::button::text(fl!("secrets-add"))
                        .on_press(Message::SecretsAdd(None))
                        .into(),
                    widget::button::text(fl!("secrets-import"))
                        .on_press(Message::SecretsImport)
                        .into(),
                ])
                .spacing(spacing.space_xxs),
            );
        }
        None => {}
    }
    column = column.push(other);

    // The policy
    let policy = secrets.policy();
    let minutes = policy.idle_lock / 60;
    let lock_labels: Vec<String> = LOCK_AFTER.iter().map(|m: &u64| { let minutes = *m; fl!("secrets-minutes", minutes = minutes) }).collect();
    let lock_index = LOCK_AFTER.iter().position(|m| *m == minutes);
    let approvals = [
        (ApprovalRule::Nothing, fl!("secrets-approve-nothing")),
        (ApprovalRule::Fingerprint, fl!("secrets-approve-fingerprint")),
        (ApprovalRule::Click, fl!("secrets-approve-click")),
        (ApprovalRule::Password, fl!("secrets-approve-password")),
    ];
    let approval_labels: Vec<String> = approvals.iter().map(|(_, l)| l.clone()).collect();
    let approval_index = approvals.iter().position(|(r, _)| *r == policy.sudo.approval);
    let fingers = if state.fingers.is_empty() {
        fl!("sudo-no-fingers")
    } else {
        state.fingers.join(", ")
    };
    let mut reader = fingers;
    if let Some(why) = &state.reader {
        reader = format!("{reader} — {}", fl!("sudo-no-reader", why = why.as_str()));
    }
    if let Some(status) = &secrets.enrolment {
        reader = format!("{reader} — {status}");
    }
    let mut settings = widget::column::with_capacity(8)
        .spacing(spacing.space_xxs)
        .push(heading(fl!("secrets-policy")))
        .push(
            widget::row::with_children(vec![
                widget::text::body(fl!("secrets-lock-after")).width(Length::Fill).into(),
                widget::dropdown(lock_labels, lock_index, move |i| {
                    Message::SecretsPolicy(Policy { idle_lock: LOCK_AFTER[i] * 60, ..policy })
                })
                .into(),
            ])
            .align_y(Alignment::Center),
        )
        .push(
            widget::checkbox(policy.lock_with_screen).label(fl!("secrets-lock-with-screen"))
                .on_toggle(move |on| Message::SecretsPolicy(Policy { lock_with_screen: on, ..policy })),
        )
        .push(
            widget::row::with_children(vec![
                widget::text::body(fl!("secrets-sudo-approval")).width(Length::Fill).into(),
                widget::dropdown(approval_labels, approval_index, move |i| {
                    let mut p = policy;
                    p.sudo.approval = approvals_rule(i);
                    Message::SecretsPolicy(p)
                })
                .into(),
            ])
            .align_y(Alignment::Center),
        )
        .push(
            widget::checkbox(policy.sudo.scope == Scope::Session).label(fl!("secrets-per-session")).on_toggle(
                move |on| {
                    let mut p = policy;
                    p.sudo.scope = if on { Scope::Session } else { Scope::Store };
                    Message::SecretsPolicy(p)
                },
            ),
        )
        .push(
            widget::checkbox(policy.paste.approval == ApprovalRule::Password).label(fl!("secrets-paste-asks"))
                .on_toggle(move |on| {
                    let mut p = policy;
                    p.paste.approval = if on { ApprovalRule::Password } else { ApprovalRule::Nothing };
                    Message::SecretsPolicy(p)
                }),
        )
        .push(widget::text::body(fl!("sudo-fingerprint")))
        .push(caption(reader))
        .push(
            widget::row::with_children(vec![
                widget::dropdown(&FINGERS, Some(secrets.finger), Message::SudoFinger).into(),
                widget::button::standard(fl!("sudo-enrol"))
                    .on_press_maybe(secrets.agent.is_some().then_some(Message::SudoEnrol))
                    .into(),
            ])
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center),
        );
    if !state.exists || !unlocked {
        settings = settings.push(caption(fl!("secrets-policy-locked")));
    }
    column = column.push(settings);

    // The drawer scrolls it and gives it its width
    column.into()
}

fn approvals_rule(index: usize) -> ApprovalRule {
    match index {
        0 => ApprovalRule::Nothing,
        1 => ApprovalRule::Fingerprint,
        2 => ApprovalRule::Click,
        _ => ApprovalRule::Password,
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! The forms a launcher shows in place of its list: a new worktree, and a
//! transfer from the other computer. Each step that touches a computer runs
//! in the background; see `cosmic_term::transfer`.

use cosmic::{
    Element,
    iced::{Alignment, Length},
    widget::{self, segmented_button},
};
use cosmic_term::{
    helper::{PlaceInfo, WorktreeOptions},
    project,
    transfer::{self, Draft, Mode, RepoChoice, Step},
};
use std::path::{Path, PathBuf};

use crate::{Message, fl};

/// Run blocking work on a thread of its own
pub fn blocking<R: Send + 'static>(
    work: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> impl Future<Output = Result<R, String>> {
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = send.send(work());
    });
    async move {
        receive
            .await
            .unwrap_or_else(|_| Err("the work stopped unexpectedly".to_string()))
    }
}

pub enum LauncherForm {
    Worktree(Box<WorktreeForm>),
    Transfer(Box<TransferForm>),
}

pub struct WorktreeForm {
    /// Identifies this opening of the form; late replies from a prior form
    /// must not replace its contents.
    pub request_id: String,
    pub options: Option<Result<WorktreeOptions, String>>,
    /// What the base dropdown offers, and its labels: the worktree the
    /// launcher was opened in, if it has anything to carry, and the remote
    /// default, then local branches
    pub bases: Vec<Base>,
    pub base_labels: Vec<String>,
    pub base: usize,
    pub branch: String,
    pub branch_id: widget::Id,
    pub path: String,
    pub path_id: widget::Id,
    pub path_edited: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    /// A worktree as it is now, uncommitted changes included
    Worktree(PathBuf),
    Ref(String),
}

#[derive(Clone, Copy, Debug)]
pub enum WorktreeField {
    Branch,
    Path,
}

/// What [`WorktreeForm::request`] asks for
pub struct WorktreeRequest {
    pub repository: PathBuf,
    pub branch: String,
    pub base: String,
    pub path: PathBuf,
    pub from: Option<PathBuf>,
}

impl WorktreeForm {
    pub fn new() -> Self {
        Self {
            request_id: cosmic_term::random_hex(16),
            options: None,
            bases: Vec::new(),
            base_labels: Vec::new(),
            base: 0,
            branch: String::new(),
            branch_id: crate::field_id("worktree-branch"),
            path_id: crate::field_id("worktree-path"),
            path: String::new(),
            path_edited: false,
            busy: false,
            error: None,
        }
    }

    pub fn set_options(&mut self, options: Result<WorktreeOptions, String>) {
        if let Ok(options) = &options {
            let mut bases: Vec<Base> = options.default_base.iter().cloned().map(Base::Ref).collect();
            for branch in &options.branches {
                if !bases.contains(&Base::Ref(branch.clone())) {
                    bases.push(Base::Ref(branch.clone()));
                }
            }
            // Started from a worktree, a new one usually carries on its work
            if let Some(current) = &options.current
                && (current.linked || current.dirty)
            {
                let at = if current.linked { 0 } else { bases.len().min(1) };
                bases.insert(at, Base::Worktree(current.top.clone()));
            }
            self.base_labels = bases
                .iter()
                .map(|base| match base {
                    Base::Ref(name) => name.clone(),
                    Base::Worktree(_) => {
                        let current = options.current.as_ref();
                        let branch = current
                            .and_then(|c| c.branch.clone())
                            .unwrap_or_else(|| fl!("worktree-detached"));
                        if current.is_some_and(|c| c.dirty) {
                            fl!("worktree-base-current-dirty", branch = branch)
                        } else {
                            fl!("worktree-base-current", branch = branch)
                        }
                    }
                })
                .collect();
            self.bases = bases;
            self.base = 0;
            if self.branch.trim().is_empty() {
                self.branch = options.suggested_branch.clone();
            }
        }
        self.options = Some(options);
        self.suggest_path();
    }

    pub fn edit(&mut self, field: WorktreeField, value: String) {
        match field {
            WorktreeField::Branch => {
                self.branch = value;
                self.suggest_path();
            }
            WorktreeField::Path => {
                self.path = value;
                self.path_edited = true;
            }
        }
    }

    fn suggest_path(&mut self) {
        if self.path_edited {
            return;
        }
        if let Some(Ok(options)) = &self.options {
            let branch = if self.branch.trim().is_empty() {
                "new"
            } else {
                self.branch.trim()
            };
            self.path =
                project::worktree_path(&options.worktree_root, &options.label, branch, |_| false)
                    .display()
                    .to_string();
        }
    }

    /// The request to make, once the form is complete
    pub fn request(&self) -> Option<WorktreeRequest> {
        let Some(Ok(options)) = &self.options else {
            return None;
        };
        let branch = self.branch.trim();
        let (base, from) = match self.bases.get(self.base)? {
            Base::Ref(name) => (name.clone(), None),
            Base::Worktree(top) => (String::new(), Some(top.clone())),
        };
        let path = self.path.trim();
        (!branch.is_empty() && Path::new(path).is_absolute()).then(|| WorktreeRequest {
            repository: options.repository.clone(),
            branch: branch.to_string(),
            base,
            path: PathBuf::from(path),
            from,
        })
    }

    pub fn view(&self, entity: segmented_button::Entity, spacing: u16) -> Element<'_, Message> {
        let mut column = widget::column::with_capacity(8)
            .spacing(spacing)
            .push(widget::text::title4(fl!("worktree-title")));
        match &self.options {
            None => column = column.push(widget::text::caption(fl!("worktree-loading"))),
            Some(Err(error)) => {
                column = column.push(widget::text::caption(fl!(
                    "launcher-failed",
                    error = error.as_str()
                )));
            }
            Some(Ok(options)) => {
                column = column.push(widget::text::caption(fl!(
                    "worktree-repository",
                    path = options.repository.display().to_string()
                )));
                column = column.push(labelled(
                    fl!("worktree-branch"),
                    widget::text_input("", &self.branch)
                        .id(self.branch_id.clone())
                        .on_focus(Message::LauncherFocus(entity, self.branch_id.clone()))
                        .on_input(move |text| {
                            Message::LauncherWorktreeEdit(entity, WorktreeField::Branch, text)
                        })
                        .on_submit(move |_| Message::LauncherWorktreeCreate(entity))
                        .into(),
                ));
                column = column.push(labelled(
                    fl!("worktree-base"),
                    widget::dropdown(&self.base_labels, Some(self.base), move |i| {
                        Message::LauncherWorktreeBase(entity, i)
                    })
                    .into(),
                ));
                if let Some(error) = &options.fetch_error {
                    column = column.push(widget::text::caption(fl!(
                        "worktree-not-fetched",
                        error = error.as_str()
                    )));
                }
                column = column.push(labelled(
                    fl!("worktree-path"),
                    widget::text_input("", &self.path)
                        .id(self.path_id.clone())
                        .on_focus(Message::LauncherFocus(entity, self.path_id.clone()))
                        .on_input(move |text| {
                            Message::LauncherWorktreeEdit(entity, WorktreeField::Path, text)
                        })
                        .on_submit(move |_| Message::LauncherWorktreeCreate(entity))
                        .into(),
                ));
            }
        }
        if self.busy {
            column = column.push(widget::text::caption(fl!("worktree-creating")));
        }
        if let Some(error) = &self.error {
            column = column.push(widget::text::body(error.clone()));
        }
        let ready = !self.busy && self.request().is_some();
        column
            .push(buttons(
                entity,
                fl!("worktree-create"),
                ready.then_some(Message::LauncherWorktreeCreate(entity)),
                self.busy,
            ))
            .into()
    }
}

pub struct TransferForm {
    pub request_id: String,
    pub place_generation: u64,
    /// The choice that opened the form, to check again after a refusal, such
    /// as when the agent was still running
    pub retry: Option<crate::LaunchChoice>,
    pub mode: Mode,
    /// The conversation's title
    pub title: String,
    pub draft: Option<Result<Draft, String>>,
    pub choice: Option<RepoChoice>,
    pub place: Option<Result<PlaceInfo, String>>,
    pub path: String,
    pub path_id: widget::Id,
    pub busy: bool,
    pub progress: Step,
    pub stalled: bool,
    pub error: Option<String>,
}

impl TransferForm {
    pub fn new(mode: Mode, title: String, retry: Option<crate::LaunchChoice>) -> Self {
        Self {
            request_id: cosmic_term::random_hex(16),
            place_generation: 0,
            retry,
            mode,
            title,
            draft: None,
            choice: None,
            place: None,
            path: String::new(),
            path_id: crate::field_id("transfer-path"),
            busy: false,
            progress: Step::Started,
            stalled: false,
            error: None,
        }
    }

    /// The settled plan, once everything is chosen
    pub fn plan(&self) -> Option<transfer::Plan> {
        let (Some(Ok(draft)), Some(choice), Some(Ok(place))) =
            (&self.draft, &self.choice, &self.place)
        else {
            return None;
        };
        let path = Some(PathBuf::from(self.path.trim())).filter(|p| p.is_absolute());
        if place.reuse.is_none() && path.is_none() {
            return None;
        }
        Some(transfer::settle(
            draft.clone(),
            choice.clone(),
            place.clone(),
            path,
        ))
    }

    pub fn view(&self, entity: segmented_button::Entity, spacing: u16) -> Element<'_, Message> {
        let heading = match self.mode {
            Mode::Move => fl!("transfer-move-title"),
            Mode::Fork => fl!("transfer-fork-title"),
        };
        let mut column = widget::column::with_capacity(12)
            .spacing(spacing)
            .push(widget::text::title4(heading))
            .push(widget::text::body(self.title.clone()));
        match &self.draft {
            None => column = column.push(widget::text::caption(fl!("transfer-checking"))),
            Some(Err(error)) => {
                column = column.push(widget::text::body(error.clone()));
                if let Some(choice) = &self.retry {
                    column = column.push(
                        widget::button::standard(fl!("transfer-check-again"))
                            .on_press(Message::LauncherChoose(entity, choice.clone())),
                    );
                }
            }
            Some(Ok(draft)) => {
                if draft.repositories.partial {
                    column = column.push(widget::text::caption(fl!("transfer-scan-partial")));
                }
                let source = &draft.source;
                let from = if source.host.hostname.is_empty() {
                    draft.conversation.host.clone()
                } else {
                    source.host.hostname.clone()
                };
                column = column.push(widget::text::caption(fl!(
                    "transfer-source",
                    branch = source.workspace.branch.clone().unwrap_or_default(),
                    host = from,
                    path = source.workspace.top.display().to_string()
                )));
                if draft.conversation.kind != draft.target || !draft.target.native_import() {
                    column = column.push(widget::text::caption(fl!(
                        "transfer-seeded",
                        agent = draft.target.label()
                    )));
                }
                column = column.push(self.repository_view(entity, draft));
                match &self.place {
                    None if self.choice.is_some() => {
                        column = column.push(widget::text::caption(fl!("transfer-checking")));
                    }
                    None => {}
                    Some(Err(error)) => column = column.push(widget::text::body(error.clone())),
                    Some(Ok(place)) => match &place.reuse {
                        Some(path) => {
                            column = column.push(widget::text::caption(fl!(
                                "transfer-reuse",
                                path = path.display().to_string()
                            )));
                        }
                        None => {
                            if self.mode == Mode::Fork {
                                column = column.push(widget::text::caption(fl!(
                                    "transfer-fork-branch",
                                    branch = place.branch.clone()
                                )));
                            }
                            column = column.push(labelled(
                                fl!("worktree-path"),
                                widget::text_input("", &self.path)
                                    .id(self.path_id.clone())
                                    .on_focus(Message::LauncherFocus(entity, self.path_id.clone()))
                                    .on_input(move |text| {
                                        Message::LauncherTransferPath(entity, text)
                                    })
                                    .into(),
                            ));
                        }
                    },
                }
            }
        }
        if self.busy {
            let label = match self.progress {
                Step::Started => fl!("transfer-stage-check"),
                Step::Snapshotted => fl!("transfer-stage-copy"),
                Step::Fetched => fl!("transfer-stage-prepare"),
                Step::Prepared => fl!("transfer-stage-files"),
                Step::Carried => fl!("transfer-stage-conversation"),
                Step::Imported => fl!("transfer-stage-commit"),
                Step::Committed => fl!("transfer-stage-cleanup"),
                Step::Done | Step::RolledBack => fl!("transfer-running"),
            };
            let completed = match self.progress {
                Step::Started => 0,
                Step::Snapshotted => 1,
                Step::Fetched => 2,
                Step::Prepared => 3,
                Step::Carried => 4,
                Step::Imported => 5,
                Step::Committed => 6,
                Step::Done | Step::RolledBack => 7,
            };
            column = column.push(widget::text::caption(label));
            column = column.push(widget::progress_bar::determinate_linear(completed as f32 / 7.0).width(Length::Fill));
            if self.stalled {
                column = column.push(widget::text::caption(fl!("transfer-stage-waiting")));
            }
        }
        if let Some(error) = &self.error {
            column = column.push(widget::text::body(error.clone()));
        }
        let ready = !self.busy && self.plan().is_some();
        let label = match self.mode {
            Mode::Move => fl!("transfer-move"),
            Mode::Fork => fl!("transfer-fork"),
        };
        column
            .push(buttons(
                entity,
                label,
                ready.then_some(Message::LauncherTransferStart(entity)),
                self.busy,
            ))
            .into()
    }

    fn repository_view<'a>(
        &'a self,
        entity: segmented_button::Entity,
        draft: &'a Draft,
    ) -> Element<'a, Message> {
        let found = &draft.repositories;
        if let Some(bound) = &found.bound {
            return widget::text::caption(fl!(
                "transfer-repository",
                path = bound.display().to_string()
            ))
            .into();
        }
        let mut column = widget::column::with_capacity(found.candidates.len() + 2)
            .spacing(4)
            .push(widget::text::body(fl!("transfer-choose-repository")));
        let option = |choice: RepoChoice, label: String| -> Element<'a, Message> {
            let selected = self.choice.as_ref() == Some(&choice);
            let button = if selected {
                widget::button::suggested(label)
            } else {
                widget::button::standard(label)
            };
            button
                .on_press_maybe(
                    (!self.busy).then(|| Message::LauncherTransferChoose(entity, choice)),
                )
                .into()
        };
        for candidate in &found.candidates {
            let mut label = candidate.repository.display().to_string();
            if candidate.same_remote {
                label = fl!("transfer-same-remote", path = label);
            }
            column = column.push(option(
                RepoChoice::Existing(candidate.repository.clone()),
                label,
            ));
        }
        column = column.push(option(
            RepoChoice::New(found.new_repository.clone()),
            fl!(
                "transfer-new-repository",
                path = found.new_repository.display().to_string()
            ),
        ));
        column.into()
    }
}

fn labelled<'a>(label: String, input: Element<'a, Message>) -> Element<'a, Message> {
    widget::column::with_children(vec![widget::text(label).into(), input])
        .spacing(4)
        .into()
}

fn buttons<'a>(
    entity: segmented_button::Entity,
    label: String,
    on_press: Option<Message>,
    busy: bool,
) -> Element<'a, Message> {
    widget::row::with_children(vec![
        widget::button::standard(fl!("cancel"))
            .on_press_maybe((!busy).then_some(Message::LauncherFormCancel(entity)))
            .into(),
        widget::button::suggested(label)
            .on_press_maybe(on_press)
            .into(),
    ])
    .spacing(8)
    .align_y(Alignment::Center)
    .width(Length::Shrink)
    .into()
}

/// A short description of an unfinished transfer, for the launcher
pub fn describe(record: &transfer::Record) -> String {
    let draft = &record.plan.draft;
    let from = if draft.source.host.hostname.is_empty() {
        draft.conversation.host.clone()
    } else {
        draft.source.host.hostname.clone()
    };
    let mut text = fl!(
        "transfer-unfinished",
        branch = record.plan.branch.clone(),
        host = from
    );
    if let Some(error) = &record.error {
        text.push_str(": ");
        text.push_str(error);
    }
    text
}

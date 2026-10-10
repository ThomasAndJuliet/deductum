// SPDX-License-Identifier: GPL-3.0-only

//! Moving or forking a conversation and its worktree to another computer.
//!
//! [`plan`] asks both computers what they have and returns a [`Draft`] for
//! the user to settle: which of the destination's repositories is the
//! project's, when that is not known yet, and where the worktree goes.
//! [`start`] then runs the steps, saving a [`Record`] of what it has done
//! before each one, on the computer the terminal runs on:
//!
//! 1. check the source again: a branch, no nested worktrees, no agent running;
//! 2. snapshot the source worktree under a transfer ref;
//! 3. bring the ref to the destination (making its repository if needed);
//! 4. set up the destination worktree, detached;
//! 5. write the carried ignored files;
//! 6. import the conversation, or write its transcript for a new one;
//! 7. move the branch there, record the project, mark the source idle;
//! 8. delete the transfer refs.
//!
//! Until step 7 a failure undoes exactly what the record lists. Step 7 is the
//! point of no return: after it, only the remaining bookkeeping is retried.
//! An interrupted transfer's record stays until it is resumed or rolled back.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    helper::{HostInfo, PlaceInfo, Reply, Repositories, Request, SourceInfo},
    project::Dormant,
    provider::{self, ConversationFile, Imported, Kind, PathMap},
    workspace::{CarriedFile, Placement, Prepared, Snapshot, Written},
};

/// One computer, as the coordinator reaches it
pub trait Machine {
    fn call(&self, request: Request) -> Result<Reply, String>;
    /// How `git` on the terminal's computer reaches a repository here
    fn git_url(&self, path: &Path) -> String;
    /// The ssh command `git` should use to reach this computer
    fn ssh_command(&self) -> Option<String>;
    /// Whether this is the computer the terminal runs on
    fn is_local(&self) -> bool;
}

fn expect<T>(
    reply: Result<Reply, String>,
    pick: impl FnOnce(Reply) -> Option<T>,
) -> Result<T, String> {
    match reply? {
        Reply::Error { message } => Err(message),
        reply => {
            let text = format!("{reply:?}");
            pick(reply).ok_or_else(|| {
                format!(
                    "unexpected reply: {}",
                    text.chars().take(120).collect::<String>()
                )
            })
        }
    }
}

fn done(reply: Result<Reply, String>) -> Result<(), String> {
    expect(reply, |r| matches!(r, Reply::Done).then_some(()))
}

/// A conversation, and which computer and agent it belongs to
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationRef {
    /// An ssh destination, or empty for the terminal's computer
    pub host: String,
    pub kind: Kind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Mode {
    /// Continue there; the source becomes an idle copy
    Move,
    /// Continue there on a new branch; the source carries on as it was
    Fork,
}

/// How the destination's agent starts
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Start {
    /// Resume an imported conversation, telling it how paths moved
    Resume { id: String, note: String },
    /// A new conversation, opened with this message
    Seeded { prompt: String },
}

/// What a finished transfer opens
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Launch {
    pub host: String,
    /// The agent to start
    pub kind: Kind,
    pub dir: PathBuf,
    pub start: Start,
}

/// What the user decides before a transfer starts
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Draft {
    pub id: String,
    pub conversation: ConversationRef,
    /// The destination, as an ssh destination or empty
    pub destination: String,
    pub target: Kind,
    pub mode: Mode,
    pub cwd: String,
    pub source: SourceInfo,
    pub repositories: Repositories,
}

/// Where the destination's worktree comes from
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepoChoice {
    Existing(PathBuf),
    /// A new repository at this path
    New(PathBuf),
}

impl RepoChoice {
    pub fn path(&self) -> &Path {
        match self {
            Self::Existing(path) | Self::New(path) => path,
        }
    }
}

/// A transfer, settled
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Plan {
    pub draft: Draft,
    pub repository: RepoChoice,
    pub placement: Placement,
    /// The branch on the destination: the source's, or a new one for a fork
    pub branch: String,
    pub idle_since: Option<u64>,
}

impl Plan {
    /// Source paths and where they are on the destination, most specific
    /// first
    pub fn path_map(&self) -> PathMap {
        let source = &self.draft.source;
        PathMap(vec![
            (
                source.workspace.top.display().to_string(),
                self.placement.path().display().to_string(),
            ),
            (
                source.host.home.display().to_string(),
                self.draft.repositories.host.home.display().to_string(),
            ),
        ])
    }

    /// The conversation's directory on the destination
    pub fn destination_cwd(&self) -> String {
        self.path_map()
            .map(&self.draft.cwd)
            .unwrap_or_else(|| self.placement.path().display().to_string())
    }

    /// Whether the conversation itself moves, rather than starting anew
    pub fn native(&self) -> bool {
        self.draft.conversation.kind == self.draft.target && self.draft.target.native_import()
    }
}

fn source_label(host: &HostInfo, ssh: &str) -> String {
    if host.hostname.is_empty() {
        ssh.to_string()
    } else {
        host.hostname.clone()
    }
}

/// Ask both computers what a transfer would involve
pub fn plan(
    source: &dyn Machine,
    destination: &dyn Machine,
    conversation: ConversationRef,
    cwd: &str,
    destination_host: &str,
    target: Kind,
    mode: Mode,
) -> Result<Draft, String> {
    let info = expect(
        source.call(Request::Inspect { dir: cwd.into() }),
        |r| match r {
            Reply::Inspect(info) => Some(info),
            _ => None,
        },
    )
    .map_err(|err| {
        format!("{cwd} is not in a Git worktree on its computer, so it cannot move ({err})")
    })?;
    check_source(&info)?;
    let repositories = expect(
        destination.call(Request::Repositories {
            project: info.project.clone(),
            label: info.label.clone(),
            remotes: info.workspace.remotes.clone(),
        }),
        |r| match r {
            Reply::Repositories(found) => Some(found),
            _ => None,
        },
    )?;
    if repositories.host.host_id == info.host.host_id {
        return Err("the conversation is already on this computer".into());
    }
    Ok(Draft {
        id: format!("{}-{}", crate::now(), crate::random_hex(6)),
        conversation,
        destination: destination_host.to_string(),
        target,
        mode,
        cwd: cwd.to_string(),
        source: info,
        repositories,
    })
}

fn check_source(info: &SourceInfo) -> Result<(), String> {
    if info.workspace.branch.is_none() {
        return Err("the worktree has no branch checked out; check one out to transfer it".into());
    }
    if !info.workspace.nested.is_empty() {
        let list: Vec<String> = info
            .workspace
            .nested
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        return Err(format!(
            "merge or remove the worktrees inside this one first: {}",
            list.join(", ")
        ));
    }
    if !info.running.is_empty() {
        return Err(format!(
            "stop the agent first (finish or interrupt its turn, then exit it): {} still running",
            info.running.join(", ")
        ));
    }
    Ok(())
}

/// Where a chosen repository would put the worktree
pub fn place(
    destination: &dyn Machine,
    draft: &Draft,
    repository: &RepoChoice,
) -> Result<PlaceInfo, String> {
    let branch = draft.source.workspace.branch.clone().unwrap_or_default();
    let (branch, new_branch) = match draft.mode {
        Mode::Move => (branch, false),
        Mode::Fork => (
            format!(
                "{branch}-{}",
                crate::project::slug(&source_label(&draft.source.host, &draft.conversation.host))
            ),
            true,
        ),
    };
    match repository {
        RepoChoice::Existing(path) => expect(
            destination.call(Request::Place {
                repository: path.clone(),
                label: draft.source.label.clone(),
                branch,
                new_branch,
            }),
            |r| match r {
                Reply::Place(place) => Some(place),
                _ => None,
            },
        ),
        // A new repository has nothing to reuse; the worktree goes beside it
        RepoChoice::New(path) => Ok(PlaceInfo {
            reuse: None,
            idle: None,
            new_path: path.with_file_name(format!(
                "{}-{}",
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                crate::project::slug(&branch)
            )),
            branch,
        }),
    }
}

/// Settle a draft: the repository, and the place [`place`] offered, with
/// the user's path for a new worktree
pub fn settle(
    draft: Draft,
    repository: RepoChoice,
    place: PlaceInfo,
    path: Option<PathBuf>,
) -> Plan {
    let placement = match place.reuse {
        Some(path) => Placement::Reuse {
            path,
            idle: place.idle.as_ref().and_then(|d| d.trees.clone()),
        },
        None => Placement::New(path.unwrap_or(place.new_path)),
    };
    Plan {
        draft,
        repository,
        placement,
        branch: place.branch,
        idle_since: place.idle.map(|d| d.at),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Step {
    Started,
    Snapshotted,
    Fetched,
    Prepared,
    Carried,
    Imported,
    /// The point of no return
    Committed,
    Done,
    RolledBack,
}

/// A transfer's progress, saved before each step
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Record {
    pub plan: Plan,
    pub step: Step,
    pub snapshot: Option<Snapshot>,
    /// The destination repository was made by this transfer
    pub initialized: bool,
    pub prepared: Option<Prepared>,
    pub written: Vec<Written>,
    pub imported: Option<Imported>,
    pub transcript: Option<PathBuf>,
    /// The first message of the new conversation, with `transcript`
    #[serde(default)]
    pub handoff: Option<String>,
    /// Bookkeeping after the point of no return, done so far
    pub bound: bool,
    pub marked: bool,
    pub source_ref_dropped: bool,
    pub destination_ref_dropped: bool,
    pub launch: Option<Launch>,
    pub error: Option<String>,
}

/// Where records are kept, on the terminal's computer
pub struct Journal {
    dir: PathBuf,
}

/// Held while one window changes a transfer record. The file remains so
/// another window can lock the same inode; dropping the handle unlocks it.
pub struct RecordLock {
    _file: fs::File,
}

impl Journal {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            dir: state_dir.join("transfers"),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn try_lock(&self, id: &str) -> Result<RecordLock, String> {
        use std::os::fd::AsRawFd;
        fs::create_dir_all(&self.dir).map_err(|err| format!("{}: {err}", self.dir.display()))?;
        let path = self.dir.join(format!("{id}.lock"));
        let file = fs::OpenOptions::new().create(true).write(true).truncate(false)
            .open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
        // SAFETY: flock only receives the descriptor owned by `file`.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Err("another window is already working on this transfer".into());
            }
            return Err(format!("{}: {err}", path.display()));
        }
        Ok(RecordLock { _file: file })
    }

    pub fn save(&self, record: &Record) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|err| format!("{}: {err}", self.dir.display()))?;
        let path = self.path(&record.plan.draft.id);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(record).map_err(|err| err.to_string())?;
        fs::write(&tmp, bytes)
            .and_then(|()| fs::rename(&tmp, &path))
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    /// Transfers neither finished nor rolled back
    pub fn unfinished(&self) -> Vec<Record> {
        let mut found: Vec<Record> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_slice::<Record>(&fs::read(e.path()).ok()?).ok())
            .filter(|r| !matches!(r.step, Step::Done | Step::RolledBack))
            .collect();
        found.sort_by(|a, b| a.plan.draft.id.cmp(&b.plan.draft.id));
        found
    }
}

/// Run a settled transfer, from the beginning
pub fn start(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    plan: Plan,
) -> Result<Launch, String> {
    start_with_progress(source, destination, journal, plan, &|_| {})
}

/// Run a transfer and report each durable step as it begins. The callback is
/// called on the coordinator's worker thread, never on the UI thread.
pub fn start_with_progress(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    plan: Plan,
    progress: &dyn Fn(Step),
) -> Result<Launch, String> {
    let mut record = Record {
        plan,
        step: Step::Started,
        snapshot: None,
        initialized: false,
        prepared: None,
        written: Vec::new(),
        imported: None,
        transcript: None,
        handoff: None,
        bound: false,
        marked: false,
        source_ref_dropped: false,
        destination_ref_dropped: false,
        launch: None,
        error: None,
    };
    journal.save(&record)?;
    resume_with_progress(source, destination, journal, &mut record, progress)
}

/// Carry on with a transfer from where its record says it stopped. Before
/// the point of no return, a failure rolls back and returns the error
pub fn resume(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    record: &mut Record,
) -> Result<Launch, String> {
    resume_with_progress(source, destination, journal, record, &|_| {})
}

pub fn resume_with_progress(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    record: &mut Record,
    progress: &dyn Fn(Step),
) -> Result<Launch, String> {
    match run_steps(source, destination, journal, record, progress) {
        Ok(launch) => Ok(launch),
        // An ssh timeout does not prove that the remote helper stopped. It
        // may still be finishing a Git operation. Keep the journal instead
        // of racing an automatic rollback against that unknown outcome.
        Err(err) if record.step < Step::Committed
            && (err.contains("timed out") || err.contains("lost the connection")) =>
        {
            record.error = Some(err.clone());
            let _ = journal.save(record);
            Err(format!("{err}; the transfer was left unfinished so it can be checked and recovered"))
        }
        Err(err) if record.step < Step::Committed => {
            record.error = Some(err.clone());
            let _ = journal.save(record);
            match rollback(source, destination, journal, record) {
                Ok(()) => Err(err),
                Err(undo) => Err(format!(
                    "{err}; undoing it also failed, so the transfer stays unfinished: {undo}"
                )),
            }
        }
        Err(err) => {
            record.error = Some(err.clone());
            let _ = journal.save(record);
            Err(format!(
                "the transfer is done, but some bookkeeping failed and will be retried: {err}"
            ))
        }
    }
}

fn run_steps(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    record: &mut Record,
    progress: &dyn Fn(Step),
) -> Result<Launch, String> {
    let id = record.plan.draft.id.clone();
    let src_top = record.plan.draft.source.workspace.top.clone();
    let src_repo = record.plan.draft.source.workspace.repository.clone();
    let dst_repo = record.plan.repository.path().to_path_buf();
    let path = record.plan.placement.path().to_path_buf();

    if record.step == Step::Started {
        progress(Step::Started);
        // The source again, just before: an agent may have been started since
        let info = expect(
            source.call(Request::Inspect {
                dir: record.plan.draft.cwd.clone().into(),
            }),
            |r| match r {
                Reply::Inspect(info) => Some(info),
                _ => None,
            },
        )?;
        check_source(&info)?;
        if info.workspace.branch != record.plan.draft.source.workspace.branch {
            return Err("the source worktree changed branch since the transfer was planned".into());
        }
        let mut snapshot = expect(
            source.call(Request::Snapshot {
                top: src_top.clone(),
                id: id.clone(),
            }),
            |r| match r {
                Reply::Snapshot(s) => Some(s),
                _ => None,
            },
        )?;
        snapshot.branch = record.plan.branch.clone();
        record.snapshot = Some(snapshot);
        record.step = Step::Snapshotted;
        journal.save(record)?;
    }
    let snapshot = record
        .snapshot
        .clone()
        .ok_or("the record has no snapshot")?;

    if record.step == Step::Snapshotted {
        progress(Step::Snapshotted);
        if let RepoChoice::New(path) = &record.plan.repository {
            record.initialized = true;
            journal.save(record)?;
            done(destination.call(Request::Init { path: path.clone() }))?;
        }
        if destination.is_local() {
            done(destination.call(Request::Fetch {
                repo: dst_repo.clone(),
                url: source.git_url(&src_repo),
                id: id.clone(),
                ssh: source.ssh_command(),
            }))?;
        } else if source.is_local() {
            done(source.call(Request::Push {
                repo: src_repo.clone(),
                url: destination.git_url(&dst_repo),
                id: id.clone(),
                ssh: destination.ssh_command(),
            }))?;
        } else {
            return Err("one of the two computers must be this one".into());
        }
        record.step = Step::Fetched;
        journal.save(record)?;
    }

    if record.step == Step::Fetched {
        progress(Step::Fetched);
        let prepared = expect(
            destination.call(Request::Prepare {
                repo: dst_repo.clone(),
                placement: record.plan.placement.clone(),
                snapshot: snapshot.clone(),
                id: id.clone(),
            }),
            |r| match r {
                Reply::Prepared(p) => Some(p),
                _ => None,
            },
        )?;
        record.prepared = Some(prepared);
        record.step = Step::Prepared;
        journal.save(record)?;
    }

    if record.step == Step::Prepared {
        progress(Step::Prepared);
        let patterns = record.plan.draft.source.carry_ignored.clone();
        if !patterns.is_empty() {
            let files: Vec<CarriedFile> = expect(
                source.call(Request::ReadCarried {
                    top: src_top.clone(),
                    patterns,
                }),
                |r| match r {
                    Reply::Carried { files } => Some(files),
                    _ => None,
                },
            )?;
            let written = expect(
                destination.call(Request::WriteCarried {
                    top: path.clone(),
                    files,
                    idle_since: record.plan.idle_since,
                }),
                |r| match r {
                    Reply::Written { written } => Some(written),
                    _ => None,
                },
            )?;
            record.written = written;
        }
        record.step = Step::Carried;
        journal.save(record)?;
    }

    if record.step == Step::Carried {
        progress(Step::Carried);
        let conversation = &record.plan.draft.conversation;
        let file: ConversationFile = expect(
            source.call(Request::Export {
                kind: conversation.kind,
                id: conversation.id.clone(),
            }),
            |r| match r {
                Reply::Exported(file) => Some(file),
                _ => None,
            },
        )?;
        let map = record.plan.path_map();
        if record.plan.native() {
            let imported = expect(
                destination.call(Request::Import {
                    file,
                    map,
                    cwd: record.plan.destination_cwd(),
                }),
                |r| match r {
                    Reply::Imported(imported) => Some(imported),
                    _ => None,
                },
            )?;
            record.imported = Some(imported);
        } else {
            let label = source_label(&record.plan.draft.source.host, &conversation.host);
            let text = provider::transcript(&file, &label);
            let path: PathBuf = expect(
                destination.call(Request::WriteTranscript {
                    name: format!("{}-{}", conversation.kind.label(), conversation.id),
                    text,
                }),
                |r| match r {
                    Reply::Path { path } => Some(path),
                    _ => None,
                },
            )?;
            record.handoff = Some(provider::handoff(&file, &label, &map, &path));
            record.transcript = Some(path);
        }
        record.step = Step::Imported;
        journal.save(record)?;
    }

    if record.step == Step::Imported {
        progress(Step::Imported);
        done(destination.call(Request::Commit {
            path: path.clone(),
            snapshot: snapshot.clone(),
        }))?;
        record.step = Step::Committed;
        record.launch = Some(launch_for(record)?);
        journal.save(record)?;
    }

    // After the point of no return: bookkeeping, each part retried until done
    if record.step == Step::Committed {
        progress(Step::Committed);
        let plan = &record.plan;
        if !record.bound {
            done(destination.call(Request::Bind {
                project: plan.draft.source.project.clone(),
                label: plan.draft.source.label.clone(),
                repository: dst_repo.clone(),
            }))?;
            done(destination.call(Request::ClearDormant {
                worktree: path.clone(),
            }))?;
            record.bound = true;
            journal.save(record)?;
        }
        if !record.marked {
            if record.plan.draft.mode == Mode::Move {
                let to_host = source_label(
                    &record.plan.draft.repositories.host,
                    &record.plan.draft.destination,
                );
                done(
                    source.call(Request::MarkDormant {
                        repository: src_repo.clone(),
                        dormant: Dormant {
                            worktree: src_top.clone(),
                            branch: record
                                .plan
                                .draft
                                .source
                                .workspace
                                .branch
                                .clone()
                                .unwrap_or_default(),
                            to_host,
                            transfer: id.clone(),
                            at: crate::now(),
                            trees: Some(snapshot.trees.clone()),
                        },
                    }),
                )?;
            }
            record.marked = true;
            journal.save(record)?;
        }
        if !record.source_ref_dropped {
            done(source.call(Request::DropRef {
                repo: src_repo.clone(),
                id: id.clone(),
            }))?;
            record.source_ref_dropped = true;
            journal.save(record)?;
        }
        if !record.destination_ref_dropped {
            done(destination.call(Request::DropRef {
                repo: dst_repo.clone(),
                id: id.clone(),
            }))?;
            record.destination_ref_dropped = true;
        }
        record.step = Step::Done;
        record.error = None;
        journal.save(record)?;
        progress(Step::Done);
    }
    record
        .launch
        .clone()
        .ok_or_else(|| "the record has nothing to open".into())
}

fn launch_for(record: &Record) -> Result<Launch, String> {
    let plan = &record.plan;
    let map = plan.path_map();
    let from = source_label(&plan.draft.source.host, &plan.draft.conversation.host);
    let to = source_label(&plan.draft.repositories.host, &plan.draft.destination);
    let start = match (&record.imported, &record.transcript) {
        (Some(imported), _) => Start::Resume {
            id: imported.id.clone(),
            note: provider::moved_note(&from, &to, &map),
        },
        (None, Some(_)) => Start::Seeded {
            prompt: record.handoff.clone().unwrap_or_default(),
        },
        (None, None) => return Err("the record has no conversation".into()),
    };
    Ok(Launch {
        host: plan.draft.destination.clone(),
        kind: plan.draft.target,
        dir: plan.destination_cwd().into(),
        start,
    })
}

/// Undo an unfinished transfer, as far as its record says it got
pub fn rollback(
    source: &dyn Machine,
    destination: &dyn Machine,
    journal: &Journal,
    record: &mut Record,
) -> Result<(), String> {
    if record.step >= Step::Committed {
        return Err("the transfer is past the point of no return; resume it instead".into());
    }
    let id = record.plan.draft.id.clone();
    let src_repo = record.plan.draft.source.workspace.repository.clone();
    let dst_repo = record.plan.repository.path().to_path_buf();
    if let Some(imported) = record.imported.take() {
        done(destination.call(Request::Remove {
            path: imported.path.clone(),
        }))
        .inspect_err(|_| record.imported = Some(imported))?;
        journal.save(record)?;
    }
    if let Some(transcript) = record.transcript.take() {
        done(destination.call(Request::Remove {
            path: transcript.clone(),
        }))
        .inspect_err(|_| record.transcript = Some(transcript))?;
        journal.save(record)?;
    }
    if !record.written.is_empty() {
        let top = record.plan.placement.path().to_path_buf();
        done(destination.call(Request::UndoCarried {
            top,
            written: record.written.clone(),
        }))?;
        record.written.clear();
        journal.save(record)?;
    }
    if let (Some(prepared), Some(snapshot)) = (record.prepared.clone(), record.snapshot.clone()) {
        done(destination.call(Request::Rollback {
            repo: dst_repo.clone(),
            prepared,
            snapshot,
            id: id.clone(),
        }))?;
        record.prepared = None;
        journal.save(record)?;
    }
    if record.step >= Step::Snapshotted || record.initialized {
        // The destination's ref may or may not have arrived
        let _ = destination.call(Request::DropRef {
            repo: dst_repo.clone(),
            id: id.clone(),
        });
        if record.initialized {
            done(destination.call(Request::Uninit {
                path: dst_repo.clone(),
            }))?;
            record.initialized = false;
        }
        done(source.call(Request::DropRef { repo: src_repo, id }))?;
    }
    record.step = Step::RolledBack;
    journal.save(record)
}

/// A conversation continued with a different agent on its own computer: its
/// transcript saved there, and the message that opens the new conversation
pub fn continue_with(
    machine: &dyn Machine,
    conversation: &ConversationRef,
) -> Result<Start, String> {
    let host = expect(machine.call(Request::Info), |r| match r {
        Reply::Info(info) => Some(info),
        _ => None,
    })?;
    let file = expect(
        machine.call(Request::Export {
            kind: conversation.kind,
            id: conversation.id.clone(),
        }),
        |r| match r {
            Reply::Exported(file) => Some(file),
            _ => None,
        },
    )?;
    let label = source_label(&host, &conversation.host);
    let path = expect(
        machine.call(Request::WriteTranscript {
            name: format!("{}-{}", conversation.kind.label(), conversation.id),
            text: provider::transcript(&file, &label),
        }),
        |r| match r {
            Reply::Path { path } => Some(path),
            _ => None,
        },
    )?;
    Ok(Start::Seeded {
        prompt: provider::handoff(&file, &label, &PathMap::default(), &path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Env,
        project::{Registry, tempdir::TempDir},
        workspace::tests::{repo, run},
    };
    use std::cell::{Cell, RefCell};

    /// A computer in a directory of its own, answering in this process,
    /// failing on request
    struct TestMachine {
        env: Env,
        local: bool,
        fail_on: RefCell<Option<&'static str>>,
        calls: Cell<usize>,
    }

    impl TestMachine {
        fn new(home: PathBuf, local: bool) -> Self {
            fs::create_dir_all(&home).unwrap();
            Self {
                env: Env::at(home),
                local,
                fail_on: RefCell::new(None),
                calls: Cell::new(0),
            }
        }
    }

    impl Machine for TestMachine {
        fn call(&self, request: Request) -> Result<Reply, String> {
            self.calls.set(self.calls.get() + 1);
            let text = serde_json::to_string(&request).unwrap();
            if let Some(op) = *self.fail_on.borrow()
                && text.starts_with(&format!(r#"{{"op":"{op}""#))
            {
                return Err(format!("connection lost during {op}"));
            }
            // Through JSON, as over ssh
            let request = serde_json::from_str(&text).unwrap();
            let reply = crate::helper::answer(&self.env, request);
            Ok(serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap())
        }

        fn git_url(&self, path: &Path) -> String {
            path.display().to_string()
        }

        fn ssh_command(&self) -> Option<String> {
            None
        }

        fn is_local(&self) -> bool {
            self.local
        }
    }

    struct World {
        _tmp: TempDir,
        laptop: TestMachine,
        desktop: TestMachine,
        journal: Journal,
        /// The laptop's worktree, and its conversation's id
        top: PathBuf,
        conversation: String,
    }

    const SIGNED: &str = r#""type":"assistant","message":{"content":[{"type":"thinking","thinking":"plan","signature":"SIG"}]}"#;

    #[test]
    fn a_transfer_record_has_one_writer_across_windows() {
        let tmp = TempDir::new();
        let journal = Journal::new(tmp.path());
        let first = journal.try_lock("example").unwrap();
        assert!(journal.try_lock("example").is_err());
        assert!(journal.try_lock("other").is_ok());
        drop(first);
        // A process another test forks meanwhile shares the lock until it
        // execs, as one any thread of a window spawns would: briefly
        let mut after = journal.try_lock("example");
        for _ in 0..40 {
            if after.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            after = journal.try_lock("example");
        }
        assert!(after.is_ok(), "{:?}", after.err());
    }

    /// A laptop with a project, its desktop clone, and a Claude
    /// conversation in a laptop worktree with work in progress
    fn world() -> World {
        let tmp = TempDir::new();
        let laptop = TestMachine::new(tmp.path().join("laptop-home"), false);
        let desktop = TestMachine::new(tmp.path().join("desktop-home"), true);
        let main = repo(&laptop.env.home.join("src/thing"));
        let top = laptop.env.home.join("worktrees/thing-feature");
        run(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                top.to_str().unwrap(),
            ],
        );
        fs::write(top.join("a.txt"), "work in progress\n").unwrap();
        fs::write(top.join("notes.txt"), "untracked\n").unwrap();
        fs::create_dir_all(top.join(".local-design")).unwrap();
        fs::write(top.join(".local-design/record.md"), "decisions\n").unwrap();
        Registry::update(&laptop.env, |r| {
            r.ensure(&main).carry_ignored = vec![".local-design/".into()];
            Ok(())
        })
        .unwrap();
        run(
            desktop.env.home.as_path(),
            &["clone", "-q", main.to_str().unwrap(), "Documents/thing"],
        );

        let conversation = "11111111-2222-4333-8444-555555555555".to_string();
        let folder = laptop
            .env
            .home
            .join(".claude/projects")
            .join(provider::claude_folder(top.to_str().unwrap()));
        fs::create_dir_all(&folder).unwrap();
        let lines = [
            format!(
                r#"{{"type":"user","message":{{"content":"carry on"}},"cwd":"{}","sessionId":"{conversation}"}}"#,
                top.display()
            ),
            format!(
                r#"{{{SIGNED},"cwd":"{}","sessionId":"{conversation}"}}"#,
                top.display()
            ),
        ];
        fs::write(
            folder.join(format!("{conversation}.jsonl")),
            lines.join("\n") + "\n",
        )
        .unwrap();
        let journal = Journal::new(&desktop.env.state_dir);
        World {
            _tmp: tmp,
            laptop,
            desktop,
            journal,
            top,
            conversation,
        }
    }

    fn draft(w: &World, mode: Mode, target: Kind) -> Draft {
        plan(
            &w.laptop,
            &w.desktop,
            ConversationRef {
                host: "au-laptop".into(),
                kind: Kind::Claude,
                id: w.conversation.clone(),
            },
            w.top.to_str().unwrap(),
            "",
            target,
            mode,
        )
        .unwrap()
    }

    /// The first transfer: the user picks the desktop's clone
    fn settled(w: &World, mode: Mode, target: Kind) -> Plan {
        let draft = draft(w, mode, target);
        assert_eq!(draft.repositories.bound, None);
        let clone = draft.repositories.candidates[0].repository.clone();
        assert_eq!(clone, w.desktop.env.home.join("Documents/thing"));
        let choice = RepoChoice::Existing(clone);
        let place = place(&w.desktop, &draft, &choice).unwrap();
        settle(draft, choice, place, None)
    }

    #[test]
    fn a_move_resumes_the_conversation_in_a_desktop_worktree() {
        let w = world();
        let plan = settled(&w, Mode::Move, Kind::Claude);
        // Beside the desktop's clone of the repository
        let dest = w.desktop.env.home.join("Documents/thing-feature");
        assert_eq!(plan.placement, Placement::New(dest.clone()));
        let stages = std::cell::RefCell::new(Vec::new());
        let launch = start_with_progress(&w.laptop, &w.desktop, &w.journal, plan,
            &|step| stages.borrow_mut().push(step)).unwrap();
        assert_eq!(*stages.borrow(), [
            Step::Started, Step::Snapshotted, Step::Fetched, Step::Prepared,
            Step::Carried, Step::Imported, Step::Committed, Step::Done,
        ]);

        assert_eq!(launch.dir, dest);
        let Start::Resume { id, note } = &launch.start else {
            panic!("{launch:?}");
        };
        assert!(
            note.contains(&format!("{} is now {}", w.top.display(), dest.display())),
            "{note}"
        );
        // Files, staged state, ignored notes, branch
        assert_eq!(
            fs::read_to_string(dest.join("a.txt")).unwrap(),
            "work in progress\n"
        );
        assert_eq!(
            fs::read_to_string(dest.join("notes.txt")).unwrap(),
            "untracked\n"
        );
        assert_eq!(
            fs::read_to_string(dest.join(".local-design/record.md")).unwrap(),
            "decisions\n"
        );
        assert_eq!(run(&dest, &["symbolic-ref", "--short", "HEAD"]), "feature");
        // The conversation, under a new id, with only cwd and id changed
        let file = provider::export(&w.desktop.env, Kind::Claude, id).unwrap();
        assert_eq!(file.cwd, dest.display().to_string());
        let text = String::from_utf8(file.data).unwrap();
        assert!(text.contains(SIGNED));
        assert!(!text.contains(&w.conversation));
        // Bookkeeping on both sides
        let desktop = Registry::load(&w.desktop.env).unwrap();
        let laptop = Registry::load(&w.laptop.env).unwrap();
        assert_eq!(desktop.projects[0].id, laptop.projects[0].id);
        assert_eq!(laptop.projects[0].dormant[0].worktree, w.top);
        assert!(crate::workspace::leftover_refs(&w.top).unwrap().is_empty());
        assert!(
            crate::workspace::leftover_refs(&w.desktop.env.home.join("Documents/thing"))
                .unwrap()
                .is_empty()
        );
        assert!(w.journal.unfinished().is_empty());
        // The laptop's copy is untouched
        assert_eq!(
            fs::read_to_string(w.top.join("a.txt")).unwrap(),
            "work in progress\n"
        );
    }

    #[test]
    fn the_second_transfer_finds_the_bound_repository() {
        let w = world();
        start(
            &w.laptop,
            &w.desktop,
            &w.journal,
            settled(&w, Mode::Move, Kind::Claude),
        )
        .unwrap();
        let draft = draft(&w, Mode::Fork, Kind::Claude);
        assert_eq!(
            draft.repositories.bound,
            Some(w.desktop.env.home.join("Documents/thing"))
        );
    }

    #[test]
    fn a_fork_makes_a_new_branch_and_leaves_the_source_active() {
        let w = world();
        let plan = settled(&w, Mode::Fork, Kind::Claude);
        assert!(plan.branch.starts_with("feature-"), "{}", plan.branch);
        let branch = plan.branch.clone();
        let launch = start(&w.laptop, &w.desktop, &w.journal, plan).unwrap();
        assert_eq!(
            run(&launch.dir, &["symbolic-ref", "--short", "HEAD"]),
            branch
        );
        assert!(
            Registry::load(&w.laptop.env).unwrap().projects[0]
                .dormant
                .is_empty()
        );
    }

    #[test]
    fn switching_agents_seeds_a_new_conversation_from_the_transcript() {
        let w = world();
        let launch = start(
            &w.laptop,
            &w.desktop,
            &w.journal,
            settled(&w, Mode::Move, Kind::Codex),
        )
        .unwrap();
        let Start::Seeded { prompt } = &launch.start else {
            panic!("{launch:?}");
        };
        let path = w
            .desktop
            .env
            .state_dir
            .join(format!("transcripts/Claude-{}.md", w.conversation));
        assert!(prompt.contains(&path.display().to_string()), "{prompt}");
        assert!(fs::read_to_string(path).unwrap().contains("carry on"));
    }

    #[test]
    fn a_failure_at_any_step_before_the_commit_leaves_nothing_behind() {
        for op in [
            "snapshot",
            "fetch",
            "prepare",
            "write_carried",
            "export",
            "import",
            "commit",
        ] {
            let w = world();
            let plan = settled(&w, Mode::Move, Kind::Claude);
            let dest = plan.placement.path().to_path_buf();
            *w.desktop.fail_on.borrow_mut() = Some(op);
            *w.laptop.fail_on.borrow_mut() = Some(op);
            let err = start(&w.laptop, &w.desktop, &w.journal, plan).unwrap_err();
            assert!(err.contains(op), "{op}: {err}");
            let clone = w.desktop.env.home.join("Documents/thing");
            assert!(!dest.exists(), "{op}: worktree left");
            assert!(
                crate::workspace::leftover_refs(&clone).unwrap().is_empty(),
                "{op}"
            );
            assert!(
                crate::workspace::leftover_refs(&w.top).unwrap().is_empty(),
                "{op}"
            );
            assert!(
                crate::workspace::git(
                    &clone,
                    &["rev-parse", "-q", "--verify", "refs/heads/feature"]
                )
                .is_err(),
                "{op}"
            );
            let imported = fs::read_dir(w.desktop.env.home.join(".claude/projects"))
                .into_iter()
                .flatten()
                .flatten()
                .flat_map(|folder| fs::read_dir(folder.path()).into_iter().flatten().flatten())
                .count();
            assert_eq!(imported, 0, "{op}: conversation left");
            assert!(w.journal.unfinished().is_empty(), "{op}");
            assert!(
                Registry::load(&w.laptop.env).unwrap().projects[0]
                    .dormant
                    .is_empty(),
                "{op}"
            );
        }
    }

    #[test]
    fn a_crash_leaves_a_record_that_resumes_to_the_end() {
        let w = world();
        let plan = settled(&w, Mode::Move, Kind::Claude);
        // Stop after preparing, as if the terminal had been killed
        let mut record = Record {
            plan,
            step: Step::Started,
            snapshot: None,
            initialized: false,
            prepared: None,
            written: Vec::new(),
            imported: None,
            transcript: None,
            handoff: None,
            bound: false,
            marked: false,
            source_ref_dropped: false,
            destination_ref_dropped: false,
            launch: None,
            error: None,
        };
        w.journal.save(&record).unwrap();
        *w.laptop.fail_on.borrow_mut() = Some("read_carried");
        assert!(run_steps(&w.laptop, &w.desktop, &w.journal, &mut record, &|_| {}).is_err());
        let mut found = w.journal.unfinished();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].step, Step::Prepared);
        *w.laptop.fail_on.borrow_mut() = None;
        let launch = resume(&w.laptop, &w.desktop, &w.journal, &mut found[0]).unwrap();
        assert!(launch.dir.join("a.txt").exists());
        assert!(w.journal.unfinished().is_empty());
    }

    #[test]
    fn a_crash_record_rolls_back() {
        let w = world();
        let plan = settled(&w, Mode::Move, Kind::Claude);
        let dest = plan.placement.path().to_path_buf();
        *w.laptop.fail_on.borrow_mut() = Some("export");
        let mut record = Record {
            plan,
            step: Step::Started,
            snapshot: None,
            initialized: false,
            prepared: None,
            written: Vec::new(),
            imported: None,
            transcript: None,
            handoff: None,
            bound: false,
            marked: false,
            source_ref_dropped: false,
            destination_ref_dropped: false,
            launch: None,
            error: None,
        };
        assert!(run_steps(&w.laptop, &w.desktop, &w.journal, &mut record, &|_| {}).is_err());
        let mut found = w.journal.unfinished();
        assert_eq!(found[0].step, Step::Carried);
        assert!(dest.join(".local-design/record.md").exists());
        rollback(&w.laptop, &w.desktop, &w.journal, &mut found[0]).unwrap();
        assert!(!dest.exists());
        assert!(w.journal.unfinished().is_empty());
    }

    #[test]
    fn a_failure_after_the_commit_is_retried_not_undone() {
        let w = world();
        let plan = settled(&w, Mode::Move, Kind::Claude);
        let dest = plan.placement.path().to_path_buf();
        *w.laptop.fail_on.borrow_mut() = Some("mark_dormant");
        let err = start(&w.laptop, &w.desktop, &w.journal, plan).unwrap_err();
        assert!(err.contains("will be retried"), "{err}");
        assert!(dest.join("a.txt").exists());
        let mut found = w.journal.unfinished();
        assert_eq!(found[0].step, Step::Committed);
        assert!(rollback(&w.laptop, &w.desktop, &w.journal, &mut found[0]).is_err());
        *w.laptop.fail_on.borrow_mut() = None;
        let launch = resume(&w.laptop, &w.desktop, &w.journal, &mut found[0]).unwrap();
        assert_eq!(launch.dir, dest);
        assert_eq!(
            Registry::load(&w.laptop.env).unwrap().projects[0]
                .dormant
                .len(),
            1
        );
    }

    #[test]
    fn running_agents_and_nested_worktrees_stop_a_transfer_before_it_starts() {
        let w = world();
        run(
            &w.top,
            &["worktree", "add", "-q", "--detach", "agent-worktrees/one"],
        );
        let err = plan(
            &w.laptop,
            &w.desktop,
            ConversationRef {
                host: "au-laptop".into(),
                kind: Kind::Claude,
                id: w.conversation.clone(),
            },
            w.top.to_str().unwrap(),
            "",
            Kind::Claude,
            Mode::Move,
        )
        .unwrap_err();
        assert!(err.contains("agent-worktrees/one"), "{err}");
    }

    #[test]
    fn a_round_trip_reuses_the_idle_copy() {
        let w = world();
        let out = start(
            &w.laptop,
            &w.desktop,
            &w.journal,
            settled(&w, Mode::Move, Kind::Claude),
        )
        .unwrap();
        let Start::Resume { id, .. } = out.start else {
            panic!()
        };
        // Work on the desktop, then back to the laptop: the desktop is now
        // the source, the laptop the destination
        fs::write(out.dir.join("a.txt"), "finished on the desktop\n").unwrap();
        fs::write(out.dir.join(".local-design/record.md"), "more decisions\n").unwrap();
        Registry::update(&w.desktop.env, |r| {
            r.projects[0].carry_ignored = vec![".local-design/".into()];
            Ok(())
        })
        .unwrap();
        let laptop_journal = Journal::new(&w.laptop.env.state_dir);
        let back_source = TestMachine::new(w.desktop.env.home.clone(), false);
        let back_destination = TestMachine::new(w.laptop.env.home.clone(), true);
        let draft = plan(
            &back_source,
            &back_destination,
            ConversationRef {
                host: "neo".into(),
                kind: Kind::Claude,
                id,
            },
            out.dir.to_str().unwrap(),
            "",
            Kind::Claude,
            Mode::Move,
        )
        .unwrap();
        let bound = draft
            .repositories
            .bound
            .clone()
            .expect("bound on the way out");
        let choice = RepoChoice::Existing(bound);
        let place = place(&back_destination, &draft, &choice).unwrap();
        assert_eq!(place.reuse.as_deref(), Some(w.top.as_path()));
        assert!(place.idle.is_some());
        let back = start(
            &back_source,
            &back_destination,
            &laptop_journal,
            settle(draft, choice, place, None),
        )
        .unwrap();
        assert_eq!(back.dir, w.top);
        assert_eq!(
            fs::read_to_string(w.top.join("a.txt")).unwrap(),
            "finished on the desktop\n"
        );
        assert_eq!(
            fs::read_to_string(w.top.join(".local-design/record.md")).unwrap(),
            "more decisions\n"
        );
        // Active again on the laptop, idle on the desktop
        assert!(
            Registry::load(&w.laptop.env).unwrap().projects[0]
                .dormant
                .is_empty()
        );
        assert_eq!(
            Registry::load(&w.desktop.env).unwrap().projects[0].dormant[0].worktree,
            out.dir
        );
    }

    #[test]
    fn a_project_new_to_the_destination_gets_a_repository() {
        let w = world();
        fs::remove_dir_all(w.desktop.env.home.join("Documents/thing")).unwrap();
        let d = draft(&w, Mode::Move, Kind::Claude);
        assert!(d.repositories.candidates.is_empty());
        let choice = RepoChoice::New(d.repositories.new_repository.clone());
        let p = place(&w.desktop, &d, &choice).unwrap();
        let launch = start(
            &w.laptop,
            &w.desktop,
            &w.journal,
            settle(d, choice, p, None),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(launch.dir.join("a.txt")).unwrap(),
            "work in progress\n"
        );
        assert_eq!(run(&launch.dir, &["log", "--format=%s", "-1"]), "first");

        // And when that fails, the new repository goes too
        let w = world();
        fs::remove_dir_all(w.desktop.env.home.join("Documents/thing")).unwrap();
        let d = draft(&w, Mode::Move, Kind::Claude);
        let repository = d.repositories.new_repository.clone();
        let choice = RepoChoice::New(repository.clone());
        let p = place(&w.desktop, &d, &choice).unwrap();
        *w.desktop.fail_on.borrow_mut() = Some("prepare");
        assert!(
            start(
                &w.laptop,
                &w.desktop,
                &w.journal,
                settle(d, choice, p, None)
            )
            .is_err()
        );
        assert!(!repository.exists());
    }

    #[test]
    fn continuing_with_another_agent_here_writes_a_transcript() {
        let w = world();
        let start = continue_with(
            &w.laptop,
            &ConversationRef {
                host: String::new(),
                kind: Kind::Claude,
                id: w.conversation.clone(),
            },
        )
        .unwrap();
        let Start::Seeded { prompt } = start else {
            panic!()
        };
        assert!(
            prompt.contains("carry on") && prompt.contains("transcripts/Claude-"),
            "{prompt}"
        );
    }
}

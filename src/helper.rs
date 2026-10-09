// SPDX-License-Identifier: GPL-3.0-only

//! The steps a transfer asks of one computer, and their answers.
//!
//! `ct-transfer` reads one [`Request`] as JSON on stdin and writes one
//! [`Reply`] on stdout. The terminal runs it on this computer and, over ssh,
//! on the other, so both computers run the same code whichever one the
//! terminal is on. [`handle`] is the whole of the helper, so tests can call
//! it directly with an [`Env`] of their own.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Env,
    project::{Dormant, HostId, ProjectId, Registry},
    provider::{self, ConversationFile, Imported, Kind, PathMap},
    workspace::{self, CarriedFile, Placement, Prepared, Remote, Snapshot, WorkspaceInfo, Written},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Info,
    /// A directory's worktree and project, registering the project if new
    Inspect {
        dir: PathBuf,
    },
    /// Which of this computer's repositories hold a project
    Repositories {
        project: ProjectId,
        label: String,
        remotes: Vec<Remote>,
    },
    /// Where a branch of a repository would go
    Place {
        repository: PathBuf,
        label: String,
        branch: String,
        new_branch: bool,
    },
    Snapshot {
        top: PathBuf,
        id: String,
    },
    DropRef {
        repo: PathBuf,
        id: String,
    },
    /// Fetch a transfer's ref from another computer into `repo`
    Fetch {
        repo: PathBuf,
        url: String,
        id: String,
        ssh: Option<String>,
    },
    /// Push a transfer's ref from `repo` to another computer
    Push {
        repo: PathBuf,
        url: String,
        id: String,
        ssh: Option<String>,
    },
    /// Make a repository for a project this computer has no clone of
    Init {
        path: PathBuf,
    },
    /// Remove a repository [`Request::Init`] made, if nothing was added
    Uninit {
        path: PathBuf,
    },
    Prepare {
        repo: PathBuf,
        placement: Placement,
        snapshot: Snapshot,
        id: String,
    },
    Rollback {
        repo: PathBuf,
        prepared: Prepared,
        snapshot: Snapshot,
        id: String,
    },
    Commit {
        path: PathBuf,
        snapshot: Snapshot,
    },
    ReadCarried {
        top: PathBuf,
        patterns: Vec<String>,
    },
    WriteCarried {
        top: PathBuf,
        files: Vec<CarriedFile>,
        idle_since: Option<u64>,
    },
    UndoCarried {
        top: PathBuf,
        written: Vec<Written>,
    },
    Export {
        kind: Kind,
        id: String,
    },
    Import {
        file: ConversationFile,
        map: PathMap,
        cwd: String,
    },
    /// Save a readable transcript for a conversation continued here
    WriteTranscript {
        name: String,
        text: String,
    },
    /// Delete a file a transfer wrote: an imported conversation or transcript
    Remove {
        path: PathBuf,
    },
    /// Record a project's clone here, under the id the other computer uses
    Bind {
        project: ProjectId,
        label: String,
        repository: PathBuf,
    },
    MarkDormant {
        repository: PathBuf,
        dormant: Dormant,
    },
    ClearDormant {
        worktree: PathBuf,
    },
    /// What the new-worktree form offers for a directory's repository
    WorktreeOptions {
        dir: PathBuf,
        fetch: bool,
    },
    CreateWorktree {
        repository: PathBuf,
        branch: String,
        base: String,
        path: PathBuf,
        /// A worktree to start from instead of `base`, with its uncommitted
        /// changes
        #[serde(default)]
        from: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    Done,
    Info(HostInfo),
    Inspect(SourceInfo),
    Repositories(Repositories),
    Place(PlaceInfo),
    Snapshot(Snapshot),
    Prepared(Prepared),
    Carried { files: Vec<CarriedFile> },
    Written { written: Vec<Written> },
    Exported(ConversationFile),
    Imported(Imported),
    Path { path: PathBuf },
    WorktreeOptions(WorktreeOptions),
    Error { message: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HostInfo {
    pub host_id: HostId,
    pub hostname: String,
    pub home: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SourceInfo {
    pub host: HostInfo,
    pub workspace: WorkspaceInfo,
    pub project: ProjectId,
    pub label: String,
    pub carry_ignored: Vec<String>,
    /// Agents still running in the worktree
    pub running: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Repositories {
    pub host: HostInfo,
    /// The repository registered for the project, if it is still one
    pub bound: Option<PathBuf>,
    /// Others that may be the project's: same remote first, then same name
    pub candidates: Vec<Candidate>,
    /// Where a new repository for the project would go
    pub new_repository: PathBuf,
    /// The search of home ran out of time, so `candidates` may be incomplete
    #[serde(default)]
    pub partial: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Candidate {
    pub repository: PathBuf,
    pub remotes: Vec<Remote>,
    /// Shares a remote with the source
    pub same_remote: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlaceInfo {
    /// The branch's existing worktree, to reuse
    pub reuse: Option<PathBuf>,
    /// `reuse`'s record as an idle copy, if it is one
    pub idle: Option<Dormant>,
    /// A free path for a new worktree
    pub new_path: PathBuf,
    /// For a new branch, a name not taken yet
    pub branch: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorktreeOptions {
    pub repository: PathBuf,
    pub label: String,
    pub branches: Vec<String>,
    /// The remote default branch, like `origin/main`, else the checked-out one
    pub default_base: Option<String>,
    /// Why the fetch failed, if it did; the base is then as last fetched
    pub fetch_error: Option<String>,
    pub worktree_root: PathBuf,
    /// The worktree the directory is in
    pub current: Option<CurrentWorktree>,
    /// A branch name that is free, and whose folder is free too
    pub suggested_branch: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CurrentWorktree {
    pub top: PathBuf,
    pub branch: Option<String>,
    /// A linked worktree, not the repository's main one
    pub linked: bool,
    /// Uncommitted changes or untracked files
    pub dirty: bool,
}

/// Today's date where the worktree is made, as `2026-10-05`
fn today() -> String {
    // SAFETY: localtime_r fills the tm it is given, from a time it reads
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
    }
}

/// A branch named for the day, `worktree-2026-10-05`, then `-b`, `-c` and on
/// for more that day, that neither `taken` nor an existing branch has
fn suggest_branch(repository: &Path, taken: impl Fn(&str) -> bool) -> String {
    let free = |name: &str| {
        !taken(name)
            && workspace::git(
                repository,
                &["rev-parse", "-q", "--verify", &format!("refs/heads/{name}")],
            )
            .is_err()
    };
    let base = format!("worktree-{}", today());
    std::iter::once(base.clone())
        .chain(('b'..='z').map(|c| format!("{base}-{c}")))
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|name| free(name))
        .unwrap_or(base)
}

fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| fs::read_to_string("/etc/hostname"))
        .map(|name| name.trim().to_string())
        .unwrap_or_default()
}

fn host_info(env: &Env) -> Result<HostInfo, String> {
    let host_id = Registry::update(env, |registry| Ok(registry.host_id.clone()))?;
    Ok(HostInfo {
        host_id,
        hostname: hostname(),
        home: env.home.clone(),
    })
}

/// A remote URL without its scheme, user and `.git`, so that the ssh and
/// https forms of one repository compare equal
pub fn normalize_remote(url: &str) -> String {
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let url = url.split_once("://").map_or(url, |(_, rest)| rest);
    let url = url.rsplit_once('@').map_or(url, |(_, rest)| rest);
    // host:path, as scp-like ssh URLs write it
    match url.split_once(':') {
        Some((host, path))
            if !host.contains('/') && !path.starts_with(|c: char| c.is_ascii_digit()) =>
        {
            format!("{host}/{}", path.trim_start_matches('/'))
        }
        _ => url.to_string(),
    }
    .to_lowercase()
}

const SCAN_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);

/// Repositories under `home`, a few levels deep, skipping hidden and build
/// directories. The breadth-first walk avoids network and FUSE mounts and
/// reports when its time or directory budget cut the search short.
fn find_repositories_within(
    home: &Path,
    remote_mounts: &std::collections::HashSet<PathBuf>,
    budget: std::time::Duration,
) -> (Vec<PathBuf>, bool) {
    if remote_mounts.iter().any(|mount| home.starts_with(mount)) {
        return (Vec::new(), true);
    }
    const DEPTH: usize = 4;
    const LIMIT: usize = 20_000;
    const SKIP: [&str; 5] = ["node_modules", "target", "build", "dist", "vendor"];
    let deadline = std::time::Instant::now() + budget;
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(home.to_path_buf(), 0)]);
    let mut visited = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > LIMIT || std::time::Instant::now() > deadline {
            return (found, true);
        }
        if dir.join(".git").is_dir() {
            found.push(dir);
            continue;
        }
        if depth == DEPTH {
            continue;
        }
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
                continue;
            }
            // The type comes from the directory listing, so a mount point
            // is skipped without touching the filesystem behind it
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) && !remote_mounts.contains(&path) {
                queue.push_back((path, depth + 1));
            }
        }
    }
    (found, false)
}

/// Mount points whose filesystems may be slow to answer
fn remote_mounts() -> std::collections::HashSet<PathBuf> {
    let mut mounts = fs::read_to_string("/proc/self/mountinfo")
        .map(|info| parse_remote_mounts(&info))
        .unwrap_or_default();
    // Mountinfo uses /var/home on Atomic, while $HOME may use /home.
    // Reading the symlink itself is safe even when a child mount is down.
    if let Ok(target) = fs::read_link("/home") {
        add_mount_aliases(&mut mounts, Path::new("/home"), &Path::new("/").join(target));
    }
    mounts
}

fn add_mount_aliases(
    mounts: &mut std::collections::HashSet<PathBuf>,
    alias: &Path,
    target: &Path,
) {
    let aliases: Vec<_> = mounts.iter()
        .filter_map(|mount| mount.strip_prefix(target).ok().map(|suffix| alias.join(suffix)))
        .collect();
    mounts.extend(aliases);
}

fn parse_remote_mounts(mountinfo: &str) -> std::collections::HashSet<PathBuf> {
    const REMOTE: [&str; 13] = [
        "nfs", "nfs4", "cifs", "smb3", "smbfs", "9p", "afs", "ceph", "glusterfs", "davfs",
        "sshfs", "autofs", "virtiofs",
    ];
    mountinfo
        .lines()
        .filter_map(|line| {
            let (mount, rest) = line.split_once(" - ")?;
            let point = mount.split(' ').nth(4)?;
            let kind = rest.split(' ').next()?;
            // fuseblk is a local disk (ntfs-3g, exfat), so it is walked
            let remote = kind == "fuse" || kind.starts_with("fuse.") || REMOTE.contains(&kind);
            remote.then(|| PathBuf::from(unescape_mount(point)))
        })
        .collect()
}

/// Undo mountinfo's octal escapes, as in `My\040Drive`
fn unescape_mount(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes.get(i + 1..i + 4).and_then(|d| {
            std::str::from_utf8(d).ok().and_then(|d| u8::from_str_radix(d, 8).ok())
        });
        match (bytes[i], octal) {
            (b'\\', Some(byte)) => {
                out.push(byte);
                i += 4;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a repository may be the project's, judged from its name and the
/// remote URLs in `.git/config` without running git. Unreadable configs
/// count as maybe, so git has the last word.
fn may_match(repository: &Path, label: &str, wanted: &[String]) -> bool {
    let same_name = repository
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(label));
    if same_name {
        return true;
    }
    let Ok(config) = fs::read_to_string(repository.join(".git/config")) else {
        return true;
    };
    config
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "url").then(|| normalize_remote(value))
        })
        .any(|url| wanted.contains(&url))
}

fn repositories(
    env: &Env,
    project: &ProjectId,
    label: &str,
    remotes: &[Remote],
) -> Result<Repositories, String> {
    let host = host_info(env)?;
    let registry = Registry::load(env)?;
    let slow_mounts = remote_mounts();
    let local = |path: &Path| !slow_mounts.iter().any(|mount| path.starts_with(mount));
    let valid = |path: &Path| {
        local(path) && workspace::repository(path).is_ok_and(|repo| workspace::same_path(&repo, path))
    };
    let bound = registry
        .project(project)
        .into_iter()
        .flat_map(|p| &p.repositories)
        .map(|r| r.path.clone())
        .find(|path| valid(path));
    let wanted: Vec<String> = remotes.iter().map(|r| normalize_remote(&r.url)).collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut candidates = Vec::new();
    let known = registry
        .projects
        .iter()
        .flat_map(|p| p.repositories.iter().map(|r| r.path.clone()));
    let (found, scan_partial) = find_repositories_within(&env.home, &slow_mounts, SCAN_BUDGET);
    let partial = scan_partial || registry.projects.iter()
        .flat_map(|project| &project.repositories)
        .any(|repository| !local(&repository.path));
    let found = found
        .into_iter()
        .filter(|repository| may_match(repository, label, &wanted));
    for repository in known.chain(found) {
        if Some(&repository) == bound.as_ref()
            || !seen.insert(repository.clone())
            || !valid(&repository)
        {
            continue;
        }
        let their = workspace::remotes(&repository).unwrap_or_default();
        let same_remote = their
            .iter()
            .any(|r| wanted.contains(&normalize_remote(&r.url)));
        let same_name = repository
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(label));
        if same_remote || same_name {
            candidates.push(Candidate {
                repository,
                remotes: their,
                same_remote,
            });
        }
    }
    candidates.sort_by_key(|c| (!c.same_remote, c.repository.clone()));
    let new_repository = (1..)
        .map(|n| {
            let name = if n == 1 {
                crate::project::slug(label)
            } else {
                format!("{}-{n}", crate::project::slug(label))
            };
            env.home.join("Documents").join(name)
        })
        .find(|path| !path.exists())
        .unwrap_or_default();
    Ok(Repositories {
        host,
        bound,
        candidates,
        new_repository,
        partial,
    })
}

fn place(
    env: &Env,
    repository: &Path,
    label: &str,
    branch: &str,
    new_branch: bool,
) -> Result<PlaceInfo, String> {
    let registry = Registry::load(env)?;
    let root = registry.worktree_root(env, repository);
    let exists = |name: &str| {
        workspace::git(
            repository,
            &["rev-parse", "-q", "--verify", &format!("refs/heads/{name}")],
        )
        .is_ok()
    };
    let branch = if new_branch {
        (1..)
            .map(|n| {
                if n == 1 {
                    branch.to_string()
                } else {
                    format!("{branch}-{n}")
                }
            })
            .find(|name| !exists(name))
            .unwrap_or_default()
    } else {
        branch.to_string()
    };
    let reuse = if new_branch {
        None
    } else {
        workspace::worktree_with_branch(repository, &branch)?
    };
    let idle = reuse.as_ref().and_then(|path| {
        registry
            .projects
            .iter()
            .flat_map(|p| &p.dormant)
            .filter(|d| &d.worktree == path)
            .max_by_key(|d| d.at)
            .cloned()
    });
    let new_path = crate::project::worktree_path(&root, label, &branch, |p| p.exists());
    Ok(PlaceInfo {
        reuse,
        idle,
        new_path,
        branch,
    })
}

fn worktree_options(env: &Env, dir: &Path, fetch: bool) -> Result<WorktreeOptions, String> {
    let repository = workspace::repository(dir)?;
    let current = workspace::inspect(dir).ok().map(|info| CurrentWorktree {
        linked: info.top != repository,
        top: info.top,
        branch: info.branch,
        dirty: info.dirty,
    });
    let label = Registry::update(env, |registry| {
        Ok(registry.ensure(&repository).label.clone())
    })?;
    let remotes = workspace::remotes(&repository)?;
    let remote = remotes
        .iter()
        .find(|r| r.name == "origin")
        .or_else(|| remotes.first())
        .map(|r| r.name.clone());
    let mut fetch_error = None;
    if fetch && let Some(remote) = &remote {
        if let Err(err) = workspace::git(&repository, &["fetch", "-q", remote]) {
            fetch_error = Some(err);
        } else {
            // Learn the remote's default branch if this clone never did
            let _ = workspace::git(&repository, &["remote", "set-head", remote, "--auto"]);
        }
    }
    let default_base = remote
        .as_ref()
        .and_then(|remote| {
            workspace::git(
                &repository,
                &["rev-parse", "--abbrev-ref", &format!("{remote}/HEAD")],
            )
            .ok()
        })
        .filter(|base| !base.is_empty() && !base.ends_with("/HEAD"))
        .or_else(|| {
            workspace::git(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
                .ok()
                .filter(|b| !b.is_empty())
        });
    let branches = workspace::git(
        &repository,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )?
    .lines()
    .map(str::to_string)
    .collect();
    let worktree_root = Registry::load(env)?.worktree_root(env, &repository);
    let suggested_branch = suggest_branch(&repository, |name| {
        crate::project::worktree_path(&worktree_root, &label, name, |_| false).exists()
    });
    Ok(WorktreeOptions {
        repository,
        label,
        branches,
        default_base,
        fetch_error,
        worktree_root,
        current,
        suggested_branch,
    })
}

fn create_worktree(
    env: &Env,
    repository: &Path,
    branch: &str,
    base: &str,
    path: &Path,
    from: Option<&Path>,
) -> Result<PathBuf, String> {
    let name = format!("refs/heads/{branch}");
    if workspace::git(repository, &["check-ref-format", &name]).is_err() {
        return Err(format!("not a valid branch name: {branch}"));
    }
    if workspace::git(repository, &["rev-parse", "-q", "--verify", &name]).is_ok() {
        return Err(format!("branch {branch} already exists"));
    }
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    let carry_ignored = Registry::update(env, |registry| {
        Ok(registry.ensure(repository).carry_ignored.clone())
    })?;
    if let Some(source) = from {
        if !workspace::same_path(&workspace::repository(source)?, repository) {
            return Err(format!(
                "{} is not a worktree of {}",
                source.display(),
                repository.display()
            ));
        }
        // Read first, so that nothing is left behind if this fails
        let carried = workspace::read_carried(source, &carry_ignored)?;
        workspace::add_worktree_from(source, path, branch)?;
        workspace::write_carried(path, &carried, None)?;
        return Ok(path.to_path_buf());
    }
    let path_str = path.to_str().ok_or("not a UTF-8 path")?;
    workspace::git(
        repository,
        &[
            "worktree",
            "add",
            "-q",
            "--no-track",
            "-b",
            branch,
            path_str,
            base,
        ],
    )?;
    Ok(path.to_path_buf())
}

/// Files a transfer may delete again: those it writes
fn removable(env: &Env, path: &Path) -> bool {
    let clean = path
        .components()
        .all(|c| !matches!(c, std::path::Component::ParentDir));
    clean
        && (path.starts_with(env.home.join(".claude/projects"))
            || path.starts_with(env.state_dir.join("transcripts")))
}

/// Answer one request on this computer
pub fn handle(env: &Env, request: Request) -> Result<Reply, String> {
    Ok(match request {
        Request::Info => Reply::Info(host_info(env)?),
        Request::Inspect { dir } => {
            let workspace = workspace::inspect(&dir)?;
            let (project, label, carry_ignored) = Registry::update(env, |registry| {
                let p = registry.ensure(&workspace.repository);
                Ok((p.id.clone(), p.label.clone(), p.carry_ignored.clone()))
            })?;
            Reply::Inspect(SourceInfo {
                host: host_info(env)?,
                running: workspace::running_agents(&workspace.top),
                workspace,
                project,
                label,
                carry_ignored,
            })
        }
        Request::Repositories {
            project,
            label,
            remotes,
        } => Reply::Repositories(repositories(env, &project, &label, &remotes)?),
        Request::Place {
            repository,
            label,
            branch,
            new_branch,
        } => Reply::Place(place(env, &repository, &label, &branch, new_branch)?),
        Request::Snapshot { top, id } => Reply::Snapshot(workspace::snapshot(&top, &id)?),
        Request::DropRef { repo, id } => {
            workspace::drop_ref(&repo, &id)?;
            Reply::Done
        }
        Request::Fetch { repo, url, id, ssh } => {
            workspace::fetch(&repo, &url, &id, ssh.as_deref())?;
            Reply::Done
        }
        Request::Push { repo, url, id, ssh } => {
            workspace::push(&repo, &url, &id, ssh.as_deref())?;
            Reply::Done
        }
        Request::Init { path } => {
            workspace::init(&path)?;
            Reply::Done
        }
        Request::Uninit { path } => {
            // Only while it has no commits of its own: a fresh `init`
            let empty = workspace::git(&path, &["for-each-ref", "refs/heads/"])
                .is_ok_and(|refs| refs.is_empty());
            let only_git = fs::read_dir(&path)
                .map(|entries| entries.flatten().all(|e| e.file_name() == ".git"))
                .unwrap_or(false);
            if empty && only_git {
                fs::remove_dir_all(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            }
            Reply::Done
        }
        Request::Prepare {
            repo,
            placement,
            snapshot,
            id,
        } => Reply::Prepared(workspace::prepare(&repo, &placement, &snapshot, &id)?),
        Request::Rollback {
            repo,
            prepared,
            snapshot,
            id,
        } => {
            workspace::rollback(&repo, &prepared, &snapshot, &id)?;
            Reply::Done
        }
        Request::Commit { path, snapshot } => {
            workspace::commit(&path, &snapshot)?;
            Reply::Done
        }
        Request::ReadCarried { top, patterns } => Reply::Carried {
            files: workspace::read_carried(&top, &patterns)?,
        },
        Request::WriteCarried {
            top,
            files,
            idle_since,
        } => Reply::Written {
            written: workspace::write_carried(&top, &files, idle_since)?,
        },
        Request::UndoCarried { top, written } => {
            workspace::undo_carried(&top, &written)?;
            Reply::Done
        }
        Request::Export { kind, id } => Reply::Exported(provider::export(env, kind, &id)?),
        Request::Import { file, map, cwd } => {
            Reply::Imported(provider::import(env, &file, &map, &cwd)?)
        }
        Request::WriteTranscript { name, text } => {
            let name = crate::project::slug(&name);
            let dir = env.state_dir.join("transcripts");
            fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
            let path = dir.join(format!("{name}.md"));
            fs::write(&path, text).map_err(|err| format!("{}: {err}", path.display()))?;
            Reply::Path { path }
        }
        Request::Remove { path } => {
            if !removable(env, &path) {
                return Err(format!("refusing to delete {}", path.display()));
            }
            match fs::remove_file(&path) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("{}: {err}", path.display()));
                }
                _ => Reply::Done,
            }
        }
        Request::Bind {
            project,
            label,
            repository,
        } => {
            Registry::update(env, |registry| {
                registry.bind(&project, &label, &repository);
                Ok(())
            })?;
            Reply::Done
        }
        Request::MarkDormant {
            repository,
            dormant,
        } => {
            Registry::update(env, |registry| {
                let record = registry.ensure(&repository);
                record.dormant.retain(|d| d.worktree != dormant.worktree);
                record.dormant.push(dormant);
                Ok(())
            })?;
            Reply::Done
        }
        Request::ClearDormant { worktree } => {
            Registry::update(env, |registry| {
                for project in &mut registry.projects {
                    project.dormant.retain(|d| d.worktree != worktree);
                }
                Ok(())
            })?;
            Reply::Done
        }
        Request::WorktreeOptions { dir, fetch } => {
            Reply::WorktreeOptions(worktree_options(env, &dir, fetch)?)
        }
        Request::CreateWorktree {
            repository,
            branch,
            base,
            path,
            from,
        } => Reply::Path {
            path: create_worktree(env, &repository, &branch, &base, &path, from.as_deref())?,
        },
    })
}

/// [`handle`], with errors as a [`Reply::Error`]
pub fn answer(env: &Env, request: Request) -> Reply {
    handle(env, request).unwrap_or_else(|message| Reply::Error { message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        project::tempdir::TempDir,
        workspace::tests::{repo, run},
    };

    #[test]
    fn remote_urls_compare_across_forms() {
        let forms = [
            "git@github.com:pop-os/cosmic-term.git",
            "https://github.com/pop-os/cosmic-term",
            "ssh://git@github.com/pop-os/cosmic-term.git/",
            "https://GitHub.com/pop-os/cosmic-term.git",
        ];
        for form in forms {
            assert_eq!(
                normalize_remote(form),
                "github.com/pop-os/cosmic-term",
                "{form}"
            );
        }
        assert_eq!(
            normalize_remote("ssh://neo:2222/srv/x.git"),
            "neo:2222/srv/x"
        );
        assert_ne!(normalize_remote("/home/a/x"), normalize_remote("/home/a/y"));
    }

    #[test]
    fn remote_mounts_are_network_and_fuse_filesystems() {
        let info = "\
29 1 0:26 /home /home rw,relatime shared:1 - btrfs /dev/mapper/luks rw
70 29 0:61 / /home/t/Drive rw,nosuid - fuse.rclone tom-gdrive: rw
71 29 0:62 / /home/t/My\\040Files rw - nfs4 server:/x rw
72 29 0:63 / /home/t/.cache/m/neo rw - fuse.sshfs neo:/ rw
73 29 8:17 / /home/t/Windows rw - fuseblk /dev/sdb1 rw
74 29 0:64 / /home/t/raw rw - fuse /dev/fuse rw
";
        let mounts = parse_remote_mounts(info);
        let expected: std::collections::HashSet<PathBuf> = [
            "/home/t/Drive",
            "/home/t/My Files",
            "/home/t/.cache/m/neo",
            "/home/t/raw",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        assert_eq!(mounts, expected);
    }

    #[test]
    fn the_walk_skips_remote_mounts_and_finds_shallow_repositories_first() {
        let tmp = TempDir::new();
        let home = tmp.path();
        for dir in ["a/b/c/deep/.git", "shallow/.git", "Drive/inside/.git"] {
            fs::create_dir_all(home.join(dir)).unwrap();
        }
        let drive = std::collections::HashSet::from([home.join("Drive")]);
        let (found, partial) =
            find_repositories_within(home, &drive, std::time::Duration::from_secs(60));
        assert!(!partial);
        assert_eq!(found, vec![home.join("shallow"), home.join("a/b/c/deep")]);

        let (found, partial) =
            find_repositories_within(home, &drive, std::time::Duration::ZERO);
        assert!(partial);
        assert!(found.is_empty());
    }

    #[test]
    fn the_walk_skips_remote_mounts_under_a_symlinked_home() {
        let tmp = TempDir::new();
        let home = tmp.path().join("var/home");
        fs::create_dir_all(home.join("Drive/hidden-repository/.git")).unwrap();
        fs::create_dir_all(home.join("local/.git")).unwrap();
        let alias = tmp.path().join("home");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        let mut mounts = std::collections::HashSet::from([home.join("Drive")]);
        add_mount_aliases(&mut mounts, &alias, &home);
        assert!(mounts.contains(&home.join("Drive")));
        let (found, partial) = find_repositories_within(&alias, &mounts, std::time::Duration::from_secs(60));
        assert!(!partial);
        assert_eq!(found, vec![alias.join("local")]);
    }

    #[test]
    fn candidates_are_prefiltered_by_name_and_configured_remote() {
        let tmp = TempDir::new();
        let wanted = vec![normalize_remote("git@github.com:pop-os/cosmic-term.git")];
        let write = |name: &str, config: &str| {
            let dir = tmp.path().join(name);
            fs::create_dir_all(dir.join(".git")).unwrap();
            fs::write(dir.join(".git/config"), config).unwrap();
            dir
        };
        let same_remote = write(
            "fork",
            "[remote \"origin\"]\n\turl = https://github.com/pop-os/cosmic-term\n",
        );
        let other = write("other", "[remote \"origin\"]\n\turl = https://example.com/x\n");
        let same_name = write("Cosmic-Term", "");
        assert!(may_match(&same_remote, "cosmic-term", &wanted));
        assert!(!may_match(&other, "cosmic-term", &wanted));
        assert!(may_match(&same_name, "cosmic-term", &wanted));
        // Without a readable config, git decides
        assert!(may_match(&tmp.path().join("missing"), "cosmic-term", &wanted));
    }

    #[test]
    fn requests_round_trip_as_json() {
        let request = Request::Fetch {
            repo: "/r".into(),
            url: "neo:/r".into(),
            id: "t".into(),
            ssh: None,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.starts_with(r#"{"op":"fetch""#), "{text}");
        assert!(matches!(
            serde_json::from_str(&text).unwrap(),
            Request::Fetch { .. }
        ));
        let reply = serde_json::to_string(&answer(
            &Env::at("/nonexistent"),
            Request::Remove {
                path: "/etc/passwd".into(),
            },
        ))
        .unwrap();
        assert!(
            reply.contains(r#""reply":"error""#) && reply.contains("refusing"),
            "{reply}"
        );
    }

    #[test]
    fn candidates_share_a_remote_or_a_name_and_bindings_win() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let upstream = repo(&tmp.path().join("upstream/thing"));
        let near = tmp.path().join("Documents/thing");
        run(
            tmp.path(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                near.to_str().unwrap(),
            ],
        );
        let named = repo(&tmp.path().join("Documents/other/thing"));
        repo(&tmp.path().join("Documents/unrelated"));
        let remotes = vec![Remote {
            name: "origin".into(),
            url: upstream.display().to_string(),
        }];
        let id = ProjectId("p".into());

        let found = repositories(&env, &id, "thing", &remotes).unwrap();
        assert_eq!(found.bound, None);
        let candidates: Vec<&Path> = found
            .candidates
            .iter()
            .map(|c| c.repository.as_path())
            .collect();
        // `upstream/thing` is found by name too, after the remote match
        assert_eq!(candidates[0], near);
        assert!(candidates.contains(&named.as_path()));
        assert!(!candidates.iter().any(|c| c.ends_with("unrelated")));
        assert_eq!(found.new_repository, tmp.path().join("Documents/thing-2"));

        handle(
            &env,
            Request::Bind {
                project: id.clone(),
                label: "thing".into(),
                repository: named.clone(),
            },
        )
        .unwrap();
        let found = repositories(&env, &id, "thing", &remotes).unwrap();
        assert_eq!(found.bound.as_deref(), Some(named.as_path()));
        assert!(!found.candidates.iter().any(|c| c.repository == named));
    }

    #[test]
    fn new_worktrees_branch_from_the_remote_default() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let upstream = repo(&tmp.path().join("upstream"));
        let clone = tmp.path().join("clone");
        run(
            tmp.path(),
            &[
                "clone",
                "-q",
                upstream.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        run(&clone, &["checkout", "-q", "-b", "old-feature"]);
        // Upstream moves on
        fs::write(upstream.join("a.txt"), "newer\n").unwrap();
        run(&upstream, &["commit", "-q", "-am", "newer"]);

        let options = worktree_options(&env, &clone, true).unwrap();
        assert_eq!(options.default_base.as_deref(), Some("origin/main"));
        assert_eq!(options.fetch_error, None);
        assert_eq!(options.label, "clone");
        assert!(options.branches.contains(&"old-feature".to_string()));
        // Beside the repository, on a branch named for the day
        assert_eq!(options.worktree_root, tmp.path());
        assert_eq!(options.suggested_branch, format!("worktree-{}", today()));
        // and the next that day after it
        run(&clone, &["branch", &options.suggested_branch]);
        let again = worktree_options(&env, &clone, false).unwrap();
        assert_eq!(again.suggested_branch, format!("worktree-{}-b", today()));

        let path = options.worktree_root.join("clone-new");
        create_worktree(&env, &options.repository, "new", "origin/main", &path, None).unwrap();
        assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "newer\n");
        assert_eq!(run(&path, &["symbolic-ref", "--short", "HEAD"]), "new");
        assert!(
            create_worktree(
                &env,
                &options.repository,
                "new",
                "origin/main",
                &tmp.path().join("x"),
                None
            )
            .is_err()
        );
        assert!(
            create_worktree(
                &env,
                &options.repository,
                "bad..name",
                "origin/main",
                &tmp.path().join("y"),
                None
            )
            .is_err()
        );

        // Offline: the base is as last fetched
        run(
            &clone,
            &["remote", "set-url", "origin", "/nonexistent/repo"],
        );
        let options = worktree_options(&env, &clone, true).unwrap();
        assert!(options.fetch_error.is_some());
        assert_eq!(options.default_base.as_deref(), Some("origin/main"));
    }

    #[test]
    fn repositories_without_a_remote_start_from_the_checked_out_branch() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let local = repo(&tmp.path().join("local"));
        run(&local, &["checkout", "-q", "-b", "topic"]);
        let options = worktree_options(&env, &local, true).unwrap();
        assert_eq!(options.default_base.as_deref(), Some("topic"));
        assert_eq!(options.fetch_error, None);
    }

    #[test]
    fn repositories_and_new_worktrees_accept_a_symlinked_home() {
        let tmp = TempDir::new();
        let home = tmp.path().join("var/home");
        fs::create_dir_all(&home).unwrap();
        let alias = tmp.path().join("home");
        std::os::unix::fs::symlink(&home, &alias).unwrap();
        let main = repo(&home.join("Documents/thing"));
        let aliased_main = alias.join("Documents/thing");
        let env = Env::at(&alias);

        let found = repositories(&env, &ProjectId("alias-test".into()), "thing", &[]).unwrap();
        assert!(found.candidates.iter().any(|c| workspace::same_path(&c.repository, &main)));
        fs::write(main.join("a.txt"), "in progress\n").unwrap();
        fs::write(main.join("new.txt"), "untracked\n").unwrap();
        let path = alias.join("worktrees/thing-new");
        create_worktree(&env, &aliased_main, "new", "main", &path, Some(&aliased_main)).unwrap();
        assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "in progress\n");
        assert_eq!(fs::read_to_string(path.join("new.txt")).unwrap(), "untracked\n");
        assert_eq!(run(&path, &["symbolic-ref", "--short", "HEAD"]), "new");

        let other = repo(&home.join("other"));
        assert!(create_worktree(&env, &aliased_main, "wrong", "main", &alias.join("wrong"), Some(&other)).is_err());
    }

    #[test]
    fn new_worktrees_can_start_from_a_worktree_with_its_changes() {
        let tmp = TempDir::new();
        let env = Env::at(tmp.path());
        let main = repo(&tmp.path().join("thing"));
        let linked = tmp.path().join("worktrees/thing-first");
        run(
            &main,
            &["worktree", "add", "-q", "-b", "first", linked.to_str().unwrap()],
        );
        fs::write(linked.join("a.txt"), "in progress\n").unwrap();
        fs::write(linked.join("new.txt"), "untracked\n").unwrap();
        fs::write(linked.join(".env"), "SECRET=1\n").unwrap();
        Registry::update(&env, |registry| {
            registry.ensure(&main).carry_ignored = vec![".env".into()];
            Ok(())
        })
        .unwrap();

        let options = worktree_options(&env, &linked, false).unwrap();
        assert_eq!(options.repository, main);
        let current = options.current.clone().unwrap();
        assert_eq!(current.top, linked);
        assert_eq!(current.branch.as_deref(), Some("first"));
        assert!(current.linked && current.dirty);
        assert!(!options.suggested_branch.is_empty());
        assert!(!options.branches.contains(&options.suggested_branch));
        let main_options = worktree_options(&env, &main, false).unwrap();
        assert!(!main_options.current.unwrap().linked);

        let path = options.worktree_root.join("thing-second");
        create_worktree(&env, &main, "second", "main", &path, Some(&linked)).unwrap();
        assert_eq!(run(&path, &["symbolic-ref", "--short", "HEAD"]), "second");
        assert_eq!(
            run(&path, &["rev-parse", "HEAD"]),
            run(&linked, &["rev-parse", "HEAD"])
        );
        assert_eq!(fs::read_to_string(path.join("a.txt")).unwrap(), "in progress\n");
        assert_eq!(fs::read_to_string(path.join("new.txt")).unwrap(), "untracked\n");
        assert_eq!(fs::read_to_string(path.join(".env")).unwrap(), "SECRET=1\n");
        // Still uncommitted, in both
        assert!(!run(&path, &["status", "--porcelain"]).is_empty());
        assert_eq!(fs::read_to_string(linked.join("a.txt")).unwrap(), "in progress\n");

        let other = repo(&tmp.path().join("other"));
        assert!(
            create_worktree(&env, &main, "third", "main", &tmp.path().join("z"), Some(&other))
                .is_err()
        );
    }
}

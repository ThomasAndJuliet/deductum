// SPDX-License-Identifier: GPL-3.0-only

//! Worktrees and their snapshots, through the `git` command.
//!
//! A transfer moves a worktree as Git objects. On the source, [`snapshot`]
//! records the index as a commit `I` on top of `HEAD`, and the whole working
//! tree, untracked files included, as a commit `W` on top of `I`, under
//! `refs/cosmic-term/transfer/<id>`, without moving any branch. The
//! destination fetches that ref, makes a worktree at `HEAD` ([`prepare`]),
//! sets its files from `W` and its index from `I`, and only then moves the
//! branch there ([`commit`]). Staged, unstaged and untracked changes all
//! arrive as they were. Ignored files travel separately, and only those the
//! project names ([`read_carried`]).

use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

/// The file in a worktree's administrative directory marking it as made by
/// an unfinished transfer, holding the transfer's id
pub const MARKER: &str = "cosmic-term-transfer";

/// Carried ignored files are refused beyond this, since they travel in
/// memory and as JSON
pub const CARRY_LIMIT: usize = 64 << 20;

/// Run git in `dir`, returning its trimmed output
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_env(dir, args, &[])
}

pub fn git_env(dir: &Path, args: &[&str], env: &[(&str, &OsStr)]) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().map_err(|err| format!("git: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            stderr.trim().lines().last().unwrap_or("failed")
        ))
    }
}

/// Whether git succeeds, for questions it answers with its exit status
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_ok()
}

fn path_arg(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("not a UTF-8 path: {}", path.display()))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

/// A worktree of a repository, as `git worktree list` reports it
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Without `refs/heads/`
    pub branch: Option<String>,
}

/// What a directory is, in Git terms
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WorkspaceInfo {
    /// The worktree's top directory
    pub top: PathBuf,
    /// The repository's main working directory, or the bare repository
    pub repository: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub remotes: Vec<Remote>,
    /// Other worktrees inside this one, as agents' subagents make
    pub nested: Vec<PathBuf>,
    /// Uncommitted changes or untracked files
    pub dirty: bool,
}

/// The repository whose common directory is `common`
fn repository_of_common(common: &Path) -> PathBuf {
    if common.file_name() == Some(OsStr::new(".git")) {
        common.parent().unwrap_or(common).to_path_buf()
    } else {
        common.to_path_buf()
    }
}

/// The repository a directory belongs to, whichever of its worktrees the
/// directory is in
pub fn repository(dir: &Path) -> Result<PathBuf, String> {
    let common = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(repository_of_common(Path::new(&common)))
}

/// Existing paths can name the same directory through a symlink, such as
/// Fedora Atomic's /home and /var/home. Distinct failed lookups never match.
pub fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

pub fn inspect(dir: &Path) -> Result<WorkspaceInfo, String> {
    let top = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?);
    let repository = repository(&top)?;
    let branch = git(&top, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty());
    let head = git(&top, &["rev-parse", "-q", "--verify", "HEAD"]).ok();
    let nested = worktrees(&top)?
        .into_iter()
        .map(|w| w.path)
        .filter(|path| path != &top && path.starts_with(&top))
        .collect();
    let dirty = !git(&top, &["status", "--porcelain", "--untracked-files=all"])?.is_empty();
    Ok(WorkspaceInfo {
        repository,
        branch,
        head,
        remotes: remotes(&top)?,
        nested,
        dirty,
        top,
    })
}

pub fn remotes(dir: &Path) -> Result<Vec<Remote>, String> {
    let names = git(dir, &["remote"])?;
    names
        .lines()
        .filter(|name| !name.is_empty())
        .map(|name| {
            Ok(Remote {
                name: name.to_string(),
                url: git(dir, &["remote", "get-url", name]).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn worktrees(dir: &Path) -> Result<Vec<Worktree>, String> {
    let output = git(dir, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    for block in output.split("\n\n") {
        let mut worktree = None;
        let (mut head, mut branch, mut bare) = (None, None, false);
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                worktree = Some(PathBuf::from(path));
            } else if let Some(oid) = line.strip_prefix("HEAD ") {
                head = Some(oid.to_string());
            } else if let Some(name) = line.strip_prefix("branch ") {
                branch = Some(name.strip_prefix("refs/heads/").unwrap_or(name).to_string());
            } else if line == "bare" {
                bare = true;
            }
        }
        if let Some(path) = worktree
            && !bare
        {
            found.push(Worktree { path, head, branch });
        }
    }
    Ok(found)
}

/// The ref a transfer keeps its snapshot under, on both computers
pub fn transfer_ref(id: &str) -> String {
    format!("refs/cosmic-term/transfer/{id}")
}

fn check_id(id: &str) -> Result<(), String> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        Ok(())
    } else {
        Err(format!("bad transfer id: {id:?}"))
    }
}

/// A worktree's index and files, as Git trees
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Trees {
    pub index: String,
    /// Every file that is not ignored, untracked ones included
    pub worktree: String,
}

/// A worktree's state, as commits
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    pub branch: String,
    /// `HEAD`
    pub base: String,
    /// `I`: the index, with parent `base`
    pub index: String,
    /// `W`: every file that is not ignored, with parent `index`
    pub worktree: String,
    pub trees: Trees,
}

/// Identity for the snapshot's commits, which are never on a branch
const COMMITTER: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "cosmic-term"),
    ("GIT_AUTHOR_EMAIL", "cosmic-term@localhost"),
    ("GIT_COMMITTER_NAME", "cosmic-term"),
    ("GIT_COMMITTER_EMAIL", "cosmic-term@localhost"),
];

fn commit_tree(dir: &Path, tree: &str, parent: &str, message: &str) -> Result<String, String> {
    let env: Vec<(&str, &OsStr)> = COMMITTER.iter().map(|(k, v)| (*k, OsStr::new(v))).collect();
    git_env(
        dir,
        &[
            "commit-tree",
            "--no-gpg-sign",
            tree,
            "-p",
            parent,
            "-m",
            message,
        ],
        &env,
    )
}

/// A worktree's state as trees, leaving its index untouched
pub fn trees(top: &Path) -> Result<Trees, String> {
    let index = git(top, &["write-tree"])
        .map_err(|err| format!("the index has unresolved conflicts ({err})"))?;
    // The working tree, through a copy of the index
    let index_path = PathBuf::from(git(
        top,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?);
    let scratch = std::env::temp_dir().join(format!("ct-index-{}", crate::random_hex(12)));
    if index_path.exists() {
        fs::copy(&index_path, &scratch).map_err(|err| format!("copying the index: {err}"))?;
    }
    let scratch_env = [("GIT_INDEX_FILE", scratch.as_os_str())];
    let worktree = git_env(top, &["add", "-A", "--", "."], &scratch_env)
        .and_then(|_| git_env(top, &["write-tree"], &scratch_env));
    let _ = fs::remove_file(&scratch);
    Ok(Trees {
        index,
        worktree: worktree?,
    })
}

/// Record a worktree under [`transfer_ref`], leaving it and its branch as
/// they are
pub fn snapshot(top: &Path, id: &str) -> Result<Snapshot, String> {
    check_id(id)?;
    let branch = git(top, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty())
        .ok_or("the worktree has no branch checked out; check one out to transfer it")?;
    let base = git(top, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])
        .map_err(|_| "the branch has no commits yet".to_string())?;
    let trees = trees(top)?;
    let index = commit_tree(top, &trees.index, &base, "cosmic-term transfer: index")?;
    let worktree = commit_tree(
        top,
        &trees.worktree,
        &index,
        "cosmic-term transfer: working tree",
    )?;
    git(top, &["update-ref", &transfer_ref(id), &worktree])?;
    Ok(Snapshot {
        branch,
        base,
        index,
        worktree,
        trees,
    })
}

/// Delete a transfer's ref, if it is there
pub fn drop_ref(repo: &Path, id: &str) -> Result<(), String> {
    check_id(id)?;
    let name = transfer_ref(id);
    if git_ok(repo, &["rev-parse", "-q", "--verify", &name]) {
        git(repo, &["update-ref", "-d", &name])?;
    }
    Ok(())
}

/// The transfer refs left in a repository, by id
pub fn leftover_refs(repo: &Path) -> Result<Vec<String>, String> {
    let output = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/cosmic-term/transfer/",
        ],
    )?;
    Ok(output
        .lines()
        .filter_map(|r| r.strip_prefix("refs/cosmic-term/transfer/"))
        .map(str::to_string)
        .collect())
}

fn ssh_env(ssh: Option<&str>) -> Vec<(&'static str, &OsStr)> {
    ssh.map(|ssh| vec![("GIT_SSH_COMMAND", OsStr::new(ssh))])
        .unwrap_or_default()
}

/// Bring a transfer's snapshot into `repo` from `url`
pub fn fetch(repo: &Path, url: &str, id: &str, ssh: Option<&str>) -> Result<(), String> {
    check_id(id)?;
    let name = transfer_ref(id);
    let refspec = format!("+{name}:{name}");
    git_env(
        repo,
        &["fetch", "--no-tags", "-q", url, &refspec],
        &ssh_env(ssh),
    )
    .map(|_| ())
}

/// Send a transfer's snapshot from `repo` to `url`
pub fn push(repo: &Path, url: &str, id: &str, ssh: Option<&str>) -> Result<(), String> {
    check_id(id)?;
    let name = transfer_ref(id);
    let refspec = format!("+{name}:{name}");
    git_env(repo, &["push", "-q", url, &refspec], &ssh_env(ssh)).map(|_| ())
}

/// Make an empty repository for a project this computer has no clone of
pub fn init(path: &Path) -> Result<(), String> {
    if path.exists() && fs::read_dir(path).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("{} exists and is not empty", path.display()));
    }
    fs::create_dir_all(path).map_err(|err| format!("{}: {err}", path.display()))?;
    git(path, &["init", "-q"]).map(|_| ())
}

/// Where the destination's worktree comes from
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Placement {
    /// A new worktree at this path
    New(PathBuf),
    /// An existing worktree with the branch checked out, usually the idle
    /// copy an earlier transfer the other way left. `idle` is its state when
    /// it went idle: it is replaced only if it still has that state, or,
    /// without one, if it has no changes at all
    Reuse { path: PathBuf, idle: Option<Trees> },
}

impl Placement {
    pub fn path(&self) -> &Path {
        match self {
            Self::New(path) | Self::Reuse { path, .. } => path,
        }
    }
}

/// How [`prepare`] left a worktree, so that it can be undone
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Prepared {
    pub path: PathBuf,
    /// For a reused worktree, how it was before
    pub previous: Option<Previous>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Previous {
    pub head: String,
    pub branch: Option<String>,
    pub trees: Trees,
}

/// Why a destination cannot take a branch, if it cannot. `base` is checked
/// only when the destination has the commit already (after fetching)
pub fn check_branch(
    repo: &Path,
    branch: &str,
    placement: &Placement,
    base: Option<&str>,
) -> Result<(), String> {
    let name = format!("refs/heads/{branch}");
    let Ok(current) = git(repo, &["rev-parse", "-q", "--verify", &name]) else {
        return Ok(());
    };
    let elsewhere = worktrees(repo)?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(branch) && !same_path(&w.path, placement.path()));
    if let Some(worktree) = elsewhere {
        return Err(format!(
            "branch {branch} is checked out in {} on this computer",
            worktree.path.display()
        ));
    }
    if let Some(base) = base
        && !git_ok(repo, &["merge-base", "--is-ancestor", &current, base])
    {
        return Err(format!(
            "branch {branch} has commits on this computer that the transferred one lacks; \
             merge them by hand first"
        ));
    }
    Ok(())
}

/// Make a worktree's files and index those of `to`, given that they are now
/// exactly `from`: files only in `from` go, untracked ones included
fn switch_trees(path: &Path, from: &Trees, to: &Trees) -> Result<(), String> {
    git(path, &["read-tree", "--reset", &from.worktree])?;
    git(path, &["read-tree", "--reset", "-u", &to.worktree])?;
    git(path, &["read-tree", "--reset", &to.index])?;
    let _ = git(path, &["update-index", "-q", "--refresh"]);
    Ok(())
}

/// Add a worktree at `path` on a new branch from `source`'s `HEAD`, with
/// `source`'s staged, unstaged and untracked changes, still uncommitted.
/// `source` is left as it is
pub fn add_worktree_from(source: &Path, path: &Path, branch: &str) -> Result<(), String> {
    let head = git(source, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])
        .map_err(|_| "the worktree has no commits yet".to_string())?;
    let to = trees(source)?;
    git(
        source,
        &[
            "worktree",
            "add",
            "-q",
            "--no-track",
            "-b",
            branch,
            path_arg(path)?,
            &head,
        ],
    )?;
    let checked_out = git(path, &["rev-parse", "HEAD^{tree}"])?;
    let from = Trees {
        index: checked_out.clone(),
        worktree: checked_out,
    };
    switch_trees(path, &from, &to)
}

/// Set up the destination's worktree from a fetched snapshot, detached at
/// its base until [`commit`]
pub fn prepare(
    repo: &Path,
    placement: &Placement,
    snapshot: &Snapshot,
    id: &str,
) -> Result<Prepared, String> {
    check_id(id)?;
    check_branch(repo, &snapshot.branch, placement, Some(&snapshot.base))?;
    let (prepared, from) = match placement {
        Placement::New(path) => {
            if path.exists() && fs::read_dir(path).map_or(true, |mut d| d.next().is_some()) {
                return Err(format!("{} already exists", path.display()));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
            }
            git(
                repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--detach",
                    path_arg(path)?,
                    &snapshot.base,
                ],
            )?;
            let from = trees(path)?;
            let prepared = Prepared {
                path: path.clone(),
                previous: None,
            };
            (prepared, from)
        }
        Placement::Reuse { path, idle } => {
            let top = git(path, &["rev-parse", "--show-toplevel"])?;
            if !same_path(Path::new(&top), path) || !same_path(&repository(path)?, repo) {
                return Err(format!(
                    "{} is not a worktree of {}",
                    path.display(),
                    repo.display()
                ));
            }
            let current = trees(path)?;
            let unchanged = match idle {
                Some(idle) => &current == idle,
                None => git(path, &["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
            };
            if !unchanged {
                return Err(format!(
                    "{} has changes made since it was transferred away; commit or discard them first",
                    path.display()
                ));
            }
            let head = git(path, &["rev-parse", "HEAD"])?;
            if !git_ok(
                path,
                &["merge-base", "--is-ancestor", &head, &snapshot.base],
            ) {
                return Err(format!(
                    "{} has commits the transferred branch lacks; merge them by hand first",
                    path.display()
                ));
            }
            let branch = git(path, &["symbolic-ref", "--short", "-q", "HEAD"])
                .ok()
                .filter(|b| !b.is_empty());
            git(path, &["update-ref", "--no-deref", "HEAD", &snapshot.base])?;
            let prepared = Prepared {
                path: path.clone(),
                previous: Some(Previous {
                    head,
                    branch,
                    trees: current.clone(),
                }),
            };
            (prepared, current)
        }
    };
    let admin = PathBuf::from(git(&prepared.path, &["rev-parse", "--absolute-git-dir"])?);
    fs::write(admin.join(MARKER), id).map_err(|err| format!("marking the worktree: {err}"))?;
    switch_trees(&prepared.path, &from, &snapshot.trees)?;
    Ok(prepared)
}

/// The transfer id a worktree is marked with, if any
pub fn marker(path: &Path) -> Option<String> {
    let admin = git(path, &["rev-parse", "--absolute-git-dir"]).ok()?;
    fs::read_to_string(Path::new(&admin).join(MARKER))
        .ok()
        .map(|id| id.trim().to_string())
}

/// Undo [`prepare`]: remove a worktree it made, or put a reused one back
pub fn rollback(
    repo: &Path,
    prepared: &Prepared,
    snapshot: &Snapshot,
    id: &str,
) -> Result<(), String> {
    let path = &prepared.path;
    if !path.exists() {
        let _ = git(repo, &["worktree", "prune"]);
        return Ok(());
    }
    if marker(path).as_deref() != Some(id) {
        return Err(format!(
            "{} is not this transfer's; left alone",
            path.display()
        ));
    }
    match &prepared.previous {
        None => {
            git(repo, &["worktree", "remove", "--force", path_arg(path)?])?;
        }
        Some(previous) => {
            switch_trees(path, &snapshot.trees, &previous.trees)?;
            match &previous.branch {
                Some(branch) => git(
                    path,
                    &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
                )?,
                None => git(path, &["update-ref", "--no-deref", "HEAD", &previous.head])?,
            };
            clear_marker(path)?;
        }
    }
    Ok(())
}

fn clear_marker(path: &Path) -> Result<(), String> {
    let admin = git(path, &["rev-parse", "--absolute-git-dir"])?;
    match fs::remove_file(Path::new(&admin).join(MARKER)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.to_string()),
        _ => Ok(()),
    }
}

/// Point the branch at the snapshot's base and check it out in the prepared
/// worktree, keeping its files and index. After this the transfer is done
pub fn commit(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let repo = repository(path)?;
    check_branch(
        &repo,
        &snapshot.branch,
        &Placement::Reuse {
            path: path.to_path_buf(),
            idle: None,
        },
        Some(&snapshot.base),
    )?;
    let name = format!("refs/heads/{}", snapshot.branch);
    let old = git(path, &["rev-parse", "-q", "--verify", &name]).unwrap_or_default();
    git(
        path,
        &[
            "update-ref",
            "-m",
            "cosmic-term transfer",
            &name,
            &snapshot.base,
            &old,
        ],
    )?;
    git(path, &["symbolic-ref", "HEAD", &name])?;
    clear_marker(path)
}

/// A worktree with `branch` checked out
pub fn worktree_with_branch(repo: &Path, branch: &str) -> Result<Option<PathBuf>, String> {
    Ok(worktrees(repo)?
        .into_iter()
        .find(|w| w.branch.as_deref() == Some(branch))
        .map(|w| w.path))
}

/// An ignored file that travels with a transfer
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CarriedFile {
    /// Relative to the worktree
    pub path: String,
    pub mode: u32,
    /// A symbolic link's target, instead of contents
    #[serde(default)]
    pub link: Option<String>,
    #[serde(with = "crate::b64::serde_bytes", default)]
    pub data: Vec<u8>,
}

/// The ignored files matching a project's `carry_ignored` pathspecs
pub fn read_carried(top: &Path, patterns: &[String]) -> Result<Vec<CarriedFile>, String> {
    use std::os::unix::fs::PermissionsExt;
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec![
        "ls-files",
        "-z",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--",
    ];
    args.extend(patterns.iter().map(String::as_str));
    let listed = git(top, &args)?;
    let mut files = Vec::new();
    let mut total = 0;
    for path in listed.split('\0').filter(|p| !p.is_empty()) {
        let full = top.join(path);
        let meta = fs::symlink_metadata(&full).map_err(|err| format!("{path}: {err}"))?;
        let (link, data) = if meta.file_type().is_symlink() {
            let target = fs::read_link(&full).map_err(|err| format!("{path}: {err}"))?;
            (Some(target.to_string_lossy().into_owned()), Vec::new())
        } else {
            (
                None,
                fs::read(&full).map_err(|err| format!("{path}: {err}"))?,
            )
        };
        total += data.len();
        if total > CARRY_LIMIT {
            return Err(format!(
                "the ignored files to carry are over {} MiB; narrow the project's list",
                CARRY_LIMIT >> 20
            ));
        }
        files.push(CarriedFile {
            path: path.to_string(),
            mode: meta.permissions().mode() & 0o7777,
            link,
            data,
        });
    }
    Ok(files)
}

/// What [`write_carried`] did to one file, so that it can be undone
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Written {
    pub path: String,
    /// The file it replaced
    pub replaced: Option<CarriedFile>,
}

fn safe_relative(path: &str) -> Result<&Path, String> {
    let relative = Path::new(path);
    if relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        Ok(relative)
    } else {
        Err(format!("refusing to write outside the worktree: {path}"))
    }
}

fn read_existing(full: &Path, path: &str) -> Option<CarriedFile> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(full).ok()?;
    let (link, data) = if meta.file_type().is_symlink() {
        (
            Some(fs::read_link(full).ok()?.to_string_lossy().into_owned()),
            Vec::new(),
        )
    } else if meta.is_file() {
        (None, fs::read(full).ok()?)
    } else {
        return None;
    };
    Some(CarriedFile {
        path: path.to_string(),
        mode: meta.permissions().mode() & 0o7777,
        link,
        data,
    })
}

/// Write carried files into a worktree. An existing file is never
/// overwritten, unless the worktree is an idle copy and the file has not
/// changed since `idle_since` (seconds since the epoch); otherwise nothing
/// is written
pub fn write_carried(
    top: &Path,
    files: &[CarriedFile],
    idle_since: Option<u64>,
) -> Result<Vec<Written>, String> {
    let mut conflicts = Vec::new();
    for file in files {
        let full = top.join(safe_relative(&file.path)?);
        if let Some(existing) = read_existing(&full, &file.path)
            && (existing.data != file.data || existing.link != file.link)
        {
            let modified = fs::symlink_metadata(&full)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(u64::MAX, |d| d.as_secs());
            if !idle_since.is_some_and(|since| modified <= since) {
                conflicts.push(file.path.clone());
            }
        } else if full.exists() && !full.is_file() && !full.is_symlink() {
            conflicts.push(file.path.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "these files exist on this computer and differ: {}",
            conflicts.join(", ")
        ));
    }
    let mut written = Vec::new();
    for file in files {
        let full = top.join(safe_relative(&file.path)?);
        let replaced = read_existing(&full, &file.path);
        if replaced
            .as_ref()
            .is_some_and(|r| r.data == file.data && r.link == file.link)
        {
            continue;
        }
        if let Err(err) = put_file(&full, file) {
            let _ = undo_carried(top, &written);
            return Err(format!("{}: {err}", file.path));
        }
        written.push(Written {
            path: file.path.clone(),
            replaced,
        });
    }
    Ok(written)
}

fn put_file(full: &Path, file: &CarriedFile) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    if full.is_symlink() || full.is_file() {
        fs::remove_file(full)?;
    }
    match &file.link {
        Some(target) => std::os::unix::fs::symlink(target, full),
        None => {
            fs::write(full, &file.data)?;
            fs::set_permissions(full, fs::Permissions::from_mode(file.mode & 0o7777))
        }
    }
}

pub fn undo_carried(top: &Path, written: &[Written]) -> Result<(), String> {
    for done in written.iter().rev() {
        let full = top.join(safe_relative(&done.path)?);
        let result = match &done.replaced {
            Some(previous) => put_file(&full, previous),
            None => fs::remove_file(&full).or_else(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(err)
                }
            }),
        };
        result.map_err(|err| format!("{}: {err}", done.path))?;
    }
    Ok(())
}

/// Coding agents running in a worktree, as `claude (pid 42)`. Only this
/// user's processes are visible, which are the ones that matter
pub fn running_agents(top: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cwd) = fs::read_link(entry.path().join("cwd")) else {
            continue;
        };
        if !cwd.starts_with(top) {
            continue;
        }
        let Ok(cmdline) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        // An agent's program, or the script node runs
        let agent = cmdline
            .split(|b| *b == 0)
            .take(2)
            .filter_map(|arg| {
                Path::new(OsStr::new(std::str::from_utf8(arg).ok()?))
                    .file_name()?
                    .to_str()
                    .map(str::to_string)
            })
            .find(|name| name == "claude" || name == "codex");
        if let Some(agent) = agent {
            found.push(format!("{agent} (pid {pid})"));
        }
    }
    found.sort();
    found
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::project::tempdir::TempDir;

    pub fn run(dir: &Path, args: &[&str]) -> String {
        let mut all = vec![
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ];
        all.extend(args);
        git(dir, &all).unwrap_or_else(|err| panic!("git {args:?}: {err}"))
    }

    /// A repository with one commit on `main`
    pub fn repo(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        run(dir, &["init", "-q", "-b", "main"]);
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        fs::write(dir.join("b.txt"), "bee\n").unwrap();
        fs::write(dir.join(".gitignore"), "target/\n.env\n.local-design/\n").unwrap();
        run(dir, &["add", "."]);
        run(dir, &["commit", "-q", "-m", "first"]);
        dir.to_path_buf()
    }

    fn status(dir: &Path) -> String {
        git(dir, &["status", "--porcelain", "--untracked-files=all"]).unwrap()
    }

    /// Staged, unstaged and untracked changes, and a deletion
    fn make_changes(dir: &Path) {
        fs::write(dir.join("a.txt"), "staged\n").unwrap();
        run(dir, &["add", "a.txt"]);
        fs::write(dir.join("a.txt"), "staged then edited\n").unwrap();
        fs::remove_file(dir.join("b.txt")).unwrap();
        fs::write(dir.join("new.txt"), "untracked\n").unwrap();
    }

    #[test]
    fn snapshot_leaves_the_source_alone() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        make_changes(&src);
        let before = status(&src);
        let head = run(&src, &["rev-parse", "HEAD"]);
        let snap = snapshot(&src, "t1").unwrap();
        assert_eq!(status(&src), before);
        assert_eq!(snap.base, head);
        assert_eq!(snap.branch, "main");
        assert_eq!(run(&src, &["rev-parse", "HEAD"]), head);
        assert_eq!(leftover_refs(&src).unwrap(), vec!["t1".to_string()]);
        drop_ref(&src, "t1").unwrap();
        assert!(leftover_refs(&src).unwrap().is_empty());
    }

    #[test]
    fn transfer_round_trip_keeps_staged_unstaged_and_untracked_changes() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("laptop/thing"));
        run(&src, &["checkout", "-q", "-b", "feature"]);
        fs::write(src.join("c.txt"), "committed on feature\n").unwrap();
        run(&src, &["add", "c.txt"]);
        run(&src, &["commit", "-q", "-m", "feature work"]);
        make_changes(&src);
        let expected = status(&src);

        // The desktop has its own clone, without the feature branch
        let dst = tmp.path().join("desktop/thing");
        run(
            tmp.path(),
            &["clone", "-q", src.to_str().unwrap(), dst.to_str().unwrap()],
        );
        run(&dst, &["checkout", "-q", "main"]);
        let _ = git(&dst, &["branch", "-D", "feature"]);

        let snap = snapshot(&src, "t2").unwrap();
        fetch(&dst, src.to_str().unwrap(), "t2", None).unwrap();
        let path = tmp.path().join("desktop/worktrees/thing-feature");
        let placement = Placement::New(path.clone());
        let prepared = prepare(&dst, &placement, &snap, "t2").unwrap();
        assert_eq!(marker(&path).as_deref(), Some("t2"));
        commit(&path, &snap).unwrap();

        assert_eq!(status(&path), expected);
        assert_eq!(
            fs::read_to_string(path.join("a.txt")).unwrap(),
            "staged then edited\n"
        );
        assert_eq!(git(&path, &["show", ":a.txt"]).unwrap(), "staged");
        assert!(!path.join("b.txt").exists());
        assert_eq!(
            fs::read_to_string(path.join("new.txt")).unwrap(),
            "untracked\n"
        );
        assert_eq!(run(&path, &["symbolic-ref", "--short", "HEAD"]), "feature");
        assert_eq!(run(&path, &["rev-parse", "HEAD"]), snap.base);
        assert_eq!(marker(&path), None);
        assert_eq!(prepared.previous, None);
        // The main checkout is untouched
        assert_eq!(status(&dst), "");
    }

    #[test]
    fn rollback_removes_a_new_worktree_and_leaves_no_branch() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        make_changes(&src);
        let dst = tmp.path().join("dst");
        run(
            tmp.path(),
            &["clone", "-q", src.to_str().unwrap(), dst.to_str().unwrap()],
        );
        run(&dst, &["checkout", "-q", "--detach"]);
        run(&dst, &["branch", "-q", "-D", "main"]);
        let snap = snapshot(&src, "t3").unwrap();
        fetch(&dst, src.to_str().unwrap(), "t3", None).unwrap();
        let path = tmp.path().join("wt");
        let prepared = prepare(&dst, &Placement::New(path.clone()), &snap, "t3").unwrap();
        rollback(&dst, &prepared, &snap, "t3").unwrap();
        drop_ref(&dst, "t3").unwrap();
        assert!(!path.exists());
        assert_eq!(worktrees(&dst).unwrap().len(), 1);
        assert!(git(&dst, &["rev-parse", "-q", "--verify", "refs/heads/main"]).is_err());
    }

    /// A laptop worktree transferred to a desktop and worked on there
    fn round_trip_setup(tmp: &TempDir) -> (PathBuf, PathBuf, PathBuf) {
        let laptop = repo(&tmp.path().join("laptop"));
        run(&laptop, &["checkout", "-q", "-b", "feature"]);
        let desktop = tmp.path().join("desktop");
        run(
            tmp.path(),
            &[
                "clone",
                "-q",
                laptop.to_str().unwrap(),
                desktop.to_str().unwrap(),
            ],
        );
        run(&desktop, &["checkout", "-q", "main"]);
        let snap = snapshot(&laptop, "out").unwrap();
        fetch(&desktop, laptop.to_str().unwrap(), "out", None).unwrap();
        let away = tmp.path().join("desktop-wt");
        prepare(&desktop, &Placement::New(away.clone()), &snap, "out").unwrap();
        commit(&away, &snap).unwrap();
        fs::write(away.join("a.txt"), "evening\n").unwrap();
        run(&away, &["commit", "-q", "-am", "work on the desktop"]);
        fs::write(away.join("wip.txt"), "unfinished\n").unwrap();
        (laptop, desktop, away)
    }

    #[test]
    fn the_way_back_fast_forwards_the_idle_copy() {
        let tmp = TempDir::new();
        let (laptop, _desktop, away) = round_trip_setup(&tmp);
        let snap = snapshot(&away, "back").unwrap();
        fetch(&laptop, away.to_str().unwrap(), "back", None).unwrap();
        let reuse = worktree_with_branch(&laptop, "feature").unwrap().unwrap();
        assert_eq!(reuse, laptop);
        let prepared = prepare(
            &laptop,
            &Placement::Reuse {
                path: reuse.clone(),
                idle: None,
            },
            &snap,
            "back",
        )
        .unwrap();
        commit(&reuse, &snap).unwrap();
        assert_eq!(
            fs::read_to_string(laptop.join("a.txt")).unwrap(),
            "evening\n"
        );
        assert_eq!(status(&laptop), "?? wip.txt");
        assert_eq!(
            run(&laptop, &["symbolic-ref", "--short", "HEAD"]),
            "feature"
        );
        assert!(prepared.previous.is_some());
    }

    #[test]
    fn reused_worktrees_accept_symlink_aliases_but_reject_subdirectories() {
        let tmp = TempDir::new();
        let (laptop, _desktop, away) = round_trip_setup(&tmp);
        let alias = tmp.path().join("home");
        std::os::unix::fs::symlink(tmp.path(), &alias).unwrap();
        let aliased_laptop = alias.join("laptop");
        let snap = snapshot(&away, "alias-back").unwrap();
        fetch(&laptop, away.to_str().unwrap(), "alias-back", None).unwrap();
        let placement = Placement::Reuse { path: aliased_laptop.clone(), idle: None };
        check_branch(&laptop, "feature", &placement, Some(&snap.base)).unwrap();

        fs::create_dir_all(aliased_laptop.join("subdir")).unwrap();
        assert!(prepare(&laptop, &Placement::Reuse { path: aliased_laptop.join("subdir"), idle: None }, &snap, "alias-back").is_err());
        assert!(prepare(&away, &placement, &snap, "alias-back").is_err());
        let prepared = prepare(&aliased_laptop, &placement, &snap, "alias-back").unwrap();
        assert_eq!(prepared.path, aliased_laptop);
        commit(&prepared.path, &snap).unwrap();
        assert_eq!(fs::read_to_string(laptop.join("a.txt")).unwrap(), "evening\n");
        assert_eq!(status(&laptop), "?? wip.txt");
        assert!(!same_path(&tmp.path().join("missing-a"), &tmp.path().join("missing-b")));
    }

    #[test]
    fn the_way_back_refuses_an_idle_copy_edited_since() {
        let tmp = TempDir::new();
        let (laptop, _desktop, away) = round_trip_setup(&tmp);
        fs::write(laptop.join("b.txt"), "quick edit on the laptop\n").unwrap();
        let snap = snapshot(&away, "back").unwrap();
        fetch(&laptop, away.to_str().unwrap(), "back", None).unwrap();
        let err = prepare(
            &laptop,
            &Placement::Reuse {
                path: laptop.clone(),
                idle: None,
            },
            &snap,
            "back",
        )
        .unwrap_err();
        assert!(err.contains("changes made since"), "{err}");
        assert_eq!(
            fs::read_to_string(laptop.join("b.txt")).unwrap(),
            "quick edit on the laptop\n"
        );
    }

    #[test]
    fn the_way_back_refuses_a_diverged_idle_copy() {
        let tmp = TempDir::new();
        let (laptop, _desktop, away) = round_trip_setup(&tmp);
        fs::write(laptop.join("b.txt"), "committed on the laptop\n").unwrap();
        run(&laptop, &["commit", "-q", "-am", "diverge"]);
        let snap = snapshot(&away, "back").unwrap();
        fetch(&laptop, away.to_str().unwrap(), "back", None).unwrap();
        let err = prepare(
            &laptop,
            &Placement::Reuse {
                path: laptop.clone(),
                idle: None,
            },
            &snap,
            "back",
        )
        .unwrap_err();
        assert!(err.contains("commits"), "{err}");
    }

    #[test]
    fn rollback_puts_a_reused_worktree_back() {
        let tmp = TempDir::new();
        let (laptop, _desktop, away) = round_trip_setup(&tmp);
        let head = run(&laptop, &["rev-parse", "HEAD"]);
        let snap = snapshot(&away, "back").unwrap();
        fetch(&laptop, away.to_str().unwrap(), "back", None).unwrap();
        let prepared = prepare(
            &laptop,
            &Placement::Reuse {
                path: laptop.clone(),
                idle: None,
            },
            &snap,
            "back",
        )
        .unwrap();
        rollback(&laptop, &prepared, &snap, "back").unwrap();
        assert_eq!(status(&laptop), "");
        assert!(!laptop.join("wip.txt").exists());
        assert_eq!(run(&laptop, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            run(&laptop, &["symbolic-ref", "--short", "HEAD"]),
            "feature"
        );
        assert_eq!(marker(&laptop), None);
    }

    #[test]
    fn a_branch_checked_out_elsewhere_is_refused() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        let snap = snapshot(&src, "t4").unwrap();
        // The source repository itself has main checked out
        let err = check_branch(
            &src,
            "main",
            &Placement::New(tmp.path().join("x")),
            Some(&snap.base),
        )
        .unwrap_err();
        assert!(err.contains("checked out"), "{err}");
    }

    #[test]
    fn nested_worktrees_are_reported() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        run(
            &src,
            &["worktree", "add", "-q", "--detach", "agent-worktrees/one"],
        );
        let info = inspect(&src).unwrap();
        assert_eq!(info.nested, vec![src.join("agent-worktrees/one")]);
        assert_eq!(info.repository, src);
        let inner = inspect(&src.join("agent-worktrees/one")).unwrap();
        assert_eq!(inner.repository, src);
        assert!(inner.nested.is_empty());
    }

    #[test]
    fn carried_files_are_only_the_named_ignored_ones() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        fs::create_dir_all(src.join(".local-design")).unwrap();
        fs::write(src.join(".local-design/record.md"), "decisions\n").unwrap();
        fs::write(src.join(".env"), "KEY=1\n").unwrap();
        fs::create_dir_all(src.join("target")).unwrap();
        fs::write(src.join("target/big"), "build output\n").unwrap();
        fs::write(src.join("untracked.txt"), "travels in W\n").unwrap();
        let files = read_carried(&src, &[".local-design/".into()]).unwrap();
        assert_eq!(
            files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            [".local-design/record.md"]
        );
        assert!(read_carried(&src, &[]).unwrap().is_empty());
    }

    #[test]
    fn carried_files_never_overwrite_unless_idle_and_unchanged() {
        let tmp = TempDir::new();
        let dst = tmp.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join(".env"), "OLD=1\n").unwrap();
        let file = CarriedFile {
            path: ".env".into(),
            mode: 0o600,
            link: None,
            data: b"NEW=1\n".to_vec(),
        };
        let err = write_carried(&dst, std::slice::from_ref(&file), None).unwrap_err();
        assert!(err.contains(".env"), "{err}");
        // Edited after the copy went idle
        let err = write_carried(&dst, std::slice::from_ref(&file), Some(0)).unwrap_err();
        assert!(err.contains(".env"), "{err}");
        // Untouched since it went idle
        let written = write_carried(&dst, std::slice::from_ref(&file), Some(u64::MAX - 1)).unwrap();
        assert_eq!(fs::read_to_string(dst.join(".env")).unwrap(), "NEW=1\n");
        undo_carried(&dst, &written).unwrap();
        assert_eq!(fs::read_to_string(dst.join(".env")).unwrap(), "OLD=1\n");
        assert!(
            write_carried(
                &dst,
                &[CarriedFile {
                    path: "../x".into(),
                    ..file
                }],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn detached_heads_cannot_be_transferred() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        run(&src, &["checkout", "-q", "--detach"]);
        assert!(snapshot(&src, "t5").unwrap_err().contains("no branch"));
    }

    #[test]
    fn a_worktree_from_a_worktree_starts_with_its_uncommitted_changes() {
        let tmp = TempDir::new();
        let src = repo(&tmp.path().join("src"));
        run(&src, &["checkout", "-q", "-b", "feature"]);
        fs::write(src.join("c.txt"), "committed on feature\n").unwrap();
        run(&src, &["add", "c.txt"]);
        run(&src, &["commit", "-q", "-m", "feature work"]);
        make_changes(&src);
        let expected = status(&src);
        let head = run(&src, &["rev-parse", "HEAD"]);

        let path = tmp.path().join("worktrees/src-copy");
        add_worktree_from(&src, &path, "copy").unwrap();
        assert_eq!(status(&src), expected);
        assert_eq!(run(&src, &["symbolic-ref", "--short", "HEAD"]), "feature");
        assert_eq!(status(&path), expected);
        assert_eq!(run(&path, &["symbolic-ref", "--short", "HEAD"]), "copy");
        assert_eq!(run(&path, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read_to_string(path.join("a.txt")).unwrap(),
            "staged then edited\n"
        );
        assert_eq!(run(&path, &["show", ":a.txt"]), "staged");
        assert!(!path.join("b.txt").exists());
    }
}

// SPDX-License-Identifier: GPL-3.0-only

//! The project registry: what one computer knows about its own projects.
//!
//! Each computer keeps its own registry and is the only one that writes it
//! (another computer asks this one's helper). It records where this
//! computer's clones of a project are, where new worktrees go, which ignored
//! files travel with a transfer, and which worktrees were transferred away
//! and are now idle copies. A project's [`ProjectId`] is shared by the
//! computers that have it, which is how a transfer finds the destination's
//! clone without guessing from names or remote URLs.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use crate::Env;

/// The registry format this build reads and writes. A newer file is never
/// written by an older build, which could drop fields it does not know
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HostId(pub String);

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectId(pub String);

impl ProjectId {
    pub fn new() -> Self {
        Self(crate::random_hex(16))
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Registry {
    #[serde(default)]
    pub schema: u32,
    /// Made once, when the registry is first written
    #[serde(default)]
    pub host_id: HostId,
    /// Where new worktrees go, unless a repository says otherwise; `None`
    /// for `~/worktrees`
    #[serde(default)]
    pub worktree_root: Option<PathBuf>,
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub label: String,
    /// Ids this project had before it was matched with the same project on
    /// another computer
    #[serde(default)]
    pub aliases: Vec<ProjectId>,
    /// This computer's clones; rarely more than one
    #[serde(default)]
    pub repositories: Vec<RepoLocation>,
    /// Ignored files that travel with a transfer from this computer, as Git
    /// pathspecs relative to the worktree, such as `.env` or `.local-design/`
    #[serde(default)]
    pub carry_ignored: Vec<String>,
    /// Worktrees transferred to another computer, kept as idle copies
    #[serde(default)]
    pub dormant: Vec<Dormant>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepoLocation {
    /// The repository's main working directory, or a bare repository
    pub path: PathBuf,
    /// Where this repository's new worktrees go, instead of the registry's
    #[serde(default)]
    pub worktree_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Dormant {
    pub worktree: PathBuf,
    pub branch: String,
    /// The computer it went to, as this computer's terminal named it
    pub to_host: String,
    pub transfer: String,
    /// Seconds since the epoch
    pub at: u64,
    /// Its index and files when it went idle, so that the way back can
    /// tell whether anyone has touched it since
    #[serde(default)]
    pub trees: Option<crate::workspace::Trees>,
}

impl Registry {
    pub fn path(env: &Env) -> PathBuf {
        env.state_dir.join("projects.json")
    }

    /// The registry, or a new empty one if there is none yet
    pub fn load(env: &Env) -> Result<Self, String> {
        let path = Self::path(env);
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|err| format!("{}: {err}", path.display()))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(err) => Err(format!("{}: {err}", path.display())),
        }
    }

    fn new() -> Self {
        Self {
            schema: SCHEMA,
            host_id: HostId(crate::random_hex(16)),
            ..Self::default()
        }
    }

    /// Change the registry under a lock, so that several windows and
    /// helpers on this computer can write it, and replace the file whole
    pub fn update<T>(
        env: &Env,
        change: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        fs::create_dir_all(&env.state_dir)
            .map_err(|err| format!("{}: {err}", env.state_dir.display()))?;
        let _lock = Lock::take(&env.state_dir.join("projects.lock"))?;
        let mut registry = Self::load(env)?;
        if registry.schema > SCHEMA {
            return Err(format!(
                "the project registry was written by a newer version (format {}, this one {SCHEMA})",
                registry.schema
            ));
        }
        registry.schema = SCHEMA;
        if registry.host_id.0.is_empty() {
            registry.host_id = HostId(crate::random_hex(16));
        }
        let result = change(&mut registry)?;
        let path = Self::path(env);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&registry).map_err(|err| err.to_string())?;
        fs::File::create(&tmp)
            .and_then(|mut file| {
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .and_then(|()| fs::rename(&tmp, &path))
            .map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(result)
    }

    /// The project with this id or alias
    pub fn project(&self, id: &ProjectId) -> Option<&ProjectRecord> {
        self.projects
            .iter()
            .find(|p| &p.id == id || p.aliases.contains(id))
    }

    pub fn project_mut(&mut self, id: &ProjectId) -> Option<&mut ProjectRecord> {
        self.projects
            .iter_mut()
            .find(|p| &p.id == id || p.aliases.contains(id))
    }

    /// The project a repository belongs to
    pub fn project_of(&self, repository: &Path) -> Option<&ProjectRecord> {
        self.projects
            .iter()
            .find(|p| p.repositories.iter().any(|r| r.path == repository))
    }

    /// Record that `repository` belongs to the project `id`. If the
    /// repository already belongs to a project under another id, that
    /// project takes `id` and keeps its old one as an alias: the same
    /// project was registered on two computers before they first met
    pub fn bind(&mut self, id: &ProjectId, label: &str, repository: &Path) -> &mut ProjectRecord {
        let existing = self
            .projects
            .iter()
            .position(|p| p.repositories.iter().any(|r| r.path == repository));
        let index = match (
            existing,
            self.projects
                .iter()
                .position(|p| &p.id == id || p.aliases.contains(id)),
        ) {
            (Some(index), Some(other)) if index == other => index,
            (Some(index), Some(other)) => {
                // Both are known: fold the repository's record into `id`'s
                let merged = self.projects.remove(index);
                let other = if other > index { other - 1 } else { other };
                let target = &mut self.projects[other];
                target.aliases.push(merged.id);
                target.aliases.extend(merged.aliases);
                target.repositories.extend(merged.repositories);
                for pattern in merged.carry_ignored {
                    if !target.carry_ignored.contains(&pattern) {
                        target.carry_ignored.push(pattern);
                    }
                }
                target.dormant.extend(merged.dormant);
                other
            }
            (Some(index), None) => {
                let project = &mut self.projects[index];
                let old = std::mem::replace(&mut project.id, id.clone());
                project.aliases.push(old);
                index
            }
            (None, Some(index)) => {
                self.projects[index].repositories.push(RepoLocation {
                    path: repository.to_path_buf(),
                    worktree_root: None,
                });
                index
            }
            (None, None) => {
                self.projects.push(ProjectRecord {
                    id: id.clone(),
                    label: label.to_string(),
                    aliases: Vec::new(),
                    repositories: vec![RepoLocation {
                        path: repository.to_path_buf(),
                        worktree_root: None,
                    }],
                    carry_ignored: Vec::new(),
                    dormant: Vec::new(),
                });
                self.projects.len() - 1
            }
        };
        &mut self.projects[index]
    }

    /// The project of `repository`, registering it under a new id if it has
    /// none
    pub fn ensure(&mut self, repository: &Path) -> &mut ProjectRecord {
        if let Some(index) = self
            .projects
            .iter()
            .position(|p| p.repositories.iter().any(|r| r.path == repository))
        {
            return &mut self.projects[index];
        }
        let label = repository
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string());
        self.bind(&ProjectId::new(), &label, repository)
    }

    /// Where new worktrees of `repository` go
    pub fn worktree_root(&self, env: &Env, repository: &Path) -> PathBuf {
        self.projects
            .iter()
            .flat_map(|p| &p.repositories)
            .find(|r| r.path == repository)
            .and_then(|r| r.worktree_root.clone())
            .or_else(|| self.worktree_root.clone())
            // Beside the repository, as `thing-worktree-2026-10-05` beside
            // `thing`
            .unwrap_or_else(|| repository.parent().unwrap_or(&env.home).to_path_buf())
    }
}

/// A name usable in a directory, from a branch: `feature/x` → `feature-x`
pub fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches(['-', '.']).to_string();
    if slug.is_empty() {
        "worktree".to_string()
    } else {
        slug
    }
}

/// `<root>/<label>-<branch>`, with `-2`, `-3`… added until it is free
pub fn worktree_path(
    root: &Path,
    label: &str,
    branch: &str,
    taken: impl Fn(&Path) -> bool,
) -> PathBuf {
    let base = format!("{}-{}", slug(label), slug(branch));
    let mut path = root.join(&base);
    let mut n = 2;
    while taken(&path) {
        path = root.join(format!("{base}-{n}"));
        n += 1;
    }
    path
}

/// An exclusive `flock`, released when dropped
struct Lock {
    _file: fs::File,
}

impl Lock {
    fn take(path: &Path) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        // SAFETY: a plain syscall on a descriptor we own
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(format!(
                "{}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env() -> (tempdir::TempDir, Env) {
        let dir = tempdir::TempDir::new();
        let env = Env::at(dir.path());
        (dir, env)
    }

    #[test]
    fn host_id_is_made_once_and_kept() {
        let (_dir, env) = temp_env();
        let first = Registry::update(&env, |r| Ok(r.host_id.clone())).unwrap();
        let second = Registry::update(&env, |r| Ok(r.host_id.clone())).unwrap();
        assert!(!first.0.is_empty());
        assert_eq!(first, second);
        assert_eq!(Registry::load(&env).unwrap().host_id, first);
    }

    #[test]
    fn refuses_to_write_a_newer_format() {
        let (_dir, env) = temp_env();
        fs::create_dir_all(&env.state_dir).unwrap();
        let newer = format!(
            r#"{{"schema": {}, "host_id": "x", "future": true}}"#,
            SCHEMA + 1
        );
        fs::write(Registry::path(&env), &newer).unwrap();
        let err = Registry::update(&env, |_| Ok(())).unwrap_err();
        assert!(err.contains("newer version"), "{err}");
        assert_eq!(fs::read_to_string(Registry::path(&env)).unwrap(), newer);
    }

    #[test]
    fn binding_a_known_repository_adopts_the_other_computers_id() {
        let mut registry = Registry::new();
        let repo = Path::new("/home/a/src/thing");
        let local = registry.ensure(repo).id.clone();
        let shared = ProjectId("from-laptop".into());
        registry.bind(&shared, "thing", repo);
        let project = registry.project_of(repo).unwrap();
        assert_eq!(project.id, shared);
        assert_eq!(project.aliases, vec![local.clone()]);
        // Found by either id afterwards
        assert_eq!(registry.project(&local).unwrap().id, shared);
        assert_eq!(registry.projects.len(), 1);
    }

    #[test]
    fn binding_merges_two_records_of_one_project() {
        let mut registry = Registry::new();
        let first = registry.ensure(Path::new("/r/one")).id.clone();
        let second = registry.ensure(Path::new("/r/two")).id.clone();
        registry.project_mut(&second).unwrap().carry_ignored = vec![".env".into()];
        registry.bind(&first, "one", Path::new("/r/two"));
        assert_eq!(registry.projects.len(), 1);
        let project = registry.project(&first).unwrap();
        assert_eq!(project.repositories.len(), 2);
        assert_eq!(project.aliases, vec![second]);
        assert_eq!(project.carry_ignored, vec![".env".to_string()]);
    }

    #[test]
    fn a_project_can_have_several_clones_on_one_computer() {
        let mut registry = Registry::new();
        let id = registry.ensure(Path::new("/r/one")).id.clone();
        registry.bind(&id, "one", Path::new("/r/one-again"));
        assert_eq!(registry.project(&id).unwrap().repositories.len(), 2);
    }

    #[test]
    fn worktree_paths_follow_the_override_then_the_default() {
        let env = Env::at("/home/u");
        let mut registry = Registry::new();
        let repo = Path::new("/home/u/src/thing");
        registry.ensure(repo);
        assert_eq!(registry.worktree_root(&env, repo), Path::new("/home/u/src"));
        registry.worktree_root = Some("/w".into());
        assert_eq!(registry.worktree_root(&env, repo), Path::new("/w"));
        registry.projects[0].repositories[0].worktree_root = Some("/x".into());
        assert_eq!(registry.worktree_root(&env, repo), Path::new("/x"));
    }

    #[test]
    fn worktree_names_are_slugged_and_made_unique() {
        let root = Path::new("/w");
        let taken = |p: &Path| p == Path::new("/w/thing-feature-x");
        assert_eq!(
            worktree_path(root, "thing", "feature/x", taken),
            Path::new("/w/thing-feature-x-2")
        );
        assert_eq!(slug("../.."), "worktree");
    }
}

/// Temporary directories for tests, without a dependency
#[cfg(test)]
pub(crate) mod tempdir {
    use std::path::{Path, PathBuf};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ct-test-{}", crate::random_hex(12)));
            std::fs::create_dir_all(&path).unwrap();
            // Canonical, since Git reports canonical paths
            Self(path.canonicalize().unwrap())
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

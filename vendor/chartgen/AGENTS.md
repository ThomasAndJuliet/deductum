# Working in this repository

This repository holds the diagram tools Juliet and Tom use for papers,
presentations and recreational things. chartgen (with figkit) is its first
contents. It is private between the two of them; anything to be made more
public goes to a separate repository.

Juliet is the principal developer: she owns the rulings and the architecture
contract. Tom is the second developer. Each works on what their own work
needs. These conventions are not enforced by GitHub; every agent working here
follows them.

## Changes

1. Every change goes through a pull request from a short-lived branch.
   Never push to `main` directly.
2. Open the pull request as a **draft when the work starts**, not when it
   ends, so the other developer's agents can see what is in progress. Before
   starting work, check the open pull requests (`gh pr list`) for work on
   the same files.
3. When CI is green, merge your own pull request:
   `gh pr checks --watch`, then `gh pr merge --squash --delete-branch`.
4. If the pull request has the label **`needs-juliet`** (CI adds it when a
   change touches `tests/rulings/`, `docs/chartgen/RULINGS.md` or
   `ARCHITECTURE.md`), **do not merge.**
   Leave it for Juliet's approval.

## Places

- `crates/` — stable: tested, follows the rules, overall shape settled
  (complete or not; missing features get added, the shape stays).
- `experimental/` — graduated, but the overall shape is still unsettled:
  usable, may change or break.
- `tests/rulings/` — Juliet's rulings as tests; `docs/chartgen/RULINGS.md`
  lists every decided ruling and its test. Changing one needs Juliet's
  approval; a new ruling is added to `RULINGS.md` (and `DECISIONS.md` where
  it has an entry) and here in the same change.
- `ARCHITECTURE.md` — the layer contract. Changing it needs Juliet's
  approval.
- `docs/` — guidance. `docs/chartgen/` was written for the Python prototype
  of chartgen; its rules carry over to this implementation.

## Rules

- **Never commit anything from `sandbox/`.** It is a private, separate git
  repository inside some clones (ignored by `.gitignore`). If you are working
  inside `sandbox/`, you are committing to that repository, not this one.
- **Nothing here may depend on `sandbox/`.** The sandbox may use this
  repository; never the reverse. CI checks this.
- **The Python prototype of chartgen is never patched.** It lives only in
  Juliet's sandbox, for the migration. A problem is fixed here, in Rust.
- Moving a crate from `experimental/` to `crates/` needs no approval, once
  its shape is settled.

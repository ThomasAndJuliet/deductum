# Patched dependencies (household fork only)

## winit-wayland

`winit-wayland` 0.31.0-beta.2 from pop-os/winit at rev 9567503 (the one
libcosmic pins), as `cargo vendor` normalises it, with one change in
`src/seat/pointer/mod.rs` (`pointer_frame`): events for a pointer released
when the seat lost its pointer capability are dropped instead of panicking
with "failed to get pointer data."

The panic takes down any libcosmic app when the last pointer device goes
away while pointer events for a newly opened window, dialog or popup are
queued. It was reproduced with upstream cosmic-files master and Fedora's
cosmic-files 1.10.0; the household cosmic-files fork carries the same
patch (commit 264d80c).

Drop this patch, and its `[patch]` entry in `Cargo.toml`, when libcosmic
moves to a winit with the fix, or moves off rev 9567503 (then the patch no
longer applies: Cargo warns that it is unused).

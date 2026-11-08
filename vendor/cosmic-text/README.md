# cosmic-text, with a hanging indent and justified buffers

Deductum's copy of [cosmic-text](https://github.com/pop-os/cosmic-text) **0.19.0** from crates.io,
used through `[patch.crates-io]` in Deductum's `Cargo.toml`. It is excluded from Deductum's
workspace, as `vendor/chartgen` is. cosmic-text's licence is MIT or Apache-2.0 (`LICENSE-MIT`,
`LICENSE-APACHE`).

## What changed, and why

A wrapped line of code continues at its own indentation plus two characters, so that the shape of
a proof survives wrapping (Tom, 8 Nov: Deductum's AUDIT.md, "Responsive order"). cosmic-text 0.19
has no hanging indent.

- `src/hanging.rs` (new): `HangingIndent { columns }`, and `with_hanging_indent`, which makes the
  buffers created inside it start with the setting.
- `src/buffer.rs`: `Buffer` has the setting (`hanging_indent`, `set_hanging_indent`), starting from
  `with_hanging_indent`'s, and passes it to the layout. `Buffer::hit`: with the setting on, a click
  left of a wrapped row's first glyph is that glyph, not the end of the row.
- `src/buffer_line.rs`: `BufferLine::layout_with_hanging_indent`; `layout` is that without one.
- `src/shape.rs`: `ShapeLine::layout_to_buffer_with_hanging_indent`; `layout_to_buffer` is that
  without one. Every row after a line's first is seeded with the indent (`VisualLine::indent`, and
  in `w`), so it wraps at the width less the indent, and its glyphs start that far in. The caret,
  clicks, selection and drawing go by glyph positions, so they follow. A word too long for a wrapped
  row breaks to fit that row. The indent is at most half the width. Right-to-left and aligned lines
  are not indented.
- Justified buffers, for prose being edited (a paragraph opened for editing stays justified as it was
  shown). `Buffer::set_justified`: lines with no alignment of their own are laid out as
  `Align::Justified`, set when a line is laid out, so lines added by editing, splitting or pasting
  are included, and the last row of each line stays ragged. `src/hanging.rs` has
  `with_justified`, as `with_hanging_indent` is for the indent, for the same reason. Caret, clicks
  and selection follow the justified glyph positions with no change of their own.
- `tests/hanging_indent.rs` (new) tests the patch.
- `Cargo.toml`: the registry's manifest without its benchmarks, tests, dev-dependencies and test
  profile, which are left out of this copy with the demo scripts, samples and most test fonts, and
  with a `[workspace]` of its own, as `vendor/chartgen` has.

Nothing changes for a buffer without the setting, whatever its text: no existing signature changed.

`with_hanging_indent` is there because iced creates an editor's `Buffer` inside
`text_editor::Content::with_text` and gives no way to reach it afterwards; changing that would need
a change in the libcosmic fork. Deductum makes its code editors' `Content` inside it
(`src/wrap.rs`); menus, prose and other text never see the setting.

## Testing

Deductum's test binary includes `tests/hanging_indent.rs`, so the usual suite covers it:

    cargo --offline --locked test --bin deductum cosmic_text_hanging

Deductum's own tests (`src/snapshot_cells.rs`, "a wrapped line of code") check it on the page. Run
alone, with `--manifest-path`, this crate also resolves its optional `vi` dependencies, which are
not in Deductum's lock or offline cache, so that needs the network.

## Upstream

A candidate for upstream: `Buffer::set_hanging_indent` and the layout change would stand alone
there, without `with_hanging_indent`.

## Moving to a newer cosmic-text

Compare this folder's `src` with the 0.19.0 crate to get the patch, and apply it to the new version:
each change is small and the layout's are all at the points where a new row starts. Then update
`Cargo.lock` and `packaging/flatpak/cargo-sources.json` (see `docs/development.md`). If iced has
meanwhile moved to a different cosmic-text version, the patch must follow it, or it will not apply.

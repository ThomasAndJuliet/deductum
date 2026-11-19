# Deductum proof-command weight comparison

Before: `7f319bffb` with the snapshot fixture of `3aa1c0b43` (Isabelle's keyword3 kept as keyword3). After: `3aa1c0b43`. Automated headless render of an open Isabelle cell from the existing `snapshot_code::code_colours_frames` harness, light and dark, the same fixture, size and theme within each pair; each 640×460 frame is cropped to the cell. Not a screenshot of a live prover session: the token classes are the fixture's, set as Isabelle reports them (`case`, `then`, `show` as keyword3).

Command: `CT_SNAPSHOTS=<dir> cargo --offline --locked test --bin deductum -- code_colours_frames`, passed (1 test) for each side.

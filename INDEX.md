# Deductum text boxes: before / after

BEFORE = origin/main ebf75a1, AFTER = feat/text-boxes b33117be. Headless snapshot harness (real widgets, 1:1, tiny-skia), made-up demo content. Each image: Before left, After right; purple 2 px = changed region.

- `01-palette.png`: Insert palette search (the bar above the goal). rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 718x794
- `02-symbols.png`: Symbols panel search. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1142x794
- `03-search.png`: Search panel query. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1030x812
- `04-docs.png`: Sidebar document filter. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 842x818
- `05-cex.png`: Counterexample parameters. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1142x812
- `06-reply.png`: Reply-to-agent-note field (in the note card). rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1816x818
- `07-tool-timeout.png`: Proof tool settings: Timeout (Sledgehammer panel). rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 542x818
- `08-tool-provers.png`: Proof tool settings: Provers (Sledgehammer panel). rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1142x812
- `09-session-folder.png`: New session: folder. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1238x812
- `10-session-logic.png`: New session: base logic. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 798x818
- `11-session-dirs.png`: New session: session directories. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 798x812
- `12-settings-search.png`: Settings search. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1108x662
- `13-settings-model.png`: Settings: model field. rows: light then dark; rest = empty, not focused; focus = clicked, text typed; text = text set, not focused. Purple = changed region. 1178x812
- `14-right-click-menu.png`: Right-click menu on the Symbols search, without and with a selection. 1182x1698
- `15-selection.png`: Selection in a field: focused (solid accent) and kept after the keys moved away (tint). 1142x538
- `16-reply-menu.png`: Reply field in a note card: right-click menu (all selected). 1956x910
- `17-ctrl-z-light.png`: Ctrl+Z / redo in the Symbols search (light). 1142x350
- `18-ctrl-z-dark.png`: Ctrl+Z / redo in the Symbols search (dark). 1142x350
- `19-open-proof-cell.png`: Open proof cell (lemma being edited): should be identical. 1718x538; full-frame differing pixels (light, dark): [('light', 'editor-area differing pixels', 0, 'whole frame', 6039, (38, 254, 855, 419)), ('dark', 'editor-area differing pixels', 0, 'whole frame', 3533, (745, 254, 855, 286))]

## Notes

- Frames come from a throw-away capture test (not committed) run against each tree with the headless harness; window 1400x900 (reply 1400x1600, settings model 1400x2400), tiny-skia, light = household light theme, dark = COSMIC dark.
- Not shown: the typed tool's folded settings (not reachable in the demo app: the Output panel has no tool-settings section without a typed tool's output). The Sledgehammer panel's two fields are shown (07, 08).
- In the Before Ctrl+Z sequence (17, 18) the app's own window key handler is not run by the harness; the field itself ignores Ctrl+Z, as shown.
- In the After right-click menu with a selection the first two rows read Copy, Cut (without a selection: Cut, Copy).
- 19: in the lemma's editor area (x 24-730, y 248-322 of the 1400x900 frame) 0 pixels differ between Before and After in both themes; the only differences in the crop are the reply field in the note card (outlined in light; in dark it is below the outline threshold) and, elsewhere in the frame, the Step palette field.

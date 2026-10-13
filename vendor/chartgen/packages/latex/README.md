# LaTeX adapter

Add `\\usepackage{chartgen}` and `\\input{chart.pgf}`. Generated PGF is a
standalone `pgfpicture`. It converts top-left 1/72-inch coordinates to TeX
`bp` and reverses Y. Plain labels are escaped before reaching TeX and inherit
the document's current font. Native labels use `\\chartgenUseNative{key}`;
register each with `\\chartgenDeclareNative{key}{content}` in the same font
context used for measurement. Missing keys cause a LaTeX package error.
`\\chartgenNativeWidth{key}` and `\\chartgenNativeHeight{key}` expose the
stored box dimensions for an external measurement helper.

Native label tokens inherit the chart's measured size, weight, slant and ink at
inclusion. `\chartgenDeclareNative` also stores a box for the external width
helpers; those helpers reflect the declaration-time font. Use the same native
content and preamble for measurement and inclusion. PGF color support currently
accepts six-digit hex and basic named colors; other colors return a render
error.

## Charts drawn inside the document

`\chartgen[options]{spec.json}` draws a chart while the document compiles, with no files
made beforehand. The engine asks for its labels to be measured; the document sets each
label where the chart stands, in its own fonts, and reports its box; the engine returns
the PGF picture, which is set in place, like `\includegraphics` places an image. A label
given in the specification for several document systems
(`{"typst": …, "latex": …, "alternative": …}`) uses its LaTeX markup.

Two routes; both need `--shell-escape`, since LuaTeX loads binary modules only with it:

| route   | engine            | how                                                                 |
|---------|-------------------|---------------------------------------------------------------------|
| `lua`   | LuaLaTeX          | `chartgen.lua` loads the `chartgen_luatex` module (`crates/chartgen-luatex`) and runs the engine in process: one pass, no files, no processes |
| `shell` | any               | `\write18` runs `chartgen-tex` (`crates/chartgen-tex`) once per measurement round; the round's files are `\jobname-chartgenN.{opt,meas,tex}` |

`route=auto`, the default, takes `lua` under LuaLaTeX when the module loads, else `shell`.
Build both with `cargo build --release -p chartgen-tex -p chartgen-luatex`, and put this
directory on `TEXINPUTS` (and `LUAINPUTS`, for `chartgen.lua`).

Options, for `\chartgenset{…}` or one chart's `[…]`:

| key          | default          | meaning                                                         |
|--------------|------------------|-----------------------------------------------------------------|
| `width`      | the spec's own `layout.width`, else `\linewidth` | the chart's width                  |
| `height`     | the spec's       | the chart's height                                              |
| `fit`        | off              | scale a chart wider than the line down to it, labels and all; otherwise such a chart is a warning |
| `theme`      | the engine's `plain` | a theme file (JSON)                                         |
| `variant`    |                  | the specification's variant                                     |
| `route`      | `auto`           | `auto`, `lua` or `shell`                                        |
| `program`    | `chartgen-tex`   | the shell route's program                                       |
| `module`     | `chartgen_luatex`| the Lua module: a name for `require`, or a path to the library  |
| `max rounds` | 8                | measurement rounds before the layout counts as unsettled        |

A specification can be written in the document with the kernel's
`\begin{filecontents*}[overwrite]{chart.json} … \end{filecontents*}` before
`\chartgen{chart.json}`. When the engine refuses a chart, or cannot be reached, the document
gets a LaTeX error naming the reason, never a broken picture. Labels are measured in the
fonts in force where the chart stands, so a layout can fit in one font and not in another.
See `examples/latex/charts.tex`, built by `examples/latex/build.sh`.

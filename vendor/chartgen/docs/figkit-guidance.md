# Authoring measured figures

Figkit uses physical points (1/72 inch), a top-left origin and downward Y. Resolve the exact typography and ink before asking the host to measure content. Keep that request, including its context, with the returned metrics. Place the measured box; a renderer preserves this placement. Changing the width, typography, font environment or native-content revision requires a new measurement. A profile that changes those inputs belongs before measurement and placement.

Inspect a figure at its intended physical size. Text must remain legible and strokes must survive that size and the intended reproduction process. Supply a useful title and alternative description that explain what the figure communicates. Native content remains in its host registry until composition completes; its key is a reference, not portable source code.

Decide explicitly which elements must remain distinguishable when colour is unavailable. Supply those pairs to the output-profile checker. Meaningful alternatives may involve labels, shapes, patterns or spatial organization, chosen by the author. Figkit does not infer those obligations from geometry and does not recolour a completed figure.

## Monochrome/print screening assumptions

`figkit_scene::profile::OutputProfile::default()` names the `monochrome-print` profile. Its configurable defaults are a white background, output scale 1, minimum contrast ratio 4.5, minimum stroke width 0.25 pt and minimum monochrome luminance difference 0.1. These defaults are screening settings, not chart policy or a claim of accessibility.

The conversion accepts opaque solid #rgb/#rrggbb and the named colours black, white, red, green, blue, gray and grey. Each sRGB channel is linearized using the sRGB transfer curve; relative luminance is `0.2126 R + 0.7152 G + 0.0722 B`. Contrast is `(lighter luminance + 0.05)/(darker luminance + 0.05)`. Distinguishability compares the absolute luminance difference of a caller-declared pair. Unsupported paints, opacity or backgrounds produce an indeterminate observation. This conversion does not model printer ink, paper, halftoning or viewing conditions. Numerical contrast checks cannot certify printed readability.

Profile reports retain the supplied assumptions and conversion provenance. They leave authored paint, metrics and placement untouched. Review observations alongside an actual-size proof. Chart generators retain their own remedies, such as choosing marker symbols for series or explaining a hatch used for uncertainty.

## Geometry observations

Generic observations identify elements whose bounds overlap or extend beyond a region. They carry no application severity or rule number. Bounds overlap means positive-area rectangle overlap, not exact intersection of arbitrary path ink. Declare allowed element pairs explicitly for deliberately composed labels. The chart adapter retains chart-specific exemptions and rules.

## Compatibility and portable output

Scene version 1 and the house SVG source profile retain legacy classes, raw attributes, definitions and baseline placeholders. Portable version 2 requires measured text and resolved resources. A legacy placeholder cannot establish a measured portable label. Raw SVG resources stay in the compatibility envelope and may be rendered only by a backend declaring that capability. Unsupported resources or native composition produce an explicit error before output is returned.

## Bounded preparation and encoding

`Limits::default()` permits depth 32, 100,000 primitives, 1,000,000 path commands, 4,096 resources, 16 MiB of input and 64 MiB of output. Symbol definitions and every expanded instance count toward primitive and path-command limits; raw compatibility resource entries count toward resource limits. Preparation checks the compact serialized figure and compatibility envelope together against input bytes before cloning. The encoder stops string growth at the output limit and returns a budget error without publishing partial output. These are configurable local budgets.

Preparation retains native blocks with no baseline. Plain SVG/PGF encoding needs a measured baseline and rejects a missing one. Direct SVG/PGF text supports composed single lines; bounded-width or multiline blocks should be supplied as measured lines or native content. Leading, trailing and repeated ASCII spaces are preserved in portable SVG/PGF text. Tabs require composed lines or native content and are rejected before encoding. PGF plain text uses an explicit NFSS family identifier. Native content can be placed by the matching Typst, browser or TeX host; an alternative string alone has not been measured and cannot substitute for native content. Typst/browser plans retain the complete request, metrics, paint and top-left placement in paint order. Native placement plans currently support rectangular clips; SVG/PGF also support typed path clips.

Generic geometric lint has separate configurable defaults of 1,000,000 pair comparisons and 100,000 observations. Checked lint returns an error if either budget is exhausted. Curves use conservative control-hull bounds and arcs use the full endpoint-derived ellipse envelope, so those overlap observations warrant visual inspection. Raw SVG resources produce an observation explaining that geometric lint cannot inspect them.

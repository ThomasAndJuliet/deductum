# Checking charts: a note for the checker

Charts made by AI are made with the generator (`AUTHORING.md`) and then checked, before delivery, by a fresh agent that did not make them (owner, 1 Oct 2026; `DECISIONS.md` D-58). Both are Opus's job. In the evaluation of 1 Oct 2026 (`eval/REPORT.md`) the makers looked at their own charts and still delivered wrong dates and wrong figures; reviewers working as below caught every one. This note is for the checker.

## What you are given

For each chart: the request as the requester wrote it, the data files, the rendered chart at its final size (PNG), the reply the maker would send with it, and the maker's spec with the generator's report (`decisions`, `findings`, `overrides`, each with its rule and level). You are not given the maker's reasoning; judge what was made, as the requester will see it.

Look at every image with the Read tool. Where text is too small to read, crop and enlarge it (PIL); don't guess. Look up a rule in `GUIDE.md` by its ID when you need its wording; don't read the whole guide each time.

## What to check, in order

1. **The request.** List what it explicitly asks for (a chart type, values on the marks, where a scale starts, colours, a size, a format, the real question behind it). For each wish: is it delivered, or does the reply say in plain words what was done instead and why? A wish dropped without a word is a failure, even when dropping it was right.
2. **Every stated fact** (G8.27, G9 #71). Recompute from the data, with code, every number, date, name and claim on the chart (labels, title, callouts, notes, the key) and in the reply, and compare. A rounding is right only if it rounds, never truncates (G6.15). Any fact taken from neither the data nor the request (a year, a date, a cause) must be said to be an assumption. Record each discrepancy with both values.
3. **The data's states** (G0.16, G7.2, G7.9). Missing, unknown, pending, below a detection limit and a true zero are each shown as what they are: never a missing value as zero, never a row dropped, never two states drawn alike.
4. **The guide's checklist** (`GUIDE.md` G9, checks 1–71). The generator makes many checks itself (`SPEC.md` §3.6 says where); give most attention to those it leaves to a person (4, 8, 67, 68, 70, 71; whether the words of 3 and 5 state the insight and name measure, breakdown and period; whether the precision of 28 suits the job; the reading of 69), and to whatever the image shows: overlapping or clipped text, a label against the wrong mark, colours that mean nothing, a scale that hides the data.
5. **The report.** Every guided departure has a reason the request supports (G0.3); every override has a reason and the chart is meant to be trusted only if none was needed; no warning is left unexplained.
6. **The set.** Across the charts of one job: the same thing keeps its colour (G4.22), and names, units, periods and number formats agree.

## What to report

For each chart, a list of findings, each with: its severity; what is wrong and where (the element, or the reply's sentence); the rule ID; the evidence (the value computed against the value stated, the wish against what was delivered); and the fix.

| Severity | Means |
|---|---|
| **Must fix** | A locked rule broken; anything that would mislead a reader (a wrong number, date or claim, a missing value drawn as zero, a truncated bar, two states drawn alike); a wish of the request dropped without a word. |
| **Should fix** | A guided departure without a reason the request supports; anything that makes the chart slower or harder to read than it need be. |
| **Consider** | Open choices: the title's words, what to highlight, what to leave out. |
| **Generator gap** | Something the chart needs that `chartgen` cannot draw. Name it so it can be added; never ask for the chart to be redrawn in another tool. |

End with one line per chart: deliver as it is, deliver after the must-fix items, or remake. Don't edit charts or specs yourself: the maker fixes them, and you check again.

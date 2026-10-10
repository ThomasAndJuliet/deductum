# Charts in COSMIC Terminal threads

The thread draws a chart from a fenced code block whose language is
`chartgen` and whose body is one chartgen specification, as JSON. The chart is
drawn natively, in the thread's colours, and its text can be selected. Use it
for charts of data rather than drawing an image.

````
```chartgen
{"chartspec": 1, "id": "headcount", "purpose": "analysis",
 "title": {"text": "Headcount by region"},
 "data": {"x": {"kind": "nominal", "values": ["East", "Central", "West"]},
          "measure": {"name": "headcount", "kind": "count", "zero": "meaningful"},
          "series": [{"id": "headcount", "name": "Headcount", "values": [48, 41, 29]}]},
 "chart": {"type": "bar", "orientation": "vertical"}}
```
````

Check a specification before showing it: `ct-chartgen check FILE` (or the
specification on standard input) lays it out as the thread will and says
which chart it drew, or why chartgen refused. chartgen refuses charts that
would misrepresent the data (a box plot of four measurements, an interval
that does not say what it is): fix the specification as the message says,
never work around it. `ct-chartgen schema` prints the full JSON schema.

## The specification

- `chartspec`: 1. `id`: a short slug. `purpose`: `analysis` or `communication`.
- `title`: `{"text": …}`.
- `data.x`: the categories or times: `{"kind": "nominal" | "ordinal" | "time", "values": [...]}`.
- `data.measure`: what the values are: `name`, `kind` (`count`, `amount`,
  `percent` (45 for 45 %), `score`, `quantity`, `change`, `index`, `rank`),
  `unit` (e.g. `{"suffix": " ms"}`), `zero` (`meaningful` or `arbitrary`),
  optional `min`/`max` and `good` (`higher` or `lower`).
- `data.series`: `[{"id", "name", "values": [...]}]`, one value per `x` value
  (`null` where missing); several series for a comparison.
- `chart.type`: `auto` (chartgen chooses), or `bar`, `line`, `dot`, `step`,
  `slope`, `stacked_bar`, `stacked_bar_100`, `stacked_area`, `waterfall`,
  `pie`, `treemap`, `heatmap`, `table`, `bullet`, `funnel` and others in the
  schema. Options: `orientation` (`vertical`/`horizontal`), `sort`
  (`desc`/`asc`/`given`), `value_labels` (`on`/`off`), `labelling`
  (`direct`/`legend`).

## Statistical charts

| `chart.type` | Shows | Series carry |
|---|---|---|
| `box` | quartiles, whiskers and outliers per category | `samples` (one array per `x` value, 5 or more each), or `summaries` |
| `strip` | every measurement, with its median | `samples` |
| `ecdf` | the share of measurements at or below each value | `samples` |
| `scatter` | two numeric measures, one mark per case | `xy` pairs; `data.x_measure` names the x axis |
| `dumbbell` | two conditions on the same categories (`view`: `pair` or `difference`) | `values` (exactly two series) |
| `interval_dot`, `interval_bar` | an estimate with an interval that says what it is | `samples`, or `successes` with `trials`, or `values` with `intervals` |
| `state_matrix` | a declared state in every cell | `states` (one id per `x` value); `data.states` declares them |
| `contingency` | counts of an exhaustive two-way partition, with marginals | `counts`; `data.parts` |

Options: `whiskers`, `quantiles`, `show_points`, `mean`, `notch`, `n`,
`interval`, `view`, `difference`, `fit`. `chart.type: auto` picks from the
series: `xy` is a scatter plot, `samples` a box plot (a strip below 5
measurements), `counts` a contingency table, `successes` an interval dot.

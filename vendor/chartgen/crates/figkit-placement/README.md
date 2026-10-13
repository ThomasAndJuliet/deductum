# Figure placement

`place(&PlacementProblem, &PlacementBudget)` places ordered measured alternatives and returns their IDs, top-left translations, connector geometry and unresolved items. The caller retains text, native blocks, composed groups and styling. Baseline-free block metrics are supported. The input is never mutated.

The raster uses one-point cells. Hard rectangular and segment obstacles prohibit placement and routing; soft obstacles penalize a candidate by `3 × em` and a route by `3 × crossing samples`. Explicit clearance expands rectangles or segment stamps. Target clearance shortens a connector before its target. Empty allowed-region lists use the problem region. Preferred top-left positions join the two-point candidate grid and add distance to the closest preference to candidate scores.

Candidate ranking follows distance to target groups, a minimum-distance penalty, an outside-plot penalty and authored alternative order. The search considers up to 60 spatially distinct candidates or six successful routed candidates. These shortlist bounds define the search domain: `NoFeasibleCandidate` does not assert global geometric infeasibility. Resource exhaustion gives `BudgetExceeded` for the current and remaining items, preserving earlier placements in the owned result. Invalid input and setup allocation exhaustion return typed errors.

`LegacyFamily` preserves ties-away coordinate/head rounding, multiplication-based quadratic sampling, and rejection of all negative occupancy query coordinates. `LegacySpecial` preserves formatting-based ties-even rounding, `powi` sampling, and truncation of negative fractional query coordinates toward zero. Segment interpolation retains each profile's original operation order. Single-target candidates share evolving route occupancy; multiple-target candidates copy it. Family selection uses `min_by`; special selection accepts only strictly better scores. These profiles preserve the independent annotators at chartgen revision `ff0a238`. Chart obstacle exclusions, reference resolution, wrapping, redraw/below fallback and report decisions remain chart-owned.

Every limit is inclusive and checked before its allocation or work. Counts accumulate through the shared `Budget`; cloning a raster retains that budget.

| Limit | Default | Counting unit |
| --- | ---: | --- |
| `max_occupancy_cells` | 16,000,000 | Cumulative allocated raster and summed-area entries, including clones |
| `max_raster_work` | 200,000,000 | Cell writes, segment samples, summed-area cell visits |
| `max_candidates` | 2,000,000 | Grid and preferred-position probes, including occupied probes |
| `max_scoring_work` | 20,000,000 | Target/preference distance evaluations, minimum one per scoring operation |
| `max_routes` | 200,000 | Start/target pairs attempted |
| `max_route_samples` | 20,000,000 | Collision samples; 306 per connector construction; produced connector occupancy points |
| `max_items` | 4,096 | Items in the problem |
| `max_alternatives` | 4,096 | Alternatives per item |
| `max_targets` | 4,096 | Targets across groups per item; starts/targets per direct routing call |
| `max_obstacles` | 100,000 | Combined rectangle and segment obstacles |
| `max_regions` | 4,096 | Allowed regions per item |
| `max_preferences` | 4,096 | Preferred positions per item |

A connector has 241 arc-length samples, 65 head-outline points, 25 shaft occupancy points and five samples per head edge. Raster allocation products use checked arithmetic. Scratch-raster mutation methods may have completed earlier stamps when a later stamp exhausts work; they do not mutate a caller figure. The family chart adapter commits its primitive, height and report changes together after success.

The crate has no chart, host, filesystem or document-engine dependencies. Low-level raster, candidate and routing functions are available for callers retaining their own selection policies.

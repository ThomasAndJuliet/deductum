# Figure protocol

`handle(bytes)` uses default `figkit_model::Limits`; `handle_with_limits(bytes, limits)`
lets a host configure finite budgets. No function performs host I/O. Version 1
retains the historical `figkit` and `render_scene` envelopes. Version 2 uses the
portable model and typed `FigureError` categories. Malformed version 1 requests
retain the historical `protocol` error kind and operation errors retain their
`raise` exception envelope.

Every request has `version` and `operation`. Version 2 operations are:

* `negotiate`: returns supported protocol/scene versions and configured limits.
* `render_scene`: requires `figure` (portable Figure) and `backend` (`svg`, `pgf`,
  `typst`, `browser`). Optional `session` checks all measured text against session
  replies before preparation. SVG/PGF output is source; Typst/browser output is
  an ordered JSON composition plan retaining complete requests, metrics and paint.
* `measure`: requires complete `requests`, optional `replies`, and either a new
  `context` or the previous `session`. Returns `session`, outstanding `requests`,
  and `complete`. Each reply contains its entire `request` and `metrics`.

A session snapshot belongs to the host. It is replay state, not an authenticated
capability. The protocol validates context, request identity and metric envelopes
on every advance. Cache keys cannot make changed typography/content/width/context
reuse old metrics. Resume with the entire request set; missing replies stay
pending. Repeating an awaiting round without replies or with no progress fails.
Replies are admitted transactionally. Registry values never cross the protocol;
hosts retain them under opaque keys until native composition finishes.

Measurement request limits count the union of admitted request IDs; submitted
batch lengths are also bounded. Round limits count batches of new replies. A new
pending batch fails immediately when no round budget remains. All limits are
inclusive. Error envelopes may exceed a deliberately tiny output-byte limit;
successful output is never truncated. `Limits::default()` documents numerical
defaults. The host must treat a changed registry/font environment/preamble as a
new context and session. Python/browser wrappers lock registry insertion when
measurement starts and retain content through composition.

For standalone native process use, `figkit-cli` builds `figkit-dispatch`: a bounded
stdin request produces one JSON response on stdout. Web and Typst bindings expose
the same dispatcher through their own ABIs; neither requires Python or chartgen.

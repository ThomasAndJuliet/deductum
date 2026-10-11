"""Prompts for asking an LLM to fill a gap or name facts, by backend (design §9.4).

Each is a ``str.format`` template. Fields: ``identifier`` (the prover's version), ``logic``
(Isabelle's base logic; empty elsewhere), ``marker``, ``n``, ``hint``, ``header`` (see the
driver's ``header_of``), ``before``, ``lemma`` (with the marker in place of the gap) and
``state`` (the proof state there). The answer is JSON: ``{"proofs": [...], "using": [...]}``,
or ``{"using": [...]}`` for the facts prompt.
"""

from typing import Dict

ISABELLE_FACTS = """You are helping to set up Sledgehammer for an Isabelle/HOL proof. The theory uses
Isabelle {identifier} with base logic {logic}. Below is the start of the theory, the text before the
gap, the lemma with the gap marked {marker}, and the proof state at the gap.

Name up to 8 existing facts (lemmas, definitions such as foo_def, simp and intro rules) that a
proof search would most need to close the goal at {marker}: they go in a "using" line in front of
Sledgehammer to guide it. Only facts that exist in this context.{hint}

Answer at once with only a JSON object and nothing else:
{{"using": ["fact_name", "..."]}}

=== theory start ===
{header}
=== text before the gap ===
{before}
=== lemma ({marker} marks the gap) ===
{lemma}
=== proof state at the gap ===
{state}
"""

ISABELLE_FILL = """You are helping to finish an Isabelle/HOL proof. The theory uses Isabelle {identifier}
with base logic {logic}. Below is the start of the theory, the text before the gap, the lemma
with the gap marked {marker}, and the proof state at the gap.

Propose up to {n} different replacements for {marker}: each must be complete proof text that
closes the goal at that point, for example "by (simp add: foo_def)", "by auto", or an Isar
proof block ("proof ... qed"), indented like the surrounding text. Prefer short, robust proofs
using facts that exist in this context.{hint}

Also name up to 8 existing facts (lemmas, definitions such as foo_def, simp rules) that a proof
search would most need here: they go in a "using" line in front of Sledgehammer to guide it.

Answer with only a JSON object and nothing else:
{{"proofs": ["best proof", "..."], "using": ["fact_name", "..."]}}

=== theory start ===
{header}
=== text before the gap ===
{before}
=== lemma ({marker} marks the gap) ===
{lemma}
=== proof state at the gap ===
{state}
"""

LEAN_FACTS = """You are helping to finish a Lean 4 proof ({identifier}). Below is the start of the file
(imports and opens), the text before the gap, the declaration with the gap marked {marker}, and the
goals at the gap.

Name up to 8 existing declarations (lemmas, theorems, definitions that unfold with `simp`) that
a proof search would most need to close the goal at {marker}: they go in a call such as
`simp [f1, f2]`. Use full names that resolve in this context (for
example `Nat.add_comm`, not a guess at a local name). Only declarations that exist.{hint}

Answer at once with only a JSON object and nothing else:
{{"using": ["Namespace.lemma_name", "..."]}}

=== file start ===
{header}
=== text before the gap ===
{before}
=== declaration ({marker} marks the gap) ===
{lemma}
=== goals at the gap ===
{state}
"""

LEAN_FILL = """You are helping to finish a Lean 4 proof ({identifier}). Below is the start of the file
(imports and opens), the text before the gap, the declaration with the gap marked {marker}, and the
goals at the gap.

Propose up to {n} different replacements for {marker}: each must be complete proof text that
closes the goal at that point, for example "by simp", "by omega", "by exact foo h", or a tactic
block starting with "by" and continuing on indented lines:

    by
      intro x
      simp [foo]

Indent continuation lines to match the surrounding text (the first line goes where the gap is).
When the gap is a tactic in the middle of a tactic block, give tactics without "by". Prefer short,
robust proofs with declarations that exist in this context; do not use `sorry`, and do not add
imports or change the statement.{hint}

Also name up to 8 existing declarations (lemma names usable inside `simp [..]`, such as
`Nat.add_comm`, or definitions) that automation would most need here.

Answer with only a JSON object and nothing else:
{{"proofs": ["best proof", "..."], "using": ["Namespace.lemma_name", "..."]}}

=== file start ===
{header}
=== text before the gap ===
{before}
=== declaration ({marker} marks the gap) ===
{lemma}
=== goals at the gap ===
{state}
"""

FILL: Dict[str, str] = {"isabelle": ISABELLE_FILL, "lean": LEAN_FILL}
FACTS: Dict[str, str] = {"isabelle": ISABELLE_FACTS, "lean": LEAN_FACTS}


def fill_prompt(backend: str) -> str:
    return FILL.get(backend) or FILL["isabelle"]


def facts_prompt(backend: str) -> str:
    return FACTS.get(backend) or FACTS["isabelle"]

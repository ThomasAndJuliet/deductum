// SPDX-License-Identifier: GPL-3.0-only

//! A self-contained Lean document for the profile's tests and the snapshot tests, modelled on
//! DSRVLean (docs, a structure, proofs by induction, bullets, `sorry`, an error).

pub const DEMO: &str = r#"import DSRV.Semantics

/-!
# Timed provenance

Every observation at variable `v` and time `t` is caused by the origin $(v, t)$, and
$$\mathrm{merge}(a, b) = a \cup b$$
holds for the causal component. The *enabling* component is kept apart.
-/

namespace DSRV.Demo

/-- A variable observation with the time at which it occurred.

```lean
def first : Origin := ⟨0, 3⟩
```
-/
structure Origin where
  input : Nat
  time : Nat
deriving Repr, DecidableEq

-- the executable merge keeps both sides
def merge (left right : List Nat) : List Nat := left ++ right

/-- Merging keeps the causes of both sides: $c \in \mathrm{merge}(a, b)$ iff $c \in a \lor c \in b$. -/
@[simp] theorem mem_merge (a b : List Nat) (c : Nat) :
    c ∈ merge a b ↔ c ∈ a ∨ c ∈ b := by
  simp [merge]

theorem append_nil_right (xs : List Nat) : xs ++ [] = xs := by
  induction xs with
  | nil => rfl
  | cons x xs ih =>
    have h : (x :: xs) ++ [] = x :: (xs ++ []) := rfl
    rw [h, ih]

theorem and_swap' (p q : Prop) (hp : p) (hq : q) : q ∧ p := by
  constructor
  · exact hq
  · exact hp

theorem later (n : Nat) (h : 0 < n) : 1 ≤ n := by
  sorry

theorem broken (n : Nat) : n < n := by
  omega

end DSRV.Demo
"#;

/// A command's status in the demo: `later` is left open, `broken` fails
pub fn status(text: &str, start: usize, name: &str) -> &'static str {
    let chars: Vec<char> = text.chars().collect();
    let before: String = chars[..start].iter().collect();
    let in_broken = before.rfind("theorem broken").is_some_and(|b| before[b..].find("\n\n").is_none());
    if in_broken && name == "omega" {
        "failed"
    } else if name == "sorry" {
        "warned"
    } else {
        "finished"
    }
}

/// The goal parts the host sends for a goal: the case tag, the hypotheses, the target
pub fn goal_parts(goals: &[(&str, &[&str], &str)]) -> Vec<serde_json::Value> {
    let mut out = vec![format!("#deductum:count {}", goals.len())];
    for (i, (case, hyps, target)) in goals.iter().enumerate() {
        let n = i + 1;
        if !case.is_empty() {
            out.push(format!("#deductum:case {n}\n{case}"));
        }
        for h in *hyps {
            out.push(format!("#deductum:param {n}\n{h}"));
        }
        out.push(format!("#deductum:concl {n}\n{target}"));
    }
    out.iter().map(|r| serde_json::json!({"kind": "writeln", "raw": r, "text": r, "spans": []})).collect()
}

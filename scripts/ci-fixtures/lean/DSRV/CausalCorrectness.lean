-- Self-contained fixture for the existing host-core integration assertions.
-- It shares no source or credentials with any personal research project.
def evalOne (n : Nat) : Nat := n + 1

def InSupport (n : Nat) : Prop := evalOne n = 0

theorem causalCorrectness (n : Nat) (reported : InSupport n) : False := by
  induction n with
  | zero => simp [evalOne, InSupport] at reported
  | succ n ih => simp [evalOne, InSupport] at reported

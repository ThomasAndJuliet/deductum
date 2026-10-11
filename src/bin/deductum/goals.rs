// SPDX-License-Identifier: GPL-3.0-only

//! The proof state in parts, from the prelude's `deductum_goals` query: the chained facts and,
//! for each subgoal, its fixed variables, assumptions and conclusion. Each part arrives as one
//! message headed `#deductum:KIND N`; the head is cut off here (with the message's colour
//! spans moved to match), so that each part shows as code of its own.

use serde_json::{Value, json};

#[derive(Clone, Debug, Default)]
pub struct Subgoal {
    pub n: usize,
    pub params: Vec<Value>,
    pub prems: Vec<Value>,
    pub concl: Option<Value>,
}

impl Subgoal {
    /// What the subgoal says, to tell the same subgoal in another state
    pub fn key(&self) -> String {
        let text = |m: &Value| m["raw"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        let mut out: Vec<String> = self.params.iter().map(text).collect();
        out.push("|".into());
        out.extend(self.prems.iter().map(text));
        out.push("|".into());
        out.extend(self.concl.iter().map(text));
        out.join(" ")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Goals {
    /// The whole state as Isabelle prints it (with the same settings, such as types)
    pub text: Option<Value>,
    pub facts: Vec<Value>,
    /// The number of subgoals, when there is a goal
    pub count: Option<usize>,
    /// In a proof, but not proving anything (forward mode)
    pub nogoal: bool,
    pub subgoals: Vec<Subgoal>,
    /// An error instead of a state (such as "Not inside a proof")
    pub error: Option<String>,
}

impl Goals {
    pub fn parse(messages: &[Value]) -> Goals {
        let mut g = Goals::default();
        for m in messages {
            let Some((kind, n, part)) = strip(m) else {
                if m["kind"] == "error" {
                    g.error = m["text"].as_str().map(|s| s.trim().to_string());
                }
                continue;
            };
            let sub = |g: &mut Goals| -> usize {
                if let Some(i) = g.subgoals.iter().position(|s| s.n == n) {
                    return i;
                }
                g.subgoals.push(Subgoal { n, ..Default::default() });
                g.subgoals.len() - 1
            };
            match kind.as_str() {
                "text" => g.text = Some(part),
                "fact" => g.facts.push(part),
                "count" => g.count = Some(n),
                "nogoal" => g.nogoal = true,
                "param" => {
                    let i = sub(&mut g);
                    g.subgoals[i].params.push(part);
                }
                "prem" => {
                    let i = sub(&mut g);
                    g.subgoals[i].prems.push(part);
                }
                "concl" => {
                    let i = sub(&mut g);
                    g.subgoals[i].concl = Some(part);
                }
                _ => {}
            }
        }
        g.subgoals.sort_by_key(|s| s.n);
        g
    }

    /// Every part, as messages to show as code
    pub fn parts(&self) -> Vec<&Value> {
        let mut out: Vec<&Value> = self.text.iter().chain(self.facts.iter()).collect();
        for s in &self.subgoals {
            out.extend(s.params.iter());
            out.extend(s.prems.iter());
            out.extend(s.concl.iter());
        }
        out
    }
}

/// A part's kind and subgoal, and the message without its head line
fn strip(m: &Value) -> Option<(String, usize, Value)> {
    let raw = m["raw"].as_str().or_else(|| m["text"].as_str())?;
    let (head, body) = raw.split_once('\n').unwrap_or((raw, ""));
    let head = head.strip_prefix("#deductum:")?;
    let (kind, n) = head.split_once(' ').unwrap_or((head, "0"));
    let cut = head.chars().count() + "#deductum:".len() + 1;
    let text = m["text"].as_str().map(|t| t.split_once('\n').map_or("", |(_, b)| b)).unwrap_or(body);
    let spans: Vec<Value> = m["spans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sp| {
            let (s, e) = (sp[0].as_u64()? as usize, sp[1].as_u64()? as usize);
            (e > cut).then(|| json!([s.saturating_sub(cut), e - cut, sp[2].clone()]))
        })
        .collect();
    let tips: Vec<Value> = m["tips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sp| {
            let (s, e) = (sp[0].as_u64()? as usize, sp[1].as_u64()? as usize);
            (e > cut).then(|| json!([s.saturating_sub(cut), e - cut, sp[2].clone()]))
        })
        .collect();
    let part = json!({"kind": "goal", "raw": body.trim_end(), "text": text.trim_end(), "spans": spans, "tips": tips, "sendbacks": []});
    Some((kind.to_string(), n.trim().parse().unwrap_or(0), part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_become_subgoals() {
        let msgs = vec![
            json!({"kind": "writeln", "raw": "#deductum:count 2\n", "text": "#deductum:count 2\n", "spans": []}),
            json!({"kind": "writeln", "raw": "#deductum:param 1\nxs :: 'a list", "text": "#deductum:param 1\nxs :: 'a list", "spans": [[18, 20, "free"]]}),
            json!({"kind": "writeln", "raw": "#deductum:prem 1\n0 < n", "text": "#deductum:prem 1\n0 < n", "spans": []}),
            json!({"kind": "writeln", "raw": "#deductum:concl 1\nxs \\<noteq> []", "text": "#deductum:concl 1\nxs ≠ []", "spans": []}),
            json!({"kind": "writeln", "raw": "#deductum:concl 2\nP n", "text": "#deductum:concl 2\nP n", "spans": []}),
        ];
        let g = Goals::parse(&msgs);
        assert_eq!(g.count, Some(2));
        assert_eq!(g.subgoals.len(), 2);
        assert_eq!(g.subgoals[0].params[0]["raw"], "xs :: 'a list");
        assert_eq!(g.subgoals[0].params[0]["spans"][0], json!([0, 2, "free"]));
        assert_eq!(g.subgoals[0].concl.as_ref().unwrap()["text"], "xs ≠ []");
        assert_eq!(g.parts().len(), 4);
        assert_ne!(g.subgoals[0].key(), g.subgoals[1].key());
    }
}

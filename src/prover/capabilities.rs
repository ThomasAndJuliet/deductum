// SPDX-License-Identifier: GPL-3.0-only

//! What a session's driver and project can do (design §5), from the session
//! summary's `capabilities`. The UI follows these, not backend names (§5.2):
//! a panel, setting, button or palette entry shows only when they offer it.
//!
//! Every field is optional. A summary without `capabilities` comes from a host
//! older than them, which ran Isabelle as it is now: its profile's defaults
//! stand in. A host that sends them says which tools exist and what they can
//! do; how a tool the profile knows is presented (its label, settings and
//! notes) is filled in from the profile where the host leaves it out.

use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Capabilities {
    /// How checking runs (`continuous`)
    pub checking: String,
    /// What Pause does (`perspective`, `close_documents`, `on_request`)
    pub pause: String,
    /// Whether running proofs can be interrupted
    pub interrupt: bool,
    pub goals: Goals,
    pub hover: bool,
    pub definition: bool,
    /// Where command spans come from (`prover`, `lexical`)
    pub spans: String,
    /// Proof tools, typed in the text or run from a panel
    pub tools: Vec<Tool>,
    /// Counterexample finders offered in their panel, by tool name
    pub counterexamples: Vec<String>,
    /// Searching for theorems, if the backend can
    pub search: Option<Search>,
    /// Commands run at the caret without editing
    pub command: Vec<String>,
    /// How LLM candidates are checked (`query`, `petanque`, `scratch`)
    pub check_candidates: String,
    pub project_actions: Vec<ProjectAction>,
    pub semantic_colours: bool,
    pub documents: Vec<String>,
    pub project_files: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Goals {
    pub structured: bool,
    pub term_goal: bool,
    pub facts: bool,
    pub shelved: bool,
}

/// A proof tool. A typed tool's findings replace the goal while the caret is on it (§9.3)
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tool {
    pub name: String,
    /// What is typed in the text to run it (empty: it is not typed)
    pub typed: String,
    /// `prove` (a proof it finds replaces it), `check`, `counterexample`
    pub kind: String,
    /// Whether it can be stopped on its own, without interrupting other proofs
    pub stop: bool,
    /// Its settings, built into a section of their own
    #[serde(deserialize_with = "lenient")]
    pub settings: Vec<Setting>,
    // How it is presented (from the profile when the host leaves it out)
    pub label: String,
    /// Also offered in the Counterexamples panel, run once at the caret without editing
    pub quick: bool,
    /// The settings passed, in order, when it runs from its panel (none: it does not)
    pub args: Vec<String>,
    /// Its status while it runs from its panel (`Sledgehammering…`)
    pub running: String,
    /// What its panel says about where it runs
    pub panel_note: String,
    /// What its panel says about typing it instead
    pub typed_note: String,
    /// What its settings section says about when they hold
    pub settings_note: String,
    /// What options typed with it look like (the Counterexamples panel's field)
    pub params: String,
}

/// One of a tool's settings
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Setting {
    pub key: String,
    /// `text`, `number` or `bool`
    pub kind: String,
    /// Before its field (none: the field is a line of its own)
    pub label: String,
    /// After its field (`seconds per prover`)
    pub unit: String,
    pub placeholder: String,
    /// Its value until the human sets one (`true` or `false` for a switch)
    pub default: String,
    /// The prover option it sets, so that it holds for the tool typed in the text
    pub option: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Search {
    pub label: String,
    /// The short label of its tab and button, if not the label
    pub tab: String,
    /// The syntax of a query (`isabelle-find`)
    pub syntax: String,
    pub placeholder: String,
    pub note: String,
    /// The query operation, and the arguments before the human's query
    pub operation: String,
    pub args: Vec<String>,
    /// Other ways to search the panel offers as buttons (Lean's `exact?`, `apply?`: typed at the
    /// caret), besides the query field
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectAction {
    pub name: String,
    pub label: String,
}

/// Settings as the design writes them may be an object (a schema); only a list of settings is
/// understood here, anything else is none
fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Setting>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

impl Capabilities {
    /// A session's capabilities from its summary, with `defaults` (its profile's) standing in
    /// for what the host does not send
    pub fn from_summary(summary: Option<&Value>, defaults: Capabilities) -> Capabilities {
        let Some(v) = summary.and_then(|s| s.get("capabilities")).filter(|c| c.is_object()) else { return defaults };
        match serde_json::from_value::<Capabilities>(v.clone()) {
            Ok(mut caps) => {
                caps.fill_from(&defaults);
                caps
            }
            Err(e) => {
                eprintln!("deductum: the session's capabilities do not parse ({e}); the profile's defaults stand in");
                defaults
            }
        }
    }

    /// Presentation the host left out, from the profile's tools of the same names
    fn fill_from(&mut self, defaults: &Capabilities) {
        for tool in &mut self.tools {
            let Some(d) = defaults.tool(&tool.name) else { continue };
            let fill = |s: &mut String, d: &String| {
                if s.is_empty() {
                    *s = d.clone();
                }
            };
            fill(&mut tool.typed, &d.typed);
            fill(&mut tool.kind, &d.kind);
            fill(&mut tool.label, &d.label);
            fill(&mut tool.running, &d.running);
            fill(&mut tool.panel_note, &d.panel_note);
            fill(&mut tool.typed_note, &d.typed_note);
            fill(&mut tool.settings_note, &d.settings_note);
            fill(&mut tool.params, &d.params);
            if tool.settings.is_empty() {
                tool.settings = d.settings.clone();
            }
            if tool.args.is_empty() {
                tool.args = d.args.clone();
            }
            tool.quick |= d.quick;
        }
        if let (Some(s), Some(d)) = (&mut self.search, &defaults.search) {
            for (field, value) in [(&mut s.label, &d.label), (&mut s.tab, &d.tab), (&mut s.placeholder, &d.placeholder), (&mut s.note, &d.note), (&mut s.operation, &d.operation)] {
                if field.is_empty() {
                    *field = value.clone();
                }
            }
            if s.args.is_empty() {
                s.args = d.args.clone();
            }
            if s.sources.is_empty() {
                s.sources = d.sources.clone();
            }
        }
    }

    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// The tool typed as `name` in the text, if it is one
    pub fn typed_tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| !t.typed.is_empty() && t.typed == name)
    }

    /// The names typed tools are typed as
    pub fn typed_tools(&self) -> Vec<String> {
        self.tools.iter().filter(|t| !t.typed.is_empty()).map(|t| t.typed.clone()).collect()
    }

    /// The tool that runs from its own panel (Isabelle's Sledgehammer), if there is one
    pub fn panel_tool(&self) -> Option<&Tool> {
        self.tools.iter().find(|t| !t.args.is_empty())
    }

    /// The Counterexamples panel's tab label: its first finder's
    pub fn counterexamples_label(&self) -> Option<String> {
        let first = self.counterexamples.first()?;
        Some(self.tool(first).map_or_else(|| first.clone(), |t| t.label.clone()))
    }
}

impl Search {
    /// What its tab and button say
    pub fn short(&self) -> &str {
        if self.tab.is_empty() { &self.label } else { &self.tab }
    }
}

impl Tool {
    /// A setting's value: the human's, else its default
    pub fn value<'a>(&'a self, values: &'a std::collections::HashMap<String, String>, key: &str) -> &'a str {
        values
            .get(&Self::setting_key(&self.name, key))
            .map(String::as_str)
            .or_else(|| self.settings.iter().find(|s| s.key == key).map(|s| s.default.as_str()))
            .unwrap_or("")
    }

    /// Where a setting's value is kept in the workspace
    pub fn setting_key(tool: &str, key: &str) -> String {
        format!("{tool}.{key}")
    }

    /// The prover options its settings set (`sledgehammer_timeout`), for the values given
    pub fn options(&self, values: &std::collections::HashMap<String, String>) -> serde_json::Map<String, Value> {
        let mut options = serde_json::Map::new();
        for s in self.settings.iter().filter(|s| !s.option.is_empty()) {
            let v = self.value(values, &s.key).trim();
            match s.kind.as_str() {
                "number" => {
                    if let Ok(n) = v.parse::<u32>() {
                        options.insert(s.option.clone(), Value::String(n.to_string()));
                    }
                }
                _ if !v.is_empty() => {
                    options.insert(s.option.clone(), Value::String(v.to_string()));
                }
                _ => {}
            }
        }
        options
    }

    /// How long a run may take, for its progress: its `timeout` setting, else that setting's
    /// placeholder, else 30 seconds
    pub fn timeout(&self, values: &std::collections::HashMap<String, String>) -> f32 {
        let placeholder = self.settings.iter().find(|s| s.key == "timeout").and_then(|s| s.placeholder.parse::<f32>().ok());
        self.value(values, "timeout").trim().parse::<f32>().ok().or(placeholder).unwrap_or(30.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prover::Profile;
    use serde_json::json;

    #[test]
    fn a_missing_capabilities_object_is_the_profiles_defaults() {
        let defaults = crate::prover::ISABELLE.default_capabilities();
        let caps = Capabilities::from_summary(Some(&json!({"id": "s1"})), defaults.clone());
        assert_eq!(caps, defaults);
    }

    #[test]
    fn what_the_host_leaves_out_comes_from_the_profile() {
        let summary = json!({"capabilities": {
            "interrupt": false,
            "tools": [{"name": "sledgehammer", "typed": "sledgehammer", "kind": "prove", "stop": true, "settings": {"type": "object"}}],
            "search": {"label": "Find theorems", "syntax": "isabelle-find"},
        }});
        let caps = Capabilities::from_summary(Some(&summary), crate::prover::ISABELLE.default_capabilities());
        assert!(!caps.interrupt);
        assert!(caps.counterexamples.is_empty(), "no Counterexamples panel");
        let sh = caps.tool("sledgehammer").unwrap();
        assert_eq!(sh.label, "Sledgehammer");
        assert!(sh.settings.iter().any(|s| s.option == "sledgehammer_timeout"), "settings from the profile");
        assert_eq!(caps.search.as_ref().unwrap().short(), "Find");
        assert_eq!(caps.typed_tools(), vec!["sledgehammer".to_string()]);
    }

    #[test]
    fn settings_become_prover_options() {
        let caps = crate::prover::ISABELLE.default_capabilities();
        let sh = caps.tool("sledgehammer").unwrap();
        let mut values = std::collections::HashMap::new();
        assert!(sh.options(&values).is_empty());
        assert_eq!(sh.value(&values, "try0"), "true");
        values.insert(Tool::setting_key("sledgehammer", "timeout"), "60".into());
        values.insert(Tool::setting_key("sledgehammer", "provers"), " z3 e ".into());
        let options = sh.options(&values);
        assert_eq!(options["sledgehammer_timeout"], "60");
        assert_eq!(options["sledgehammer_provers"], "z3 e");
        assert_eq!(sh.timeout(&values), 60.0);
    }
}

//! What reading a spec reports: issues (refusals, warnings, infos, each with its rule, path and
//! what to supply) and the choices that were defaulted. The policy layer turns these into
//! chartgen findings and decisions; nothing here knows about reports.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Refuses the chart.
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub rule: String,
    pub level: Level,
    pub message: String,
    pub path: String,
    pub supply: Option<String>,
}

/// A choice the generator made for the caller, to be recorded and stated in the chart's note.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub what: String,
    pub value: Value,
    pub why: String,
}

#[derive(Debug, Clone, Default)]
pub struct Notes {
    pub issues: Vec<Issue>,
    pub choices: Vec<Choice>,
}

impl Notes {
    pub fn refuse(&mut self, rule: &str, message: impl Into<String>, path: &str, supply: &str) {
        self.issues.push(Issue {
            rule: rule.into(),
            level: Level::Error,
            message: message.into(),
            path: path.into(),
            supply: Some(supply.into()),
        });
    }
    pub fn warn(&mut self, rule: &str, message: impl Into<String>, path: &str) {
        self.issues.push(Issue {
            rule: rule.into(),
            level: Level::Warn,
            message: message.into(),
            path: path.into(),
            supply: None,
        });
    }
    pub fn info(&mut self, rule: &str, message: impl Into<String>, path: &str) {
        self.issues.push(Issue {
            rule: rule.into(),
            level: Level::Info,
            message: message.into(),
            path: path.into(),
            supply: None,
        });
    }
    pub fn decide(&mut self, what: &str, value: impl Into<Value>, why: &str) {
        self.choices.push(Choice {
            what: what.into(),
            value: value.into(),
            why: why.into(),
        });
    }
    pub fn refused(&self) -> bool {
        self.issues.iter().any(|i| i.level == Level::Error)
    }
    pub fn has(&self, rule: &str) -> bool {
        self.issues.iter().any(|i| i.rule == rule)
    }
}

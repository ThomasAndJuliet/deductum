//! Categorical matrices and exhaustive two-way count tables (XS-MX-*, XS-CT-*).

use crate::notes::Notes;
use crate::read::{labels, series};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct StateMatrix {
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    /// `(id, name)` of each declared state, in legend order.
    pub states: Vec<(String, String)>,
    /// `[row][col]` index into `states`.
    pub cells: Vec<Vec<usize>>,
}

impl StateMatrix {
    pub fn note(&self) -> String {
        "Each cell shows its state by colour and by shape, so the chart reads without colour."
            .into()
    }
    /// Counts of each state per row, in state order.
    pub fn counts(&self) -> Vec<Vec<usize>> {
        self.cells
            .iter()
            .map(|r| {
                let mut c = vec![0; self.states.len()];
                for k in r {
                    c[*k] += 1;
                }
                c
            })
            .collect()
    }
}

pub fn parse_matrix(spec: &Value, n: &mut Notes) -> Option<StateMatrix> {
    let cols = labels(spec, n);
    let raw = series(spec, n);
    let rows: Vec<String> = raw.iter().map(|r| r.name.clone()).collect();
    let states: Vec<(String, String)> = spec
        .pointer("/data/states")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let id = s.get("id")?.as_str()?;
                    Some((
                        id.to_string(),
                        s.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(id)
                            .to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if states.is_empty() {
        n.refuse(
            "XS-MX-01",
            "every cell state is declared with an id and a name",
            "/data/states",
            "data.states: [{id, name}]",
        );
    }
    if states.len() > 6 {
        n.refuse(
            "XS-MX-02",
            format!(
                "{} states cannot be told apart by colour and shape together",
                states.len()
            ),
            "/data/states",
            "at most six states; merge or filter",
        );
    }
    if n.refused() {
        return None;
    }
    let mut grid = vec![];
    for (ri, r) in raw.iter().enumerate() {
        let Some(cells) = r
            .json
            .get("states")
            .and_then(Value::as_array)
            .filter(|c| c.len() == cols.len())
        else {
            n.refuse(
                "R15-2",
                format!(
                    "series '{}' needs one state per column ({})",
                    r.id,
                    cols.len()
                ),
                &format!("/data/series/{ri}/states"),
                "states: one declared state id per x value",
            );
            return None;
        };
        let mut out = vec![];
        for (ci, c) in cells.iter().enumerate() {
            let id = c.as_str().unwrap_or("");
            match states.iter().position(|(s, _)| s == id) {
                Some(k) => out.push(k),
                None => {
                    n.refuse(
                        "XS-MX-01",
                        format!(
                            "cell ({}, {}) has the undeclared state '{id}'; a blank is not a state",
                            rows[ri], cols[ci]
                        ),
                        &format!("/data/series/{ri}/states/{ci}"),
                        "declare the state, or use an explicit 'missing' state",
                    );
                    return None;
                }
            }
        }
        grid.push(out);
    }
    Some(StateMatrix {
        rows,
        cols,
        states,
        cells: grid,
    })
}

#[derive(Debug, Clone)]
pub struct Contingency {
    pub row_name: String,
    pub col_name: String,
    pub rows: Vec<String>,
    pub cols: Vec<String>,
    pub counts: Vec<Vec<u64>>,
    pub row_totals: Vec<u64>,
    pub col_totals: Vec<u64>,
    pub total: u64,
}

impl Contingency {
    pub fn note(&self) -> String {
        format!(
            "Counts of cases; every case falls in exactly one cell. Shading is the cell's share of all {} cases.",
            self.total
        )
    }
}

pub fn parse_contingency(spec: &Value, n: &mut Notes) -> Option<Contingency> {
    let parts = spec.pointer("/data/parts");
    if parts
        .and_then(|p| p.get("all_parts"))
        .and_then(Value::as_bool)
        != Some(true)
        || parts
            .and_then(|p| p.get("overlap"))
            .and_then(Value::as_bool)
            != Some(false)
    {
        n.refuse(
            "XS-CT-01",
            "a table of counts with marginals asserts that every case is in exactly one cell",
            "/data/parts",
            "data.parts: {all_parts: true, overlap: false}, once that is so",
        );
        return None;
    }
    let cols = labels(spec, n);
    let raw = series(spec, n);
    if n.refused() {
        return None;
    }
    let col_name = spec
        .pointer("/data/x/title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let row_name = spec
        .pointer("/data/series_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let rows: Vec<String> = raw.iter().map(|r| r.name.clone()).collect();
    let mut counts: Vec<Vec<u64>> = vec![];
    for (ri, r) in raw.iter().enumerate() {
        let arr = r.json.get("counts").and_then(Value::as_array);
        let row: Vec<u64> = arr
            .map(|a| a.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        if row.len() != cols.len() || arr.is_none_or(|a| a.len() != cols.len()) {
            n.refuse(
                "R15-3",
                format!(
                    "series '{}' must hold {} non-negative integer counts",
                    r.id,
                    cols.len()
                ),
                &format!("/data/series/{ri}/counts"),
                "integer counts, one per x value",
            );
            return None;
        }
        counts.push(row);
    }
    let row_totals: Vec<u64> = counts.iter().map(|r| r.iter().sum()).collect();
    let col_totals: Vec<u64> = (0..cols.len())
        .map(|j| counts.iter().map(|r| r[j]).sum())
        .collect();
    let total: u64 = row_totals.iter().sum();
    if total == 0 {
        n.refuse(
            "XS-CT-02",
            "a table of no cases shows nothing",
            "/data/series",
            "at least one case",
        );
        return None;
    }
    Some(Contingency {
        row_name,
        col_name,
        rows,
        cols,
        counts,
        row_totals,
        col_totals,
        total,
    })
}

// SPDX-License-Identifier: GPL-3.0-only

//! Text changes, as the session host has them (`host/deductum_ot.py`):
//! replacements `(start, end, text)` against one revision, sorted and not
//! overlapping, with offsets in code points. The app needs the host's `merge`
//! rebase to keep the human's typing over edits that arrive meanwhile (design
//! C11), and position mapping to keep the caret where it was.

pub type Replacement = (usize, usize, String);
pub type Change = Vec<Replacement>;

/// Byte offset of code point `cp` in `text`
pub fn byte_of(text: &str, cp: usize) -> usize {
    text.char_indices().nth(cp).map_or(text.len(), |(i, _)| i)
}

/// Code point offset of byte `byte` in `text`
pub fn cp_of(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

pub fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Byte offsets of every code point, and of the end
pub fn char_starts(text: &str) -> Vec<usize> {
    text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect()
}

pub fn apply(text: &str, change: &Change) -> String {
    let idx = char_starts(text);
    let byte = |cp: usize| idx[cp.min(idx.len() - 1)];
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (s, e, t) in change {
        out.push_str(&text[last..byte(*s).max(last)]);
        out.push_str(t);
        last = byte(*e).max(last);
    }
    out.push_str(&text[last..]);
    out
}

/// One replacement turning `old` into `new`: the span between their common affixes
pub fn diff(old: &str, new: &str) -> Change {
    if old == new {
        return Vec::new();
    }
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new.chars().collect();
    let n = a.len().min(b.len());
    let mut p = 0;
    while p < n && a[p] == b[p] {
        p += 1;
    }
    let mut q = 0;
    while q < n - p && a[a.len() - 1 - q] == b[b.len() - 1 - q] {
        q += 1;
    }
    vec![(p, a.len() - q, b[p..b.len() - q].iter().collect())]
}

/// Where `pos` lies after `change`; see the host's `map_pos`
pub fn map_pos(pos: usize, change: &Change, right: bool) -> usize {
    let mut delta: isize = 0;
    for (s, e, t) in change {
        let (s, e, len) = (*s, *e, t.chars().count());
        if s == e {
            if pos > s || (pos == s && right) {
                delta += len as isize;
                continue;
            }
            break;
        }
        if pos >= e {
            delta += len as isize - (e - s) as isize;
            continue;
        }
        if pos < s || (pos == s && !right) {
            break;
        }
        return (s as isize + delta) as usize + if right { len } else { 0 };
    }
    (pos as isize + delta).max(0) as usize
}

fn overlap(a: &Replacement, b: &Replacement) -> bool {
    let (s, e) = (a.0, a.1);
    let (bs, be) = (b.0, b.1);
    s.max(bs) < e.min(be) || (s == e && bs < s && s < be) || (bs == be && s < bs && bs < e)
}

fn insertion_point(pos: usize, change: &Change) -> usize {
    let mut delta: isize = 0;
    for (s, e, t) in change {
        let (s, e, len) = (*s, *e, t.chars().count());
        if s == e {
            if pos >= s {
                delta += len as isize;
                continue;
            }
            break;
        }
        if pos >= e {
            delta += len as isize - (e - s) as isize;
            continue;
        }
        if pos <= s {
            break;
        }
        return (s as isize + delta) as usize;
    }
    (pos as isize + delta).max(0) as usize
}

fn moved(a: &Replacement, later: &Change) -> Replacement {
    let (s, e, t) = a;
    if s == e {
        let ns = insertion_point(*s, later);
        return (ns, ns, t.clone());
    }
    let ns = map_pos(*s, later, true);
    let ne = map_pos(*e, later, false).max(ns);
    (ns, ne, t.clone())
}

fn merge_one(a: &Replacement, hits: &[&Replacement], later: &Change) -> Change {
    let (s, e, t) = a;
    let mut sorted: Vec<&&Replacement> = hits.iter().collect();
    sorted.sort_by_key(|r| (r.0, r.1));
    let mut segments = Vec::new();
    let mut cur = *s;
    for b in sorted {
        if b.0 > cur {
            segments.push((cur, b.0.min(*e)));
        }
        cur = cur.max(b.1);
    }
    if cur < *e {
        segments.push((cur, *e));
    }
    let inside = hits.iter().any(|b| b.0 < *s && *s < b.1);
    let ins = map_pos(*s, later, inside);
    let mut out = Vec::new();
    if !t.is_empty() {
        out.push((ins, ins, t.clone()));
    }
    for (ss, ee) in segments {
        if ee > ss {
            let ns = map_pos(ss, later, true);
            out.push((ns, map_pos(ee, later, false).max(ns), String::new()));
        }
    }
    out
}

fn merge(mut reps: Change) -> Change {
    reps.sort_by_key(|r| (r.0, r.1));
    let mut out: Change = Vec::new();
    for (s, e, t) in reps {
        if let Some(last) = out.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
            last.2.push_str(&t);
            continue;
        }
        out.push((s, e, t));
    }
    out.retain(|r| r.0 != r.1 || !r.2.is_empty());
    out
}

/// Rebase `change` past `later` (both against the same revision) without
/// dropping anyone's text: the host's `merge` policy
pub fn rebase_merge(change: &Change, later: &Change) -> Change {
    let mut out = Vec::new();
    for a in change {
        let hits: Vec<&Replacement> = later.iter().filter(|b| overlap(a, b)).collect();
        if hits.is_empty() {
            out.push(moved(a, later));
        } else {
            out.extend(merge_one(a, &hits, later));
        }
    }
    merge(out)
}

/// 1-based line and 0-based column (code points) of `offset`
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 0;
    for (i, c) in text.chars().enumerate() {
        if i == offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_and_diffs() {
        let change = vec![(0, 5, "goodbye".to_string()), (6, 11, "moon".to_string())];
        assert_eq!(apply("hello world", &change), "goodbye moon");
        assert_eq!(diff("abcdef", "abXdef"), vec![(2, 3, "X".to_string())]);
        assert_eq!(apply("∀x. P", &vec![(0, 1, "∃".to_string())]), "∃x. P");
    }

    #[test]
    fn merge_keeps_other_text() {
        let text = "0123456789";
        let agent = vec![(4, 6, "AGENT".to_string())];
        let human = vec![(2, 8, "h".to_string())];
        let merged = rebase_merge(&human, &agent);
        assert_eq!(apply(&apply(text, &agent), &merged), "01hAGENT89");
    }

    #[test]
    fn maps_positions() {
        let change = vec![(2, 4, "XYZ".to_string())];
        assert_eq!(map_pos(1, &change, false), 1);
        assert_eq!(map_pos(3, &change, true), 5);
        assert_eq!(map_pos(6, &change, false), 7);
    }

    /// The same cases as the host's tests produce the same results
    #[test]
    fn agrees_with_the_host() {
        let cases: &[(&str, Change, Change, &str)] = &[
            ("aa", vec![(0, 0, "z".into())], vec![(0, 1, "x".into())], "zxa"),
            ("0123456789", vec![(7, 8, "A".into())], vec![(1, 3, "BBBB".into())], "0BBBB3456A89"),
        ];
        for (text, a, b, want) in cases {
            let a2 = rebase_merge(a, b);
            assert_eq!(apply(&apply(text, b), &a2), *want);
        }
    }
}

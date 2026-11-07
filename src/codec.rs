// SPDX-License-Identifier: GPL-3.0-only

//! Source and display text (design C14, provers §7.1). A backend may write
//! characters in its source differently from how both views show them:
//! Isabelle writes `\<forall>` and shows `∀`. Its codec decodes the source for
//! display and keeps a map between the two texts' offsets, so an edit made in
//! the decoded text lands on the right source characters and leaves every
//! other escape as it was. A backend whose source is already what it shows
//! (Lean, Rocq) uses [`Plain`].

/// How a decoded code point is set, beyond its character
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Bold,
    /// A glyph of the scripts font, which must be drawn in it
    Script,
    /// A control symbol shown as a marker (⇩, ⇧, ❙ …) because the next character has no
    /// subscript or superscript form
    Control,
}

/// Decoding a document's source for display, and encoding what is typed back into source
pub trait Codec: Send + Sync {
    /// `source` as shown, and the map between offsets
    fn decode(&self, source: &str) -> Decoded;
    /// The source for text typed or pasted into a decoded view
    fn encode(&self, text: &str) -> String;
}

/// A codec and an input table together: what the editors need of a backend's characters
pub trait Symbols: Codec + super::input::Input {}

impl<T: Codec + super::input::Input> Symbols for T {}

/// Source shown as it is written, one character for one
#[allow(dead_code)] // (the Lean profile's)
pub struct Plain;

impl Codec for Plain {
    fn decode(&self, source: &str) -> Decoded {
        let n = source.chars().count();
        Decoded { text: source.to_string(), src_of: (0..=n).collect(), styles: Vec::new() }
    }

    fn encode(&self, text: &str) -> String {
        text.to_string()
    }
}

/// A decoded text and, for each of its code points (and its end), where it
/// starts in the source
#[derive(Clone, Debug, Default)]
pub struct Decoded {
    pub text: String,
    pub src_of: Vec<usize>,
    /// Decoded offsets set bold, or shown as control markers
    pub styles: Vec<(usize, Style)>,
}

impl Decoded {
    pub fn to_source(&self, decoded: usize) -> usize {
        self.src_of[decoded.min(self.src_of.len() - 1)]
    }

    /// The decoded offset at or before source offset `source`
    pub fn to_decoded(&self, source: usize) -> usize {
        match self.src_of.binary_search(&source) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// The source change for a change of the decoded text from `old` to the
    /// edited text `new`: the source between the mapped ends of the edited
    /// span is replaced by the encoded new text, everything else untouched
    pub fn source_change(&self, codec: &dyn Codec, new: &str) -> super::ot::Change {
        let change = super::ot::diff(&self.text, new);
        change
            .into_iter()
            .map(|(s, e, t)| (self.to_source(s), self.to_source(e), codec.encode(&t)))
            .collect()
    }
}

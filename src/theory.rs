// SPDX-License-Identifier: GPL-3.0-only

//! A document as the literate view lays it out: blocks of source, each a run of
//! commands (design C4, P1). Prose becomes rendered text; a theorem and its
//! proof, or a definition, one formal block. Every block keeps its exact source
//! span, so edits go back to the document's buffer. How a document splits into
//! blocks is the profile's (`prover::Profile::blocks`); these are the pieces
//! every backend's split is made of.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Header,
    Heading(u8),
    Text,
    Formal,
    Comment,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Finished,
    Warned,
    Unprocessed,
    Running,
    Failed,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "finished" => Status::Finished,
            "warned" => Status::Warned,
            "running" => Status::Running,
            "failed" | "canceled" => Status::Failed,
            _ => Status::Unprocessed,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub status: Status,
}

impl Block {
    /// Whether the block's source starts with `prefix` (in `text`, the document)
    pub fn source_start_is(&self, text: &str, prefix: &str) -> bool {
        text.chars().skip(self.start).take(prefix.chars().count()).collect::<String>() == prefix
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: Kind,
    /// Source span in code points, without surrounding blank space
    pub start: usize,
    pub end: usize,
    /// The span of a prose block's text, inside its delimiters
    pub body: Option<(usize, usize)>,
    pub name: String,
    pub status: Status,
    pub commands: Vec<Command>,
}

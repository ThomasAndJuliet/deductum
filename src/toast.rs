// SPDX-License-Identifier: GPL-3.0-only

//! Toasts: what passed ("Saved", "Copied", "Block deleted", "Reverted the edit") said at the
//! bottom centre of the window, going by themselves after a few seconds, with Undo where there is
//! an undo (household rules 18 and 19; Tom, 8 Nov, t28). libcosmic's toast widget draws them; this
//! keeps a list of what is showing beside its own, which the widget does not let anyone read.

use std::time::Duration;

use cosmic::app::Task;
use cosmic::widget::{self, Toast, ToastId, Toasts};

use super::shell::Msg;
use super::Message;

/// How long a toast stays
pub const STAY: Duration = Duration::from_secs(5);

/// One toast that is showing
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub id: ToastId,
    pub text: String,
    /// It has an Undo
    pub undo: bool,
}

pub struct Toaster {
    toasts: Toasts<Message>,
    pub live: Vec<Live>,
}

impl Default for Toaster {
    fn default() -> Self {
        Self { toasts: Toasts::new(|id| Message::Shell(Msg::ToastGone(id))), live: Vec::new() }
    }
}

impl Toaster {
    /// Show `text`, with an Undo if `undo`; the task takes it away again after `stay`
    pub fn push(&mut self, text: impl Into<String>, undo: bool, stay: Duration) -> Task<Message> {
        let text = text.into();
        let mut toast = Toast::new(text.clone()).duration(stay);
        if undo {
            toast = toast.action("Undo".to_string(), |id| Message::Shell(Msg::ToastUndo(id)));
        }
        // (the widget keeps five: the oldest goes first, and the ledger follows)
        let id = self.toasts.push_persistent(toast);
        self.live.push(Live { id, text, undo });
        while self.live.len() > 5 {
            self.live.remove(0);
        }
        Task::perform(
            async move {
                tokio::time::sleep(stay).await;
                id
            },
            |id| cosmic::Action::App(Message::Shell(Msg::ToastGone(id))),
        )
    }

    /// A toast is gone (its time ran out, or it was closed)
    pub fn gone(&mut self, id: ToastId) {
        self.toasts.remove(id);
        self.live.retain(|l| l.id != id);
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// `content` with the toasts over it, at the bottom centre
    pub fn over<'a>(&'a self, content: impl Into<cosmic::Element<'a, Message>>) -> cosmic::Element<'a, Message> {
        widget::toaster(&self.toasts, content)
    }
}

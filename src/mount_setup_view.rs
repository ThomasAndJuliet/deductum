// SPDX-License-Identifier: GPL-3.0-only

//! Persistent, per-computer help for opening remote files.

use crate::{Message, fl};
use cosmic::{Element, widget};
use cosmic_term::mount_setup::Status;
use std::collections::BTreeMap;

pub struct Check {
    pub request_id: String,
    pub result: Option<Result<Status, String>>,
}

pub fn pane<'a>(
    machines: Vec<(String, String)>,
    checks: &'a BTreeMap<String, Check>,
    errors: &'a BTreeMap<String, String>,
) -> Element<'a, Message> {
    let mut column = widget::column::with_capacity(machines.len() * 2 + 2)
        .spacing(16)
        .push(widget::text(fl!("remote-files-intro")))
        .push(widget::text::caption(fl!("remote-files-ssh")));
    for (host, name) in machines {
        let check = checks.get(&host);
        let checking = check.is_some_and(|check| check.result.is_none());
        let mut detail = widget::column::with_capacity(10).spacing(8);
        match check.and_then(|check| check.result.as_ref()) {
            None if checking => {
                detail = detail.push(widget::text::caption(fl!("remote-files-checking")))
            }
            None => detail = detail.push(widget::text(fl!("remote-files-not-checked"))),
            Some(Err(error)) => {
                detail = detail
                    .push(widget::text(fl!("remote-files-check-failed")))
                    .push(widget::text(error.clone()));
            }
            Some(Ok(status)) => {
                detail = detail.push(widget::text::caption(status.system.clone()));
                if status.ready() {
                    detail = detail.push(widget::text(fl!("remote-files-ready")));
                } else if status.reboot_pending {
                    detail =
                        detail
                            .push(widget::text(fl!("remote-files-pending")))
                            .push(widget::text(fl!(
                                "remote-files-reboot",
                                machine = name.as_str()
                            )));
                } else {
                    if !status.missing.is_empty() {
                        detail = detail.push(widget::text(fl!(
                            "remote-files-missing",
                            tools = status.missing.join(", ")
                        )));
                    }
                    if let Some(command) = status.install_command() {
                        detail = detail
                            .push(widget::text(fl!(
                                "remote-files-install-on",
                                machine = name.as_str()
                            )))
                            .push(widget::text(command.clone()))
                            .push(
                                widget::row::with_children(vec![
                                    widget::button::suggested(fl!("remote-files-install"))
                                        .on_press(Message::RemoteFilesInstall(host.clone()))
                                        .into(),
                                    widget::button::standard(fl!("remote-files-copy"))
                                        .on_press(Message::RemoteFilesCopy(host.clone()))
                                        .into(),
                                ])
                                .spacing(8),
                            );
                    } else if !status.missing.is_empty() {
                        detail = detail.push(widget::text(fl!("remote-files-manual")));
                    }
                    if status.atomic && !status.missing.is_empty() {
                        detail = detail.push(widget::text(fl!(
                            "remote-files-reboot",
                            machine = name.as_str()
                        )));
                    } else if !status.missing.is_empty() {
                        detail = detail.push(widget::text(fl!("remote-files-after-install")));
                    }
                    if !status.fuse_access && status.missing.is_empty() {
                        detail = detail.push(widget::text(fl!("remote-files-fuse")));
                    }
                }
            }
        }
        if let Some(error) = errors.get(&host) {
            detail = detail
                .push(widget::text(fl!("remote-files-connection")))
                .push(widget::text(error.clone()));
        }
        detail = detail.push(
            widget::button::standard(fl!("remote-files-check"))
                .on_press_maybe((!checking).then_some(Message::RemoteFilesCheck(host))),
        );
        column = column.push(widget::settings::section().title(name).add(detail));
    }
    widget::scrollable(column).into()
}

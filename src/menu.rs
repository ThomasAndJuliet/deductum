// SPDX-License-Identifier: GPL-3.0-only

use cosmic::widget::menu::action::MenuAction;
use cosmic::widget::menu::key_bind::KeyBind;
use cosmic::widget::menu::{Item as MenuItem, menu_button};
use cosmic::{
    Element,
    app::Core,
    iced::core::Border,
    iced::{Background, Length},
    widget::{
        self,
        menu::{ItemHeight, ItemWidth},
        responsive_menu_bar, segmented_button,
    },
};
use std::{collections::HashMap, sync::LazyLock};

use crate::{Action, ColorSchemeId, ColorSchemeKind, Config, Message, fl};

static MENU_ID: LazyLock<cosmic::widget::Id> =
    LazyLock::new(|| cosmic::widget::Id::new("responsive-menu"));

/// What the terminal found under the right-click
#[derive(Debug, Clone)]
pub struct MenuState {
    pub link: Option<String>,
    /// The word under the pointer, when there is no link
    pub word: Option<String>,
}

impl MenuState {
    /// A path named by the link or word, for opening a remote file locally
    pub fn path(&self) -> Option<String> {
        self.link
            .as_deref()
            .or(self.word.as_deref())
            .and_then(crate::agent::link_path)
    }
}

/// A context menu action dispatched to the tab the menu was opened on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TabAction(pub segmented_button::Entity, pub Action);

impl MenuAction for TabAction {
    type Message = Message;

    fn message(&self) -> Message {
        Message::TabContextAction(self.0, self.1)
    }
}

/// A thread's right-click menu
pub fn thread_context_menu(has_selection: bool, paper: bool) -> Vec<widget::menu::Tree<Message>> {
    let copy = if has_selection {
        MenuItem::Button(fl!("copy"), None, Action::ThreadCopy)
    } else {
        MenuItem::ButtonDisabled(fl!("copy"), None, Action::ThreadCopy)
    };
    let items = vec![
        copy,
        MenuItem::Button(fl!("select-all"), None, Action::ThreadSelectAll),
        MenuItem::Divider,
        MenuItem::CheckBox(fl!("thread-paper"), None, paper, Action::ThreadPaper),
        MenuItem::CheckBox(fl!("dark-mode"), None, cosmic::theme::is_dark(), Action::DarkModeToggle),
    ];
    widget::menu::items(&HashMap::<KeyBind, Action>::new(), items)
}

pub fn context_menu(
    config: &Config,
    key_binds: &HashMap<KeyBind, Action>,
    entity: segmented_button::Entity,
    link: Option<String>,
    has_session: bool,
    open_locally: bool,
    passwords: Option<Vec<(u64, String)>>,
) -> Vec<widget::menu::Tree<Message>> {
    let item =
        |label: String, action: Action| MenuItem::Button(label, None, TabAction(entity, action));

    let mut items = vec![
        item(fl!("new-tab"), Action::TabNew),
        MenuItem::Divider,
        item(fl!("copy"), Action::Copy),
        item(fl!("paste"), Action::Paste),
    ];
    // The secrets store's saved passwords, to paste; or, while it is locked,
    // the way to unlock it
    match passwords {
        None => items.push(item(fl!("paste-password-unlock"), Action::SecretsToggle)),
        Some(passwords) if !passwords.is_empty() => items.push(MenuItem::Folder(
            fl!("paste-password"),
            passwords
                .into_iter()
                .map(|(id, label)| item(label, Action::PastePassword(id)))
                .collect(),
        )),
        Some(_) => {}
    }
    items.extend([
        item(fl!("select-all"), Action::SelectAll),
        MenuItem::Divider,
        item(fl!("clear-scrollback"), Action::ClearScrollback),
        MenuItem::Divider,
        item(fl!("split-horizontal"), Action::PaneSplitHorizontal),
        item(fl!("split-vertical"), Action::PaneSplitVertical),
        item(fl!("pane-toggle-maximize"), Action::PaneToggleMaximized),
        MenuItem::Divider,
        MenuItem::CheckBox(
            fl!("window-header"),
            None,
            config.show_headerbar,
            TabAction(entity, Action::ShowHeaderBar(!config.show_headerbar)),
        ),
        MenuItem::Divider,
    ]);
    #[cfg(feature = "password_manager")]
    items.push(item(fl!("menu-password-manager"), Action::PasswordManager));
    items.push(item(fl!("menu-settings"), Action::Settings));
    if has_session {
        items.push(MenuItem::Divider);
        // Closing a tab only detaches it; say so, next to the action that
        // does end the session
        items.push(item(fl!("detach-session"), Action::TabClose));
        items.push(item(fl!("move-to-new-window"), Action::TabPopOut));
        items.push(item(fl!("end-remote-session"), Action::RemoteEnd));
    }
    // If we have a link, prepend the link items
    if link.is_some() {
        items.insert(0, item(fl!("open-link"), Action::LaunchUrlByMenu));
        items.insert(1, item(fl!("copy-link"), Action::CopyUrlByMenu));
        items.insert(2, MenuItem::Divider);
    }
    if open_locally {
        items.insert(0, item(fl!("open-locally"), Action::OpenLocallyByMenu));
        if link.is_none() {
            items.insert(1, MenuItem::Divider);
        }
    }

    let key_binds: HashMap<KeyBind, TabAction> = key_binds
        .iter()
        .map(|(key_bind, action)| (key_bind.clone(), TabAction(entity, *action)))
        .collect();
    widget::menu::items(&key_binds, items)
}

/// The menus of the tabs in a tab bar, one per tab in order
pub fn tab_menu(
    tab_model: &segmented_button::Model<segmented_button::SingleSelect>,
    destinations: &HashMap<segmented_button::Entity, Vec<(String, crate::config::ProfileId)>>,
) -> Vec<widget::menu::Tree<Message>> {
    let menus = tab_model
        .iter()
        .map(|entity| {
            let item = |label: String, action: Action| {
                MenuItem::Button(label, None, TabAction(entity, action))
            };
            let mut items = vec![item(fl!("new-tab"), Action::TabNew), MenuItem::Divider];
            if tab_model.data::<crate::agent::Session>(entity).is_some() {
                // Closing only detaches; say so, beside the action that
                // ends the session
                items.push(item(fl!("detach-session"), Action::TabClose));
                items.push(item(fl!("move-to-new-window"), Action::TabPopOut));
                items.push(item(fl!("end-remote-session"), Action::RemoteEnd));
                // An agent's conversation, and its worktree, to another computer
                let targets = destinations.get(&entity).map(Vec::as_slice).unwrap_or_default();
                if !targets.is_empty() {
                    items.push(MenuItem::Divider);
                    for (name, profile_id) in targets {
                        items.push(item(fl!("move-to", profile = name.as_str()), Action::TabMoveTo(*profile_id)));
                    }
                    for (name, profile_id) in targets {
                        items.push(item(fl!("fork-to", profile = name.as_str()), Action::TabForkTo(*profile_id)));
                    }
                }
            } else if tab_model.data::<crate::thread::tab::ThreadTab>(entity).is_some() {
                // A thread: renamed, moved or forked as a session is
                items.push(item(fl!("close-tab"), Action::TabClose));
                items.push(item(fl!("move-to-new-window"), Action::TabPopOut));
                items.push(item(fl!("end-remote-session"), Action::RemoteEnd));
                items.push(item(fl!("rename-menu"), Action::SessionRename));
                let targets = destinations.get(&entity).map(Vec::as_slice).unwrap_or_default();
                if !targets.is_empty() {
                    items.push(MenuItem::Divider);
                    for (name, profile_id) in targets {
                        items.push(item(fl!("move-to", profile = name.as_str()), Action::TabMoveTo(*profile_id)));
                    }
                    for (name, profile_id) in targets {
                        items.push(item(fl!("fork-to", profile = name.as_str()), Action::TabForkTo(*profile_id)));
                    }
                }
            } else {
                items.push(item(fl!("close-tab"), Action::TabClose));
            }
            items
        })
        .collect();
    // Shortcuts would match only one tab's actions, so none are shown
    widget::menu::nav_context(&HashMap::<KeyBind, TabAction>::new(), menus)
}

pub fn color_scheme_menu<'a>(
    kind: ColorSchemeKind,
    id_opt: Option<ColorSchemeId>,
    name: &str,
) -> Element<'a, Message> {
    let menu_item =
        |label, message| menu_button(vec![widget::text(label).into()]).on_press(message);

    let mut column =
        widget::menu::menu_column::MenuColumn::with_capacity(if id_opt.is_some() { 3 } else { 1 });
    if let Some(id) = id_opt {
        column = column.push(menu_item(
            fl!("rename"),
            Message::ColorSchemeRename(kind, id, name.to_string()),
        ));
    }
    column = column.push(menu_item(
        fl!("export"),
        Message::ColorSchemeExport(kind, id_opt),
    ));
    if let Some(id) = id_opt {
        column = column.push(menu_item(
            fl!("delete"),
            Message::ColorSchemeDelete(kind, id),
        ));
    }

    widget::container(column)
        .padding(1)
        //TODO: move style to libcosmic
        .style(|theme| {
            let cosmic = theme.cosmic();
            let component = &cosmic.background(theme.transparent).component;
            widget::container::Style {
                icon_color: Some(component.on.into()),
                text_color: Some(component.on.into()),
                background: Some(Background::Color(component.base.into())),
                border: Border {
                    radius: cosmic.radius_s().map(|x| x + 1.0).into(),
                    width: 1.0,
                    color: component.divider.into(),
                },
                ..Default::default()
            }
        })
        .width(Length::Fixed(120.0))
        .into()
}

/// What the Session menu offers for the tab in front
pub struct SessionMenu {
    pub session: bool,
    /// An agent's session, whose conversation can be renamed and moved
    pub agent: bool,
    /// The agent profiles on other computers it can move or fork to
    pub destinations: Vec<(String, crate::config::ProfileId)>,
    /// The tab in front is a thread, which has no terminal to clear
    pub thread: bool,
}

pub fn menu_bar<'a>(
    core: &Core,
    config: &Config,
    key_binds: &HashMap<KeyBind, Action>,
    session: &SessionMenu,
) -> Element<'a, Message> {
    let mut profile_items = Vec::with_capacity(config.profiles.len());
    for (name, id) in config.profile_names() {
        profile_items.push(MenuItem::Button(name, None, Action::ProfileOpen(id)));
    }

    let color_scheme_kind = config.color_scheme_kind(core.system_theme());

    // Items that act on the tab in front, available only when it suits
    let when = |enabled: bool, label: String, action: Action| {
        if enabled {
            MenuItem::Button(label, None, action)
        } else {
            MenuItem::ButtonDisabled(label, None, action)
        }
    };
    let mut session_items = vec![
        when(session.agent, fl!("rename-menu"), Action::SessionRename),
        MenuItem::Divider,
        when(session.session, fl!("detach-session"), Action::TabClose),
        when(session.session || session.thread, fl!("move-to-new-window"), Action::TabPopOut),
    ];
    if session.agent && !session.destinations.is_empty() {
        session_items.push(MenuItem::Folder(
            fl!("move-to-menu"),
            session
                .destinations
                .iter()
                .map(|(name, id)| MenuItem::Button(name.clone(), None, Action::TabMoveTo(*id)))
                .collect(),
        ));
        session_items.push(MenuItem::Folder(
            fl!("fork-to-menu"),
            session
                .destinations
                .iter()
                .map(|(name, id)| MenuItem::Button(name.clone(), None, Action::TabForkTo(*id)))
                .collect(),
        ));
    }
    session_items.extend([
        when(session.session || session.thread, fl!("end-remote-session"), Action::RemoteEnd),
        MenuItem::Divider,
        MenuItem::Button(fl!("open-in-zed"), None, Action::OpenInZed),
        MenuItem::Button(fl!("open-in-files"), None, Action::OpenInFiles),
    ]);

    responsive_menu_bar()
        .item_height(ItemHeight::Dynamic(40))
        .item_width(ItemWidth::Uniform(320))
        .spacing(4.0)
        .into_element(
            core,
            key_binds,
            MENU_ID.clone(),
            Message::Surface,
            vec![
                (
                    fl!("file"),
                    vec![
                        MenuItem::Button(fl!("new-tab"), None, Action::TabNew),
                        MenuItem::Button(fl!("new-window"), None, Action::WindowNew),
                        MenuItem::Divider,
                        MenuItem::Folder(fl!("profile"), profile_items),
                        MenuItem::Button(fl!("menu-profiles"), None, Action::Profiles),
                        MenuItem::Button(fl!("menu-secrets"), None, Action::SecretsToggle),
                        MenuItem::Button(fl!("remote-files-title"), None, Action::RemoteFiles),
                        MenuItem::Button(fl!("menu-settings"), None, Action::Settings),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("close-tab"), None, Action::TabClose),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("quit"), None, Action::WindowClose),
                    ],
                ),
                (
                    fl!("edit"),
                    vec![
                        MenuItem::Button(fl!("copy"), None, Action::Copy),
                        MenuItem::Button(fl!("paste"), None, Action::Paste),
                        MenuItem::Button(fl!("select-all"), None, Action::SelectAll),
                        MenuItem::Divider,
                        when(!session.thread, fl!("clear-scrollback"), Action::ClearScrollback),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("find"), None, Action::Find),
                    ],
                ),
                (
                    fl!("view"),
                    vec![
                        MenuItem::Button(fl!("menu-sidebar"), None, Action::SidebarToggle),
                        MenuItem::Divider,
                        MenuItem::CheckBox(fl!("thread-paper"), None, config.thread_paper, Action::ThreadPaper),
                        MenuItem::CheckBox(fl!("dark-mode"), None, cosmic::theme::is_dark(), Action::DarkModeToggle),
                        MenuItem::CheckBox(fl!("thread-match-colours"), None, config.thread_match_colours, Action::ThreadMatchColours),
                        MenuItem::CheckBox(fl!("thread-show-thinking"), None, config.thread_show_thinking, Action::ThreadShowThinking),
                        MenuItem::CheckBox(fl!("thread-show-tools"), None, config.thread_show_tools, Action::ThreadShowTools),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("zoom-in"), None, Action::ZoomIn),
                        MenuItem::Button(fl!("zoom-reset"), None, Action::ZoomReset),
                        MenuItem::Button(fl!("zoom-out"), None, Action::ZoomOut),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("next-tab"), None, Action::TabNext),
                        MenuItem::Button(fl!("previous-tab"), None, Action::TabPrev),
                        MenuItem::Divider,
                        MenuItem::Button(
                            fl!("split-horizontal"),
                            None,
                            Action::PaneSplitHorizontal,
                        ),
                        MenuItem::Button(fl!("split-vertical"), None, Action::PaneSplitVertical),
                        MenuItem::Button(
                            fl!("pane-toggle-maximize"),
                            None,
                            Action::PaneToggleMaximized,
                        ),
                        MenuItem::Divider,
                        MenuItem::Button(
                            fl!("menu-color-schemes"),
                            None,
                            Action::ColorSchemes(color_scheme_kind),
                        ),
                    ],
                ),
                (fl!("session-menu"), session_items),
                (
                    fl!("help-menu"),
                    vec![
                        MenuItem::Button(
                            fl!("menu-keyboard-shortcuts"),
                            None,
                            Action::KeyboardShortcuts,
                        ),
                        MenuItem::Divider,
                        MenuItem::Button(fl!("menu-about"), None, Action::About),
                    ],
                ),
            ],
        )
}

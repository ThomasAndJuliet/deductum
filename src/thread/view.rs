// SPDX-License-Identifier: GPL-3.0-only

//! How a thread looks.
//!
//! As in Zed's agent panel, the thread is a column of entries: the user's
//! messages as cards, the assistant's prose as rich text, thinking and tool
//! calls as cards that open, with diffs and plans open from the start (as in
//! Delta, nothing that changed the code is hidden). Permission prompts sit
//! above the composer, where t3code puts them, so they stay in view.
//!
//! In maths typesetting mode the prose is set as a paper: a serif text font
//! (Libertinus Serif by default, as Typst sets documents), justified, in a
//! column of a fixed measure, on a page of its own colour.

use std::sync::Arc;

use cosmic::iced::{Alignment, Color, Length, Padding};
use cosmic::widget::{self, button, text_editor};
use cosmic::{Element, theme};

use super::claude::Decision;
use super::model::{Activity, ItemKind, ToolStatus};
use super::rich::{Typography, rich_text};
use super::tab::{BODY, INPUT, Load, MODES, Msg, OUTPUT, ThreadTab};
use crate::Message;
use crate::config::{Config, PaperTone};
use crate::fl;

/// The typography and colours of threads, from the settings and theme
pub struct ThreadStyle {
    pub paper: bool,
    pub body: Arc<Typography>,
    pub thinking: Arc<Typography>,
    /// Tool input and output, and notices
    pub small: Arc<Typography>,
    /// The text column's width
    pub measure: f32,
    /// The page's colour, when it is not the theme's
    pub page: Option<Color>,
    /// The theme the thread is set in, when it is not the app's
    pub chrome: Option<cosmic::Theme>,
    /// Thinking and tool calls among the replies; those not are shown only
    /// while current, above the composer
    pub show_thinking: bool,
    pub show_tools: bool,
}

impl ThreadStyle {
    pub fn new(config: &Config, theme: &cosmic::Theme) -> Self {
        let settings = &config.threads;
        let paper = config.thread_paper;
        // A white or sepia paper sets the whole thread in the light theme
        let light = cosmic::Theme::light();
        let chrome = if paper && settings.paper_tone != PaperTone::Theme { Some(light) } else { None };
        let cosmic = chrome.as_ref().unwrap_or(theme).cosmic();
        let mut text: Color = cosmic.on_bg_color().into();
        let mut code_background: Color = cosmic.bg_component_color().into();
        let mut page = None;
        if paper {
            match settings.paper_tone {
                PaperTone::Theme => {}
                PaperTone::White => {
                    page = Some(Color::WHITE);
                    text = Color::from_rgb8(0x1a, 0x1a, 0x1a);
                    code_background = Color::from_rgb8(0xf2, 0xf2, 0xf0);
                }
                PaperTone::Sepia => {
                    page = Some(Color::from_rgb8(0xf6, 0xef, 0xdf));
                    text = Color::from_rgb8(0x3b, 0x2f, 0x23);
                    code_background = Color::from_rgb8(0xec, 0xe3, 0xcf);
                }
            }
        }
        let dark_page = page.is_none() && cosmic.is_dark;
        let muted = Color { a: 0.68, ..text };
        let accent: Color = if page.is_some() { Color::from_rgb8(0x1f, 0x5f, 0xa8) } else { cosmic.accent_text_color().into() };
        let selection = Color { a: 0.32, ..Color::from(cosmic.accent.base) };
        let (added, removed) = if dark_page {
            (Color::from_rgb8(0x8f, 0xd6, 0x8f), Color::from_rgb8(0xf0, 0x8a, 0x8a))
        } else {
            (Color::from_rgb8(0x1d, 0x7a, 0x2a), Color::from_rgb8(0xb0, 0x22, 0x22))
        };
        let mono = config.font_name.clone();
        let (body_family, size, line_height, justify) = if paper {
            (
                settings.paper_font.family().to_string(),
                f32::from(settings.paper_size),
                f32::from(settings.paper_line_height) / 100.0,
                settings.paper_justify,
            )
        } else {
            (String::new(), f32::from(settings.text_size), 1.5, false)
        };
        let base = Typography {
            body: body_family,
            mono,
            font_overrides: Vec::new(),
            size,
            line_height,
            paragraph_spacing: 0.7,
            math_scale: f32::from(settings.math_scale) / 100.0,
            justify,
            italic: false,
            text,
            muted,
            link: accent,
            code: text,
            code_background,
            page: page.unwrap_or_else(|| cosmic.bg_color().into()),
            rule: Color { a: 0.25, ..text },
            selection,
            error: cosmic.destructive_text_color().into(),
            added,
            removed,
            image_width: 520.0,
        };
        let thinking = Typography { italic: true, text: muted, justify: false, ..base.clone() };
        let small = Typography {
            size: (size * 0.86).round(),
            line_height: 1.4,
            justify: false,
            body: String::new(),
            ..base.clone()
        };
        let measure = if paper { f32::from(settings.paper_measure) * size } else { 52.0 * size };
        Self {
            paper,
            chrome,
            show_thinking: config.thread_show_thinking,
            show_tools: config.thread_show_tools,
            body: Arc::new(base),
            thinking: Arc::new(thinking),
            small: Arc::new(small),
            measure,
            page,
        }
    }
}

fn icon(name: &'static str) -> widget::icon::Icon {
    crate::icon_cache_get(name, 16)
}

/// A button showing only an icon, as the header bar's are
fn icon_button<'a>(name: &'static str) -> button::Button<'a, Message> {
    button::custom(icon(name))
        .class(theme::Button::Icon)
        .padding(theme::active().cosmic().spacing.space_xxs)
}

/// A tool call's status as an icon and colour
fn status_icon(status: ToolStatus) -> (&'static str, bool) {
    match status {
        ToolStatus::Pending | ToolStatus::Running => ("view-more-symbolic", false),
        ToolStatus::Asking => ("dialog-warning-symbolic", true),
        ToolStatus::Done => ("emblem-ok-symbolic", false),
        ToolStatus::Failed => ("dialog-error-symbolic", true),
        ToolStatus::Denied => ("window-close-symbolic", true),
    }
}

/// Tools whose changes are shown open, so nothing that changed the code is
/// hidden
/// Whether an item is left out of the replies, as the view's options say
fn hidden(item: &super::model::Item, style: &ThreadStyle) -> bool {
    !super::tab::Shown { thinking: style.show_thinking, tools: style.show_tools }.shows(item)
}

/// What the agent is doing now, when the replies leave it out: its thought
/// or the tool it runs, above the composer, opened to read on demand
fn activity<'a>(tab: &'a ThreadTab, style: &ThreadStyle) -> Option<Element<'a, Message>> {
    if !tab.is_working() {
        return None;
    }
    let entries = tab.entries.as_ref()?;
    // The latest of this turn, if it is one left out
    let item = entries.items.iter().rev().take_while(|i| !matches!(i.kind, ItemKind::User { .. })).next()?;
    if !hidden(item, style) {
        return None;
    }
    let id = tab.id;
    let spacing = theme::active().cosmic().spacing;
    let (status, label, parts) = match &item.kind {
        ItemKind::Thinking { done, .. } => (
            "view-more-symbolic",
            if *done { fl!("thread-thought") } else { fl!("thread-thinking") },
            vec![(BODY, style.thinking.clone())],
        ),
        ItemKind::Tool(tool) => (
            status_icon(tool.status).0,
            relative_title(&tool.title(), &tab.cwd),
            vec![(INPUT, style.small.clone()), (OUTPUT, style.small.clone())],
        ),
        _ => return None,
    };
    let open = tab.activity_open;
    let header = widget::mouse_area(
        widget::row::with_children(vec![
            icon(status).into(),
            widget::text::body(ellipsize(&label, 160)).width(Length::Fill).into(),
            icon(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }).into(),
        ])
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center),
    )
    .on_press(Message::Thread(id, Msg::ToggleActivity));
    let mut column = vec![header.into()];
    if open {
        let docs: Vec<Element<'a, Message>> = parts
            .into_iter()
            .filter_map(|(part, typography)| Some(rich_text::<Message>(tab.doc(item, part)?, typography).into()))
            .collect();
        if !docs.is_empty() {
            column.push(
                widget::container(
                    widget::scrollable(widget::column::with_children(docs).spacing(spacing.space_xs))
                        .anchor_bottom()
                        .height(Length::Shrink),
                )
                .max_height(240.0)
                .into(),
            );
        }
    }
    let card = widget::container(widget::column::with_children(column).spacing(spacing.space_xs))
        .class(theme::Container::Card)
        .padding([spacing.space_xxs, spacing.space_s])
        .width(Length::Fill)
        .max_width(style.measure + 2.0 * f32::from(spacing.space_m));
    Some(
        widget::container(card)
            .center_x(Length::Fill)
            .padding(Padding::from([spacing.space_xxs, spacing.space_m]))
            .into(),
    )
}

fn open_by_default(name: &str, status: ToolStatus) -> bool {
    // A question is asked in its own card, not as its input
    matches!(name, "Edit" | "MultiEdit" | "Write" | "TodoWrite") || (status == ToolStatus::Asking && name != "AskUserQuestion")
}

pub fn view<'a>(tab: &'a ThreadTab, style: &ThreadStyle) -> Element<'a, Message> {
    let id = tab.id;
    let cosmic = theme::active();
    let spacing = cosmic.cosmic().spacing;
    let msg = move |m: Msg| Message::Thread(id, m);

    // The entries
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    match (&tab.load, &tab.entries) {
        (Load::Loading, None) => items.push(widget::text::body(fl!("thread-loading")).into()),
        (Load::Failed(err), _) => {
            items.push(widget::text::body(fl!("thread-load-failed", error = err.as_str())).into())
        }
        _ => {}
    }
    if tab.earlier > 0 {
        items.push(
            widget::container(
                button::standard(fl!("thread-load-earlier", count = tab.earlier)).on_press(msg(Msg::LoadEarlier)),
            )
            .center_x(Length::Fill)
            .into(),
        );
    }
    let entries = tab.entries.as_ref();
    let empty = entries.is_none_or(|entries| entries.items.is_empty());
    if empty && tab.load == Load::Ready {
        items.push(
            widget::container(
                widget::column::with_children(vec![
                    widget::text::title3(fl!("thread-empty-title", agent = tab.agent.label())).into(),
                    widget::text::body(fl!("thread-empty-body", agent = tab.agent.label())).into(),
                ])
                .spacing(spacing.space_xs)
                .align_x(Alignment::Center),
            )
            .center_x(Length::Fill)
            .padding([spacing.space_xl, 0])
            .into(),
        );
    }
    for (index, item) in entries.map(|e| e.items.as_slice()).unwrap_or_default().iter().enumerate() {
        if hidden(item, style) {
            continue;
        }
        let item_id = item.id;
        let rich = |part, typography: &Arc<Typography>| -> Option<Element<'a, Message>> {
            let doc = tab.doc(item, part)?;
            Some(
                rich_text(doc, typography.clone())
                    .selection(tab.selection_in(index, part))
                    .dragging(tab.dragging)
                    .base_dir((tab.host.is_empty()).then_some(std::path::Path::new(tab.cwd.as_str())))
                    .on_select(move |select| Message::Thread(id, Msg::Select(item_id, part, select)))
                    .on_link(Message::LaunchUrl)
                    .on_image(Message::OpenImage)
                    .on_diagram(Message::OpenDiagram)
                    .into(),
            )
        };
        let element: Element<'a, Message> = match &item.kind {
            ItemKind::User { images, .. } => {
                let mut column = vec![widget::text::caption(fl!("thread-you")).into()];
                column.extend(rich(BODY, &style.body));
                if !images.is_empty() {
                    column.push(thumbnails(images.iter().map(|i| i.path.clone()).collect()));
                }
                let card = widget::container(widget::column::with_children(column).spacing(spacing.space_xxs))
                    .padding([spacing.space_s, spacing.space_m])
                    .width(Length::Fill);
                if style.paper {
                    card.class(theme::Container::Transparent).into()
                } else {
                    card.class(theme::Container::Card).into()
                }
            }
            ItemKind::Text { .. } => rich(BODY, &style.body).unwrap_or_else(|| widget::Space::new().into()),
            ItemKind::Thinking { text, tokens, done } => {
                let open = tab.expanded.contains(&item_id);
                let label = match (*done, *tokens) {
                    (false, _) => fl!("thread-thinking"),
                    (true, 0) => fl!("thread-thought"),
                    (true, tokens) => fl!("thread-thought-tokens", tokens = tokens),
                };
                let mut column = vec![disclosure(label, open, !text.trim().is_empty(), msg(Msg::Toggle(item_id)))];
                if open {
                    column.extend(rich(BODY, &style.thinking));
                }
                widget::column::with_children(column).spacing(spacing.space_xxs).into()
            }
            ItemKind::Tool(tool) => {
                let open = tab.expanded.contains(&item_id) != open_by_default(&tool.name, tool.status);
                let (status, alert) = status_icon(tool.status);
                let mut status_icon = icon(status);
                if alert {
                    status_icon = status_icon.class(theme::Svg::Custom(std::rc::Rc::new(|theme: &cosmic::Theme| {
                        cosmic::iced::widget::svg::Style { color: Some(theme.cosmic().destructive_text_color().into()) }
                    })));
                }
                let title = relative_title(&tool.title(), &tab.cwd);
                let header = widget::mouse_area(
                    widget::row::with_children(vec![
                        status_icon.into(),
                        widget::text::body(ellipsize(&title, 160)).width(Length::Fill).into(),
                        icon(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }).into(),
                    ])
                    .spacing(spacing.space_xs)
                    .align_y(Alignment::Center)
                    .padding([spacing.space_xs, spacing.space_s]),
                )
                .interaction(cosmic::iced::mouse::Interaction::Pointer)
                .on_press(msg(Msg::Toggle(item_id)));
                let mut column = vec![header.into()];
                if open {
                    let mut body = Vec::new();
                    body.extend(rich(INPUT, &style.small));
                    body.extend(rich(OUTPUT, &style.small));
                    if !tool.images.is_empty() {
                        body.push(thumbnails(tool.images.iter().map(|i| i.path.clone()).collect()));
                    }
                    if !body.is_empty() {
                        column.push(
                            widget::column::with_children(body)
                                .spacing(spacing.space_xs)
                                .padding([0, spacing.space_s, spacing.space_s, spacing.space_s])
                                .into(),
                        );
                    }
                }
                widget::container(widget::column::with_children(column))
                    .class(theme::Container::Card)
                    .width(Length::Fill)
                    .into()
            }
            ItemKind::Notice { error, .. } => {
                let mut typography = (*style.small).clone();
                typography.text = if *error { typography.error } else { typography.muted };
                let typography = Arc::new(typography);
                rich(BODY, &typography).unwrap_or_else(|| widget::Space::new().into())
            }
        };
        items.push(element);
    }
    if entries.is_some_and(|e| e.activity == Activity::Working) {
        items.push(widget::text::caption(fl!("thread-working")).into());
    }

    let column = widget::column::with_children(items)
        .spacing(if style.paper { spacing.space_l } else { spacing.space_m })
        .padding([spacing.space_l, spacing.space_m])
        .max_width(style.measure + 2.0 * f32::from(spacing.space_m));
    let page = widget::container(column).center_x(Length::Fill);
    let page: Element<'a, Message> = match style.page {
        Some(color) => page
            .class(theme::Container::custom(move |_| cosmic::iced::widget::container::Style {
                background: Some(color.into()),
                ..Default::default()
            }))
            .into(),
        None => page.into(),
    };
    let scroll = widget::scrollable(page)
        .id(tab.scroll_id.clone())
        .on_scroll(move |viewport| Message::Thread(id, Msg::Scrolled(viewport)))
        .height(Length::Fill)
        .width(Length::Fill);
    let scroll = widget::context_menu(scroll, Some(crate::menu::thread_context_menu(tab.has_selection(), style.paper)));

    let mut column = vec![scroll.into()];
    if let Some(bar) = activity(tab, style) {
        column.push(bar);
    }
    if let Some(entries) = entries
        && let Some(request) = entries.permissions.first()
    {
        column.push(if request.questions.is_empty() { permission(tab, request, style) } else { questions(tab, request, style) });
    }
    column.push(composer(tab, style));
    // The thread draws its own ground, as a terminal does
    let page = style.page;
    let ground = widget::container(widget::column::with_children(column).width(Length::Fill).height(Length::Fill))
        .class(theme::Container::custom(move |theme| {
            // Opaque, whatever the window's transparency: a translucent
            // theme's colours here would leave the text invisible
            let cosmic = theme.cosmic();
            let on: Color = cosmic.on_bg_color().into();
            cosmic::iced::widget::container::Style {
                background: Some(page.unwrap_or_else(|| cosmic.bg_color().into()).into()),
                text_color: Some(Color { a: 1.0, ..on }),
                icon_color: Some(Color { a: 1.0, ..on }),
                ..Default::default()
            }
        }))
        .width(Length::Fill)
        .height(Length::Fill)
        ;
    match &style.chrome {
        Some(theme) => cosmic::iced::widget::themer(Some(theme.clone()), ground).into(),
        None => ground.into(),
    }
}

fn disclosure<'a>(label: String, open: bool, can_open: bool, on_press: Message) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    let mut row = vec![widget::text::caption(label).into()];
    if can_open {
        row.insert(0, icon(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }).into());
    }
    button::custom(widget::row::with_children(row).spacing(spacing.space_xxs).align_y(Alignment::Center))
        .class(theme::Button::Text)
        .padding([spacing.space_xxxs, spacing.space_xxs])
        .on_press_maybe(can_open.then_some(on_press))
        .into()
}

fn thumbnails<'a>(paths: Vec<std::path::PathBuf>) -> Element<'a, Message> {
    let spacing = theme::active().cosmic().spacing;
    widget::row::with_children(
        paths
            .into_iter()
            .map(|path| {
                let shown = super::rich::thumbnail_of(&path);
                widget::mouse_area(
                    widget::image(widget::image::Handle::from_path(shown))
                        .height(Length::Fixed(160.0))
                        .border_radius([6.0; 4]),
                )
                .interaction(cosmic::iced::mouse::Interaction::Pointer)
                .on_press(Message::OpenImage(path))
                .into()
            })
            .collect::<Vec<Element<'a, Message>>>(),
    )
    .spacing(spacing.space_xs)
    .wrap()
    .into()
}

/// A tool's title with paths in the thread's directory made relative
fn relative_title(title: &str, cwd: &str) -> String {
    let cwd = cwd.trim_end_matches('/');
    if cwd.len() < 2 {
        return title.to_string();
    }
    title.replace(&format!("{cwd}/"), "")
}

fn ellipsize(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// A permission prompt: allow once, allow for the session, or deny
fn permission<'a>(tab: &'a ThreadTab, request: &'a super::model::PermissionRequest, style: &ThreadStyle) -> Element<'a, Message> {
    let id = tab.id;
    let spacing = theme::active().cosmic().spacing;
    let tool = tab
        .entries
        .as_ref()
        .and_then(|entries| {
            entries.items.iter().find(|item| matches!(&item.kind, ItemKind::Tool(t) if t.id == request.tool_use_id))
        });
    let title = match tool.map(|item| &item.kind) {
        Some(ItemKind::Tool(tool)) => tool.title(),
        _ => request.tool_name.clone(),
    };
    let mut column = vec![
        widget::row::with_children(vec![
            icon("dialog-warning-symbolic").into(),
            widget::text::heading(fl!("thread-permission", agent = tab.agent.label(), tool = request.tool_name.as_str())).into(),
        ])
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .into(),
        widget::text::body(ellipsize(&title, 300)).into(),
    ];
    if !request.title.is_empty() && request.title != title {
        column.push(widget::text::caption(request.title.clone()).into());
    }
    if let Some(item) = tool
        && let Some(doc) = tab.doc(item, INPUT)
    {
        column.push(
            widget::container(widget::scrollable(rich_text::<Message>(doc, style.small.clone())).height(Length::Shrink))
                .max_height(220.0)
                .into(),
        );
    }
    let request_id = request.request_id.clone();
    let mut buttons = vec![
        button::suggested(fl!("thread-allow"))
            .on_press(Message::Thread(id, Msg::Answer(request_id.clone(), Decision::Allow)))
            .into(),
    ];
    if request.always_allowed && !request.suggestions.is_null() {
        buttons.push(
            button::standard(fl!("thread-allow-always"))
                .on_press(Message::Thread(id, Msg::Answer(request_id.clone(), Decision::AllowAlways)))
                .into(),
        );
    }
    buttons.push(
        button::destructive(fl!("thread-deny"))
            .on_press(Message::Thread(id, Msg::Answer(request_id, Decision::Deny)))
            .into(),
    );
    column.push(widget::row::with_children(buttons).spacing(spacing.space_xs).into());
    let card = widget::container(widget::column::with_children(column).spacing(spacing.space_xs))
        .class(theme::Container::Card)
        .padding(spacing.space_s)
        .max_width(style.measure + 2.0 * f32::from(spacing.space_m));
    widget::container(card)
        .center_x(Length::Fill)
        .padding(Padding::from([spacing.space_xxs, spacing.space_m]))
        .into()
}

/// The agent's questions: each with its answers as buttons, and the
/// composer for an answer of the user's own
fn questions<'a>(tab: &'a ThreadTab, request: &'a super::model::PermissionRequest, style: &ThreadStyle) -> Element<'a, Message> {
    let id = tab.id;
    let spacing = theme::active().cosmic().spacing;
    let mut column = vec![
        widget::row::with_children(vec![
            icon("dialog-warning-symbolic").into(),
            widget::text::heading(fl!("thread-question", agent = tab.agent.label())).into(),
        ])
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center)
        .into(),
    ];
    for question in &request.questions {
        if !question.header.is_empty() {
            column.push(widget::text::caption(question.header.clone()).into());
        }
        column.push(widget::text::body(question.question.clone()).into());
        let chosen = tab.answers.get(&question.key);
        let options: Vec<Element<'a, Message>> = question
            .options
            .iter()
            .map(|(label, description)| {
                let choose = Message::Thread(id, Msg::Choose(request.request_id.clone(), question.key.clone(), label.clone()));
                let button: Element<'a, Message> = if chosen == Some(label) {
                    button::suggested(label.clone()).on_press(choose).into()
                } else {
                    button::standard(label.clone()).on_press(choose).into()
                };
                if description.is_empty() {
                    button
                } else {
                    widget::tooltip(button, widget::text::body(description.clone()), widget::tooltip::Position::Top).into()
                }
            })
            .collect();
        if !options.is_empty() {
            column.push(widget::flex_row(options).row_spacing(spacing.space_xxs).column_spacing(spacing.space_xxs).into());
        }
        if let Some(answer) = chosen.filter(|a| !question.options.iter().any(|(label, _)| label == *a)) {
            column.push(widget::text::caption(fl!("thread-answered", answer = answer.as_str())).into());
        } else if question.free && chosen.is_none() {
            column.push(widget::text::caption(fl!("thread-answer-below")).into());
        }
    }
    column.push(
        widget::row::with_children(vec![
            button::standard(fl!("thread-skip"))
                .on_press(Message::Thread(id, Msg::Answer(request.request_id.clone(), Decision::Deny)))
                .into(),
        ])
        .into(),
    );
    let card = widget::container(widget::column::with_children(column).spacing(spacing.space_xs))
        .class(theme::Container::Card)
        .padding(spacing.space_s)
        .width(Length::Fill)
        .max_width(style.measure + 2.0 * f32::from(spacing.space_m));
    widget::container(card)
        .center_x(Length::Fill)
        .padding(Padding::from([spacing.space_xxs, spacing.space_m]))
        .into()
}

/// The composer: the message, its images, the model and mode, and send
fn composer<'a>(tab: &'a ThreadTab, style: &ThreadStyle) -> Element<'a, Message> {
    let id = tab.id;
    let spacing = theme::active().cosmic().spacing;
    let msg = move |m: Msg| Message::Thread(id, m);
    let working = tab.is_working();
    let editor = text_editor::text_editor(&tab.composer)
        .id(tab.composer_id.clone())
        .placeholder(fl!("thread-placeholder", agent = tab.agent.label()))
        .on_action(move |action| Message::Thread(id, Msg::Edit(action)))
        .size(style.small.size.max(14.0))
        .padding(spacing.space_xs)
        .min_height(56.0)
        .max_height(280.0)
        .key_binding(move |press| {
            use cosmic::iced::keyboard::{Key, key::Named};
            if matches!(press.key, Key::Named(Named::Enter)) && !press.modifiers.shift() {
                return Some(text_editor::Binding::Custom(Message::Thread(id, Msg::Send)));
            }
            text_editor::Binding::from_key_press(press)
        });
    let mut column: Vec<Element<'a, Message>> = Vec::new();
    if !tab.attachments.is_empty() {
        column.push(
            widget::row::with_children(
                tab.attachments
                    .iter()
                    .enumerate()
                    .map(|(n, path)| {
                        widget::row::with_children(vec![
                            widget::image(widget::image::Handle::from_path(super::rich::thumbnail_of(path)))
                                .height(Length::Fixed(48.0))
                                .into(),
                            icon_button("window-close-symbolic").on_press(msg(Msg::Detach(n))).into(),
                        ])
                        .align_y(Alignment::Center)
                        .into()
                    })
                    .collect::<Vec<Element<'a, Message>>>(),
            )
            .spacing(spacing.space_xs)
            .into(),
        );
    }
    column.push(editor.into());
    let mut model_names: Vec<String> = tab.models.iter().map(|(_, name)| name.clone()).collect();
    let mut model_selected = tab.models.iter().position(|(value, _)| *value == tab.model);
    // A model the CLI's list doesn't name, as one it resolved an alias to,
    // shows as itself
    if model_selected.is_none() && !tab.model.is_empty() && !model_names.is_empty() {
        model_selected = Some(model_names.len());
        model_names.push(tab.model.clone());
    }
    let mode_names: Vec<String> = MODES.iter().map(|(_, name)| name.to_string()).collect();
    let mode_selected = MODES.iter().position(|(value, _)| *value == tab.mode).or(Some(0));
    let mut controls: Vec<Element<'a, Message>> = Vec::new();
    if !model_names.is_empty() {
        controls.push(widget::dropdown(model_names, model_selected, move |n| Message::Thread(id, Msg::SetModel(n))).into());
    } else if !tab.model.is_empty() {
        controls.push(widget::text::caption(tab.model.clone()).into());
    }
    controls.push(widget::dropdown(mode_names, mode_selected, move |n| Message::Thread(id, Msg::SetMode(n))).into());
    controls.push(widget::Space::new().width(Length::Fill).into());
    controls.push(
        widget::tooltip(
            icon_button("insert-image-symbolic").on_press(msg(Msg::Attach)),
            widget::text::body(fl!("thread-attach")),
            widget::tooltip::Position::Top,
        )
        .into(),
    );
    controls.push(if working {
        button::destructive(fl!("thread-stop")).on_press(msg(Msg::Stop)).into()
    } else {
        button::suggested(fl!("thread-send")).on_press(msg(Msg::Send)).into()
    });
    column.push(widget::row::with_children(controls).spacing(spacing.space_xs).align_y(Alignment::Center).into());
    widget::container(
        widget::container(widget::column::with_children(column).spacing(spacing.space_xs))
            .class(theme::Container::Card)
            .padding(spacing.space_xs)
            .max_width(style.measure + 2.0 * f32::from(spacing.space_m)),
    )
    .center_x(Length::Fill)
    .padding([spacing.space_xs, spacing.space_m, spacing.space_m, spacing.space_m])
    .into()
}

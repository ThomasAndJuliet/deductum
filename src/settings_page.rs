// SPDX-License-Identifier: GPL-3.0-only

//! The settings page, laid out as COSMIC apps lay out theirs: titled sections of items, each a
//! label (with a line saying what it does) and its control. It takes the document's place while
//! it is open (the header's settings button, or Ctrl+,). Every change applies at once and is
//! written at once; there is nothing to confirm.

use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget::{self, settings};
use cosmic::iced;

use super::settings::{DisplayChange, Loogle, TextFont, ThemeChoice};
use super::shell::{self, Msg, sp};
use super::{App, Message};

fn set(m: Msg) -> Message {
    Message::Shell(m)
}

pub fn page(app: &App) -> Element<'_, Message> {
    let s = sp();
    let st = &app.shell.settings;
    let zoom = (super::view::zoom() * 100.0).round() as i32;

    let themes: Vec<&'static str> = ThemeChoice::ALL.iter().map(|t| t.label()).collect();
    let theme_index = ThemeChoice::ALL.iter().position(|t| *t == st.appearance.theme);
    let appearance = settings::section()
        .title("Appearance")
        .add(settings::item::builder("Theme").control(widget::dropdown(themes, theme_index, |i| set(Msg::Theme(ThemeChoice::ALL[i])))))
        .add(
            settings::item::builder("Zoom")
                .description("Also Ctrl+= and Ctrl+−; Ctrl+0 for 100%")
                .control(widget::spin_button(format!("{zoom}%"), zoom, 10, 50, 300, |p| set(Msg::TextSize(p)))),
        )
        .add(
            settings::item::builder("The goal beside the text")
                .description("When the window has room; otherwise under the proof, with the page centred")
                .toggler(super::view::goal_beside_on(), |on| set(Msg::GoalBeside(on))),
        );

    let d = &st.display;
    let choose = |label: &'static str, values: &'static [f32], value: f32, suffix: &'static str, change: fn(f32) -> DisplayChange| {
        let mut choices = values.to_vec();
        if !choices.iter().any(|v| (*v - value).abs() < 0.001) { choices.push(value); choices.sort_by(f32::total_cmp); }
        let selected = choices.iter().position(|v| (*v - value).abs() < 0.001);
        let options: Vec<String> = choices.iter().map(|v| format!("{v}{suffix}")).collect();
        settings::item::builder(label).control(widget::dropdown(options, selected, move |i| set(Msg::Display(change(choices[i])))))
    };
    let mut width_values: Vec<f32> = vec![0.0, 28.0, 32.0, 36.0, 40.0, 48.0, 52.0, 60.0, 72.0];
    if !width_values.iter().any(|v| (*v - d.measure).abs() < 0.001) { width_values.push(d.measure); width_values.sort_by(f32::total_cmp); }
    let width_selected = width_values.iter().position(|v| (*v - d.measure).abs() < 0.001);
    let widths: Vec<String> = width_values.iter().map(|v| if *v == 0.0 { "Automatic".into() } else { format!("{v} em") }).collect();
    let display = settings::section()
        .title("Display")
        .add(settings::item::builder("Text font").description("The literate document and its prose editor; Sans matches Terminal threads")
            .control(widget::dropdown(TextFont::ALL.iter().map(|f| f.label()).collect::<Vec<_>>(), TextFont::ALL.iter().position(|f| *f == d.font), |i| set(Msg::Display(DisplayChange::Font(TextFont::ALL[i]))))))
        .add(choose("Text size", &[12.0, 14.0, 16.0, 17.0, 18.0, 20.0, 22.0, 24.0, 28.0, 32.0], d.text_size, " px", DisplayChange::TextSize))
        .add(choose("Line spacing", &[1.2, 1.3, 1.4, 1.45, 1.5, 1.55, 1.6, 1.8, 2.0], d.line_height, "", DisplayChange::LineHeight))
        .add(settings::item::builder("Text width").description("Maximum reading width in ems; Automatic fits the proof layout")
            .control(widget::dropdown(widths, width_selected, move |i| set(Msg::Display(DisplayChange::Measure(width_values[i]))))))
        .add(choose("Paragraph spacing", &[0.4, 0.65, 0.7, 0.9, 1.2, 1.5], d.paragraph_spacing, " em", DisplayChange::ParagraphSpacing))
        .add(choose("Maths size", &[0.9, 1.0, 1.1, 1.21, 1.3, 1.5], d.math_scale, " × text", DisplayChange::MathScale))
        .add(settings::item::builder("Justify paragraphs").toggler(d.justify, |v| set(Msg::Display(DisplayChange::Justify(v)))))
        .add(settings::item::builder("Centre the column").toggler(d.centred, |v| set(Msg::Display(DisplayChange::Centred(v)))));

    let pause = st.editing.keyword_pause_ms;
    let editing = settings::section()
        .title("Editing")
        .add(settings::item::builder("Vim keys").description("Normal, insert and visual modes in every editor; j and k move between blocks").toggler(app.vim_enabled, |on| set(Msg::Vim(on))))
        .add(
            settings::item::builder("Indent width")
                .description("Spaces that Tab inserts and Shift+Tab takes away")
                .control(widget::spin_button(format!("{}", st.editing.indent_width), st.editing.indent_width, 1, 1, 8, |n| set(Msg::IndentWidth(n)))),
        )
        .add(
            settings::item::builder("Completion while typing")
                .description("Words and steps from the goal as you type; \\ symbols and @names are always offered")
                .toggler(st.editing.complete_while_typing, |on| set(Msg::CompleteWhileTyping(on))),
        )
        .add(
            settings::item::builder("Keyword lines ask for proofs")
                .description("A line reading claude, codex or @name asks that LLM or agent for a proof")
                .toggler(st.editing.keywords, |on| set(Msg::Keywords(on))),
        )
        .add(
            settings::item::builder("Pause before asking")
                .description("How long typing pauses on a keyword line before it asks; Enter asks at once")
                .control(widget::spin_button(format!("{:.1} s", f64::from(pause) / 1000.0), pause, 500, 500, 10_000, |ms| set(Msg::KeywordPause(ms)))),
        );

    let proof_state = settings::section()
        .title("Proof state")
        .add(
            settings::item::builder("Show types")
                .description("Where the prover can print them; the Types switch beside the goal starts here")
                .toggler(st.proof_state.show_types, |on| set(Msg::ShowTypes(on))),
        )
        .add(
            settings::item::builder("Show what a step changed")
                .description("The subgoals the step at the caret solved and the new ones")
                .toggler(st.proof_state.show_changes, |on| set(Msg::ShowChanges(on))),
        );

    // Where this session's keyword lines go is the session's (the host keeps it): shown here
    // while a session is open
    let route_item: Element<'_, Message> = match &app.workspace {
        Some(ws) => {
            let routes = shell::routes(ws);
            let current = shell::current_route(ws);
            let labels: Vec<String> = routes.iter().map(|(_, l)| l.clone()).collect();
            let index = routes.iter().position(|(v, _)| *v == current);
            settings::item::builder("Where keyword lines go")
                .description("For this session: its helper conversation remembers earlier attempts")
                .control(widget::dropdown(labels, index, |i| set(Msg::Route(i))))
                .into()
        }
        None => settings::item::builder("Where keyword lines go").description("Each session's own choice: open a session to set it").control(widget::Space::new()).into(),
    };
    let llm = settings::section()
        .title("LLM help")
        .add(route_item)
        .add(
            settings::item::builder("Claude's model")
                .description("When a claude line names none, as claude[opus] does")
                .control(widget::text_input("Claude's default", &st.llm.claude_model).on_input(|m| set(Msg::ClaudeModel(m))).width(Length::Fixed(200.0))),
        )
        .add(
            settings::item::builder("Codex's model")
                .description("When a codex line names none")
                .control(widget::text_input("Codex's default", &st.llm.codex_model).on_input(|m| set(Msg::CodexModel(m))).width(Length::Fixed(200.0))),
        );

    let loogles: Vec<&'static str> = Loogle::ALL.iter().map(|l| l.label()).collect();
    let loogle_index = Loogle::ALL.iter().position(|l| *l == st.lean.loogle);
    let lean = settings::section().title("Lean").add(
        settings::item::builder("Loogle search")
            .description("Finding lemmas by name and type pattern: off, a local Loogle, or the public service")
            .control(widget::dropdown(loogles, loogle_index, |i| set(Msg::Loogle(Loogle::ALL[i])))),
    );

    let mut machines = settings::section().title("Machines");
    for link in &app.links {
        let mut destination = match &link.target.destination {
            Some(d) => format!("Over ssh: {d}"),
            None => "This computer".to_string(),
        };
        // (its host's version, once it has said hello)
        if let Some(v) = link.hello.as_ref().and_then(|h| h["version"].as_str()) {
            destination.push_str(&format!(" · session host {v}"));
        }
        machines = machines.add(settings::item::builder(link.name.clone()).description(destination).control(machine_state(link)));
    }
    if app.links.is_empty() {
        machines = machines.add(settings::item::builder("No machines").description("Machines come from COSMIC Terminal Agents").control(widget::Space::new()));
    }

    let about = settings::section()
        .title("About")
        .add(settings::item::builder("Deductum").description("Literate proof documents for Isabelle and Lean, shared with agents").control(widget::text::body(format!("Version {}", env!("CARGO_PKG_VERSION")))))
        .add(settings::item::builder("Settings file").control(widget::text::caption(super::settings::path().display().to_string()).wrapping(iced::widget::text::Wrapping::WordOrGlyph)));

    let head = widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .spacing(s.space_s)
        .push(widget::text::title3("Settings").width(Length::Fill))
        .push(widget::button::standard("Done").on_press(set(Msg::Settings(false))));
    let col = widget::column::with_capacity(10)
        .spacing(s.space_l)
        .padding([s.space_l, s.space_l])
        .max_width(760)
        .push(head)
        .push(appearance)
        .push(display)
        .push(editing)
        .push(proof_state)
        .push(llm)
        .push(lean)
        .push(machines)
        .push(about);
    widget::scrollable(widget::container(col).center_x(Length::Fill)).height(Length::Fill).into()
}

/// A machine's connection, in a word with a coloured dot, as the sidebar and the status bar say it
fn machine_state<'a>(link: &super::Link) -> Element<'a, Message> {
    let conn = shell::connection(link);
    let mut word = conn.word().to_string();
    word[..1].make_ascii_uppercase();
    if let Some(d) = conn.detail() {
        word = format!("{word}: {d}");
    }
    widget::row::with_capacity(2)
        .spacing(6)
        .align_y(Alignment::Center)
        .push(conn.dot(8.0))
        .push(widget::text::body(word).wrapping(iced::widget::text::Wrapping::None))
        .into()
}

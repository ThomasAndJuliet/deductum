// SPDX-License-Identifier: GPL-3.0-only

//! Threads drawn headless, as the user would see them, and driven with
//! pointer events, as the user would drive them.
//!
//! The view is built and laid out as in the app, events go through iced's
//! `UserInterface` to the real widgets, and their messages go back to the
//! thread, so selection, copying and the rest are tested end to end. The
//! tiny-skia renderer draws a frame without a window or a GPU. Set
//! `CT_SNAPSHOTS` to a directory to keep the frames as PNG files.

use std::sync::Arc;

use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Event, Font, Pixels, Point, Size, mouse};

use super::tab::{Msg, Shown, ThreadTab};
use super::view::{ThreadStyle, view};
use crate::Message;
use crate::config::Config;

const SIZE: Size = Size::new(900.0, 1400.0);

pub struct Harness {
    pub tab: ThreadTab,
    pub config: Config,
    renderer: cosmic::Renderer,
    cache: Cache,
    cursor: mouse::Cursor,
}

impl Harness {
    pub fn new(tab: ThreadTab) -> Self {
        {
            let mut font_system = cosmic::iced::advanced::graphics::text::font_system().write().unwrap();
            for data in super::FONTS {
                font_system.raw().db_mut().load_font_data(data.to_vec());
            }
        }
        let renderer = cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("a headless renderer");
        Self { tab, config: Config::default(), renderer, cache: Cache::default(), cursor: mouse::Cursor::Unavailable }
    }

    fn theme(&self) -> cosmic::Theme {
        if self.config.app_theme == crate::config::AppTheme::Light {
            cosmic::Theme::light()
        } else {
            cosmic::Theme::dark()
        }
    }

    /// Send `events` through the view, then give the thread what its
    /// widgets said, one event at a time, rebuilding the view between them
    /// as the app does
    pub fn events(&mut self, events: &[Event]) {
        for event in events {
            if let Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                self.cursor = mouse::Cursor::Available(*position);
            }
            let theme = self.theme();
            let style = ThreadStyle::new(&self.config, &theme);
            let element = view(&self.tab, &style);
            let mut ui = UserInterface::build(element, SIZE, std::mem::take(&mut self.cache), &mut self.renderer);
            let mut messages = Vec::new();
            let _ = ui.update(
                std::slice::from_ref(event),
                self.cursor,
                &mut self.renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            self.cache = ui.into_cache();
            for message in messages {
                self.apply(message);
            }
        }
    }

    fn apply(&mut self, message: Message) {
        match message {
            Message::Thread(_, msg) => {
                let _ = self.tab.update(msg, &mut Vec::new());
            }
            Message::CopyPrimary(None) => self.tab.dragging = false,
            _ => {}
        }
    }

    fn operate(&mut self, operation: &mut dyn cosmic::iced::advanced::widget::Operation) {
        let theme = self.theme();
        let style = ThreadStyle::new(&self.config, &theme);
        let element = view(&self.tab, &style);
        let mut ui = UserInterface::build(element, SIZE, std::mem::take(&mut self.cache), &mut self.renderer);
        ui.operate(&self.renderer, operation);
        self.cache = ui.into_cache();
    }

    /// Press at `from`, drag to `to`, release
    pub fn drag(&mut self, from: Point, to: Point) {
        let mut events = vec![
            Event::Mouse(mouse::Event::CursorMoved { position: from }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ];
        for step in 1..=8 {
            let t = step as f32 / 8.0;
            events.push(Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t),
            }));
        }
        events.push(Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
        self.events(&events);
        // The app ends drags on any release, wherever it happens
        self.tab.dragging = false;
    }

    /// Draw any element, as [`Self::snapshot`] draws the thread
    pub fn draw(&mut self, element: cosmic::Element<'_, Message>, name: &str, size: Size) -> Vec<u8> {
        let theme = self.theme();
        let mut ui = UserInterface::build(element, size, Cache::default(), &mut self.renderer);
        let _ = ui.update(
            &[Event::Window(cosmic::iced::window::Event::RedrawRequested(std::time::Instant::now()))],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        let text: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
        ui.draw(&mut self.renderer, &theme, &Style { icon_color: text, text_color: text, scale_factor: 1.0 }, mouse::Cursor::Unavailable);
        let physical = cosmic::iced::Size::new(size.width as u32, size.height as u32);
        let rgba = self.renderer.screenshot(physical, 1.0, theme.cosmic().bg_color().into());
        if let Some(dir) = std::env::var_os("CT_SNAPSHOTS").filter(|dir| !dir.is_empty()) {
            image::RgbaImage::from_raw(physical.width, physical.height, rgba.clone())
                .expect("a frame")
                .save(std::path::Path::new(&dir).join(format!("{name}.png")))
                .expect("the frame saved");
        }
        rgba
    }

    /// Draw a frame, and keep it as `<name>.png` if `CT_SNAPSHOTS` is set
    pub fn snapshot(&mut self, name: &str) -> Vec<u8> {
        let theme = self.theme();
        let style = ThreadStyle::new(&self.config, &theme);
        let element = view(&self.tab, &style);
        let mut ui = UserInterface::build(element, SIZE, std::mem::take(&mut self.cache), &mut self.renderer);
        let _ = ui.update(
            &[Event::Window(cosmic::iced::window::Event::RedrawRequested(std::time::Instant::now()))],
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        let text: cosmic::iced::Color = theme.cosmic().on_bg_color().into();
        ui.draw(
            &mut self.renderer,
            &theme,
            &Style { icon_color: text, text_color: text, scale_factor: 1.0 },
            self.cursor,
        );
        self.cache = ui.into_cache();
        let size = cosmic::iced::Size::new(SIZE.width as u32, SIZE.height as u32);
        let background: cosmic::iced::Color = theme.cosmic().bg_color().into();
        let rgba = self.renderer.screenshot(size, 1.0, background);
        if let Some(dir) = std::env::var_os("CT_SNAPSHOTS").filter(|dir| !dir.is_empty()) {
            let path = std::path::Path::new(&dir).join(format!("{name}.png"));
            image::RgbaImage::from_raw(size.width, size.height, rgba.clone())
                .expect("a frame")
                .save(&path)
                .expect("the frame saved");
        }
        rgba
    }
}

/// A conversation with everything a thread shows: markdown, inline and
/// display math, an environment, lists, a table, a failed formula, a
/// link, tool calls with a command, a diff, a todo list and an image
pub fn demo_transcript(dir: &std::path::Path) -> String {
    let png = {
        let mut image = image::RgbImage::new(240, 120);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let t = (-(((x as f32 - 120.0) / 40.0).powi(2))).exp();
            let curve = (110.0 - 100.0 * t) as u32;
            *pixel = if y.abs_diff(curve) < 2 { image::Rgb([200, 60, 40]) } else { image::Rgb([245, 245, 240]) };
        }
        let path = dir.join("plot.png");
        image.save(&path).unwrap();
        std::fs::read(&path).unwrap()
    };
    let text = r"## The Gaussian integral

Let $I = \int_{-\infty}^{\infty} e^{-x^2}\,dx$. Squaring and switching to polar coordinates $x = r\cos\theta$, $y = r\sin\theta$ gives

$$
I^2 = \int_0^{2\pi}\!\int_0^\infty e^{-r^2}\, r\,dr\,d\theta = 2\pi \cdot \tfrac12 = \pi ,
$$

so $I = \sqrt{\pi} \approx 1.7725$. More generally, for $a > 0$,

\begin{align}
\int_{-\infty}^{\infty} e^{-a x^2}\,dx &= \sqrt{\frac{\pi}{a}}, \\
\int_{-\infty}^{\infty} x^2 e^{-x^2}\,dx &= \frac{\sqrt{\pi}}{2}.
\end{align}

Key facts:

1. The integrand is **even**, so $\int_0^\infty e^{-x^2}dx = \frac{\sqrt\pi}{2}$.
2. It underlies the normal density $\varphi(x) = \frac{1}{\sqrt{2\pi}} e^{-x^2/2}$.
   - nested point with *emphasis* and `inline code`
3. In $n$ dimensions, $\int_{\mathbb{R}^n} e^{-x^\top A x}\,dx = \sqrt{\frac{\pi^n}{\det A}}$ for $A \succ 0$.

| $a$ | $\sqrt{\pi/a}$ | numeric |
|---|---|---|
| 1 | $\sqrt{\pi}$ | 1.7725 |
| 2 | $\sqrt{\pi/2}$ | 1.2533 |

> A formula that fails to parse shows its source: $\frac{1}{$.

See [the Wikipedia article](https://en.wikipedia.org/wiki/Gaussian_integral).

```python
import math
print(math.sqrt(math.pi))
```
";
    let cwd = dir.display().to_string();
    let lines = [
        serde_json::json!({"type":"user","sessionId":"demo","message":{"role":"user","content":"Can you explain the Gaussian integral $\\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx$ and check it numerically?"}}),
        serde_json::json!({"type":"assistant","message":{"id":"m1","content":[{"type":"thinking","thinking":"The polar-coordinates proof, then a numeric check."}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":text}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"python3 -c 'import math; print(math.sqrt(math.pi))'","description":"Check sqrt(pi) numerically"}}]}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"1.7724538509055159","is_error":false}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":format!("{cwd}/notes.md"),"old_string":"I = ?\nTODO","new_string":"I = \\sqrt{\\pi}"}}]}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"The file has been updated.","is_error":false}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"t3","name":"TodoWrite","input":{"todos":[{"content":"Derive the integral","status":"completed"},{"content":"Check numerically","status":"in_progress"},{"content":"Write it up","status":"pending"}]}}]}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"ok"}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m4","content":[{"type":"tool_use","id":"t4","name":"Read","input":{"file_path":format!("{cwd}/plot.png")}}]}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t4","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":cosmic_term::b64::encode(&png)}}]}]}}),
        serde_json::json!({"type":"assistant","message":{"id":"m5","content":[{"type":"text","text":"The numeric check agrees: $\\sqrt{\\pi} = 1.7724538509\\ldots$ ✓"}]}}),
    ];
    lines.iter().map(|line| format!("{line}\n")).collect()
}

pub fn demo_tab() -> ThreadTab {
    let dir = std::env::temp_dir().join(format!("ct-thread-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: tests that read the image cache set it alike
    unsafe { std::env::set_var("XDG_CACHE_HOME", dir.join("cache")) };
    let transcript = demo_transcript(&dir);
    let history = super::session::read(std::io::Cursor::new(transcript), 20);
    let mut tab = ThreadTab::new(1, None, String::new(), dir.display().to_string(), String::new());
    tab.entries = Some(history.entries);
    tab.session_id = Some("demo".into());
    tab.title = "Gaussian integral".into();
    tab
}

#[cfg(test)]
mod tests {
    use super::*;

    fn harness() -> Harness {
        let mut harness = Harness::new(demo_tab());
        harness.config.thread_paper = false;
        harness
    }

    fn redraw(harness: &mut Harness) {
        harness.events(&[Event::Window(cosmic::iced::window::Event::RedrawRequested(std::time::Instant::now()))]);
    }

    fn scroll_offset(harness: &mut Harness) -> f32 {
        use cosmic::iced::advanced::widget::{Id, Operation, operation::Scrollable};
        use cosmic::iced::{Rectangle, Vector};
        struct Offset { id: Id, y: Option<f32> }
        impl Operation for Offset {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) { operate(self); }
            fn scrollable(&mut self, id: Option<&Id>, _: Rectangle, _: Rectangle, translation: Vector, _: &mut dyn Scrollable) {
                if id == Some(&self.id) { self.y = Some(translation.y); }
            }
        }
        let mut offset = Offset { id: harness.tab.scroll_id.clone(), y: None };
        harness.operate(&mut offset);
        offset.y.expect("the transcript scrollable")
    }

    #[test]
    fn reading_near_the_bottom_survives_composer_focus_and_output() {
        use cosmic::iced::advanced::widget::operation::{focusable, scrollable};
        let mut harness = harness();
        let mut bottom = scrollable::snap_to(
            harness.tab.scroll_id.clone(), scrollable::RelativeOffset { x: None, y: Some(1.0) },
        );
        harness.operate(&mut bottom);
        redraw(&mut harness);
        assert!(harness.tab.content_height > harness.tab.page_height);
        assert!(harness.tab.follow);
        harness.events(&[
            Event::Mouse(mouse::Event::CursorMoved { position: Point::new(300.0, 300.0) }),
            Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 6.0 } }),
        ]);
        redraw(&mut harness);
        assert!(!harness.tab.follow, "even a small upward scroll should stop following output");
        let reading_position = scroll_offset(&mut harness);
        for _ in 0..3 {
            harness.operate(&mut focusable::focus(harness.tab.composer_id.clone()));
            redraw(&mut harness);
            assert!(!harness.tab.follow, "focusing the composer must not resume following");
            assert_eq!(scroll_offset(&mut harness), reading_position);
        }
        let task = harness.tab.update(
            Msg::Cli(0, super::super::session::Output::Events(vec![super::super::model::Event::Notice("More output".into())])),
            &mut Vec::new(),
        );
        assert_eq!(task.units(), 0, "incoming output must not request a scroll while reading");
        redraw(&mut harness);
        assert_eq!(scroll_offset(&mut harness), reading_position);
        harness.operate(&mut bottom);
        redraw(&mut harness);
        assert!(harness.tab.follow, "scrolling back to the bottom should resume following");
    }

    #[test]
    fn composer_layout_changes_do_not_resume_following() {
        use cosmic::iced::advanced::widget::operation::scrollable;
        let mut harness = harness();
        harness.tab.composer = cosmic::widget::text_editor::Content::with_text("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight");
        harness.operate(&mut scrollable::snap_to(
            harness.tab.scroll_id.clone(), scrollable::RelativeOffset { x: None, y: Some(1.0) },
        ));
        redraw(&mut harness);
        harness.events(&[
            Event::Mouse(mouse::Event::CursorMoved { position: Point::new(300.0, 300.0) }),
            Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Pixels { x: 0.0, y: 30.0 } }),
        ]);
        redraw(&mut harness);
        assert!(!harness.tab.follow);
        // A shorter composer exposes the end without the user scrolling there.
        harness.tab.composer = cosmic::widget::text_editor::Content::new();
        redraw(&mut harness);
        assert!(!harness.tab.follow, "layout changes must not resume following output");
    }

    #[test]
    fn draws_a_thread() {
        let mut harness = harness();
        let frame = harness.snapshot("plain");
        // Something besides the background was drawn
        let first = &frame[..4];
        assert!(frame.chunks(4).filter(|px| *px != first).count() > 10_000);
        harness.config.thread_paper = true;
        harness.config.threads.paper_tone = crate::config::PaperTone::White;
        harness.snapshot("paper-white");
        harness.config.threads.paper_tone = crate::config::PaperTone::Sepia;
        harness.config.threads.paper_font = crate::config::PaperFont::NewComputerModern;
        harness.snapshot("paper-sepia-cm");
        harness.config.threads.paper_tone = crate::config::PaperTone::Theme;
        harness.config.app_theme = crate::config::AppTheme::Light;
        harness.snapshot("paper-light");
    }

    #[test]
    fn dragging_selects_and_copies_text_with_its_latex() {
        let mut harness = harness();
        harness.snapshot("before");
        // From the left of the thread's first lines to well down the answer
        harness.drag(Point::new(150.0, 106.0), Point::new(700.0, 366.0));
        harness.snapshot("selected");
        let text = harness.tab.selected_text().expect("a selection");
        assert!(text.contains("Gaussian integral"), "{text}");
        assert!(text.contains("$I = \\int_{-\\infty}^{\\infty} e^{-x^2}\\,dx$"), "{text}");
    }

    #[test]
    fn select_all_copies_every_entry() {
        let mut harness = harness();
        harness.tab.select_all();
        harness.snapshot("all");
        let text = harness.tab.selected_text().unwrap();
        assert!(text.contains("$$\nI^2 = "), "{text}");
        assert!(text.contains("The numeric check agrees: $\\sqrt{\\pi} = 1.7724538509\\ldots$"), "{text}");
        let _ = Arc::new(());
    }

    #[test]
    fn a_double_click_selects_a_word() {
        let mut harness = harness();
        harness.snapshot("w");
        let at = Point::new(470.0, 252.0);
        let click = [
            Event::Mouse(mouse::Event::CursorMoved { position: at }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ];
        harness.events(&click);
        harness.events(&click[1..]);
        let text = harness.tab.selected_text().unwrap_or_default();
        assert!(!text.is_empty() && !text.contains(' '), "{text:?}");
    }

    #[test]
    fn the_composer_takes_a_click_typing_and_a_selection() {
        use cosmic::iced::keyboard::{self, Key, Modifiers, key};
        let mut harness = harness();
        harness.snapshot("composer-before");
        // Into the composer's message box, near the bottom of the frame
        let at = Point::new(300.0, SIZE.height - 110.0);
        harness.events(&[
            Event::Mouse(mouse::Event::CursorMoved { position: at }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
        let typed: Vec<Event> = "hello world"
            .chars()
            .map(|c| {
                let text = c.to_string();
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Character(text.clone().into()),
                    modified_key: Key::Character(text.clone().into()),
                    physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                    location: keyboard::Location::Standard,
                    modifiers: Modifiers::empty(),
                    text: Some(text.into()),
                    repeat: false,
                })
            })
            .collect();
        harness.events(&typed);
        assert_eq!(harness.tab.composer.text().trim_end(), "hello world");
        harness.snapshot("composer-typed");
        harness.drag(Point::new(62.0, SIZE.height - 140.0), Point::new(130.0, SIZE.height - 140.0));
        harness.snapshot("composer-selected");
        assert!(harness.tab.composer.selection().is_some_and(|s| !s.is_empty()), "nothing selected in the composer");
    }

    /// A thread whose answer holds a bar chart and a box plot
    fn chart_tab() -> ThreadTab {
        let bar = r#"{"chartspec":1,"id":"headcount","purpose":"analysis","title":{"text":"Headcount by region"},
"data":{"x":{"kind":"nominal","values":["East","Central","West"]},
"measure":{"name":"headcount","kind":"count","zero":"meaningful"},
"series":[{"id":"headcount","name":"Headcount","values":[48,41,29]}]},
"chart":{"type":"bar","orientation":"vertical","value_labels":"off"}}"#;
        let samples = |base: f64| -> Vec<f64> { (0..12).map(|i| base + f64::from(i % 5) * 1.5 + f64::from(i) * 0.25).collect() };
        let boxes = serde_json::json!({
            "chartspec": 1, "id": "latency", "purpose": "analysis",
            "title": {"text": "Latency by witness"},
            "data": {
                "x": {"kind": "nominal", "values": ["W1", "W2", "W3"]},
                "measure": {"name": "latency", "kind": "quantity", "unit": {"suffix": " ms"}, "zero": "meaningful"},
                "series": [{"id": "r", "name": "Latency", "samples": [samples(5.0), samples(9.0), samples(14.0)]}]
            },
            "chart": {"type": "box"}
        });
        let text = format!("Two charts.\n\n```chartgen\n{bar}\n```\n\n```chartgen\n{boxes}\n```\n\nThe end.");
        let lines = [
            serde_json::json!({"type":"user","sessionId":"charts","message":{"role":"user","content":"Chart it"}}),
            serde_json::json!({"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":text}]}}),
        ];
        let transcript: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let history = super::super::session::read(std::io::Cursor::new(transcript), 20);
        let mut tab = ThreadTab::new(1, None, String::new(), "/tmp".into(), String::new());
        tab.entries = Some(history.entries);
        tab
    }

    #[test]
    fn draws_charts_whose_text_selects_and_copies() {
        let mut harness = Harness::new(chart_tab());
        harness.config.thread_paper = false;
        harness.snapshot("charts");
        harness.tab.select_all();
        let all = harness.tab.selected_text().unwrap();
        assert!(all.contains("```chartgen\n{\"chartspec\":1"), "{all}");
        assert!(!all.contains("chartgen: "), "a chart failed: {all}");
        harness.config.app_theme = crate::config::AppTheme::Light;
        harness.snapshot("charts-light");
        // A drag over the bar chart's labels selects them as text
        harness.tab.selection = None;
        harness.drag(Point::new(270.0, 386.0), Point::new(680.0, 396.0));
        let part = harness.tab.selected_text().unwrap_or_default();
        harness.snapshot("charts-selected");
        assert!(part.contains("Central") && !part.contains("chartspec"), "{part:?}");
    }

    const FLOWCHART: &str = "flowchart TD\n    A[Parse markdown] --> B{Mermaid fence?}\n    B -->|closed| C[Draw the diagram]\n    B -->|still open| D[Show it as code]\n    C --> E[(Diagram cache)]";
    const ARCHITECTURE: &str = "architecture-beta\n    group api(cloud)[API]\n    service db(database)[Database] in api\n    service disk(disk)[Storage] in api\n    service server(server)[Server] in api\n    service net(internet)[Internet]\n    db:L -- R:server\n    disk:T -- B:server\n    net:R -- L:server";

    /// A thread whose answer holds a flowchart and an architecture diagram
    fn diagram_tab() -> ThreadTab {
        let text = format!("Two diagrams.\n\n```mermaid\n{FLOWCHART}\n```\n\nAnd the services:\n\n```mermaid\n{ARCHITECTURE}\n```\n\nThe end.");
        let lines = [
            serde_json::json!({"type":"user","sessionId":"diagrams","message":{"role":"user","content":"Draw it"}}),
            serde_json::json!({"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":text}]}}),
        ];
        let transcript: String = lines.iter().map(|line| format!("{line}\n")).collect();
        let history = super::super::session::read(std::io::Cursor::new(transcript), 20);
        let mut tab = ThreadTab::new(1, None, String::new(), "/tmp".into(), String::new());
        tab.entries = Some(history.entries);
        tab
    }

    #[test]
    fn draws_diagrams_whose_text_selects_and_copies() {
        let mut harness = Harness::new(diagram_tab());
        harness.config.thread_paper = false;
        harness.snapshot("diagrams");
        harness.tab.select_all();
        let all = harness.tab.selected_text().unwrap();
        assert!(all.contains(&format!("```mermaid\n{FLOWCHART}\n```")), "{all}");
        assert!(all.contains(&format!("```mermaid\n{ARCHITECTURE}\n```")), "{all}");
        assert!(!all.contains("mermaid: "), "a diagram failed: {all}");
        harness.snapshot("diagrams-all");
        harness.config.app_theme = crate::config::AppTheme::Light;
        harness.snapshot("diagrams-light");
        // A drag across the flowchart's first label selects it as text
        harness.tab.selection = None;
        harness.drag(Point::new(380.0, 208.0), Point::new(528.0, 208.0));
        let part = harness.tab.selected_text().unwrap_or_default();
        harness.snapshot("diagrams-selected");
        assert_eq!(part, "Parse markdown");
        harness.config.app_theme = crate::config::AppTheme::Dark;
        harness.config.thread_paper = true;
        harness.snapshot("diagrams-paper");
    }

    #[test]
    fn draws_a_diagram_window_with_its_source() {
        use super::super::viewer::{Action, ImageWindow, view_with};
        let mut harness = Harness::new(diagram_tab());
        let id = cosmic::iced::window::Id::unique();
        let mut window = ImageWindow::diagram(ARCHITECTURE.to_string(), None);
        // Every control, as the app gives each a message
        let on = move |_: Action| Some(Message::ImageZoom(id, super::super::viewer::Zoom::Fit));
        harness.draw(view_with(&window, on), "diagram-window", Size::new(900.0, 700.0));
        let _ = window.act(id, Action::Source);
        harness.draw(view_with(&window, on), "diagram-window-source", Size::new(900.0, 700.0));
        let _ = window.act(id, Action::Source);
        // At its own size; the window's colours are the app's theme's
        window.zoom(super::super::viewer::Zoom::Actual);
        let frame = harness.draw(view_with(&window, on), "diagram-window-actual", Size::new(900.0, 700.0));
        let first = &frame[..4];
        assert!(frame.chunks(4).filter(|px| *px != first).count() > 10_000);
    }

    #[test]
    fn hidden_thinking_and_tools_are_not_copied() {
        let mut harness = harness();
        harness.tab.select_all();
        let all = harness.tab.selected_text().unwrap();
        assert!(all.contains("1.7724538509055159") && all.contains("polar-coordinates proof"), "{all}");
        harness.config.thread_show_thinking = false;
        harness.config.thread_show_tools = false;
        harness.tab.shown = Shown { thinking: false, tools: false };
        harness.snapshot("hidden-details");
        let shown = harness.tab.selected_text().unwrap();
        assert!(shown.contains("The numeric check agrees"), "{shown}");
        assert!(!shown.contains("1.7724538509055159") && !shown.contains("polar-coordinates proof"), "{shown}");
    }

    #[test]
    fn asks_the_agents_questions() {
        use super::super::model::{Event, PermissionRequest, Question};
        let mut harness = harness();
        let question = |key: &str, options: &[&str], free: bool| Question {
            key: key.into(),
            header: "Store".into(),
            question: format!("{key}?"),
            options: options.iter().map(|o| (o.to_string(), String::new())).collect(),
            free,
        };
        harness.tab.entries.as_mut().unwrap().apply(Event::Permission(PermissionRequest {
            request_id: "q".into(),
            tool_name: "AskUserQuestion".into(),
            tool_use_id: String::new(),
            title: String::new(),
            input: serde_json::Value::Null,
            suggestions: serde_json::Value::Null,
            always_allowed: false,
            questions: vec![question("Which database", &["SQLite", "Postgres"], true), question("Its name", &[], true)],
        }));
        let _ = harness.tab.update(Msg::Choose("q".into(), "Which database".into(), "SQLite".into()), &mut Vec::new());
        harness.snapshot("questions");
        // The composer answers the question left, and both go
        harness.tab.composer = cosmic::widget::text_editor::Content::with_text("inventory");
        let _ = harness.tab.update(Msg::Send, &mut Vec::new());
        let entries = harness.tab.entries.as_ref().unwrap();
        assert!(entries.permissions.is_empty());
        assert!(harness.tab.answers.is_empty());
        assert!(harness.tab.composer.text().trim().is_empty());
    }

    #[test]
    fn draws_an_image_window() {
        let tab = demo_tab();
        let path = std::path::PathBuf::from(&tab.cwd).join("plot.png");
        let mut harness = Harness::new(tab);
        let mut window = super::super::viewer::ImageWindow::new(path);
        assert_eq!(window.size, Some((240, 120)));
        for (name, zoom) in [("image-fit", None), ("image-zoomed", Some(super::super::viewer::Zoom::In))] {
            if let Some(zoom) = zoom {
                window.zoom(zoom);
                window.zoom(super::super::viewer::Zoom::In);
            }
            let id = cosmic::iced::window::Id::unique();
            let element = super::super::viewer::view_with(&window, move |action| Some(crate::Message::Viewer(id, action)));
            harness.draw(element, name, Size::new(700.0, 420.0));
        }
    }

    #[test]
    fn toggling_a_tool_opens_it() {
        let mut harness = harness();
        let bash = harness
            .tab
            .entries
            .as_ref()
            .unwrap()
            .items
            .iter()
            .find(|item| matches!(&item.kind, super::super::model::ItemKind::Tool(t) if t.name == "Bash"))
            .unwrap()
            .id;
        let _ = harness.tab.update(Msg::Toggle(bash), &mut Vec::new());
        assert!(harness.tab.expanded.contains(&bash));
        harness.snapshot("tool-open");
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// How long laying out a long answer takes, as streaming does on every
    /// burst of text
    #[test]
    #[ignore]
    fn time_layout() {
        let mut harness = Harness::new(demo_tab());
        // A long answer: the demo's, ten times over
        let text = {
            let entries = harness.tab.entries.as_ref().unwrap();
            entries
                .items
                .iter()
                .find_map(|item| match &item.kind {
                    super::super::model::ItemKind::Text { text, .. } if text.len() > 500 => Some(text.clone()),
                    _ => None,
                })
                .unwrap()
                .repeat(10)
        };
        eprintln!("answer of {} bytes", text.len());
        let start = std::time::Instant::now();
        let doc = super::super::markdown::parse(&text);
        eprintln!("parse: {:?}", start.elapsed());
        let entries = harness.tab.entries.as_mut().unwrap();
        entries.items.retain(|item| !matches!(item.kind, super::super::model::ItemKind::Text { .. }));
        entries.push(super::super::model::ItemKind::Text { text, doc: Arc::new(doc) });
        harness.snapshot("long-first");
        let start = std::time::Instant::now();
        for _ in 0..5 {
            // A new document each time, as a streamed delta makes
            let entries = harness.tab.entries.as_mut().unwrap();
            if let Some(super::super::model::ItemKind::Text { text, doc }) = entries.items.last_mut().map(|i| &mut i.kind) {
                text.push('x');
                *doc = Arc::new(super::super::markdown::parse(text));
            }
            harness.events(&[Event::Mouse(mouse::Event::CursorMoved { position: Point::new(1.0, 1.0) })]);
        }
        eprintln!("relayout, each: {:?}", start.elapsed() / 5);
        let start = std::time::Instant::now();
        for _ in 0..5 {
            harness.snapshot("long");
        }
        eprintln!("unchanged: layout and software draw, each: {:?}", start.elapsed() / 5);
    }
}

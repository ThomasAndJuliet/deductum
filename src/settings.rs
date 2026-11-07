// SPDX-License-Identifier: GPL-3.0-only

//! The human's settings, kept between runs in `~/.config/deductum/settings.json`.
//!
//! One typed, versioned record. Every field has a default, so a file written by an older or
//! newer app still loads: what it lacks takes its default, and what it has that this app does not
//! know is ignored. Version 0 is the flat file of the first previews (`vim`, `zoom`, sidebar
//! widths, `goal_beside` at the top level); [`Settings::from_json`] moves those fields into their
//! sections.
//!
//! The app holds the settings (`App::shell.settings`) and writes them at every change, so a
//! change applies at once and persists. Code far from the app (the editors' key bindings, the
//! goal panel, the Lean search) reads the copy published here with [`publish`], through the
//! accessors at the end of this module, rather than having the settings passed down to it.
//!
//! Some settings live elsewhere while the app runs and are only kept here: the zoom and whether
//! the goal goes beside the text are the view's (`view::zoom`, `view::goal_beside_on`), and vim,
//! the sidebar widths and whether the sidebars show are fields of the app. `App::save_settings`
//! gathers them before writing.

use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version this app writes
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    pub display: Display,
    pub layout: Layout,
    pub editing: Editing,
    pub proof_state: ProofState,
    pub llm: Llm,
    pub lean: Lean,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeChoice,
    /// The text zoom (1.0 is 100%), also set with Ctrl+= and Ctrl+-
    pub zoom: f32,
    /// The goal beside the text when the window has room (else under the proof, the page centred)
    pub goal_beside: bool,
}

/// Reading controls shared in vocabulary with Cerea and Terminal's threads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    pub font: TextFont,
    pub text_size: f32,
    pub line_height: f32,
    /// Maximum reading width in ems; zero keeps the automatic proof layout.
    pub measure: f32,
    pub paragraph_spacing: f32,
    pub math_scale: f32,
    pub justify: bool,
    pub centred: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TextFont {
    Libertinus,
    Serif,
    Sans,
    #[default]
    #[serde(other)]
    NewComputerModern,
}

impl TextFont {
    pub const ALL: [Self; 4] = [Self::Libertinus, Self::NewComputerModern, Self::Serif, Self::Sans];

    pub fn family(self) -> &'static str {
        match self {
            Self::NewComputerModern => "NewComputerModern",
            Self::Libertinus => "Libertinus Serif",
            Self::Serif => "serif",
            Self::Sans => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NewComputerModern => "New Computer Modern",
            Self::Libertinus => "Libertinus Serif",
            Self::Serif => "System serif",
            Self::Sans => "Interface sans-serif",
        }
    }

    pub fn editor_font(self) -> cosmic::iced::Font {
        use cosmic::iced::font::Family;
        cosmic::iced::Font {
            family: match self {
                Self::Sans => Family::SansSerif,
                Self::Serif => Family::Serif,
                _ => Family::Name(self.family()),
            },
            ..cosmic::font::default()
        }
    }
}

impl Default for Display {
    fn default() -> Self {
        Self { font: TextFont::default(), text_size: 17.0, line_height: 1.5,
            measure: 0.0, paragraph_spacing: 0.7, math_scale: 1.1,
            justify: false, centred: true }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DisplayChange {
    Font(TextFont),
    TextSize(f32),
    LineHeight(f32),
    Measure(f32),
    ParagraphSpacing(f32),
    MathScale(f32),
    Justify(bool),
    Centred(bool),
}

impl Display {
    pub fn update(&mut self, change: DisplayChange) {
        match change {
            DisplayChange::Font(v) => self.font = v,
            DisplayChange::TextSize(v) => self.text_size = v,
            DisplayChange::LineHeight(v) => self.line_height = v,
            DisplayChange::Measure(v) => self.measure = v,
            DisplayChange::ParagraphSpacing(v) => self.paragraph_spacing = v,
            DisplayChange::MathScale(v) => self.math_scale = v,
            DisplayChange::Justify(v) => self.justify = v,
            DisplayChange::Centred(v) => self.centred = v,
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        fn finite(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
            if value.is_finite() { value.clamp(min, max) } else { fallback }
        }
        self.text_size = finite(self.text_size, 17.0, 10.0, 36.0);
        self.line_height = finite(self.line_height, 1.5, 1.0, 2.5);
        self.measure = if self.measure == 0.0 { 0.0 } else { finite(self.measure, 0.0, 24.0, 200.0) };
        self.paragraph_spacing = finite(self.paragraph_spacing, 0.7, 0.2, 2.0);
        self.math_scale = finite(self.math_scale, 1.1, 0.75, 2.0);
    }
}

/// Where the sidebars were left
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub left_width: f32,
    pub right_width: f32,
    pub show_left: bool,
    pub show_right: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Editing {
    pub vim: bool,
    /// Spaces that Tab inserts and Shift+Tab takes away
    pub indent_width: u8,
    /// Offer completions (words, steps from the goal) as you type; `\` symbols and `@names` are
    /// offered either way, since they are how those are typed
    pub complete_while_typing: bool,
    /// Lines reading `claude`, `codex` or `@name` ask for a proof
    pub keywords: bool,
    /// How long typing must pause on such a line before it asks (Enter asks at once)
    pub keyword_pause_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProofState {
    /// The Types switch's position in a newly opened session
    pub show_types: bool,
    /// Say what the step at the caret changed (subgoals solved and new)
    pub show_changes: bool,
}

/// LLM help for keyword lines. Where their queries go is the session's own setting (the host's
/// `query_route`), shown with these
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Llm {
    /// The model `claude` asks when the line names none (empty: Claude's own default)
    pub claude_model: String,
    /// The model `codex` asks when the line names none (empty: Codex's own default)
    pub codex_model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lean {
    /// Where Lean's Search looks names up (design C17e)
    pub loogle: Loogle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// The desktop's theme, following it as it changes
    #[default]
    System,
    Light,
    Dark,
}

/// Loogle, Lean's search by name and type pattern: not used, a copy run on the session's machine,
/// or the public service at loogle.lean-lang.org
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loogle {
    Off,
    Local,
    #[default]
    Public,
}

impl Loogle {
    pub const ALL: [Loogle; 3] = [Loogle::Off, Loogle::Local, Loogle::Public];

    /// As the host and the settings file name it
    pub fn as_str(self) -> &'static str {
        match self {
            Loogle::Off => "off",
            Loogle::Local => "local",
            Loogle::Public => "public",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Loogle::Off => "Off",
            Loogle::Local => "Local",
            Loogle::Public => "Public (loogle.lean-lang.org)",
        }
    }
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Match desktop",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    /// The theme to apply. A desktop that never saved one of its themes (or saved only some of its
    /// fields) gives libcosmic's dark defaults for what is missing, so the light theme would then
    /// be dark: COSMIC's own stands in for it, as COSMIC Terminal does
    pub fn theme(self) -> cosmic::Theme {
        use cosmic::theme;
        match self {
            ThemeChoice::System => theme::system_preference(),
            ThemeChoice::Dark => {
                let mut t = theme::system_dark();
                if !t.cosmic().is_dark {
                    return theme::Theme::dark();
                }
                t.theme_type.prefer_dark(Some(true));
                t
            }
            ThemeChoice::Light => {
                let mut t = theme::system_light();
                if t.cosmic().is_dark {
                    return theme::Theme::light();
                }
                t.theme_type.prefer_dark(Some(false));
                t
            }
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            appearance: Appearance::default(),
            display: Display::default(),
            layout: Layout::default(),
            editing: Editing::default(),
            proof_state: ProofState::default(),
            llm: Llm::default(),
            lean: Lean::default(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: ThemeChoice::System, zoom: 1.0, goal_beside: true }
    }
}

impl Default for Layout {
    fn default() -> Self {
        // (both sidebars start hidden, so a first window is the document alone)
        Self { left_width: 290.0, right_width: 420.0, show_left: false, show_right: false }
    }
}

impl Default for Editing {
    fn default() -> Self {
        Self { vim: false, indent_width: 2, complete_while_typing: true, keywords: true, keyword_pause_ms: 2500 }
    }
}

impl Default for ProofState {
    fn default() -> Self {
        Self { show_types: false, show_changes: true }
    }
}

impl Settings {
    /// Settings from a file's JSON, of any version. Version 0 (no `version` field) kept its few
    /// fields at the top level; they move into their sections here. A field that does not parse
    /// takes its default rather than losing the whole file
    pub fn from_json(v: &Value) -> Settings {
        let mut v = v.clone();
        if v.get("version").is_none() && v.is_object() {
            v = migrate_v0(&v);
        }
        match serde_json::from_value::<Settings>(v.clone()) {
            Ok(mut s) => {
                s.version = VERSION;
                s.clamp();
                s
            }
            Err(e) => {
                eprintln!("deductum: the settings do not parse as a whole ({e}); reading them section by section");
                let mut s = Settings::default();
                fn part<T: serde::de::DeserializeOwned + Default>(v: &Value, key: &str) -> T {
                    v.get(key).and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or_default()
                }
                s.appearance = part(&v, "appearance");
                s.display = part(&v, "display");
                s.layout = part(&v, "layout");
                s.editing = part(&v, "editing");
                s.proof_state = part(&v, "proof_state");
                s.llm = part(&v, "llm");
                s.lean = part(&v, "lean");
                s.clamp();
                s
            }
        }
    }

    /// Values a hand-edited file may have put out of range
    fn clamp(&mut self) {
        self.display.clamp();
        self.appearance.zoom = if self.appearance.zoom.is_finite() { self.appearance.zoom.clamp(0.5, 3.0) } else { 1.0 };
        self.editing.indent_width = self.editing.indent_width.clamp(1, 8);
        self.editing.keyword_pause_ms = self.editing.keyword_pause_ms.clamp(250, 30_000);
        self.layout.left_width = self.layout.left_width.clamp(180.0, 1200.0);
        self.layout.right_width = self.layout.right_width.clamp(260.0, 1600.0);
    }

    /// The settings on disk, or the defaults
    pub fn load() -> Settings {
        let Ok(text) = std::fs::read_to_string(path()) else { return Settings::default() };
        match serde_json::from_str::<Value>(&text) {
            Ok(v) => Settings::from_json(&v),
            Err(e) => {
                eprintln!("deductum: {} is not JSON ({e}); the defaults stand in", path().display());
                Settings::default()
            }
        }
    }

    /// Write the settings (to a temporary file, renamed over the old one, so a crash never leaves
    /// half a file)
    pub fn save(&self) {
        let path = path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// How long typing pauses on a keyword line before it asks
    pub fn keyword_pause(&self) -> Duration {
        Duration::from_millis(u64::from(self.editing.keyword_pause_ms))
    }

    /// What the host is told for this app's keyword lines and searches (`client_settings`)
    pub fn host_json(&self) -> Value {
        let model = |m: &str| if m.trim().is_empty() { Value::Null } else { Value::String(m.trim().to_string()) };
        serde_json::json!({
            "keywords": self.editing.keywords,
            "models": {"claude": model(&self.llm.claude_model), "codex": model(&self.llm.codex_model)},
            "loogle": self.lean.loogle.as_str(),
        })
    }
}

/// The first previews' flat file, in this version's sections
fn migrate_v0(v: &Value) -> Value {
    let mut out = serde_json::json!({"version": VERSION});
    let mut put = |section: &str, key: &str, from: &str| {
        if let Some(x) = v.get(from).filter(|x| !x.is_null()) {
            out[section][key] = x.clone();
        }
    };
    put("editing", "vim", "vim");
    put("appearance", "zoom", "zoom");
    put("appearance", "goal_beside", "goal_beside");
    put("layout", "left_width", "left_width");
    put("layout", "right_width", "right_width");
    put("layout", "show_left", "show_left");
    put("layout", "show_right", "show_right");
    out
}

/// Where the settings are kept: `$DEDUCTUM_CONFIG_DIR/settings.json` if that is set, else
/// `~/.config/deductum/settings.json`. Tests keep theirs in a directory of their own, so that
/// running them never changes the human's settings
pub fn path() -> PathBuf {
    if let Some(dir) = std::env::var_os("DEDUCTUM_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("settings.json");
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("deductum-test-config-{}", std::process::id())).join("settings.json");
    }
    super::platform::config_dir().join("settings.json")
}

// ----- the published copy

static CURRENT: LazyLock<RwLock<Settings>> = LazyLock::new(|| RwLock::new(Settings::default()));

/// Make these the settings that code away from the app reads
pub fn publish(settings: &Settings) {
    if let Ok(mut current) = CURRENT.write() {
        *current = settings.clone();
    }
}

fn read<T>(f: impl FnOnce(&Settings) -> T) -> T {
    match CURRENT.read() {
        Ok(s) => f(&s),
        Err(e) => f(&e.into_inner()),
    }
}

/// Where Lean's Search looks names up (Off, Local or Public): read by the Lean search panel
#[allow(dead_code)]
pub fn loogle() -> Loogle {
    read(|s| s.lean.loogle)
}

/// The spaces Tab inserts and Shift+Tab takes away
pub fn indent_width() -> usize {
    read(|s| usize::from(s.editing.indent_width.max(1)))
}

/// Whether completions other than `\` symbols and `@names` are offered as you type
pub fn complete_while_typing() -> bool {
    read(|s| s.editing.complete_while_typing)
}

/// Whether the goal panel says what the step at the caret changed
pub fn show_changes() -> bool {
    read(|s| s.proof_state.show_changes)
}

pub fn display() -> Display {
    read(|s| s.display.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_previews_flat_file_moves_into_sections() {
        let old = json!({"vim": true, "zoom": 1.21, "left_width": 333.0, "right_width": 500.0,
                         "show_left": true, "show_right": null, "goal_beside": false});
        let s = Settings::from_json(&old);
        assert_eq!(s.version, VERSION);
        assert!(s.editing.vim);
        assert_eq!(s.appearance.zoom, 1.21);
        assert!(!s.appearance.goal_beside);
        assert_eq!((s.layout.left_width, s.layout.right_width), (333.0, 500.0));
        assert!(s.layout.show_left);
        assert!(!s.layout.show_right, "a null takes the default");
        // what the old file did not have takes its default
        assert_eq!(s.lean.loogle, Loogle::Public);
        assert_eq!(s.editing.keyword_pause_ms, 2500);
        assert!(s.editing.keywords && s.editing.complete_while_typing && s.proof_state.show_changes);
    }

    #[test]
    fn a_file_round_trips_and_a_bad_field_loses_only_its_section() {
        let mut s = Settings::default();
        s.appearance.theme = ThemeChoice::Light;
        s.lean.loogle = Loogle::Local;
        s.llm.claude_model = "opus".into();
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["lean"]["loogle"], "local");
        assert_eq!(v["appearance"]["theme"], "light");
        assert_eq!(Settings::from_json(&v), s);
        let mut broken = v.clone();
        broken["lean"]["loogle"] = json!("everywhere");
        let read = Settings::from_json(&broken);
        assert_eq!(read.lean.loogle, Loogle::Public, "the broken section takes its default");
        assert_eq!(read.appearance.theme, ThemeChoice::Light, "the other sections are kept");
        assert_eq!(read.llm.claude_model, "opus");
    }

    #[test]
    fn out_of_range_values_are_brought_back() {
        let s = Settings::from_json(&json!({"version": 1, "appearance": {"zoom": 40.0}, "editing": {"indent_width": 0, "keyword_pause_ms": 5}}));
        assert_eq!(s.appearance.zoom, 3.0);
        assert_eq!(s.editing.indent_width, 1);
        assert_eq!(s.editing.keyword_pause_ms, 250);
    }

    #[test]
    fn the_host_hears_the_models_only_when_set() {
        let mut s = Settings::default();
        assert_eq!(s.host_json()["models"]["claude"], Value::Null);
        s.llm.codex_model = " astra ".into();
        s.editing.keywords = false;
        let h = s.host_json();
        assert_eq!(h["models"]["codex"], "astra");
        assert_eq!(h["keywords"], false);
        assert_eq!(h["loogle"], "public");
    }

    #[test]
    fn display_choices_round_trip_and_old_settings_keep_the_existing_look() {
        let old = Settings::from_json(&json!({"version": 1, "appearance": {"zoom": 1.2}}));
        assert_eq!(old.display, Display::default());
        for font in TextFont::ALL {
            let mut settings = old.clone();
            settings.display = Display { font, text_size: 20.0, line_height: 1.6,
                measure: 40.0, paragraph_spacing: 0.9, math_scale: 1.3,
                justify: true, centred: false };
            assert_eq!(Settings::from_json(&serde_json::to_value(&settings).unwrap()), settings);
        }
        assert_eq!(TextFont::Sans.family(), "", "Terminal's normal threads use the generic sans family");
        assert_eq!(TextFont::Sans.editor_font().family, cosmic::iced::font::Family::SansSerif);
        assert_eq!(TextFont::Serif.editor_font().family, cosmic::iced::font::Family::Serif);
    }

    #[test]
    fn display_updates_preserve_other_choices_and_bound_layout_values() {
        let mut display = Display::default();
        display.update(DisplayChange::Font(TextFont::Sans));
        display.update(DisplayChange::TextSize(1000.0));
        display.update(DisplayChange::LineHeight(f32::NAN));
        display.update(DisplayChange::Measure(40.0));
        display.update(DisplayChange::Justify(true));
        assert_eq!(display.font, TextFont::Sans);
        assert_eq!((display.text_size, display.line_height, display.measure), (36.0, 1.5, 40.0));
        assert!(display.justify);
        let future = Settings::from_json(&json!({"version": 1,
            "appearance": {"zoom": 1.2}, "display": {"font": "FutureFamily", "line_height": 1.6}}));
        assert_eq!(future.display.font, TextFont::default());
        assert_eq!(future.display.line_height, 1.6);
        assert_eq!(future.appearance.zoom, 1.2);
    }
}

// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry},
    theme,
};
use cosmic_text::{Metrics, Stretch, Weight};
use hex_color::HexColor;
use serde::{Deserialize, Serialize};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::{fl, localize::LANGUAGE_SORTER, shortcuts::Shortcuts};

pub const CONFIG_VERSION: u64 = 1;
pub const COSMIC_THEME_DARK: &str = "COSMIC Dark";
pub const COSMIC_THEME_LIGHT: &str = "COSMIC Light";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum AppTheme {
    Dark,
    Light,
    System,
}

impl AppTheme {
    pub fn theme(&self) -> theme::Theme {
        // A desktop that never saved one of its themes, or saved only some
        // of its fields, gives libcosmic's dark defaults for what is missing:
        // the light theme would then be dark. COSMIC's own stands in for it
        match self {
            Self::Dark => {
                let mut t = theme::system_dark();
                if !t.cosmic().is_dark {
                    return theme::Theme::dark();
                }
                t.theme_type.prefer_dark(Some(true));
                t
            }
            Self::Light => {
                let mut t = theme::system_light();
                if t.cosmic().is_dark {
                    return theme::Theme::light();
                }
                t.theme_type.prefer_dark(Some(false));
                t
            }
            Self::System => theme::system_preference(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ColorSchemeKind {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ColorSchemeId(pub u64);

//TODO: there is a lot of extra code to keep the exported color scheme clean,
//consider how to reduce this
fn de_color_opt<'de, D>(deserializer: D) -> Result<Option<HexColor>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let hex_color: HexColor = Deserialize::deserialize(deserializer)?;
    Ok(Some(hex_color))
}

fn ser_color_opt<S>(hex_color_opt: &Option<HexColor>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::Error as _;
    match hex_color_opt {
        Some(hex_color) => Serialize::serialize(hex_color, serializer),
        None => Err(S::Error::custom("ser_color_opt called with None")),
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorSchemeAnsi {
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub black: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub red: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub green: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub yellow: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub blue: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub magenta: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub cyan: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub white: Option<HexColor>,
}

impl ColorSchemeAnsi {
    pub fn is_empty(&self) -> bool {
        self.black.is_none()
            && self.red.is_none()
            && self.green.is_none()
            && self.yellow.is_none()
            && self.blue.is_none()
            && self.magenta.is_none()
            && self.cyan.is_none()
            && self.white.is_none()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorScheme {
    pub name: String,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub foreground: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub background: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursor: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub bright_foreground: Option<HexColor>,
    #[serde(
        deserialize_with = "de_color_opt",
        serialize_with = "ser_color_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub dim_foreground: Option<HexColor>,
    #[serde(skip_serializing_if = "ColorSchemeAnsi::is_empty")]
    pub normal: ColorSchemeAnsi,
    #[serde(skip_serializing_if = "ColorSchemeAnsi::is_empty")]
    pub bright: ColorSchemeAnsi,
    #[serde(skip_serializing_if = "ColorSchemeAnsi::is_empty")]
    pub dim: ColorSchemeAnsi,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProfileId(pub u64);

/// What a profile runs
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Agent {
    /// The profile's command line, or the user's shell
    #[default]
    Shell,
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Self; 3] = [Self::Shell, Self::Claude, Self::Codex];

    /// The CLI to run, or `None` for a shell profile
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::Shell => None,
            Self::Claude => Some("claude"),
            Self::Codex => Some("codex"),
        }
    }

    /// The word naming this kind in session names
    pub fn session_kind(self) -> &'static str {
        self.program().unwrap_or("shell")
    }

    /// Claude's terracotta, Codex's green; plain terminals are unmarked
    pub fn default_color(self) -> Option<TabColor> {
        match self {
            Self::Shell => None,
            Self::Claude => TabColor::parse(TabColor::PALETTE[0].1),
            Self::Codex => TabColor::parse(TabColor::PALETTE[1].1),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Shell => fl!("agent-shell"),
            Self::Claude => "Claude".to_string(),
            Self::Codex => "Codex".to_string(),
        }
    }
}

/// Identifies a machine in [`Config::machines`]
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct MachineId(pub u64);

/// Another computer the terminal works with, over ssh
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Machine {
    pub name: String,
    /// What ssh connects to: an alias from `~/.ssh/config`, a host name or
    /// an address
    pub host: String,
    /// The account to log in as, or empty for ssh's choice: the alias's
    /// `User`, else this computer's user name
    #[serde(default)]
    pub user: String,
    /// What the last check found there
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub home: String,
    /// Its id in its project registry, which tells computers apart whatever
    /// they are called; empty until a transfer has made one
    #[serde(default)]
    pub host_id: String,
}

impl Machine {
    /// The ssh destination: `user@host`, or `host` alone
    pub fn destination(&self) -> String {
        let (user, host) = (self.user.trim(), self.host.trim());
        if user.is_empty() {
            host.to_string()
        } else {
            format!("{user}@{host}")
        }
    }

    /// A machine for an ssh destination, named after its host
    pub fn from_destination(destination: &str) -> Self {
        let destination = destination.trim();
        let (user, host) = destination
            .rsplit_once('@')
            .unwrap_or(("", destination));
        Self {
            name: host.split('.').next().unwrap_or(host).to_string(),
            host: host.to_string(),
            user: user.to_string(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub syntax_theme_dark: String,
    #[serde(default)]
    pub syntax_theme_light: String,
    #[serde(default)]
    pub tab_title: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub drain_on_exit: bool,
    #[serde(default)]
    pub agent: Agent,
    /// An ssh destination, or empty to run locally. Kept equal to the
    /// destination of `machine` when that is set
    #[serde(default)]
    pub host: String,
    /// The machine it runs on, or `None` for this computer
    #[serde(default)]
    pub machine: Option<MachineId>,
    /// Extra arguments for the agent CLI
    #[serde(default)]
    pub args: String,
    /// Tab colour as `#rrggbb`, or empty for the agent's default
    #[serde(default)]
    pub color: String,
    /// A Claude profile's conversations open as rich-text threads, not in a
    /// terminal
    #[serde(default)]
    pub thread: bool,
}

impl Profile {
    pub fn agent(name: &str, agent: Agent, host: &str) -> Self {
        Self {
            name: name.to_string(),
            agent,
            host: host.to_string(),
            ..Self::default()
        }
    }

    pub fn is_remote(&self) -> bool {
        !self.host.trim().is_empty()
    }

    /// The colour tabs of this profile are marked with, if any
    pub fn tab_color(&self) -> Option<TabColor> {
        TabColor::parse(&self.color).or_else(|| self.agent.default_color())
    }
}

/// A section of a launcher tab
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum LauncherSection {
    Running,
    Projects,
    Conversations,
}

/// An `#rrggbb` colour that marks a tab
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TabColor(pub [u8; 3]);

impl TabColor {
    /// Colours offered in the profile editor, with their names
    pub const PALETTE: [(&'static str, &'static str); 9] = [
        ("Orange", "#d97757"),
        ("Green", "#10a37f"),
        ("Blue", "#3584e4"),
        ("Purple", "#9141ac"),
        ("Pink", "#d6569e"),
        ("Red", "#e01b24"),
        ("Yellow", "#e5a50a"),
        ("Teal", "#2190a4"),
        ("Grey", "#77767b"),
    ];

    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.trim().strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        Some(Self([byte(0)?, byte(2)?, byte(4)?]))
    }

    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: fl!("new-profile"),
            command: String::new(),
            syntax_theme_dark: COSMIC_THEME_DARK.to_string(),
            syntax_theme_light: COSMIC_THEME_LIGHT.to_string(),
            tab_title: String::new(),
            working_directory: String::new(),
            drain_on_exit: false,
            agent: Agent::Shell,
            host: String::new(),
            machine: None,
            args: String::new(),
            color: String::new(),
            thread: false,
        }
    }
}

#[derive(Clone, CosmicConfigEntry, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Config {
    pub app_theme: AppTheme,
    pub color_schemes_dark: BTreeMap<ColorSchemeId, ColorScheme>,
    pub color_schemes_light: BTreeMap<ColorSchemeId, ColorScheme>,
    pub font_name: String,
    pub font_size: u16,
    pub font_weight: u16,
    pub dim_font_weight: u16,
    pub bold_font_weight: u16,
    pub font_stretch: u16,
    pub font_size_zoom_step_mul_100: u16,
    pub opacity: u8,
    pub profiles: BTreeMap<ProfileId, Profile>,
    pub show_headerbar: bool,
    pub show_pane_borders: bool,
    pub use_bright_bold: bool,
    pub syntax_theme_dark: String,
    pub syntax_theme_light: String,
    pub focus_follow_mouse: bool,
    #[serde(default)]
    pub tab_new_inherit_working_directory: bool,
    pub default_profile: Option<ProfileId>,
    #[serde(default)]
    pub shortcuts_custom: Shortcuts,
    /// Whether the built-in agent profiles have been added, so that deleting
    /// one does not bring it back
    #[serde(default)]
    pub agent_profiles_seeded: bool,
    /// How many rounds of built-in profiles have been added; see
    /// `agent::seed_profiles`
    #[serde(default)]
    pub profile_seed_version: u32,
    /// Launcher sections the user has collapsed
    #[serde(default)]
    pub launcher_collapsed: BTreeSet<LauncherSection>,
    /// Other computers, for profiles to run on and sessions to move between
    #[serde(default)]
    pub machines: BTreeMap<MachineId, Machine>,
    /// Whether the sidebar of running sessions is shown
    #[serde(default)]
    pub show_sidebar: bool,
    /// Whether the Secrets pane is shown, on the right
    #[serde(default)]
    pub show_secrets: bool,
    /// Whether the sidebar shows one line per session, its preview on hover
    #[serde(default)]
    pub sidebar_compact: bool,
    /// Whether the accent colour is that of the tab in front, when it has one
    #[serde(default = "default_true")]
    pub accent_follows_tab: bool,
    /// Threads are set as a paper: serif text, justified, in a column
    #[serde(default = "default_true")]
    pub thread_paper: bool,
    /// Threads tell the agent the view's colours, for the images it draws
    /// to match them
    #[serde(default = "default_true")]
    pub thread_match_colours: bool,
    /// Threads show the agent's thinking among its replies; else only
    /// while it thinks, above the composer
    #[serde(default = "default_true")]
    pub thread_show_thinking: bool,
    /// Threads show each tool call among the replies; else only the one
    /// running, above the composer
    #[serde(default = "default_true")]
    pub thread_show_tools: bool,
    #[serde(default)]
    pub threads: ThreadSettings,
}

/// The font a paper is set in
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PaperFont {
    /// Typst's default
    #[default]
    Libertinus,
    /// LaTeX's Computer Modern, as New Computer Modern
    NewComputerModern,
    /// The system's serif
    Serif,
    /// The interface's sans-serif
    Sans,
}

impl PaperFont {
    pub const ALL: [Self; 4] = [Self::Libertinus, Self::NewComputerModern, Self::Serif, Self::Sans];

    /// The family name, or empty for the theme's sans-serif
    pub fn family(self) -> &'static str {
        match self {
            Self::Libertinus => "Libertinus Serif",
            Self::NewComputerModern => "NewComputerModern",
            Self::Serif => "serif",
            Self::Sans => "",
        }
    }
}

/// The colours of a paper
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum PaperTone {
    /// The theme's own, light or dark
    #[default]
    Theme,
    /// Black on white, whatever the theme
    White,
    /// Dark brown on cream
    Sepia,
}

impl PaperTone {
    pub const ALL: [Self; 3] = [Self::Theme, Self::White, Self::Sepia];
}

/// How threads are set, and how much of them is kept in memory
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ThreadSettings {
    /// Text size in normal mode, in pixels
    pub text_size: u16,
    pub paper_font: PaperFont,
    /// Text size of a paper, in pixels
    pub paper_size: u16,
    /// A paper's line height, in percent of its size
    pub paper_line_height: u16,
    /// A paper's text width, in ems
    pub paper_measure: u16,
    pub paper_justify: bool,
    pub paper_tone: PaperTone,
    /// Formulas' size against the text's, in percent
    pub math_scale: u16,
    /// Minutes an idle agent keeps running before it is stopped; its thread
    /// starts it again when needed
    pub idle_minutes: u16,
    /// Agents kept running at once, at most
    pub live_agents: u16,
    /// Threads kept in memory while not shown, at most
    pub loaded_threads: u16,
    /// User turns read back from a conversation at first
    pub history_turns: u16,
}

impl Default for ThreadSettings {
    fn default() -> Self {
        Self {
            text_size: 15,
            paper_font: PaperFont::default(),
            paper_size: 20,
            paper_line_height: 145,
            paper_measure: 48,
            paper_justify: true,
            paper_tone: PaperTone::default(),
            math_scale: 110,
            idle_minutes: 10,
            live_agents: 3,
            loaded_threads: 4,
            history_turns: 20,
        }
    }
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_theme: AppTheme::System,
            bold_font_weight: Weight::BOLD.0,
            color_schemes_dark: BTreeMap::new(),
            color_schemes_light: BTreeMap::new(),
            dim_font_weight: Weight::NORMAL.0,
            focus_follow_mouse: false,
            tab_new_inherit_working_directory: false,
            font_name: "Noto Sans Mono".to_string(),
            font_size: 14,
            font_size_zoom_step_mul_100: 100,
            font_stretch: Stretch::Normal.to_number(),
            font_weight: Weight::NORMAL.0,
            opacity: 100,
            profiles: BTreeMap::new(),
            show_headerbar: true,
            show_pane_borders: false,
            syntax_theme_dark: COSMIC_THEME_DARK.to_string(),
            syntax_theme_light: COSMIC_THEME_LIGHT.to_string(),
            use_bright_bold: false,
            default_profile: None,
            shortcuts_custom: Shortcuts::default(),
            agent_profiles_seeded: false,
            profile_seed_version: 0,
            launcher_collapsed: BTreeSet::new(),
            machines: BTreeMap::new(),
            show_sidebar: false,
            sidebar_compact: false,
            show_secrets: false,
            accent_follows_tab: true,
            thread_paper: true,
            thread_match_colours: true,
            thread_show_thinking: true,
            thread_show_tools: true,
            threads: ThreadSettings::default(),
        }
    }
}

impl Config {
    /// Give every remote profile a machine, adding one for each host that
    /// has none, and keep profiles' hosts equal to their machines'
    /// destinations. Returns whether anything changed
    pub fn adopt_machines(&mut self) -> bool {
        let mut changed = false;
        let ids: Vec<ProfileId> = self.profiles.keys().copied().collect();
        for id in ids {
            let profile = &self.profiles[&id];
            let machine = profile.machine.filter(|m| self.machines.contains_key(m));
            let machine = match machine {
                Some(machine) => Some(machine),
                None if profile.host.trim().is_empty() => None,
                None => {
                    let host = profile.host.trim().to_string();
                    Some(
                        self.machines
                            .iter()
                            .find(|(_, m)| m.destination() == host)
                            .map(|(id, _)| *id)
                            .unwrap_or_else(|| self.add_machine(Machine::from_destination(&host))),
                    )
                }
            };
            let host = machine.map(|m| self.machines[&m].destination()).unwrap_or_default();
            let profile = self.profiles.get_mut(&id).expect("listed above");
            if profile.machine != machine || profile.host != host {
                profile.machine = machine;
                profile.host = host;
                changed = true;
            }
        }
        changed
    }

    /// Add a machine, returning its id
    pub fn add_machine(&mut self, machine: Machine) -> MachineId {
        let id = self
            .machines
            .last_key_value()
            .map(|(id, _)| MachineId(id.0 + 1))
            .unwrap_or_default();
        self.machines.insert(id, machine);
        id
    }

    /// The machine reached by an ssh destination
    pub fn machine_for_host(&self, host: &str) -> Option<&Machine> {
        let host = host.trim();
        (!host.is_empty())
            .then(|| self.machines.values().find(|m| m.destination() == host))
            .flatten()
    }

    pub fn color_schemes(
        &self,
        color_scheme_kind: ColorSchemeKind,
    ) -> &BTreeMap<ColorSchemeId, ColorScheme> {
        match color_scheme_kind {
            ColorSchemeKind::Dark => &self.color_schemes_dark,
            ColorSchemeKind::Light => &self.color_schemes_light,
        }
    }

    pub fn color_schemes_mut(
        &mut self,
        color_scheme_kind: ColorSchemeKind,
    ) -> &mut BTreeMap<ColorSchemeId, ColorScheme> {
        match color_scheme_kind {
            ColorSchemeKind::Dark => &mut self.color_schemes_dark,
            ColorSchemeKind::Light => &mut self.color_schemes_light,
        }
    }

    pub fn color_scheme_kind(&self, system_theme: &theme::Theme) -> ColorSchemeKind {
        match self.app_theme {
            AppTheme::Dark => ColorSchemeKind::Dark,
            AppTheme::Light => ColorSchemeKind::Light,
            AppTheme::System => {
                if system_theme.theme_type.is_dark() {
                    ColorSchemeKind::Dark
                } else {
                    ColorSchemeKind::Light
                }
            }
        }
    }

    // Get a sorted and adjusted for duplicates list of color scheme names and ids
    pub fn color_scheme_names(
        &self,
        color_scheme_kind: ColorSchemeKind,
    ) -> Vec<(String, ColorSchemeId)> {
        let color_schemes = self.color_schemes(color_scheme_kind);
        let mut color_scheme_names =
            Vec::<(String, ColorSchemeId)>::with_capacity(color_schemes.len());
        for (color_scheme_id, color_scheme) in color_schemes {
            let mut name = color_scheme.name.clone();

            let mut copies = 1;
            while color_scheme_names.iter().any(|x| x.0 == name) {
                copies += 1;
                name = format!("{} ({})", color_scheme.name, copies);
            }

            color_scheme_names.push((name, *color_scheme_id));
        }
        color_scheme_names.sort_by(|a, b| LANGUAGE_SORTER.compare(&a.0, &b.0));
        color_scheme_names
    }

    fn font_size_adjusted(&self, zoom_adj: i8) -> f32 {
        let font_size = f32::from(self.font_size).max(1.0);
        let adj = f32::from(zoom_adj);
        let adj_step = f32::from(self.font_size_zoom_step_mul_100) / 100.0;
        (font_size + adj * adj_step).max(1.0)
    }

    // Calculate metrics from font size
    pub fn metrics(&self, zoom_adj: i8) -> Metrics {
        let font_size = self.font_size_adjusted(zoom_adj);
        let line_height = (font_size * 1.4).ceil();
        Metrics::new(font_size, line_height)
    }

    pub fn opacity_ratio(&self) -> f32 {
        f32::from(self.opacity) / 100.0
    }

    // Get a sorted and adjusted for duplicates list of profile names and ids
    pub fn profile_names(&self) -> Vec<(String, ProfileId)> {
        let mut profile_names = Vec::<(String, ProfileId)>::with_capacity(self.profiles.len());
        for (profile_id, profile) in &self.profiles {
            let mut name = profile.name.clone();

            let mut copies = 1;
            while profile_names.iter().any(|x| x.0 == name) {
                copies += 1;
                name = format!("{} ({})", profile.name, copies);
            }

            profile_names.push((name, *profile_id));
        }
        profile_names.sort_by(|a, b| LANGUAGE_SORTER.compare(&a.0, &b.0));
        profile_names
    }

    // Get current syntax theme based on dark mode
    pub fn syntax_theme(
        &self,
        color_scheme_kind: ColorSchemeKind,
        profile_id_opt: Option<ProfileId>,
    ) -> (String, ColorSchemeKind) {
        let theme_name = match profile_id_opt.and_then(|profile_id| self.profiles.get(&profile_id))
        {
            Some(profile) => match color_scheme_kind {
                ColorSchemeKind::Dark => profile.syntax_theme_dark.clone(),
                ColorSchemeKind::Light => profile.syntax_theme_light.clone(),
            },
            None => match color_scheme_kind {
                ColorSchemeKind::Dark => self.syntax_theme_dark.clone(),
                ColorSchemeKind::Light => self.syntax_theme_light.clone(),
            },
        };
        (theme_name, color_scheme_kind)
    }

    pub fn typed_font_stretch(&self) -> Stretch {
        macro_rules! populate_num_typed_map {
            ($($stretch:ident,)+) => {
                let mut map = BTreeMap::new();
                $(map.insert(Stretch::$stretch.to_number(), Stretch::$stretch);)+
                map
            };
        }

        static NUM_TO_TYPED_MAP: OnceLock<BTreeMap<u16, Stretch>> = OnceLock::new();

        NUM_TO_TYPED_MAP.get_or_init(|| {
            populate_num_typed_map! {
                UltraCondensed, ExtraCondensed, Condensed, SemiCondensed,
                Normal, SemiExpanded, Expanded, ExtraExpanded, UltraExpanded,
            }
        })[&self.font_stretch]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_become_machines_once_and_profiles_follow_them() {
        let mut config = Config::default();
        for (id, host) in [(0, ""), (1, "twright@neo"), (2, "twright@neo"), (3, "au-laptop")] {
            config
                .profiles
                .insert(ProfileId(id), Profile::agent("p", Agent::Claude, host));
        }
        assert!(config.adopt_machines());
        assert!(!config.adopt_machines());
        let names: Vec<(&str, String)> = config
            .machines
            .values()
            .map(|m| (m.name.as_str(), m.destination()))
            .collect();
        assert_eq!(names, [("neo", "twright@neo".to_string()), ("au-laptop", "au-laptop".to_string())]);
        assert_eq!(config.profiles[&ProfileId(0)].machine, None);
        assert_eq!(config.profiles[&ProfileId(1)].machine, config.profiles[&ProfileId(2)].machine);

        // A changed user name reaches every profile on that machine
        let neo = config.profiles[&ProfileId(1)].machine.unwrap();
        config.machines.get_mut(&neo).unwrap().user = "tom".into();
        assert!(config.adopt_machines());
        assert_eq!(config.profiles[&ProfileId(2)].host, "tom@neo");
        assert_eq!(config.machine_for_host("tom@neo").map(|m| m.name.as_str()), Some("neo"));
        assert!(config.machine_for_host("").is_none());
    }

    #[test]
    fn destinations_split_into_user_and_host() {
        let machine = Machine::from_destination(" au759518@d59500.tail57c02a.ts.net ");
        assert_eq!((machine.name.as_str(), machine.user.as_str()), ("d59500", "au759518"));
        assert_eq!(machine.destination(), "au759518@d59500.tail57c02a.ts.net");
        assert_eq!(Machine::from_destination("neo").destination(), "neo");
    }
}

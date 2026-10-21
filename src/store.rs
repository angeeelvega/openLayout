use openlayout::engine::{Action, Binding, CenterMode, ColumnMode, DeskMode, PinEdge, Profile};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackpadMode {
    Off,
    WithModifier,
    Horizontal,
}

impl Default for TrackpadMode {
    fn default() -> Self {
        Self::WithModifier
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppBehavior {
    Tile,
    Float,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    pub bundle_id: String,
    pub name: String,
    pub behavior: AppBehavior,
    pub width_ratio: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub gap: f64,
    pub outer_gap: f64,
    pub presets: Vec<f64>,
    pub center_mode: CenterMode,
    pub desk_mode: DeskMode,
    pub linked_scroll: bool,
    pub keep_original_height: bool,
    pub focus_border: bool,
    pub dim_inactive: bool,
    pub focus_follows_mouse: bool,
    pub mouse_follows_focus: bool,
    pub trackpad: TrackpadMode,
    pub launch_at_login: bool,
    pub tiling_enabled: bool,
    pub bindings: Vec<Binding>,
    pub rules: Vec<AppRule>,
    pub default_width: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            gap: 8.0,
            outer_gap: 8.0,
            presets: vec![0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 0.75],
            center_mode: CenterMode::WhenOverflow,
            desk_mode: DeskMode::Independent,
            linked_scroll: false,
            keep_original_height: false,
            focus_border: false,
            dim_inactive: false,
            focus_follows_mouse: false,
            mouse_follows_focus: false,
            trackpad: TrackpadMode::WithModifier,
            launch_at_login: false,
            tiling_enabled: false,
            bindings: Binding::defaults(),
            rules: default_rules(),
            default_width: 0.5,
        }
    }
}

fn default_rules() -> Vec<AppRule> {
    [
        ("com.apple.calculator", "Calculator"),
        ("com.apple.systempreferences", "System Preferences"),
        ("com.apple.SystemSettings", "System Settings"),
        ("com.apple.QuickLookUIService", "Quick Look"),
        ("com.apple.siri.parsec.HashtagImagesApp", "Siri"),
    ]
    .into_iter()
    .map(|(bundle_id, name)| AppRule {
        bundle_id: bundle_id.into(),
        name: name.into(),
        behavior: AppBehavior::Float,
        width_ratio: None,
    })
    .collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedColumn {
    pub bundle_ids: Vec<String>,
    pub titles: Vec<String>,
    pub width_ratio: f64,
    pub height_weights: Vec<f64>,
    pub mode: Option<ColumnMode>,
    pub pin: Option<PinEdge>,
    pub height_fill: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDisplay {
    pub display_id: u32,
    pub index: usize,
    pub columns: Vec<SavedColumn>,
    pub scroll: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDesk {
    pub displays: Vec<SavedDisplay>,
    pub unified: Vec<SavedColumn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedState {
    pub settings: Settings,
    pub profiles: Vec<Profile>,
    pub desk: SavedDesk,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            profiles: Vec::new(),
            desk: SavedDesk::default(),
        }
    }
}

pub fn state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let directory = PathBuf::from(home).join("Library/Application Support/openLayout");
    let _ = std::fs::create_dir_all(&directory);
    directory.join("state.json")
}

pub fn load() -> SavedState {
    let path = state_path();
    let Ok(bytes) = std::fs::read(&path) else {
        return SavedState::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(state: &SavedState) {
    let path = state_path();
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(path, bytes);
    }
}

pub fn preset_text(presets: &[f64]) -> String {
    presets
        .iter()
        .map(|value| format!("{}", (value * 100.0).round() as i32))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn parse_presets(text: &str) -> Vec<f64> {
    let values: Vec<f64> = text
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter_map(|part| part.trim().parse::<f64>().ok())
        .map(|value| if value > 1.5 { value / 100.0 } else { value })
        .filter(|value| *value >= 0.1 && *value <= 1.0)
        .collect();
    if values.is_empty() {
        Settings::default().presets
    } else {
        values
    }
}

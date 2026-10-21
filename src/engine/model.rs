use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::geom::{Rect, WindowId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeskMode {
    Independent,
    Unified,
}

impl Default for DeskMode {
    fn default() -> Self {
        Self::Independent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CenterMode {
    Never,
    WhenOverflow,
    Always,
}

impl Default for CenterMode {
    fn default() -> Self {
        Self::WhenOverflow
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColumnMode {
    Stacked,
    Tabbed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PinEdge {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnState {
    pub id: u64,
    pub windows: Vec<WindowId>,
    pub width: f64,
    pub height_weights: Vec<f64>,
    pub mode: ColumnMode,
    pub active_index: usize,
    pub pin: Option<PinEdge>,
    pub natural_heights: Vec<f64>,
    pub height_fill: f64,
}

impl ColumnState {
    pub fn plain(id: u64, window: WindowId, width: f64) -> Self {
        Self {
            id,
            windows: vec![window],
            width,
            height_weights: vec![1.0],
            mode: ColumnMode::Stacked,
            active_index: 0,
            pin: None,
            natural_heights: Vec::new(),
            height_fill: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayoutRequest {
    pub displays: Vec<super::geom::DisplaySlice>,
    pub mode: DeskMode,
    pub gap: f64,
    pub columns_by_display: HashMap<u32, Vec<ColumnState>>,
    pub scroll_by_display: HashMap<u32, f64>,
    pub unified_columns: Vec<ColumnState>,
    pub unified_scroll: f64,
    pub keep_original_height: bool,
}

impl LayoutRequest {
    pub fn independent(
        displays: Vec<super::geom::DisplaySlice>,
        gap: f64,
        columns_by_display: HashMap<u32, Vec<ColumnState>>,
        scroll_by_display: HashMap<u32, f64>,
    ) -> Self {
        Self {
            displays,
            mode: DeskMode::Independent,
            gap,
            columns_by_display,
            scroll_by_display,
            unified_columns: Vec::new(),
            unified_scroll: 0.0,
            keep_original_height: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub frame: Rect,
    pub parked: bool,
    pub cropped: bool,
    pub display_id: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripMetrics {
    pub viewport_width: f64,
    pub content_width: f64,
    pub origins: HashMap<u64, f64>,
}

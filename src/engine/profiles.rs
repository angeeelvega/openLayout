use serde::{Deserialize, Serialize};

use super::geom::WindowId;
use super::model::{ColumnMode, DeskMode, PinEdge};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCandidate {
    pub id: WindowId,
    pub bundle_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSlot {
    pub bundle_id: String,
    pub title: String,
    pub width_ratio: f64,
    pub height_weight: f64,
    pub mode: ColumnMode,
    pub pin: Option<PinEdge>,
    pub display_index: usize,
    pub column_key: u64,
    pub height_fill: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub mode: DeskMode,
    pub linked_scroll: bool,
    pub slots: Vec<ProfileSlot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssignedColumn {
    pub display_index: usize,
    pub width_ratio: f64,
    pub height_weights: Vec<f64>,
    pub mode: ColumnMode,
    pub pin: Option<PinEdge>,
    pub height_fill: f64,
    pub window_ids: Vec<WindowId>,
}

pub fn assign_profile(profile: &Profile, windows: &[WindowCandidate]) -> Vec<AssignedColumn> {
    let mut remaining: Vec<&WindowCandidate> = windows.iter().collect();
    let mut matched: Vec<(&ProfileSlot, WindowId)> = Vec::new();
    for slot in &profile.slots {
        let index = remaining
            .iter()
            .position(|window| window.bundle_id == slot.bundle_id && window.title == slot.title)
            .or_else(|| remaining.iter().position(|window| window.bundle_id == slot.bundle_id));
        if let Some(index) = index {
            let window = remaining.remove(index);
            matched.push((slot, window.id));
        }
    }

    let mut order: Vec<(usize, u64)> = Vec::new();
    let mut groups: Vec<((usize, u64), AssignedColumn)> = Vec::new();
    for (slot, id) in matched {
        let key = (slot.display_index, slot.column_key);
        if let Some(position) = groups.iter().position(|(group_key, _)| *group_key == key) {
            groups[position].1.window_ids.push(id);
            groups[position].1.height_weights.push(slot.height_weight);
        } else {
            order.push(key);
            groups.push((
                key,
                AssignedColumn {
                    display_index: slot.display_index,
                    width_ratio: slot.width_ratio,
                    height_weights: vec![slot.height_weight],
                    mode: slot.mode,
                    pin: slot.pin,
                    height_fill: slot.height_fill,
                    window_ids: vec![id],
                },
            ));
        }
    }
    let _ = order;
    groups.into_iter().map(|(_, column)| column).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_match_wins_over_bundle_match() {
        let safari = WindowCandidate {
            id: WindowId::new(1, 1),
            bundle_id: "com.apple.Safari".into(),
            title: "Mail".into(),
        };
        let docs = WindowCandidate {
            id: WindowId::new(1, 2),
            bundle_id: "com.apple.Safari".into(),
            title: "Docs".into(),
        };
        let profile = Profile {
            id: 1,
            name: "Work".into(),
            mode: DeskMode::Independent,
            linked_scroll: false,
            slots: vec![ProfileSlot {
                bundle_id: "com.apple.Safari".into(),
                title: "Docs".into(),
                width_ratio: 0.5,
                height_weight: 1.0,
                mode: ColumnMode::Stacked,
                pin: None,
                display_index: 1,
                column_key: 0,
                height_fill: 1.0,
            }],
        };
        let assigned = assign_profile(&profile, &[safari, docs]);
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].window_ids, vec![WindowId::new(1, 2)]);
        assert_eq!(assigned[0].display_index, 1);
    }
}

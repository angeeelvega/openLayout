use std::collections::HashMap;

use super::geom::{order_displays, DisplaySlice, Rect, WindowId};
use super::model::{
    CenterMode, ColumnMode, ColumnState, DeskMode, LayoutRequest, PinEdge, Placement, StripMetrics, VerticalAlign,
};

pub fn layout(request: &LayoutRequest) -> HashMap<WindowId, Placement> {
    let displays = order_displays(request.displays.clone());
    if displays.is_empty() {
        return HashMap::new();
    }
    match request.mode {
        DeskMode::Independent => {
            let mut placed = HashMap::new();
            for display in &displays {
                let columns = request
                    .columns_by_display
                    .get(&display.id)
                    .cloned()
                    .unwrap_or_default();
                let scroll = request.scroll_by_display.get(&display.id).copied().unwrap_or(0.0);
                let lanes = vec![independent_lane(display, &columns, request.gap)];
                let piece = place(
                    &columns,
                    &lanes,
                    &displays,
                    scroll,
                    request.gap,
                    request.keep_original_height,
                    placed.len(),
                );
                placed.extend(piece);
            }
            placed
        }
        DeskMode::Unified => {
            let lanes = unified_lanes(&displays, &request.unified_columns, request.gap);
            place(
                &request.unified_columns,
                &lanes,
                &displays,
                request.unified_scroll,
                request.gap,
                request.keep_original_height,
                0,
            )
        }
    }
}

pub fn metrics(columns: &[ColumnState], gap: f64, usable_width: f64) -> StripMetrics {
    let unpinned: Vec<&ColumnState> = columns.iter().filter(|column| column.pin.is_none()).collect();
    let mut width = usable_width;
    if let Some(left) = columns.iter().find(|column| column.pin == Some(PinEdge::Left)) {
        width -= pin_width(left, width - gap) + gap;
    }
    if let Some(right) = columns.iter().find(|column| column.pin == Some(PinEdge::Right)) {
        width -= pin_width(right, (width - gap).max(0.0)) + gap;
    }
    origins_and_content(unpinned, gap, width.max(0.0))
}

pub fn unified_metrics(displays: &[DisplaySlice], columns: &[ColumnState], gap: f64) -> StripMetrics {
    let lanes = unified_lanes(&order_displays(displays.to_vec()), columns, gap);
    let viewport = lanes.iter().map(|lane| lane.width).sum::<f64>().max(80.0);
    let unpinned: Vec<&ColumnState> = columns.iter().filter(|column| column.pin.is_none()).collect();
    let mut metrics = origins_and_content(unpinned, gap, viewport);
    metrics.viewport_width = viewport;
    metrics
}

pub fn scroll_target(
    scroll: f64,
    viewport: f64,
    content_width: f64,
    column_x: f64,
    column_width: f64,
    mode: CenterMode,
) -> f64 {
    let max_scroll = (content_width - viewport).max(0.0);
    let target = match mode {
        CenterMode::Never => {
            if column_x < scroll {
                column_x
            } else if column_x + column_width > scroll + viewport {
                column_x + column_width - viewport
            } else {
                scroll
            }
        }
        CenterMode::Always => column_x - (viewport - column_width) / 2.0,
        CenterMode::WhenOverflow => {
            if content_width <= viewport + 0.5 {
                0.0
            } else {
                column_x - (viewport - column_width) / 2.0
            }
        }
    };
    target.clamp(0.0, max_scroll)
}

pub fn clamp_scroll(scroll: f64, viewport: f64, content_width: f64) -> f64 {
    scroll.clamp(0.0, (content_width - viewport).max(0.0))
}

struct Lane {
    display: DisplaySlice,
    cocoa_x: f64,
    virtual_start: f64,
    width: f64,
}

fn pin_width(column: &ColumnState, limit: f64) -> f64 {
    column.width.min(limit.max(80.0))
}

fn independent_lane(display: &DisplaySlice, columns: &[ColumnState], gap: f64) -> Lane {
    let mut x = display.usable.x;
    let mut width = display.usable.width;
    if let Some(left) = columns.iter().find(|column| column.pin == Some(PinEdge::Left)) {
        let reserved = pin_width(left, width - gap);
        x += reserved + gap;
        width -= reserved + gap;
    }
    if let Some(right) = columns.iter().find(|column| column.pin == Some(PinEdge::Right)) {
        let reserved = pin_width(right, (width - gap).max(0.0));
        width -= reserved + gap;
    }
    Lane {
        display: display.clone(),
        cocoa_x: x,
        virtual_start: 0.0,
        width: width.max(0.0),
    }
}

fn unified_lanes(displays: &[DisplaySlice], columns: &[ColumnState], gap: f64) -> Vec<Lane> {
    if displays.is_empty() {
        return Vec::new();
    }
    let left = columns.iter().find(|column| column.pin == Some(PinEdge::Left));
    let right = columns.iter().find(|column| column.pin == Some(PinEdge::Right));
    let mut lanes = Vec::with_capacity(displays.len());
    let mut virtual_start = 0.0;
    for (index, display) in displays.iter().enumerate() {
        let mut x = display.usable.x;
        let mut width = display.usable.width;
        if index == 0 {
            if let Some(left) = left {
                let reserved = pin_width(left, width - gap);
                x += reserved + gap;
                width -= reserved + gap;
            }
        }
        if index + 1 == displays.len() {
            if let Some(right) = right {
                let reserved = pin_width(right, (width - gap).max(0.0));
                width -= reserved + gap;
            }
        }
        width = width.max(0.0);
        lanes.push(Lane {
            display: display.clone(),
            cocoa_x: x,
            virtual_start,
            width,
        });
        virtual_start += width;
    }
    lanes
}

fn origins_and_content(unpinned: Vec<&ColumnState>, gap: f64, viewport: f64) -> StripMetrics {
    let mut origins = HashMap::<u64, f64>::new();
    let mut cursor = 0.0;
    for (index, column) in unpinned.iter().enumerate() {
        origins.insert(column.id, cursor);
        cursor += column.width;
        if index + 1 < unpinned.len() {
            cursor += gap;
        }
    }
    StripMetrics {
        viewport_width: viewport,
        content_width: cursor,
        origins,
    }
}

fn place(
    columns: &[ColumnState],
    lanes: &[Lane],
    others: &[DisplaySlice],
    scroll: f64,
    gap: f64,
    keep_original_height: bool,
    mut park_slot: usize,
) -> HashMap<WindowId, Placement> {
    let Some(first) = lanes.first() else {
        return HashMap::new();
    };
    let mut placements = HashMap::new();
    let left = columns.iter().find(|column| column.pin == Some(PinEdge::Left));
    let right = columns.iter().find(|column| column.pin == Some(PinEdge::Right));
    let unpinned: Vec<&ColumnState> = columns.iter().filter(|column| column.pin.is_none()).collect();

    if let Some(left) = left {
        let display = &first.display;
        let width = pin_width(left, display.usable.width - gap);
        let box_rect = Rect::new(display.usable.x, display.usable.y, width, display.usable.height);
        place_column(
            left,
            box_rect,
            display,
            others,
            gap,
            keep_original_height,
            &mut placements,
            &mut park_slot,
        );
    }

    if let Some(right) = right {
        if let Some(last) = lanes.last() {
            let display = &last.display;
            let width = pin_width(right, display.usable.width - gap);
            let box_rect = Rect::new(
                display.usable.max_x() - width,
                display.usable.y,
                width,
                display.usable.height,
            );
            place_column(
                right,
                box_rect,
                display,
                others,
                gap,
                keep_original_height,
                &mut placements,
                &mut park_slot,
            );
        }
    }

        let viewport = lanes.iter().map(|lane| lane.width).sum::<f64>().max(1.0);
        let mut fitted: Vec<ColumnState> = unpinned.into_iter().cloned().collect();
        fit_viewport(&mut fitted, gap, viewport, scroll);
        let mut content_x = 0.0;
        for column in &fitted {
            let virtual_left = content_x - scroll;
            let virtual_center = virtual_left + column.width / 2.0;
            let lane = lane_for(virtual_center, lanes).or(if virtual_center < 0.0 { lanes.first() } else { lanes.last() });
            if let Some(lane) = lane {
                let local = virtual_left - lane.virtual_start;
                let box_rect = Rect::new(
                    lane.cocoa_x + local,
                    lane.display.usable.y,
                    column.width,
                    lane.display.usable.height,
                );
                place_column(
                    column,
                    box_rect,
                    &lane.display,
                    others,
                    gap,
                    keep_original_height,
                    &mut placements,
                    &mut park_slot,
                );
            }
            content_x += column.width + gap;
        }
    placements
}

/// When the strip is resting on a column edge, grow or shrink the windows that
/// actually fit on screen so they meet the edges instead of leaving a gap.
fn fit_viewport(columns: &mut [ColumnState], gap: f64, viewport: f64, scroll: f64) {
    if columns.is_empty() || viewport < 80.0 {
        return;
    }
    let mut origin = 0.0;
    let mut start = None;
    for (index, column) in columns.iter().enumerate() {
        if (origin - scroll).abs() <= 1.0 {
            start = Some(index);
            break;
        }
        origin += column.width + gap;
    }
    let Some(start) = start else {
        return;
    };
    if columns.get(start).is_some_and(|column| column.arrange_lock) {
        return;
    }
    let mut chosen = 0usize;
    let mut used = 0.0;
    for column in &columns[start..] {
        let add = if chosen == 0 { column.width } else { gap + column.width };
        if used + add <= viewport + 24.0 {
            used += add;
            chosen += 1;
            continue;
        }
        let visible = viewport - used - if chosen == 0 { 0.0 } else { gap };
        if visible > column.width * 0.55 {
            chosen += 1;
        }
        break;
    }
    if chosen == 0 {
        return;
    }
    let gaps = gap * chosen.saturating_sub(1) as f64;
    let width_sum: f64 = columns[start..start + chosen].iter().map(|column| column.width).sum();
    if width_sum < 40.0 {
        return;
    }
    let scale = ((viewport - gaps) / width_sum).clamp(0.55, 4.0);
    if (scale - 1.0).abs() < 0.012 {
        return;
    }
    for column in &mut columns[start..start + chosen] {
        column.width *= scale;
    }
}

fn lane_for(virtual_x: f64, lanes: &[Lane]) -> Option<&Lane> {
    lanes
        .iter()
        .find(|lane| virtual_x >= lane.virtual_start && virtual_x < lane.virtual_start + lane.width)
}

fn place_column(
    column: &ColumnState,
    box_rect: Rect,
    display: &DisplaySlice,
    others: &[DisplaySlice],
    gap: f64,
    keep_original_height: bool,
    placements: &mut HashMap<WindowId, Placement>,
    park_slot: &mut usize,
) {
    for (id, frame, force_park) in frames_inside(column, box_rect, gap, keep_original_height) {
        if force_park {
            placements.insert(id, parked(*park_slot, display.id));
            *park_slot += 1;
            continue;
        }
        if let Some((rect, cropped)) = fit(frame, display, others) {
            placements.insert(
                id,
                Placement {
                    frame: rect,
                    parked: false,
                    cropped,
                    display_id: display.id,
                },
            );
        } else {
            placements.insert(id, parked(*park_slot, display.id));
            *park_slot += 1;
        }
    }
}

fn parked(slot: usize, display_id: u32) -> Placement {
    Placement {
        frame: Rect::new(-32000.0, -32000.0 - slot as f64 * 48.0, 480.0, 320.0),
        parked: true,
        cropped: false,
        display_id,
    }
}

fn frames_inside(
    column: &ColumnState,
    box_rect: Rect,
    gap: f64,
    keep_original_height: bool,
) -> Vec<(WindowId, Rect, bool)> {
    if column.windows.is_empty() {
        return Vec::new();
    }
    let fill = column.height_fill.clamp(0.2, 1.0);
    let box_height = (box_rect.height * fill).max(40.0);
    let y = match column.vertical {
        VerticalAlign::Bottom => box_rect.y,
        VerticalAlign::Center => box_rect.y + (box_rect.height - box_height) / 2.0,
        VerticalAlign::Top => box_rect.max_y() - box_height,
    };
    let column_box = Rect::new(box_rect.x, y, box_rect.width, box_height);

    if column.mode == ColumnMode::Tabbed {
        let index = column.active_index.min(column.windows.len() - 1);
        return column
            .windows
            .iter()
            .enumerate()
            .map(|(offset, id)| {
                (
                    *id,
                    if offset == index {
                        single_frame(column_box, natural_height(column, offset), keep_original_height)
                    } else {
                        column_box
                    },
                    false,
                )
            })
            .collect();
    }

    if column.windows.len() == 1 {
        return vec![(
            column.windows[0],
            single_frame(column_box, natural_height(column, 0), keep_original_height),
            false,
        )];
    }

    let weights = normalized(&column.height_weights, column.windows.len());
    let gaps = gap * (column.windows.len() - 1) as f64;
    let available = (column_box.height - gaps).max(40.0);
    let mut heights: Vec<f64> = weights.iter().map(|weight| weight * available).collect();
    if keep_original_height {
        let naturals: Vec<f64> = column
            .windows
            .iter()
            .enumerate()
            .map(|(index, _)| natural_height(column, index).unwrap_or(heights[index]))
            .collect();
        let natural_total: f64 = naturals.iter().sum();
        if natural_total + gaps <= column_box.height {
            heights = naturals;
        }
    }

    let mut top = column_box.max_y();
    let mut frames = Vec::with_capacity(column.windows.len());
    for (index, id) in column.windows.iter().enumerate() {
        let height = heights[index].max(40.0);
        top -= height;
        frames.push((*id, Rect::new(column_box.x, top, column_box.width, height), false));
        top -= gap;
    }
    frames
}

fn single_frame(column_box: Rect, natural: Option<f64>, keep_original_height: bool) -> Rect {
    let Some(natural) = natural else {
        return column_box;
    };
    if !keep_original_height || natural <= 40.0 {
        return column_box;
    }
    let height = column_box.height.min(natural);
    Rect::new(column_box.x, column_box.max_y() - height, column_box.width, height)
}

fn natural_height(column: &ColumnState, index: usize) -> Option<f64> {
    column.natural_heights.get(index).copied().filter(|value| *value > 40.0)
}

fn normalized(raw: &[f64], count: usize) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    let mut weights = raw.to_vec();
    if weights.len() < count {
        weights.resize(count, 1.0);
    }
    weights.truncate(count);
    let sum: f64 = weights.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / count as f64; count];
    }
    weights.into_iter().map(|weight| weight / sum).collect()
}

fn fit(rect: Rect, display: &DisplaySlice, others: &[DisplaySlice]) -> Option<(Rect, bool)> {
    if rect.width < 12.0 || rect.height < 12.0 {
        return None;
    }
    let spills = others
        .iter()
        .any(|other| other.id != display.id && rect.intersects(other.frame));
    if !spills {
        // Keep the window in the strip, even when it sits past the edge of this display.
        return Some((rect, false));
    }
    if let Some(cropped) = rect.intersection(display.frame) {
        if cropped.width >= 12.0 && cropped.height >= 12.0 {
            return Some((cropped, true));
        }
    }
    Some((clamp_onto(rect, display), true))
}

fn clamp_onto(rect: Rect, display: &DisplaySlice) -> Rect {
    let mut rect = rect;
    if rect.x + rect.width * 0.5 >= display.frame.mid_x() {
        rect.x = (display.frame.max_x() - rect.width).max(display.frame.x);
    } else {
        rect.x = display.frame.x;
    }
    if rect.y < display.frame.y {
        rect.y = display.frame.y;
    }
    if rect.max_y() > display.frame.max_y() {
        rect.y = (display.frame.max_y() - rect.height).max(display.frame.y);
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::geom::WindowId;

    fn laptop() -> DisplaySlice {
        DisplaySlice {
            id: 1,
            frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
            usable: Rect::new(8.0, 8.0, 1424.0, 860.0),
            name: "Laptop".into(),
        }
    }

    fn external() -> DisplaySlice {
        DisplaySlice {
            id: 2,
            frame: Rect::new(1440.0, 0.0, 1920.0, 1080.0),
            usable: Rect::new(1448.0, 8.0, 1904.0, 1040.0),
            name: "External".into(),
        }
    }

    fn window(number: u32) -> WindowId {
        WindowId::new(10, number)
    }

    fn column(id: WindowId, width: f64) -> ColumnState {
        ColumnState::plain(id.number as u64, id, width)
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 0.1, "{actual} != {expected}");
    }

    #[test]
    fn each_display_keeps_its_own_column() {
        let left = window(1);
        let right = window(2);
        let mut columns = HashMap::new();
        columns.insert(1, vec![column(left, 700.0)]);
        columns.insert(2, vec![column(right, 900.0)]);
        let placed = layout(&LayoutRequest::independent(vec![laptop(), external()], 8.0, columns, HashMap::new()));
        let laptop_frame = placed[&left];
        let external_frame = placed[&right];
        assert!(!laptop_frame.parked);
        assert!(!external_frame.parked);
        assert_eq!(laptop_frame.display_id, 1);
        assert_eq!(external_frame.display_id, 2);
        close(laptop_frame.frame.x, 8.0);
        close(external_frame.frame.x, 1448.0);
        assert!(!laptop_frame.frame.intersects(external().frame));
    }

    #[test]
    fn offscreen_column_stays_on_its_own_display() {
        let id = window(3);
        let mut columns = HashMap::new();
        columns.insert(1, vec![column(id, 700.0)]);
        let mut scroll = HashMap::new();
        scroll.insert(1, -1600.0);
        let placed = layout(&LayoutRequest::independent(vec![laptop(), external()], 8.0, columns, scroll));
        let frame = placed[&id];
        assert!(!frame.parked);
        assert!(!frame.frame.intersects(external().frame));
        assert!(frame.frame.intersects(laptop().frame));
        assert!(frame.frame.y >= laptop().frame.y);
    }

    #[test]
    fn short_columns_meet_the_screen_edges() {
        let left = window(7);
        let right = window(8);
        let mut columns = HashMap::new();
        columns.insert(1, vec![column(left, 500.0), column(right, 500.0)]);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, HashMap::new()));
        let left_frame = placed[&left].frame;
        let right_frame = placed[&right].frame;
        close(left_frame.x, 8.0);
        close(left_frame.max_x() + 8.0, right_frame.x);
        close(right_frame.max_x(), laptop().usable.max_x());
        close(left_frame.height, laptop().usable.height);
    }

    #[test]
    fn a_sliver_does_not_leave_a_hole() {
        let left = window(1);
        let middle = window(2);
        let extra = window(9);
        let mut columns = HashMap::new();
        columns.insert(1, vec![column(left, 640.0), column(middle, 640.0), column(extra, 700.0)]);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, HashMap::new()));
        assert!(!placed[&extra].parked);
        close(placed[&middle].frame.max_x(), laptop().usable.max_x());
        assert!(placed[&extra].frame.x >= laptop().usable.max_x() - 1.0);
    }

    #[test]
    fn partial_column_does_not_cross_onto_the_next_display() {
        let id = window(4);
        let mut columns = HashMap::new();
        columns.insert(1, vec![column(id, 800.0)]);
        let mut scroll = HashMap::new();
        scroll.insert(1, -1000.0);
        let placed = layout(&LayoutRequest::independent(vec![laptop(), external()], 0.0, columns, scroll));
        let frame = placed[&id];
        assert!(!frame.parked);
        assert!(frame.cropped);
        assert!(!frame.frame.intersects(external().frame));
        assert!(frame.frame.max_x() <= laptop().frame.max_x());
    }

    #[test]
    fn pin_stays_put_while_the_strip_starts_beside_it() {
        let pinned = window(5);
        let scrolling = window(6);
        let mut pinned_column = column(pinned, 300.0);
        pinned_column.pin = Some(PinEdge::Left);
        let mut columns = HashMap::new();
        columns.insert(1, vec![pinned_column, column(scrolling, 600.0)]);
        let mut scroll = HashMap::new();
        scroll.insert(1, 40.0);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, scroll));
        close(placed[&pinned].frame.x, 8.0);
        close(placed[&scrolling].frame.x, 8.0 + 300.0 + 8.0 - 40.0);
    }

    #[test]
    fn stack_splits_height_without_overlap() {
        let top = window(7);
        let bottom = window(8);
        let stacked = ColumnState {
            id: 1,
            windows: vec![top, bottom],
            width: 640.0,
            height_weights: vec![1.0, 1.0],
            mode: ColumnMode::Stacked,
            active_index: 0,
            pin: None,
            natural_heights: Vec::new(),
            height_fill: 1.0,
            vertical: VerticalAlign::Top,
            arrange_lock: false,
        };
        let mut columns = HashMap::new();
        columns.insert(1, vec![stacked]);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, HashMap::new()));
        let top_frame = placed[&top].frame;
        let bottom_frame = placed[&bottom].frame;
        assert!(!top_frame.intersects(bottom_frame));
        close(top_frame.height + bottom_frame.height + 8.0, laptop().usable.height);
        assert!(top_frame.min_y() > bottom_frame.min_y());
    }

    #[test]
    fn bottom_half_sits_on_the_bottom_edge() {
        let window = window(11);
        let mut column = column(window, 1424.0);
        column.height_fill = 0.5;
        column.vertical = VerticalAlign::Bottom;
        let mut columns = HashMap::new();
        columns.insert(1, vec![column]);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, HashMap::new()));
        close(placed[&window].frame.min_y(), laptop().usable.min_y());
        close(placed[&window].frame.height, laptop().usable.height / 2.0);
    }

    #[test]
    fn a_locked_half_does_not_stretch_to_fill_the_screen() {
        let window = window(12);
        let mut column = column(window, 712.0);
        column.arrange_lock = true;
        let mut columns = HashMap::new();
        columns.insert(1, vec![column]);
        let placed = layout(&LayoutRequest::independent(vec![laptop()], 8.0, columns, HashMap::new()));
        close(placed[&window].frame.width, 712.0);
    }

    #[test]
    fn unified_desk_flows_onto_the_second_display() {
        let first = window(9);
        let second = window(10);
        let request = LayoutRequest {
            displays: vec![laptop(), external()],
            mode: DeskMode::Unified,
            gap: 0.0,
            columns_by_display: HashMap::new(),
            scroll_by_display: HashMap::new(),
            unified_columns: vec![column(first, 1424.0), column(second, 800.0)],
            unified_scroll: 0.0,
            keep_original_height: false,
        };
        let placed = layout(&request);
        assert_eq!(placed[&first].display_id, 1);
        assert_eq!(placed[&second].display_id, 2);
        assert!(placed[&second].frame.x >= external().frame.min_x());
    }

    #[test]
    fn scroll_target_centers_only_when_the_strip_overflows() {
        close(
            scroll_target(0.0, 1000.0, 1800.0, 900.0, 400.0, CenterMode::WhenOverflow),
            600.0,
        );
        close(
            scroll_target(40.0, 1000.0, 800.0, 100.0, 400.0, CenterMode::WhenOverflow),
            0.0,
        );
    }
}

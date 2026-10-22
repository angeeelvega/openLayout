use crate::platform::ax::{self, RawWindow};
use crate::platform::events::{BusEvent, MouseKind};
use crate::platform::screens::{self, primary_height};
use crate::platform::spaces;
use crate::store::{self, AppBehavior, SavedColumn, SavedDesk, SavedDisplay, SavedState, Settings};
use openlayout::engine::{
    assign_profile, clamp_scroll, layout, metrics, order_displays, ratios, scroll_target, take_groups, unified_metrics, Action,
    Arrange, AssignedColumn, Binding, CenterMode, ColumnMode, ColumnState, Command, DeskMode, DisplaySlice, LayoutRequest,
    Pane, PinEdge, Placement, Profile, ProfileSlot, Rect, VerticalAlign, WindowCandidate, WindowId,
};
use std::collections::{HashMap, HashSet};
use std::mem::forget;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct IndicatorTile {
    pub column_id: u64,
    pub title: String,
    pub focused: bool,
    pub on_screen: bool,
}

#[derive(Clone, Debug)]
pub struct SnapshotColumn {
    pub id: u64,
    pub display_id: u32,
    pub title: String,
    pub app_name: String,
    pub frame: Rect,
    pub focused: bool,
    pub pinned: bool,
    pub windows: usize,
}

#[derive(Clone, Debug)]
pub struct DeskSnapshot {
    pub displays: Vec<DisplaySlice>,
    pub columns: Vec<SnapshotColumn>,
    pub focus_frame: Option<Rect>,
    pub indicator: Vec<IndicatorTile>,
    pub indicator_visible: bool,
    pub indicator_display: u32,
    pub status: String,
    pub tiling: bool,
    pub trusted: bool,
    pub input_monitoring: bool,
    pub window_count: usize,
}

impl Default for DeskSnapshot {
    fn default() -> Self {
        Self {
            displays: Vec::new(),
            columns: Vec::new(),
            focus_frame: None,
            indicator: Vec::new(),
            indicator_visible: false,
            indicator_display: 0,
            status: "Paused".into(),
            tiling: false,
            trusted: false,
            input_monitoring: false,
            window_count: 0,
        }
    }
}

#[derive(Clone)]
struct Column {
    id: u64,
    windows: Vec<WindowId>,
    width_ratio: f64,
    height_weights: Vec<f64>,
    mode: ColumnMode,
    pin: Option<PinEdge>,
    active_index: usize,
    height_fill: f64,
    vertical: VerticalAlign,
    arrange_lock: bool,
}

#[derive(Clone, Default)]
struct Strip {
    columns: Vec<Column>,
    scroll: f64,
    scroll_target: f64,
}

#[derive(Clone, Default)]
struct SpaceDesk {
    strips: HashMap<u32, Strip>,
    unified: Strip,
}

struct LiveWindow {
    element: usize,
    bundle_id: String,
    app_name: String,
    title: String,
    frame: Rect,
    resizable: bool,
    natural_height: f64,
    rigid_width: Option<f64>,
    checked_size: bool,
    space: u64,
}

impl Drop for LiveWindow {
    fn drop(&mut self) {
        ax::release_element(self.element);
    }
}

struct Drag {
    window: WindowId,
    x: f64,
    y: f64,
}

pub struct Session {
    pub settings: Settings,
    pub profiles: Vec<Profile>,
    pub snapshot: DeskSnapshot,
    pub dirty_ui: bool,
    pub dirty_controls: bool,
    pub show_overview: bool,
    pub show_settings: bool,
    recording: Option<Action>,
    strips: HashMap<u32, Strip>,
    unified: Strip,
    windows: HashMap<WindowId, LiveWindow>,
    floating: HashSet<WindowId>,
    focused: Option<WindowId>,
    focus_by_display: HashMap<u32, WindowId>,
    displays: Vec<DisplaySlice>,
    primary_height: f64,
    placements: HashMap<WindowId, Placement>,
    column_frames: HashMap<u64, Rect>,
    next_id: u64,
    frame_tick: u64,
    animating: bool,
    needs_apply: bool,
    gesture_until: Option<Instant>,
    dirty_save: bool,
    dragging: bool,
    drag: Option<Drag>,
    modifier_held: bool,
    indicator_until: Option<Instant>,
    shown_display: u32,
    suppress_mouse_until: Option<Instant>,
    profile_cursor: usize,
    trusted: bool,
    input_monitoring: bool,
    input_prompted: bool,
    tap_ready: bool,
    tap_attempted: bool,
    space_id: u64,
    space_desks: HashMap<u64, SpaceDesk>,
    parked_screens: HashMap<u32, Strip>,
    layout_restored: bool,
}

impl Session {
    pub fn new(saved: SavedState) -> Self {
        let mut session = Self {
            settings: saved.settings,
            profiles: saved.profiles,
            snapshot: DeskSnapshot::default(),
            dirty_ui: true,
            dirty_controls: true,
            show_overview: false,
            show_settings: false,
            recording: None,
            strips: HashMap::new(),
            unified: Strip::default(),
            windows: HashMap::new(),
            floating: HashSet::new(),
            focused: None,
            focus_by_display: HashMap::new(),
            displays: Vec::new(),
            primary_height: 0.0,
            placements: HashMap::new(),
            column_frames: HashMap::new(),
            next_id: 1,
            frame_tick: 0,
            animating: false,
            needs_apply: false,
            gesture_until: None,
            dirty_save: false,
            dragging: false,
            drag: None,
            modifier_held: false,
            indicator_until: None,
            shown_display: 0,
            suppress_mouse_until: None,
            profile_cursor: 0,
            trusted: ax::trusted(),
            input_monitoring: ax::input_monitoring(),
            input_prompted: false,
            tap_ready: false,
            tap_attempted: false,
            space_id: 0,
            space_desks: HashMap::new(),
            parked_screens: HashMap::new(),
            layout_restored: false,
        };
        session.sync_hotkeys();
        session.refresh_status();
        session
    }

    pub fn note_permissions(&mut self) {
        self.trusted = ax::confirm_trust();
        self.input_monitoring = ax::input_monitoring();
        if self.trusted {
            self.tap_attempted = false;
        }
        self.refresh_status();
    }

    pub fn tick(&mut self) {
        self.frame_tick = self.frame_tick.wrapping_add(1);
        let trusted = if self.frame_tick % 20 == 0 {
            ax::confirm_trust()
        } else {
            ax::trusted() || self.trusted
        };
        if trusted != self.trusted {
            self.trusted = trusted;
            self.dirty_ui = true;
            self.dirty_controls = true;
            if trusted {
                self.tap_attempted = false;
            }
            self.refresh_status();
        }
        let listening = ax::input_monitoring();
        if listening != self.input_monitoring {
            self.input_monitoring = listening;
            self.dirty_ui = true;
            self.refresh_status();
        }
        if self.trusted && !self.input_monitoring && !self.input_prompted {
            self.input_prompted = true;
            ax::request_input_monitoring();
            self.input_monitoring = ax::input_monitoring();
            self.refresh_status();
        }
        if self.trusted && !self.tap_ready && !self.tap_attempted {
            self.tap_attempted = true;
            self.tap_ready = crate::platform::events::install();
        }
        if self.animating {
            self.step_scroll();
            self.apply();
        } else {
            self.flush();
        }
        if self.gesture_until.is_some_and(|until| Instant::now() >= until) {
            self.gesture_until = None;
            if self.settings.tiling_enabled {
                self.publish_snapshot();
                self.dirty_ui = true;
            }
        }
        if self.settings.tiling_enabled && self.trusted && !self.dragging && !self.animating && !self.gesture_active() {
            let active = spaces::active_space();
            if (active != 0 && active != self.space_id) || self.frame_tick % 30 == 0 {
                self.reconcile();
            }
        }
        if let Some(until) = self.indicator_until {
            if Instant::now() > until && !self.gesture_active() && !self.animating {
                self.indicator_until = None;
                self.snapshot.indicator_visible = false;
                self.dirty_ui = true;
            }
        }
        if self.dirty_save && self.frame_tick % 120 == 0 {
            self.save();
        }
    }

    pub fn handle_bus(&mut self, event: BusEvent) {
        match event {
            BusEvent::Action(action) => self.perform(action),
            BusEvent::Scroll { delta, x, y } => {
                if self.settings.tiling_enabled {
                    self.scroll_by(delta, x, y);
                }
            }
            BusEvent::Flags(held) => {
                self.modifier_held = held;
                if !held && !self.gesture_active() && !self.animating {
                    self.indicator_until = None;
                    self.snapshot.indicator_visible = false;
                    self.dirty_ui = true;
                }
            }
            BusEvent::Capture(code, bits) => self.capture(code, bits),
            BusEvent::Mouse { x, y, kind } => self.note_mouse(x, y, kind),
            BusEvent::Command(command) => {
                if let Some(command) = openlayout::engine::parse_command(&command) {
                    self.handle_command(command);
                }
            }
        }
    }

    pub fn handle_command(&mut self, command: Command) {
        match command {
            Command::Pause => self.set_tiling(false),
            Command::Resume => self.set_tiling(true),
            Command::Toggle => self.set_tiling(!self.settings.tiling_enabled),
            Command::Overview => {
                self.show_overview = true;
                self.dirty_ui = true;
            }
            Command::Mode(mode) => self.set_mode(mode),
            Command::ProfileSave(name) => self.save_profile(name),
            Command::ProfileApply(name) => self.apply_profile_named(&name),
            Command::FocusNextDisplay => self.focus_next_display(),
        }
    }

    pub fn perform(&mut self, action: Action) {
        match action {
            Action::TogglePause => self.set_tiling(!self.settings.tiling_enabled),
            Action::ToggleOverview => {
                self.show_overview = true;
                self.dirty_ui = true;
            }
            Action::CycleProfile => self.cycle_profile(),
            _ if !self.settings.tiling_enabled => {}
            Action::FocusLeft => self.focus_horizontal(-1),
            Action::FocusRight => self.focus_horizontal(1),
            Action::FocusUp => self.focus_vertical(-1),
            Action::FocusDown => self.focus_vertical(1),
            Action::FocusFirst => self.focus_end(true),
            Action::FocusLast => self.focus_end(false),
            Action::FocusNextDisplay => self.focus_next_display(),
            Action::MoveLeft => self.move_horizontal(-1),
            Action::MoveRight => self.move_horizontal(1),
            Action::MoveUp => self.move_vertical(-1),
            Action::MoveDown => self.move_vertical(1),
            Action::MoveToNextDisplay => self.move_to_next_display(),
            Action::CycleWidth => self.cycle_width(),
            Action::FullWidth => self.set_full_width(),
            Action::CenterColumn => self.center_focused(),
            Action::Wider => self.nudge_width(0.05),
            Action::Narrower => self.nudge_width(-0.05),
            Action::Taller => self.nudge_height(0.08),
            Action::Shorter => self.nudge_height(-0.08),
            Action::EqualizeHeights => self.equalize(),
            Action::PinLeft => self.pin(PinEdge::Left),
            Action::PinRight => self.pin(PinEdge::Right),
            Action::StackIntoLeft => self.stack_into(-1),
            Action::StackIntoRight => self.stack_into(1),
            Action::PopOutLeft => self.pop_out(-1),
            Action::PopOutRight => self.pop_out(1),
            Action::ToggleTabs => self.toggle_tabs(),
            Action::ToggleFloat => self.toggle_float(),
            action => {
                if let Some(index) = action.jump_index() {
                    self.jump(index);
                }
            }
        }
    }

    pub fn set_gap(&mut self, gap: f64) {
        self.settings.gap = gap.clamp(0.0, 64.0);
        self.finish_setting();
    }

    pub fn set_outer_gap(&mut self, gap: f64) {
        self.settings.outer_gap = gap.clamp(0.0, 80.0);
        self.displays = screens::display_slices(self.settings.outer_gap);
        self.finish_setting();
    }

    pub fn set_center_mode(&mut self, mode: CenterMode) {
        self.settings.center_mode = mode;
        self.finish_setting();
    }

    pub fn set_trackpad(&mut self, mode: crate::store::TrackpadMode) {
        self.settings.trackpad = mode;
        self.sync_hotkeys();
        self.finish_setting();
    }

    pub fn set_default_width(&mut self, width: f64) {
        self.settings.default_width = width.clamp(0.2, 1.0);
        self.finish_setting();
    }

    pub fn set_presets_text(&mut self, text: &str) {
        self.settings.presets = store::parse_presets(text);
        self.finish_setting();
    }

    pub fn set_flag(&mut self, flag: &str, enabled: bool) {
        match flag {
            "height" => self.settings.keep_original_height = enabled,
            "border" => self.settings.focus_border = enabled,
            "dim" => self.settings.dim_inactive = enabled,
            "focus-mouse" => {
                self.settings.focus_follows_mouse = enabled;
                self.sync_hotkeys();
            }
            "mouse-focus" => self.settings.mouse_follows_focus = enabled,
            "linked" => self.settings.linked_scroll = enabled,
            "login" => {
                self.settings.launch_at_login = enabled;
                if let Err(error) = screens::set_launch_at_login(enabled) {
                    self.snapshot.status = error;
                    self.settings.launch_at_login = false;
                }
            }
            _ => {}
        }
        self.finish_setting();
    }

    pub fn set_mode(&mut self, mode: DeskMode) {
        if mode == self.settings.desk_mode {
            return;
        }
        if mode == DeskMode::Unified {
            let mut columns = Vec::new();
            for display in order_displays(self.displays.clone()) {
                if let Some(strip) = self.strips.get(&display.id) {
                    columns.extend(strip.columns.iter().cloned());
                }
            }
            self.unified = Strip { columns, scroll: 0.0, scroll_target: 0.0 };
            self.strips.clear();
        } else {
            self.strips.clear();
            let frames = self.column_frames.clone();
            let fallback = self.displays.first().map(|display| display.id).unwrap_or(0);
            let displays = self.displays.clone();
            for column in self.unified.columns.drain(..) {
                let display = frames
                    .get(&column.id)
                    .and_then(|frame| {
                        displays
                            .iter()
                            .find(|item| item.frame.contains(frame.mid_x(), frame.mid_y()))
                            .map(|item| item.id)
                    })
                    .unwrap_or(fallback);
                self.strips.entry(display).or_default().columns.push(column);
            }
        }
        self.settings.desk_mode = mode;
        self.finish_setting();
    }

    pub fn begin_recording(&mut self, action: Action) {
        self.recording = Some(action);
        self.sync_hotkeys();
        self.dirty_controls = true;
        self.dirty_ui = true;
    }

    pub fn reset_bindings(&mut self) {
        self.settings.bindings = Binding::defaults();
        self.recording = None;
        self.sync_hotkeys();
        self.finish_setting();
    }

    pub fn recording_action(&self) -> Option<Action> {
        self.recording
    }

    pub fn save_profile(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let slots = self.profile_slots();
        if let Some(profile) = self.profiles.iter_mut().find(|profile| profile.name == name) {
            profile.mode = self.settings.desk_mode;
            profile.linked_scroll = self.settings.linked_scroll;
            profile.slots = slots;
        } else {
            let id = self.fresh_id();
            self.profiles.push(Profile {
                id,
                name,
                mode: self.settings.desk_mode,
                linked_scroll: self.settings.linked_scroll,
                slots,
            });
        }
        self.dirty_save = true;
        self.dirty_controls = true;
        self.dirty_ui = true;
        self.save();
    }

    pub fn apply_profile_named(&mut self, name: &str) {
        let Some(profile) = self.profiles.iter().find(|profile| profile.name == name).cloned() else {
            self.snapshot.status = format!("No profile named {name}");
            self.dirty_ui = true;
            return;
        };
        self.apply_profile(&profile);
    }

    pub fn move_column(&mut self, id: u64, to_display: u32, before: Option<u64>) {
        let Some(mut column) = self.take_column(id) else {
            return;
        };
        column.pin = None;
        if self.settings.desk_mode == DeskMode::Unified {
            let index = before
                .and_then(|marker| self.unified.columns.iter().position(|column| column.id == marker))
                .unwrap_or(self.unified.columns.len());
            self.unified.columns.insert(index.min(self.unified.columns.len()), column);
        } else {
            let strip = self.strips.entry(to_display).or_default();
            let index = before
                .and_then(|marker| strip.columns.iter().position(|column| column.id == marker))
                .unwrap_or(strip.columns.len());
            strip.columns.insert(index.min(strip.columns.len()), column);
        }
        self.reveal_focused(false);
        self.needs_apply = true;
        self.dirty_save = true;
        self.dirty_ui = true;
    }

    pub fn focus_column(&mut self, id: u64) {
        let windows = self.column_by_id(id).map(|column| column.windows.clone());
        if let Some(windows) = windows {
            if let Some(window) = windows.first().copied() {
                self.focus_window(window, true);
            }
        }
    }

    pub fn save(&mut self) {
        let state = SavedState {
            settings: self.settings.clone(),
            profiles: self.profiles.clone(),
            desk: self.saved_desk(),
        };
        store::save(&state);
        self.dirty_save = false;
    }

    fn finish_setting(&mut self) {
        self.needs_apply = true;
        self.dirty_save = true;
        self.dirty_ui = true;
        self.dirty_controls = true;
    }

    fn sync_hotkeys(&self) {
        crate::platform::events::update(
            &self.settings.bindings,
            self.recording.is_some(),
            self.settings.trackpad,
            self.settings.focus_follows_mouse,
        );
    }

    fn set_tiling(&mut self, enabled: bool) {
        if enabled {
            if !ax::confirm_trust() {
                self.trusted = false;
                self.refresh_status();
                self.dirty_controls = true;
                return;
            }
            self.trusted = true;
            self.input_monitoring = ax::input_monitoring();
            if !self.input_monitoring {
                ax::request_input_monitoring();
                self.input_prompted = true;
                self.input_monitoring = ax::input_monitoring();
            }
            self.tap_ready = crate::platform::events::install();
        }
        self.settings.tiling_enabled = enabled;
        self.dirty_ui = true;
        self.dirty_controls = true;
        if !enabled {
            self.save();
        } else {
            self.dirty_save = true;
        }
        if enabled {
            self.reconcile();
            if self.columns_empty() {
                self.seed_from_windows();
            }
            self.reveal_focused(false);
            self.needs_apply = true;
            self.apply();
        }
        self.refresh_status();
        self.sync_hotkeys();
    }

    fn reconcile(&mut self) {
        self.primary_height = primary_height();
        self.displays = screens::display_slices(self.settings.outer_gap);
        let raw = ax::list_windows(self.primary_height);
        let mut old = std::mem::take(&mut self.windows);
        let mut seen = Vec::new();
        for window in raw {
            if window.minimized || window.fullscreen {
                continue;
            }
            let id = WindowId::new(window.pid, window.number);
            seen.push((id, window));
        }
        for (id, window) in seen {
            if let Some(mut live) = old.remove(&id) {
                live.title = window.title;
                live.frame = window.frame;
                live.resizable = window.resizable;
                live.app_name = window.app_name;
                self.windows.insert(id, live);
            } else if !self.should_float_raw(&window) {
                if self.layout_restored {
                    self.insert_new(id, window);
                } else {
                    let natural = window.frame.height;
                    self.windows.insert(id, live_from_raw(window, false));
                    if let Some(live) = self.windows.get_mut(&id) {
                        live.natural_height = natural;
                    }
                }
            } else {
                self.floating.insert(id);
                self.windows.insert(id, live_from_raw(window, true));
            }
        }
        if !self.layout_restored {
            self.layout_restored = true;
            if !self.saved_desk_restore() {
                self.arrange_on_screens();
            }
        }
        let live_ids: HashSet<WindowId> = self.windows.keys().copied().collect();
        self.retain_live(&live_ids);
        self.rehome_missing_displays();
        self.sync_desktops();
        if self.focused.is_some_and(|id| !live_ids.contains(&id)) {
            self.focused = None;
        }
        self.needs_apply = true;
        if self.settings.tiling_enabled {
            self.apply();
        } else {
            self.refresh_status();
        }
    }

    fn should_float_raw(&self, window: &RawWindow) -> bool {
        if self.floating.contains(&WindowId::new(window.pid, window.number)) {
            return true;
        }
        if let Some(rule) = self.settings.rules.iter().find(|rule| rule.bundle_id == window.bundle_id) {
            return rule.behavior == AppBehavior::Float;
        }
        !window.resizable && window.frame.width < 420.0 && window.frame.height < 320.0
    }

    fn insert_new(&mut self, id: WindowId, window: RawWindow) {
        let ratio = self
            .settings
            .rules
            .iter()
            .find(|rule| rule.bundle_id == window.bundle_id)
            .and_then(|rule| rule.width_ratio)
            .unwrap_or(self.settings.default_width);
        let display = self.display_of(window.frame).or_else(|| self.displays.first().map(|display| display.id));
        let natural = window.frame.height;
        let front = front_pid();
        self.windows.insert(id, live_from_raw(window, false));
        if let Some(live) = self.windows.get_mut(&id) {
            live.natural_height = natural;
        }
        let Some(display) = display else {
            return;
        };
        let column = self.make_column(vec![id], ratio);
        let steal_focus = front == id.pid;
        if self.settings.desk_mode == DeskMode::Unified {
            let index = self.focused_column_index().map(|index| index + 1).unwrap_or(self.unified.columns.len());
            self.unified.columns.insert(index.min(self.unified.columns.len()), column);
        } else {
            let strip = self.strips.entry(display).or_default();
            let index = if steal_focus {
                self.focused
                    .and_then(|focused| strip.columns.iter().position(|column| column.windows.contains(&focused)))
                    .map(|index| index + 1)
                    .unwrap_or(strip.columns.len())
            } else {
                strip.columns.len()
            };
            strip.columns.insert(index.min(strip.columns.len()), column);
        }
        if steal_focus {
            self.focus_window(id, true);
        } else {
            self.needs_apply = true;
        }
    }

    fn retain_live(&mut self, live_ids: &HashSet<WindowId>) {
        for strip in self.strips.values_mut() {
            retain_strip(strip, live_ids);
        }
        retain_strip(&mut self.unified, live_ids);
        for desk in self.space_desks.values_mut() {
            for strip in desk.strips.values_mut() {
                retain_strip(strip, live_ids);
            }
            retain_strip(&mut desk.unified, live_ids);
        }
        for strip in self.parked_screens.values_mut() {
            retain_strip(strip, live_ids);
        }
        self.parked_screens.retain(|_, strip| !strip.columns.is_empty());
        self.floating.retain(|id| live_ids.contains(id));
    }

    fn sync_desktops(&mut self) {
        let numbers: Vec<u32> = self.windows.keys().map(|id| id.number).collect();
        let found = spaces::spaces_for(&numbers);
        let active = spaces::active_space();
        if active == 0 {
            return;
        }
        for (number, space) in &found {
            if let Some((_, window)) = self.windows.iter_mut().find(|(id, _)| id.number == *number) {
                window.space = *space;
            }
        }
        if self.space_id != active {
            if self.space_id != 0 {
                self.space_desks.insert(
                    self.space_id,
                    SpaceDesk {
                        strips: std::mem::take(&mut self.strips),
                        unified: std::mem::take(&mut self.unified),
                    },
                );
            }
            if let Some(desk) = self.space_desks.remove(&active) {
                self.strips = desk.strips;
                self.unified = desk.unified;
            } else if self.space_id != 0 {
                self.strips.clear();
                self.unified = Strip::default();
            }
            self.space_id = active;
            self.placements.clear();
            self.focused = self.focused.filter(|id| self.windows.get(id).is_some_and(|window| window.space == 0 || window.space == active));
        }
        self.park_foreign(active);
        self.adopt_current(active);
        self.rehome_missing_displays();
    }

    fn park_foreign(&mut self, active: u64) {
        let foreign: Vec<(WindowId, u64)> = self
            .windows
            .iter()
            .filter(|(id, window)| !self.floating.contains(id) && window.space != 0 && window.space != active)
            .map(|(id, window)| (*id, window.space))
            .collect();
        for (id, space) in foreign {
            if self.live_contains(id) {
                self.detach_live(id);
            }
            if !self.desk_contains(space, id) {
                self.stash(space, id);
            }
        }
    }

    fn adopt_current(&mut self, active: u64) {
        let missing: Vec<WindowId> = self
            .windows
            .keys()
            .copied()
            .filter(|id| {
                !self.floating.contains(id)
                    && self.windows.get(id).is_some_and(|window| window.space == 0 || window.space == active)
                    && !self.live_contains(*id)
            })
            .collect();
        for id in missing {
            self.detach_desks(id);
            let display = self
                .windows
                .get(&id)
                .and_then(|window| self.display_of(window.frame))
                .or_else(|| self.displays.first().map(|display| display.id));
            let Some(display) = display else {
                continue;
            };
            let column = self.make_column(vec![id], self.settings.default_width);
            if self.settings.desk_mode == DeskMode::Unified {
                self.unified.columns.push(column);
            } else {
                self.strips.entry(display).or_default().columns.push(column);
            }
        }
    }

    fn stash(&mut self, space: u64, id: WindowId) {
        if self.desk_contains(space, id) {
            return;
        }
        let display = self.windows.get(&id).and_then(|window| self.display_of(window.frame)).unwrap_or(0);
        let column = self.make_column(vec![id], self.settings.default_width);
        self.space_desks.entry(space).or_default().strips.entry(display).or_default().columns.push(column);
    }

    fn live_contains(&self, id: WindowId) -> bool {
        self.strips.values().any(|strip| strip.columns.iter().any(|column| column.windows.contains(&id)))
            || self.unified.columns.iter().any(|column| column.windows.contains(&id))
            || self.parked_screens.values().any(|strip| strip.columns.iter().any(|column| column.windows.contains(&id)))
    }

    fn desk_contains(&self, space: u64, id: WindowId) -> bool {
        self.space_desks.get(&space).is_some_and(|desk| {
            desk.strips.values().any(|strip| strip.columns.iter().any(|column| column.windows.contains(&id)))
                || desk.unified.columns.iter().any(|column| column.windows.contains(&id))
        })
    }

    fn detach_live(&mut self, id: WindowId) {
        for strip in self.strips.values_mut() {
            pull_window(strip, id);
        }
        pull_window(&mut self.unified, id);
    }

    fn detach_desks(&mut self, id: WindowId) {
        for desk in self.space_desks.values_mut() {
            for strip in desk.strips.values_mut() {
                pull_window(strip, id);
            }
            pull_window(&mut desk.unified, id);
        }
    }

    fn rehome_missing_displays(&mut self) {
        let known: HashSet<u32> = self.displays.iter().map(|display| display.id).collect();
        let stale: Vec<u32> = self.strips.keys().copied().filter(|id| !known.contains(id)).collect();
        let mut changed = !stale.is_empty();
        for id in stale {
            if let Some(strip) = self.strips.remove(&id) {
                if !strip.columns.is_empty() {
                    self.parked_screens.insert(id, strip);
                }
            }
        }
        let returned: Vec<u32> = self
            .parked_screens
            .keys()
            .copied()
            .filter(|id| known.contains(id))
            .collect();
        for id in returned {
            changed = true;
            if let Some(strip) = self.parked_screens.remove(&id) {
                let live = self.strips.entry(id).or_default();
                if live.columns.is_empty() {
                    *live = strip;
                } else {
                    live.columns.extend(strip.columns);
                }
            }
        }
        if changed {
            self.save();
        }
    }

    fn seed_from_windows(&mut self) {
        if !self.saved_desk_restore() {
            self.arrange_on_screens();
        }
        self.dirty_save = true;
    }

    fn arrange_on_screens(&mut self) {
        let ids: Vec<(WindowId, Rect)> = self
            .windows
            .iter()
            .filter(|(id, _)| !self.floating.contains(id) && !self.live_contains(**id))
            .map(|(id, window)| (*id, window.frame))
            .collect();
        for (id, frame) in ids {
            let Some(display) = self.display_of(frame) else {
                continue;
            };
            let usable = self.displays.iter().find(|item| item.id == display).map(|item| item.usable.width).unwrap_or(1400.0);
            let ratio = tidy_ratio((frame.width / usable).clamp(0.2, 1.0), &self.settings.presets);
            let mut column = self.make_column(vec![id], ratio);
            column.height_fill = 1.0;
            self.strips.entry(display).or_default().columns.push(column);
        }
        let order: std::collections::HashMap<u64, f64> = self
            .strips
            .values()
            .flat_map(|strip| strip.columns.iter())
            .map(|column| {
                let x = column
                    .windows
                    .first()
                    .and_then(|id| self.windows.get(id))
                    .map(|window| window.frame.x)
                    .unwrap_or(0.0);
                (column.id, x)
            })
            .collect();
        for strip in self.strips.values_mut() {
            strip.columns.sort_by(|left, right| {
                let lx = order.get(&left.id).copied().unwrap_or(0.0);
                let rx = order.get(&right.id).copied().unwrap_or(0.0);
                lx.partial_cmp(&rx).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    fn saved_desk_restore(&mut self) -> bool {
        let saved = store::load().desk;
        if saved.displays.is_empty() && saved.unified.is_empty() {
            return false;
        }
        let mut pool: Vec<WindowId> = self.windows.keys().copied().filter(|id| !self.floating.contains(id)).collect();
        if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns = self.claim_columns(&saved.unified, &mut pool);
        } else {
            let ordered = order_displays(self.displays.clone());
            let mut used = HashSet::new();
            for (index, display) in ordered.iter().enumerate() {
                let source = saved
                    .displays
                    .iter()
                    .find(|item| item.display_id == display.id)
                    .or_else(|| saved.displays.iter().find(|item| item.index == index && !used.contains(&item.display_id)));
                if let Some(source) = source {
                    used.insert(source.display_id);
                    let columns = self.claim_columns(&source.columns, &mut pool);
                    self.strips.insert(display.id, Strip { columns, scroll: source.scroll, scroll_target: source.scroll });
                }
            }
            let pending: Vec<SavedDisplay> = saved
                .displays
                .iter()
                .chain(saved.offline.iter())
                .filter(|item| !used.contains(&item.display_id) && !self.strips.contains_key(&item.display_id))
                .cloned()
                .collect();
            for source in pending {
                let columns = self.claim_columns(&source.columns, &mut pool);
                if columns.is_empty() {
                    continue;
                }
                let strip = Strip { columns, scroll: source.scroll, scroll_target: source.scroll };
                if self.displays.iter().any(|display| display.id == source.display_id) {
                    self.strips.insert(source.display_id, strip);
                } else {
                    self.parked_screens.insert(source.display_id, strip);
                }
                used.insert(source.display_id);
            }
        }
        for id in pool {
            let ratio = self.settings.default_width;
            let frame = self.windows.get(&id).map(|window| window.frame).unwrap_or(Rect::new(0.0, 0.0, 100.0, 100.0));
            let display = self.display_of(frame).or_else(|| self.displays.first().map(|display| display.id));
            let column = self.make_column(vec![id], ratio);
            if self.settings.desk_mode == DeskMode::Unified {
                self.unified.columns.push(column);
            } else if let Some(display) = display {
                self.strips.entry(display).or_default().columns.push(column);
            }
        }
        true
    }

    fn claim_columns(&mut self, saved: &[SavedColumn], pool: &mut Vec<WindowId>) -> Vec<Column> {
        let mut columns = Vec::new();
        for slot in saved {
            let mut windows = Vec::new();
            let count = slot.bundle_ids.len().max(slot.titles.len());
            for index in 0..count {
                let bundle = slot.bundle_ids.get(index).map(String::as_str).unwrap_or("");
                let title = slot.titles.get(index).map(String::as_str).unwrap_or("");
                let found = pool
                    .iter()
                    .position(|id| self.windows.get(id).is_some_and(|window| window.bundle_id == bundle && window.title == title))
                    .or_else(|| pool.iter().position(|id| self.windows.get(id).is_some_and(|window| window.bundle_id == bundle)));
                if let Some(found) = found {
                    windows.push(pool.remove(found));
                }
            }
            if windows.is_empty() {
                continue;
            }
            let mut column = self.make_column(windows, slot.width_ratio.max(0.15));
            column.height_weights = if slot.height_weights.is_empty() { vec![1.0] } else { slot.height_weights.clone() };
            column.mode = slot.mode.unwrap_or(ColumnMode::Stacked);
            column.pin = slot.pin;
            column.height_fill = if slot.height_fill <= 0.0 { 1.0 } else { slot.height_fill };
            column.vertical = slot.vertical;
            column.arrange_lock = slot.arrange_lock;
            columns.push(column);
        }
        columns
    }

    fn apply(&mut self) {
        self.needs_apply = false;
        if !self.settings.tiling_enabled || self.displays.is_empty() {
            self.refresh_status();
            return;
        }
        self.sanitize();
        let request = self.layout_request();
        let placed = layout(&request);
        self.column_frames.clear();
        for (id, placement) in &placed {
            if placement.parked {
                continue;
            }
            if let Some(column_id) = self.column_id_of(*id) {
                self.column_frames
                    .entry(column_id)
                    .and_modify(|frame| *frame = union_rect(*frame, placement.frame))
                    .or_insert(placement.frame);
            }
        }
        for (id, placement) in &placed {
            let Some(window) = self.windows.get_mut(id) else {
                continue;
            };
            let previous = self.placements.get(id);
            if previous.is_some_and(|prior| near(prior.frame, placement.frame) && prior.parked == placement.parked) {
                continue;
            }
            let size_same = previous.is_some_and(|prior| {
                !prior.parked
                    && !placement.parked
                    && (prior.frame.width - placement.frame.width).abs() < 0.75
                    && (prior.frame.height - placement.frame.height).abs() < 0.75
            });
            if size_same {
                let x = placement.frame.x;
                let y = self.primary_height - placement.frame.y - placement.frame.height;
                if !spaces::move_window(id.number, x, y) {
                    ax::set_position(window.element, placement.frame, self.primary_height);
                }
            } else {
                ax::set_frame(window.element, placement.frame, self.primary_height);
            }
            if !self.animating && !placement.cropped && !window.checked_size && !placement.parked {
                if let Some(actual) = ax::read_frame(window.element, self.primary_height) {
                    if (actual.width - placement.frame.width).abs() > 16.0 {
                        window.rigid_width = Some(actual.width);
                        self.needs_apply = true;
                    }
                }
                window.checked_size = true;
            }
        }
        self.placements = placed;
        if !self.animating && !self.gesture_active() {
            self.raise_front_tabs();
            self.publish_snapshot();
            self.dirty_ui = true;
        } else {
            self.refresh_indicator();
            if self.snapshot.indicator_visible {
                self.dirty_ui = true;
            }
        }
    }

    fn layout_request(&self) -> LayoutRequest {
        let mut columns_by_display = HashMap::new();
        let mut scroll_by_display = HashMap::new();
        for display in &self.displays {
            let strip = self.strips.get(&display.id);
            let columns = strip
                .map(|strip| strip.columns.iter().map(|column| self.column_state(column, display.usable.width)).collect())
                .unwrap_or_default();
            scroll_by_display.insert(display.id, strip.map(|strip| strip.scroll).unwrap_or(0.0));
            columns_by_display.insert(display.id, columns);
        }
        let unit = self.displays.first().map(|display| display.usable.width).unwrap_or(1400.0);
        LayoutRequest {
            displays: self.displays.clone(),
            mode: self.settings.desk_mode,
            gap: self.settings.gap,
            columns_by_display,
            scroll_by_display,
            unified_columns: self.unified.columns.iter().map(|column| self.column_state(column, unit)).collect(),
            unified_scroll: self.unified.scroll,
            keep_original_height: self.settings.keep_original_height,
        }
    }

    fn column_state(&self, column: &Column, usable_width: f64) -> ColumnState {
        let rigid = column
            .windows
            .iter()
            .filter_map(|id| self.windows.get(id).and_then(|window| window.rigid_width))
            .fold(0.0, f64::max);
        let width = if rigid > 40.0 {
            rigid
        } else {
            (column.width_ratio * usable_width).max(160.0)
        };
        let natural_heights = column
            .windows
            .iter()
            .map(|id| self.windows.get(id).map(|window| window.natural_height).unwrap_or(0.0))
            .collect();
        let active = self
            .focused
            .and_then(|focused| column.windows.iter().position(|id| *id == focused))
            .unwrap_or(column.active_index);
        ColumnState {
            id: column.id,
            windows: column.windows.clone(),
            width,
            height_weights: column.height_weights.clone(),
            mode: column.mode,
            active_index: active.min(column.windows.len().saturating_sub(1)),
            pin: column.pin,
            natural_heights,
            height_fill: column.height_fill,
            vertical: column.vertical,
            arrange_lock: column.arrange_lock,
        }
    }

    fn step_scroll(&mut self) {
        let mut moving = false;
        if self.settings.desk_mode == DeskMode::Unified {
            moving = step_value(&mut self.unified.scroll, self.unified.scroll_target);
        } else {
            for strip in self.strips.values_mut() {
                moving |= step_value(&mut strip.scroll, strip.scroll_target);
            }
        }
        self.animating = moving;
    }

    fn scroll_by(&mut self, delta: f64, x: f64, y: f64) {
        let advance = |current: f64| (current - delta).max(0.0);
        if self.settings.desk_mode == DeskMode::Unified {
            let current = self.unified.scroll;
            follow_scroll(&mut self.unified, advance(current));
        } else if self.settings.linked_scroll {
            for strip in self.strips.values_mut() {
                follow_scroll(strip, advance(strip.scroll));
            }
        } else if let Some(display) = self.scroll_display(x, y) {
            if let Some(strip) = self.strips.get_mut(&display) {
                follow_scroll(strip, advance(strip.scroll));
            }
            self.shown_display = display;
        }
        self.animating = true;
        self.needs_apply = true;
        self.gesture_until = Some(Instant::now() + Duration::from_millis(140));
        self.reveal_indicator();
        self.apply();
    }

    fn gesture_active(&self) -> bool {
        self.gesture_until.is_some_and(|until| Instant::now() < until)
    }

    pub fn flush(&mut self) {
        if self.needs_apply && !self.animating {
            self.apply();
        }
    }

    pub fn reveal_activated(&mut self, pid: i32) {
        if !self.settings.tiling_enabled || pid <= 0 || pid == std::process::id() as i32 {
            return;
        }
        ax::deminiaturize(pid);
        if self.window_for_app(pid).is_none() {
            self.reconcile();
        }
        let Some(id) = self.window_for_app(pid) else {
            return;
        };
        self.focus_window(id, false);
        if let Some(column_id) = self.column_id_of(id) {
            self.align_column(column_id);
        }
        self.animating = false;
        self.apply();
    }

    fn window_for_app(&self, pid: i32) -> Option<WindowId> {
        if self.focused.is_some_and(|id| id.pid == pid && self.live_contains(id)) {
            return self.focused;
        }
        if self.settings.desk_mode == DeskMode::Unified {
            return self
                .unified
                .columns
                .iter()
                .flat_map(|column| column.windows.iter().copied())
                .find(|id| id.pid == pid);
        }
        for display in &self.displays {
            let Some(strip) = self.strips.get(&display.id) else {
                continue;
            };
            if let Some(id) = strip
                .columns
                .iter()
                .flat_map(|column| column.windows.iter().copied())
                .find(|id| id.pid == pid)
            {
                return Some(id);
            }
        }
        None
    }

    fn align_column(&mut self, column_id: u64) {
        if self.settings.desk_mode == DeskMode::Unified {
            let (viewport, content, origin, _) = self.unified_column_metrics(column_id);
            follow_scroll(&mut self.unified, clamp_scroll(origin, viewport, content));
            return;
        }
        let Some(display) = self.column_display(column_id) else {
            return;
        };
        let (viewport, content, origin, _) = self.display_column_metrics(display, column_id);
        let next = clamp_scroll(origin, viewport, content);
        if let Some(strip) = self.strips.get_mut(&display) {
            follow_scroll(strip, next);
        }
        self.shown_display = display;
    }

    fn reveal_focused(&mut self, force_center: bool) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let mode = if force_center { CenterMode::Always } else { self.settings.center_mode };
        if self.settings.desk_mode == DeskMode::Unified {
            let (viewport, content, origin, width) = self.unified_column_metrics(column_id);
            let target = scroll_target(self.unified.scroll, viewport, content, origin, width, mode);
            self.unified.scroll_target = target;
        } else if let Some(display) = self.column_display(column_id) {
            let before = self.strips.get(&display).map(|strip| strip.scroll_target).unwrap_or(0.0);
            let (viewport, content, origin, width) = self.display_column_metrics(display, column_id);
            let target = scroll_target(before, viewport, content, origin, width, mode);
            if self.settings.linked_scroll {
                let delta = target - before;
                for strip in self.strips.values_mut() {
                    strip.scroll_target = (strip.scroll_target + delta).max(0.0);
                }
            } else if let Some(strip) = self.strips.get_mut(&display) {
                strip.scroll_target = target;
            }
        }
        self.animating = true;
        self.reveal_indicator();
    }

    fn focus_horizontal(&mut self, direction: isize) {
        let visual = self.visual_ids();
        let current = self.focused.and_then(|id| self.column_id_of(id));
        let index = current.and_then(|id| visual.iter().position(|column| *column == id));
        if let Some(index) = index {
            let next = index as isize + direction;
            if next >= 0 && (next as usize) < visual.len() {
                self.focus_column(visual[next as usize]);
                return;
            }
        }
        if self.settings.desk_mode == DeskMode::Independent {
            self.focus_next_display_direction(direction);
        }
    }

    fn focus_vertical(&mut self, direction: isize) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let windows = self.column_by_id(column_id).map(|column| column.windows.clone()).unwrap_or_default();
        let Some(index) = windows.iter().position(|id| *id == focused) else {
            return;
        };
        let next = index as isize + direction;
        if next >= 0 && (next as usize) < windows.len() {
            if let Some(column) = self.column_by_id_mut(column_id) {
                column.active_index = next as usize;
            }
            self.focus_window(windows[next as usize], false);
            self.needs_apply = true;
        }
    }

    fn focus_end(&mut self, first: bool) {
        let visual = self.visual_ids();
        let Some(id) = if first { visual.first() } else { visual.last() }.copied() else {
            return;
        };
        self.focus_column(id);
    }

    fn focus_next_display(&mut self) {
        self.focus_next_display_direction(1);
    }

    fn focus_next_display_direction(&mut self, direction: isize) {
        let ordered = order_displays(self.displays.clone());
        if ordered.is_empty() {
            return;
        }
        let current = self.focused_display().or_else(|| ordered.first().map(|display| display.id));
        let Some(current) = current else {
            return;
        };
        let index = ordered.iter().position(|display| display.id == current).unwrap_or(0);
        let next = (index as isize + direction).rem_euclid(ordered.len() as isize) as usize;
        let display = ordered[next].id;
        if let Some(window) = self.focus_by_display.get(&display).copied() {
            if self.windows.contains_key(&window) {
                self.focus_window(window, true);
                return;
            }
        }
        if let Some(column) = self.strips.get(&display).and_then(|strip| strip.columns.first()) {
            if let Some(window) = column.windows.first().copied() {
                self.focus_window(window, true);
            }
        }
    }

    fn move_horizontal(&mut self, direction: isize) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.pin = None;
        }
        let visual = self.visual_ids();
        let Some(index) = visual.iter().position(|id| *id == column_id) else {
            return;
        };
        let next = index as isize + direction;
        if next >= 0 && (next as usize) < visual.len() {
            let before = if direction > 0 {
                visual.get(next as usize + 1).copied()
            } else {
                Some(visual[next as usize])
            };
            let display = self.column_display(column_id).unwrap_or(0);
            self.move_column(column_id, display, before);
            return;
        }
        self.move_to_display_direction(column_id, direction);
    }

    fn move_vertical(&mut self, direction: isize) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let Some(index) = self.column_by_id(column_id).and_then(|column| column.windows.iter().position(|id| *id == focused)) else {
            return;
        };
        let next = index as isize + direction;
        let len = self.column_by_id(column_id).map(|column| column.windows.len()).unwrap_or(0);
        if next < 0 || next as usize >= len {
            return;
        }
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.windows.swap(index, next as usize);
            if column.height_weights.len() == len {
                column.height_weights.swap(index, next as usize);
            }
        }
        self.needs_apply = true;
        self.dirty_save = true;
        self.reveal_indicator();
    }

    fn move_to_next_display(&mut self) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        self.move_to_display_direction(column_id, 1);
    }

    fn move_to_display_direction(&mut self, column_id: u64, direction: isize) {
        let ordered = order_displays(self.displays.clone());
        if ordered.len() < 2 {
            return;
        }
        let current = self.column_display(column_id).or_else(|| ordered.first().map(|display| display.id));
        let Some(current) = current else {
            return;
        };
        let index = ordered.iter().position(|display| display.id == current).unwrap_or(0);
        let next = ordered[(index as isize + direction).rem_euclid(ordered.len() as isize) as usize].id;
        self.move_column(column_id, next, None);
        if let Some(window) = self.column_by_id(column_id).and_then(|column| column.windows.first()).copied() {
            self.focus_window(window, true);
        }
    }

    fn stack_into(&mut self, direction: isize) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let visual = self.visual_ids();
        let Some(index) = visual.iter().position(|id| *id == column_id) else {
            return;
        };
        let neighbor = index as isize + direction;
        if neighbor < 0 || neighbor as usize >= visual.len() {
            return;
        }
        let neighbor_id = visual[neighbor as usize];
        self.take_window_from_column(column_id, focused);
        if let Some(column) = self.column_by_id_mut(neighbor_id) {
            column.windows.push(focused);
            column.height_weights.push(1.0);
            column.mode = ColumnMode::Stacked;
        }
        self.needs_apply = true;
        self.dirty_save = true;
        self.reveal_focused(false);
    }

    fn pop_out(&mut self, direction: isize) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let count = self.column_by_id(column_id).map(|column| column.windows.len()).unwrap_or(0);
        if count < 2 {
            return;
        }
        self.take_window_from_column(column_id, focused);
        let ratio = self.settings.default_width;
        let column = self.make_column(vec![focused], ratio);
        let new_id = column.id;
        self.insert_column_near(column_id, column, direction);
        self.focus_column(new_id);
        self.dirty_save = true;
    }

    fn pin(&mut self, edge: PinEdge) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        let already = self.column_by_id(column_id).and_then(|column| column.pin) == Some(edge);
        self.for_each_column_mut(|column| {
            if column.pin == Some(edge) {
                column.pin = None;
            }
        });
        if !already {
            if let Some(column) = self.column_by_id_mut(column_id) {
                column.pin = Some(edge);
            }
        }
        self.finish_setting();
        self.reveal_indicator();
    }

    fn cycle_width(&mut self) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        let mut presets = self.settings.presets.clone();
        presets.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        let current = self.column_by_id(column_id).map(|column| column.width_ratio).unwrap_or(0.5);
        let next = presets.iter().copied().find(|preset| *preset > current + 0.02).unwrap_or(presets.first().copied().unwrap_or(0.5));
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.width_ratio = next;
            column.arrange_lock = false;
        }
        self.clear_rigid(column_id);
        self.finish_setting();
        self.reveal_focused(false);
    }

    fn set_full_width(&mut self) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.width_ratio = 1.0;
            column.height_fill = 1.0;
            column.vertical = VerticalAlign::Top;
            column.arrange_lock = false;
            column.pin = None;
        }
        self.clear_rigid(column_id);
        self.finish_setting();
        self.reveal_focused(true);
    }

    pub fn arrange(&mut self, kind: Arrange) {
        if !self.settings.tiling_enabled {
            self.set_tiling(true);
            if !self.settings.tiling_enabled {
                return;
            }
        }
        if let Some(pane) = kind.pane() {
            if !self.place_focused_pane(pane) {
                return;
            }
        } else if let Some(stacks) = kind.stacks() {
            if !self.fill_screen(stacks) {
                return;
            }
        }
        self.snapshot.status = format!("Arranged {}", kind.title());
        self.dirty_ui = true;
        self.reveal_indicator();
    }

    fn place_focused_pane(&mut self, pane: Pane) -> bool {
        let Some(column_id) = self.isolate_focused() else {
            self.snapshot.status = "Focus a window first.".into();
            self.dirty_ui = true;
            return false;
        };
        if let Some(edge) = pane.pin {
            self.for_each_column_mut(|column| {
                if column.pin == Some(edge) {
                    column.pin = None;
                }
            });
        }
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.width_ratio = pane.fraction.clamp(0.15, 1.0);
            column.height_fill = pane.height.clamp(0.2, 1.0);
            column.vertical = pane.vertical;
            column.pin = pane.pin;
            column.arrange_lock = true;
            column.mode = ColumnMode::Stacked;
        }
        self.clear_rigid(column_id);
        if pane.pin.is_none() {
            self.move_column_first(column_id);
        }
        let viewport = self.column_viewport(column_id);
        let scroll = if pane.center {
            let width = pane.fraction.clamp(0.15, 1.0) * viewport;
            -((viewport - width) / 2.0)
        } else if pane.pin.is_none() {
            0.0
        } else {
            self.column_scroll(column_id)
        };
        self.set_column_scroll(column_id, scroll);
        self.finish_setting();
        self.apply();
        true
    }

    fn fill_screen(&mut self, stacks: &[openlayout::engine::Stack]) -> bool {
        let windows = self.screen_windows();
        if windows.is_empty() {
            self.snapshot.status = "No windows on this screen.".into();
            self.dirty_ui = true;
            return false;
        }
        let (groups, extra) = take_groups(stacks, &windows);
        let viewport = self.screen_viewport();
        let shares: Vec<f64> = groups.iter().map(|(stack, _)| stack.share).collect();
        let widths = ratios(&shares, self.settings.gap, viewport);
        let mut columns = Vec::new();
        for ((stack, ids), ratio) in groups.into_iter().zip(widths) {
            let mut column = self.make_column(ids, ratio);
            column.arrange_lock = true;
            column.height_fill = 1.0;
            column.vertical = VerticalAlign::Top;
            if !stack.weights.is_empty() {
                column.height_weights = stack.weights.to_vec();
            }
            columns.push(column);
        }
        for id in extra {
            columns.push(self.make_column(vec![id], self.settings.default_width));
        }
        self.replace_screen_columns(columns);
        self.finish_setting();
        self.apply();
        true
    }

    fn isolate_focused(&mut self) -> Option<u64> {
        let focused = self.focused?;
        let column_id = self.column_id_of(focused)?;
        let len = self.column_by_id(column_id).map(|column| column.windows.len()).unwrap_or(0);
        if len <= 1 {
            return Some(column_id);
        }
        self.take_window_from_column(column_id, focused);
        let column = self.make_column(vec![focused], self.settings.default_width);
        let id = column.id;
        if self.column_by_id(column_id).is_some() {
            self.insert_column_near(column_id, column, 1);
        } else {
            self.insert_column_first(column);
        }
        Some(id)
    }

    fn screen_windows(&self) -> Vec<WindowId> {
        let mut ids = Vec::new();
        for column in self.screen_columns() {
            ids.extend(column.windows.iter().copied().filter(|id| !self.floating.contains(id)));
        }
        if let Some(focused) = self.focused {
            if let Some(index) = ids.iter().position(|id| *id == focused) {
                let id = ids.remove(index);
                ids.insert(0, id);
            }
        }
        ids
    }

    fn screen_columns(&self) -> Vec<Column> {
        if self.settings.desk_mode == DeskMode::Unified {
            return self.unified.columns.clone();
        }
        let display = self.focused_display().or_else(|| self.displays.first().map(|display| display.id));
        display.and_then(|id| self.strips.get(&id).map(|strip| strip.columns.clone())).unwrap_or_default()
    }

    fn screen_viewport(&self) -> f64 {
        if self.settings.desk_mode == DeskMode::Unified {
            return self.displays.iter().map(|display| display.usable.width).sum::<f64>().max(320.0);
        }
        let display = self.focused_display().or_else(|| self.displays.first().map(|display| display.id));
        self.displays.iter().find(|item| Some(item.id) == display).map(|item| item.usable.width).unwrap_or(1400.0)
    }

    fn replace_screen_columns(&mut self, columns: Vec<Column>) {
        if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns = columns;
            follow_scroll(&mut self.unified, 0.0);
            return;
        }
        let display = self.focused_display().or_else(|| self.displays.first().map(|display| display.id));
        if let Some(display) = display {
            let strip = self.strips.entry(display).or_default();
            strip.columns = columns;
            follow_scroll(strip, 0.0);
        }
    }

    fn move_column_first(&mut self, id: u64) {
        let display = self.column_display(id);
        let Some(column) = self.take_column(id) else {
            return;
        };
        self.insert_column_first_on(display, column);
    }

    fn insert_column_first(&mut self, column: Column) {
        let display = self.focused_display().or_else(|| self.displays.first().map(|display| display.id));
        self.insert_column_first_on(display, column);
    }

    fn insert_column_first_on(&mut self, display: Option<u32>, column: Column) {
        if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns.insert(0, column);
            return;
        }
        if let Some(display) = display {
            self.strips.entry(display).or_default().columns.insert(0, column);
        }
    }

    fn column_viewport(&self, column_id: u64) -> f64 {
        if self.settings.desk_mode == DeskMode::Unified {
            return self.screen_viewport();
        }
        let display = self.column_display(column_id);
        self.displays.iter().find(|item| Some(item.id) == display).map(|item| item.usable.width).unwrap_or(1400.0)
    }

    fn column_scroll(&self, column_id: u64) -> f64 {
        if self.settings.desk_mode == DeskMode::Unified {
            return self.unified.scroll;
        }
        self.column_display(column_id).and_then(|id| self.strips.get(&id).map(|strip| strip.scroll)).unwrap_or(0.0)
    }

    fn set_column_scroll(&mut self, column_id: u64, scroll: f64) {
        if self.settings.desk_mode == DeskMode::Unified {
            follow_scroll(&mut self.unified, scroll);
            return;
        }
        if let Some(display) = self.column_display(column_id) {
            if let Some(strip) = self.strips.get_mut(&display) {
                follow_scroll(strip, scroll);
            }
        }
    }

    fn nudge_width(&mut self, delta: f64) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.width_ratio = (column.width_ratio + delta).clamp(0.15, 1.0);
            column.arrange_lock = false;
        }
        self.clear_rigid(column_id);
        self.finish_setting();
    }

    fn nudge_height(&mut self, delta: f64) {
        let Some(focused) = self.focused else {
            return;
        };
        let Some(column_id) = self.column_id_of(focused) else {
            return;
        };
        let len = self.column_by_id(column_id).map(|column| column.windows.len()).unwrap_or(0);
        if len <= 1 {
            if let Some(column) = self.column_by_id_mut(column_id) {
                column.height_fill = (column.height_fill + delta).clamp(0.35, 1.0);
            }
        } else if let Some(column) = self.column_by_id_mut(column_id) {
            if column.height_weights.len() != len {
                column.height_weights = vec![1.0; len];
            }
            if let Some(index) = column.windows.iter().position(|id| *id == focused) {
                column.height_weights[index] = (column.height_weights[index] + delta).max(0.2);
            }
        }
        self.finish_setting();
    }

    fn equalize(&mut self) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.height_weights = vec![1.0; column.windows.len().max(1)];
            column.height_fill = 1.0;
        }
        self.finish_setting();
    }

    fn toggle_tabs(&mut self) {
        let Some(column_id) = self.focused.and_then(|id| self.column_id_of(id)) else {
            return;
        };
        if let Some(column) = self.column_by_id_mut(column_id) {
            column.mode = if column.mode == ColumnMode::Tabbed { ColumnMode::Stacked } else { ColumnMode::Tabbed };
        }
        self.finish_setting();
    }

    fn toggle_float(&mut self) {
        let Some(focused) = self.focused else {
            return;
        };
        if self.floating.contains(&focused) {
            self.floating.remove(&focused);
            let ratio = self.settings.default_width;
            let column = self.make_column(vec![focused], ratio);
            if self.settings.desk_mode == DeskMode::Unified {
                self.unified.columns.push(column);
            } else {
                let display = self
                    .windows
                    .get(&focused)
                    .and_then(|window| self.display_of(window.frame))
                    .or_else(|| self.displays.first().map(|display| display.id));
                if let Some(display) = display {
                    self.strips.entry(display).or_default().columns.push(column);
                }
            }
            self.focus_window(focused, true);
        } else if let Some(column_id) = self.column_id_of(focused) {
            self.take_window_from_column(column_id, focused);
            self.floating.insert(focused);
            self.focused = None;
            self.needs_apply = true;
            self.dirty_save = true;
        }
        self.dirty_ui = true;
    }

    fn jump(&mut self, index: usize) {
        let visual = self.visual_ids();
        if let Some(id) = visual.get(index).copied() {
            self.focus_column(id);
        }
    }

    fn center_focused(&mut self) {
        self.reveal_focused(true);
    }

    fn cycle_profile(&mut self) {
        if self.profiles.is_empty() {
            self.snapshot.status = "Save a profile before cycling them.".into();
            self.dirty_ui = true;
            return;
        }
        self.profile_cursor %= self.profiles.len();
        let profile = self.profiles[self.profile_cursor].clone();
        self.profile_cursor = (self.profile_cursor + 1) % self.profiles.len();
        self.apply_profile(&profile);
    }

    fn apply_profile(&mut self, profile: &Profile) {
        if !self.settings.tiling_enabled {
            self.set_tiling(true);
        }
        self.set_mode(profile.mode);
        self.settings.linked_scroll = profile.linked_scroll;
        let candidates: Vec<WindowCandidate> = self
            .windows
            .iter()
            .map(|(id, window)| WindowCandidate {
                id: *id,
                bundle_id: window.bundle_id.clone(),
                title: window.title.clone(),
            })
            .collect();
        let assigned = assign_profile(profile, &candidates);
        let used: HashSet<WindowId> = assigned.iter().flat_map(|column| column.window_ids.iter().copied()).collect();
        self.strips.clear();
        self.unified.columns.clear();
        let ordered = order_displays(self.displays.clone());
        for column in assigned {
            self.place_assigned(column, &ordered);
        }
        let extras: Vec<WindowId> = self
            .windows
            .keys()
            .copied()
            .filter(|id| !used.contains(id) && !self.floating.contains(id))
            .collect();
        for id in extras {
            let column = self.make_column(vec![id], self.settings.default_width);
            if self.settings.desk_mode == DeskMode::Unified {
                self.unified.columns.push(column);
            } else if let Some(display) = ordered.first() {
                self.strips.entry(display.id).or_default().columns.push(column);
            }
        }
        self.reveal_focused(false);
        self.finish_setting();
        self.snapshot.status = format!("Applied profile {}", profile.name);
    }

    fn place_assigned(&mut self, assigned: AssignedColumn, ordered: &[DisplaySlice]) {
        let mut column = self.make_column(assigned.window_ids, assigned.width_ratio);
        column.height_weights = assigned.height_weights;
        column.mode = assigned.mode;
        column.pin = assigned.pin;
        column.height_fill = assigned.height_fill;
        if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns.push(column);
        } else {
            let display = ordered.get(assigned.display_index).or_else(|| ordered.first());
            if let Some(display) = display {
                self.strips.entry(display.id).or_default().columns.push(column);
            }
        }
    }

    fn focus_window(&mut self, id: WindowId, warp: bool) {
        let Some(window) = self.windows.get(&id) else {
            return;
        };
        let element = window.element;
        let pid = id.pid;
        let frame = window.frame;
        ax::focus(element, pid);
        self.focused = Some(id);
        if let Some(display) = self.display_of(frame) {
            self.focus_by_display.insert(display, id);
        }
        if let Some(column_id) = self.column_id_of(id) {
            if let Some(column) = self.column_by_id_mut(column_id) {
                if let Some(index) = column.windows.iter().position(|window| *window == id) {
                    column.active_index = index;
                }
            }
        }
        if warp && self.settings.mouse_follows_focus {
            ax::warp_cursor(frame, self.primary_height);
            self.suppress_mouse_until = Some(Instant::now() + Duration::from_millis(350));
        }
        self.reveal_focused(false);
        self.needs_apply = true;
    }

    fn note_mouse(&mut self, x: f64, y: f64, kind: MouseKind) {
        let cocoa_y = self.primary_height - y;
        match kind {
            MouseKind::Down => {
                self.drag = self.hit_window(x, cocoa_y).map(|window| Drag { window, x, y: cocoa_y });
                self.dragging = self.drag.is_some();
            }
            MouseKind::Up => {
                self.dragging = false;
                if let Some(drag) = self.drag.take() {
                    let distance = (drag.x - x).abs() + (drag.y - cocoa_y).abs();
                    if distance > 16.0 {
                        self.drop_window(drag.window, x, cocoa_y);
                    }
                }
            }
            MouseKind::Moved => {
                if !self.settings.focus_follows_mouse || self.dragging {
                    return;
                }
                if self.suppress_mouse_until.is_some_and(|until| Instant::now() < until) {
                    return;
                }
                if let Some(id) = self.hit_window(x, cocoa_y) {
                    if self.focused != Some(id) && !self.floating.contains(&id) {
                        self.focus_window(id, false);
                    }
                }
            }
        }
    }

    fn drop_window(&mut self, id: WindowId, x: f64, y: f64) {
        let Some(source) = self.column_id_of(id) else {
            return;
        };
        let target = self.column_frames.iter().find(|(_, frame)| frame.contains(x, y)).map(|(column, _)| *column);
        if target == Some(source) {
            return;
        }
        let display = self.display_of(Rect::new(x, y, 1.0, 1.0)).unwrap_or(0);
        let count = self.column_by_id(source).map(|column| column.windows.len()).unwrap_or(1);
        let moving = if count > 1 {
            self.take_window_from_column(source, id);
            let column = self.make_column(vec![id], self.settings.default_width);
            let new_id = column.id;
            self.insert_column_near(source, column, 1);
            new_id
        } else {
            source
        };
        self.move_column(moving, display, target);
    }

    fn hit_window(&self, x: f64, y: f64) -> Option<WindowId> {
        self.placements.iter().find(|(_, placement)| !placement.parked && placement.frame.contains(x, y)).map(|(id, _)| *id)
    }

    fn capture(&mut self, code: u16, bits: u8) {
        let Some(action) = self.recording.take() else {
            return;
        };
        let modifiers = openlayout::engine::Mod::new(bits);
        self.settings.bindings.retain(|binding| binding.action != action);
        self.settings.bindings.retain(|binding| !(binding.key_code == code && binding.modifiers == modifiers));
        self.settings.bindings.push(Binding::new(code, modifiers, action));
        self.sync_hotkeys();
        self.finish_setting();
    }

    fn reveal_indicator(&mut self) {
        self.indicator_until = Some(Instant::now() + Duration::from_millis(700));
        self.snapshot.indicator_visible = true;
        self.dirty_ui = true;
    }

    fn publish_snapshot(&mut self) {
        let mut columns = Vec::new();
        for column_id in self.visual_ids() {
            let Some(column) = self.column_by_id(column_id) else {
                continue;
            };
            let title = column
                .windows
                .iter()
                .find_map(|id| self.windows.get(id).map(|window| window.title.clone()))
                .filter(|title| !title.is_empty())
                .or_else(|| column.windows.iter().find_map(|id| self.windows.get(id).map(|window| window.app_name.clone())))
                .unwrap_or_else(|| "Window".into());
            let app_name = column
                .windows
                .first()
                .and_then(|id| self.windows.get(id).map(|window| window.app_name.clone()))
                .unwrap_or_default();
            let focused = column.windows.iter().any(|id| self.focused == Some(*id));
            if let Some(frame) = self.column_frames.get(&column_id).copied() {
                columns.push(SnapshotColumn {
                    id: column_id,
                    display_id: self.column_display(column_id).unwrap_or(placement_display(&self.placements, &column.windows)),
                    title,
                    app_name,
                    frame,
                    focused,
                    pinned: column.pin.is_some(),
                    windows: column.windows.len(),
                });
            }
        }
        let focus_frame = self.focused.and_then(|id| self.placements.get(&id)).filter(|placement| !placement.parked).map(|placement| placement.frame);
        self.snapshot.displays = self.displays.clone();
        self.snapshot.columns = columns;
        self.snapshot.focus_frame = focus_frame;
        self.snapshot.tiling = self.settings.tiling_enabled;
        self.snapshot.trusted = self.trusted;
        self.snapshot.input_monitoring = self.input_monitoring;
        self.snapshot.window_count = self.windows.len();
        self.refresh_status();
        self.refresh_indicator();
    }

    fn refresh_indicator(&mut self) {
        let (display, tiles) = self.indicator_tiles();
        let visible = self.settings.tiling_enabled
            && !tiles.is_empty()
            && (self.animating || self.gesture_active() || self.indicator_until.is_some_and(|until| Instant::now() < until));
        self.snapshot.indicator = tiles;
        self.snapshot.indicator_visible = visible;
        self.snapshot.indicator_display = display;
    }

    fn indicator_tiles(&self) -> (u32, Vec<IndicatorTile>) {
        let (display, columns, scroll, usable) = self.indicator_source();
        let states: Vec<ColumnState> = columns.iter().map(|column| self.column_state(column, usable)).collect();
        let measured = if self.settings.desk_mode == DeskMode::Unified {
            unified_metrics(&self.displays, &states, self.settings.gap)
        } else {
            metrics(&states, self.settings.gap, usable)
        };
        let mut ordered = Vec::new();
        ordered.extend(columns.iter().filter(|column| column.pin == Some(PinEdge::Left)));
        ordered.extend(columns.iter().filter(|column| column.pin.is_none()));
        ordered.extend(columns.iter().filter(|column| column.pin == Some(PinEdge::Right)));
        let tiles = ordered
            .into_iter()
            .map(|column| {
                let width = states.iter().find(|state| state.id == column.id).map(|state| state.width).unwrap_or(0.0);
                let on_screen = measured.origins.get(&column.id).copied().is_none_or(|origin| {
                    origin < scroll + measured.viewport_width && origin + width > scroll
                });
                let title = column
                    .windows
                    .iter()
                    .find_map(|id| self.windows.get(id).map(|window| window.title.clone()))
                    .filter(|title| !title.is_empty())
                    .or_else(|| column.windows.iter().find_map(|id| self.windows.get(id).map(|window| window.app_name.clone())))
                    .unwrap_or_else(|| "Window".into());
                let focused = column.windows.iter().any(|id| self.focused == Some(*id));
                IndicatorTile { column_id: column.id, title, focused, on_screen }
            })
            .collect();
        (display, tiles)
    }

    fn indicator_source(&self) -> (u32, Vec<Column>, f64, f64) {
        let display = self.shown_display_or_focused();
        if self.settings.desk_mode == DeskMode::Unified {
            let unit = self.displays.first().map(|item| item.usable.width).unwrap_or(1400.0);
            return (display, self.unified.columns.clone(), self.unified.scroll, unit);
        }
        let scroll = self.strips.get(&display).map(|strip| strip.scroll).unwrap_or(0.0);
        let columns = self.strips.get(&display).map(|strip| strip.columns.clone()).unwrap_or_default();
        let usable = self.displays.iter().find(|item| item.id == display).map(|item| item.usable.width).unwrap_or(1400.0);
        (display, columns, scroll, usable)
    }

    fn shown_display_or_focused(&self) -> u32 {
        if self.shown_display != 0
            && (self.settings.desk_mode == DeskMode::Unified || self.strips.contains_key(&self.shown_display))
        {
            return self.shown_display;
        }
        self.focused_display()
            .or_else(|| self.displays.first().map(|display| display.id))
            .unwrap_or(0)
    }

    fn raise_front_tabs(&self) {
        let columns: Vec<Column> = if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns.clone()
        } else {
            self.strips.values().flat_map(|strip| strip.columns.iter().cloned()).collect()
        };
        for column in columns {
            if column.mode != ColumnMode::Tabbed || column.windows.len() < 2 {
                continue;
            }
            let index = column.active_index.min(column.windows.len() - 1);
            let Some(id) = column.windows.get(index).copied() else {
                continue;
            };
            if let Some(window) = self.windows.get(&id) {
                ax::raise(window.element);
            }
        }
    }

    fn refresh_status(&mut self) {
        self.snapshot.status = if !self.trusted {
            "Accessibility is off.".into()
        } else if !self.input_monitoring {
            "Accessibility is on. Input Monitoring is off, so scrolling stays blocked.".into()
        } else if !self.settings.tiling_enabled {
            format!("Accessibility is on. {} displays connected. Tiling is paused.", self.displays.len().max(screens::screens().len()))
        } else {
            format!("Managing {} windows across {} displays.", self.snapshot.window_count, self.displays.len())
        };
        self.snapshot.trusted = self.trusted;
        self.snapshot.input_monitoring = self.input_monitoring;
        self.snapshot.tiling = self.settings.tiling_enabled;
        self.dirty_ui = true;
    }

    fn profile_slots(&self) -> Vec<ProfileSlot> {
        let ordered = order_displays(self.displays.clone());
        let mut slots = Vec::new();
        let mut key = 0u64;
        if self.settings.desk_mode == DeskMode::Unified {
            for column in &self.unified.columns {
                key += 1;
                slots.extend(self.slots_for(column, 0, key));
            }
        } else {
            for (index, display) in ordered.iter().enumerate() {
                if let Some(strip) = self.strips.get(&display.id) {
                    for column in &strip.columns {
                        key += 1;
                        slots.extend(self.slots_for(column, index, key));
                    }
                }
            }
        }
        slots
    }

    fn slots_for(&self, column: &Column, display_index: usize, column_key: u64) -> Vec<ProfileSlot> {
        column
            .windows
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let window = self.windows.get(id)?;
                Some(ProfileSlot {
                    bundle_id: window.bundle_id.clone(),
                    title: window.title.clone(),
                    width_ratio: column.width_ratio,
                    height_weight: column.height_weights.get(index).copied().unwrap_or(1.0),
                    mode: column.mode,
                    pin: column.pin,
                    display_index,
                    column_key,
                    height_fill: column.height_fill,
                })
            })
            .collect()
    }

    fn saved_desk(&self) -> SavedDesk {
        let ordered = order_displays(self.displays.clone());
        SavedDesk {
            displays: ordered
                .iter()
                .enumerate()
                .map(|(index, display)| SavedDisplay {
                    display_id: display.id,
                    index,
                    columns: self.strips.get(&display.id).map(|strip| strip.columns.iter().map(|column| self.save_column(column)).collect()).unwrap_or_default(),
                    scroll: self.strips.get(&display.id).map(|strip| strip.scroll).unwrap_or(0.0),
                })
                .collect(),
            unified: self.unified.columns.iter().map(|column| self.save_column(column)).collect(),
            offline: self
                .parked_screens
                .iter()
                .map(|(id, strip)| SavedDisplay {
                    display_id: *id,
                    index: usize::MAX,
                    columns: strip.columns.iter().map(|column| self.save_column(column)).collect(),
                    scroll: strip.scroll,
                })
                .collect(),
        }
    }

    fn save_column(&self, column: &Column) -> SavedColumn {
        SavedColumn {
            bundle_ids: column.windows.iter().filter_map(|id| self.windows.get(id).map(|window| window.bundle_id.clone())).collect(),
            titles: column.windows.iter().filter_map(|id| self.windows.get(id).map(|window| window.title.clone())).collect(),
            width_ratio: column.width_ratio,
            height_weights: column.height_weights.clone(),
            mode: Some(column.mode),
            pin: column.pin,
            height_fill: column.height_fill,
            vertical: column.vertical,
            arrange_lock: column.arrange_lock,
        }
    }

    fn visual_ids(&self) -> Vec<u64> {
        let columns: Vec<Column> = if self.settings.desk_mode == DeskMode::Unified {
            self.unified.columns.clone()
        } else if let Some(display) = self.focused_display() {
            self.strips.get(&display).map(|strip| strip.columns.clone()).unwrap_or_default()
        } else {
            self.strips.values().next().map(|strip| strip.columns.clone()).unwrap_or_default()
        };
        let mut ids = Vec::new();
        ids.extend(columns.iter().filter(|column| column.pin == Some(PinEdge::Left)).map(|column| column.id));
        ids.extend(columns.iter().filter(|column| column.pin.is_none()).map(|column| column.id));
        ids.extend(columns.iter().filter(|column| column.pin == Some(PinEdge::Right)).map(|column| column.id));
        ids
    }

    fn scroll_display(&self, x: f64, y: f64) -> Option<u32> {
        self.display_under_pointer(x, y)
            .filter(|id| self.strips.get(id).is_some_and(|strip| !strip.columns.is_empty()))
            .or_else(|| self.focused_display().filter(|id| self.strips.get(id).is_some_and(|strip| !strip.columns.is_empty())))
            .or_else(|| {
                self.displays
                    .iter()
                    .find(|display| self.strips.get(&display.id).is_some_and(|strip| !strip.columns.is_empty()))
                    .map(|display| display.id)
            })
    }

    fn display_under_pointer(&self, cg_x: f64, cg_y: f64) -> Option<u32> {
        if !cg_x.is_finite() || !cg_y.is_finite() || self.primary_height <= 0.0 {
            return None;
        }
        let y = self.primary_height - cg_y;
        self.displays.iter().find(|display| display.frame.contains(cg_x, y)).map(|display| display.id)
    }

    fn focused_display(&self) -> Option<u32> {
        self.focused.and_then(|id| self.column_id_of(id)).and_then(|column| self.column_display(column)).or_else(|| {
            self.focused.and_then(|id| self.windows.get(&id)).and_then(|window| self.display_of(window.frame))
        })
    }

    fn focused_column_index(&self) -> Option<usize> {
        let focused = self.focused?;
        self.unified.columns.iter().position(|column| column.windows.contains(&focused))
    }

    fn column_id_of(&self, window: WindowId) -> Option<u64> {
        self.unified
            .columns
            .iter()
            .find(|column| column.windows.contains(&window))
            .map(|column| column.id)
            .or_else(|| {
                self.strips.values().find_map(|strip| strip.columns.iter().find(|column| column.windows.contains(&window)).map(|column| column.id))
            })
    }

    fn column_display(&self, column_id: u64) -> Option<u32> {
        if self.settings.desk_mode == DeskMode::Unified {
            return self.column_frames.get(&column_id).and_then(|frame| self.display_of(*frame)).or_else(|| self.displays.first().map(|display| display.id));
        }
        self.strips.iter().find(|(_, strip)| strip.columns.iter().any(|column| column.id == column_id)).map(|(id, _)| *id)
    }

    fn column_by_id(&self, id: u64) -> Option<&Column> {
        self.unified.columns.iter().find(|column| column.id == id).or_else(|| {
            self.strips.values().find_map(|strip| strip.columns.iter().find(|column| column.id == id))
        })
    }

    fn column_by_id_mut(&mut self, id: u64) -> Option<&mut Column> {
        if let Some(column) = self.unified.columns.iter_mut().find(|column| column.id == id) {
            return Some(column);
        }
        self.strips.values_mut().find_map(|strip| strip.columns.iter_mut().find(|column| column.id == id))
    }

    fn for_each_column_mut(&mut self, mut body: impl FnMut(&mut Column)) {
        for column in &mut self.unified.columns {
            body(column);
        }
        for strip in self.strips.values_mut() {
            for column in &mut strip.columns {
                body(column);
            }
        }
    }

    fn take_column(&mut self, id: u64) -> Option<Column> {
        if let Some(index) = self.unified.columns.iter().position(|column| column.id == id) {
            return Some(self.unified.columns.remove(index));
        }
        for strip in self.strips.values_mut() {
            if let Some(index) = strip.columns.iter().position(|column| column.id == id) {
                return Some(strip.columns.remove(index));
            }
        }
        None
    }

    fn take_window_from_column(&mut self, column_id: u64, window: WindowId) {
        let remove_column = if let Some(column) = self.column_by_id_mut(column_id) {
            column.windows.retain(|id| *id != window);
            column.windows.is_empty()
        } else {
            false
        };
        if remove_column {
            self.take_column(column_id);
        }
    }

    fn insert_column_near(&mut self, anchor: u64, column: Column, direction: isize) {
        if self.settings.desk_mode == DeskMode::Unified {
            let index = self.unified.columns.iter().position(|item| item.id == anchor).unwrap_or(self.unified.columns.len().saturating_sub(1));
            let insert_at = if direction > 0 { index + 1 } else { index };
            self.unified.columns.insert(insert_at.min(self.unified.columns.len()), column);
            return;
        }
        let Some(display) = self.column_display(anchor) else {
            return;
        };
        let Some(strip) = self.strips.get_mut(&display) else {
            return;
        };
        let index = strip.columns.iter().position(|item| item.id == anchor).unwrap_or(0);
        let insert_at = if direction > 0 { index + 1 } else { index };
        strip.columns.insert(insert_at.min(strip.columns.len()), column);
    }

    fn clear_rigid(&mut self, column_id: u64) {
        let windows = self.column_by_id(column_id).map(|column| column.windows.clone()).unwrap_or_default();
        for id in windows {
            if let Some(window) = self.windows.get_mut(&id) {
                window.rigid_width = None;
                window.checked_size = false;
            }
        }
    }

    fn make_column(&mut self, windows: Vec<WindowId>, ratio: f64) -> Column {
        let id = self.fresh_id();
        Column {
            id,
            height_weights: vec![1.0; windows.len().max(1)],
            windows,
            width_ratio: ratio.clamp(0.15, 1.0),
            mode: ColumnMode::Stacked,
            pin: None,
            active_index: 0,
            height_fill: 1.0,
            vertical: VerticalAlign::Top,
            arrange_lock: false,
        }
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn columns_empty(&self) -> bool {
        self.unified.columns.is_empty() && self.strips.values().all(|strip| strip.columns.is_empty())
    }

    fn sanitize(&mut self) {
        for strip in self.strips.values_mut() {
            sanitize_strip(strip);
        }
        sanitize_strip(&mut self.unified);
    }

    fn display_of(&self, frame: Rect) -> Option<u32> {
        self.displays
            .iter()
            .find(|display| display.frame.contains(frame.mid_x(), frame.mid_y()))
            .or_else(|| self.displays.iter().find(|display| display.frame.intersects(frame)))
            .map(|display| display.id)
    }

    fn unified_metrics(&self) -> (f64, f64) {
        let unit = self.displays.first().map(|display| display.usable.width).unwrap_or(1400.0);
        let columns: Vec<_> = self.unified.columns.iter().map(|column| self.column_state(column, unit)).collect();
        let metrics = unified_metrics(&self.displays, &columns, self.settings.gap);
        (metrics.viewport_width, metrics.content_width)
    }

    fn unified_column_metrics(&self, column_id: u64) -> (f64, f64, f64, f64) {
        let unit = self.displays.first().map(|display| display.usable.width).unwrap_or(1400.0);
        let columns: Vec<_> = self.unified.columns.iter().map(|column| self.column_state(column, unit)).collect();
        let metrics = unified_metrics(&self.displays, &columns, self.settings.gap);
        let origin = metrics.origins.get(&column_id).copied().unwrap_or(0.0);
        let width = columns.iter().find(|column| column.id == column_id).map(|column| column.width).unwrap_or(0.0);
        (metrics.viewport_width, metrics.content_width, origin, width)
    }

    fn display_column_metrics(&self, display: u32, column_id: u64) -> (f64, f64, f64, f64) {
        let usable = self.displays.iter().find(|item| item.id == display).map(|item| item.usable.width).unwrap_or(1400.0);
        let columns = self
            .strips
            .get(&display)
            .map(|strip| strip.columns.iter().map(|column| self.column_state(column, usable)).collect::<Vec<_>>())
            .unwrap_or_default();
        let metrics = metrics(&columns, self.settings.gap, usable);
        let origin = metrics.origins.get(&column_id).copied().unwrap_or(0.0);
        let width = columns.iter().find(|column| column.id == column_id).map(|column| column.width).unwrap_or(0.0);
        (metrics.viewport_width, metrics.content_width, origin, width)
    }
}

fn live_from_raw(window: RawWindow, keep: bool) -> LiveWindow {
    let element = window.element.raw();
    if keep {
        forget(window.element);
    } else {
        forget(window.element);
    }
    LiveWindow {
        element,
        bundle_id: window.bundle_id,
        app_name: window.app_name,
        title: window.title,
        frame: window.frame,
        resizable: window.resizable,
        natural_height: window.frame.height,
        rigid_width: None,
        checked_size: false,
        space: 0,
    }
}

fn retain_strip(strip: &mut Strip, live_ids: &HashSet<WindowId>) {
    for column in &mut strip.columns {
        column.windows.retain(|id| live_ids.contains(id));
    }
    strip.columns.retain(|column| !column.windows.is_empty());
}

fn sanitize_strip(strip: &mut Strip) {
    strip.columns.retain(|column| !column.windows.is_empty());
    for column in &mut strip.columns {
        if column.active_index >= column.windows.len() {
            column.active_index = 0;
        }
        if column.height_weights.len() != column.windows.len() {
            column.height_weights.resize(column.windows.len(), 1.0);
        }
        column.height_fill = column.height_fill.clamp(0.2, 1.0);
        column.width_ratio = column.width_ratio.clamp(0.15, 1.0);
    }
}

fn tidy_ratio(ratio: f64, presets: &[f64]) -> f64 {
    if ratio >= 0.9 {
        return 1.0;
    }
    let Some(nearest) = presets.iter().copied().min_by(|left, right| {
        (left - ratio).abs().partial_cmp(&(right - ratio).abs()).unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return ratio;
    };
    if (nearest - ratio).abs() <= 0.08 { nearest } else { ratio }
}

fn pull_window(strip: &mut Strip, id: WindowId) {
    for column in &mut strip.columns {
        column.windows.retain(|window| *window != id);
    }
    strip.columns.retain(|column| !column.windows.is_empty());
}

fn follow_scroll(strip: &mut Strip, next: f64) {
    strip.scroll = next;
    strip.scroll_target = next;
}

fn step_value(value: &mut f64, target: f64) -> bool {
    let delta = target - *value;
    if delta.abs() < 0.5 {
        *value = target;
        return false;
    }
    // 120Hz timer: close about 80% of the gap every 16ms, so a focus change slides and settles.
    *value += delta * 0.55;
    true
}

fn near(left: Rect, right: Rect) -> bool {
    (left.x - right.x).abs() < 1.5
        && (left.y - right.y).abs() < 1.5
        && (left.width - right.width).abs() < 1.5
        && (left.height - right.height).abs() < 1.5
}

fn union_rect(left: Rect, right: Rect) -> Rect {
    let x = left.min_x().min(right.min_x());
    let y = left.min_y().min(right.min_y());
    let max_x = left.max_x().max(right.max_x());
    let max_y = left.max_y().max(right.max_y());
    Rect::new(x, y, max_x - x, max_y - y)
}

fn placement_display(placements: &HashMap<WindowId, Placement>, windows: &[WindowId]) -> u32 {
    windows.iter().find_map(|id| placements.get(id).map(|placement| placement.display_id)).unwrap_or(0)
}

fn front_pid() -> i32 {
    unsafe {
        let app: crate::platform::objc::Id = objc::msg_send![objc::class!(NSWorkspace), sharedWorkspace];
        let front: crate::platform::objc::Id = objc::msg_send![app, frontmostApplication];
        if front.is_null() {
            0
        } else {
            objc::msg_send![front, processIdentifier]
        }
    }
}

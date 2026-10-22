use crate::app::{self, with_session};
use crate::platform::events;
use crate::platform::objc::{activate, color, from_ns, ns_string, release, shared_app, Id, NsPoint, NsRect, NsSize, NIL};
use crate::session::{DeskSnapshot, IndicatorTile, Session};
use crate::store::{self, Settings, TrackpadMode};
use objc::declare::ClassDecl;
use objc::runtime::{Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use openlayout::engine::{key_title, Action, Arrange, CenterMode, DeskMode, DisplaySlice, Rect};
use std::cell::RefCell;

struct Widgets {
    controller: Id,
    start_item: Id,
    status_item_label: Id,
    settings_window: Id,
    overview_window: Id,
    overview_view: Id,
    gap: Id,
    outer: Id,
    mode: Id,
    center: Id,
    trackpad: Id,
    width: Id,
    presets: Id,
    profile_name: Id,
    profile_popup: Id,
    action_popup: Id,
    shortcut_label: Id,
    status_label: Id,
    access_button: Id,
    input_button: Id,
    flags: [Id; 7],
    border_window: Id,
    indicator_window: Id,
    indicator_host: Id,
    indicator_viewport: Id,
    indicator_signature: String,
    dim_windows: Vec<(u32, Id)>,
}

thread_local! {
    static UI: RefCell<Option<Widgets>> = const { RefCell::new(None) };
    static HITS: RefCell<Vec<Hit>> = const { RefCell::new(Vec::new()) };
    static DRAG: RefCell<Option<u64>> = const { RefCell::new(None) };
}

struct Hit {
    column: Option<u64>,
    display: u32,
    rect: NsRect,
}

pub fn run() {
    unsafe {
        let pool: Id = msg_send![class!(NSAutoreleasePool), new];
        let app = shared_app();
        let _: () = msg_send![app, setActivationPolicy: 1isize];
        register_classes();
        let controller: Id = msg_send![class!(OLController), new];
        let _: () = msg_send![app, setDelegate: controller];
        app::install(Session::new(store::load()));
        UI.with(|cell| *cell.borrow_mut() = Some(build_widgets(controller)));
        events::install_scroll_monitor();
        let center: Id = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let _: () = msg_send![center, addObserver: controller
            selector: sel!(onCommand:)
            name: ns_string("com.openlayout.command")
            object: NIL];
        let workspace: Id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let notes: Id = msg_send![workspace, notificationCenter];
        let _: () = msg_send![notes, addObserver: controller
            selector: sel!(activateApp:)
            name: ns_string("NSWorkspaceDidActivateApplicationNotification")
            object: NIL];
        for name in [
            "NSWorkspaceDidLaunchApplicationNotification",
            "NSWorkspaceDidTerminateApplicationNotification",
            "NSWorkspaceActiveSpaceDidChangeNotification",
            "NSWorkspaceDidWakeNotification",
            "NSWorkspaceScreensDidWakeNotification",
        ] {
            let _: () = msg_send![notes, addObserver: controller selector: sel!(tick:) name: ns_string(name) object: NIL];
        }
        let timer: Id = msg_send![class!(NSTimer), timerWithTimeInterval: (1.0 / 120.0)
            target: controller
            selector: sel!(tick:)
            userInfo: NIL
            repeats: 1i8];
        let run_loop: Id = msg_send![class!(NSRunLoop), mainRunLoop];
        let _: () = msg_send![run_loop, addTimer: timer forMode: ns_string("NSRunLoopCommonModes")];
        sync();
        let _: () = msg_send![app, run];
        release(pool);
    }
}

pub fn sync() {
    let Some(model) = with_session(|session| {
        let refresh = session.dirty_ui || session.dirty_controls || session.show_overview || session.show_settings;
        if !refresh {
            return None;
        }
        let model = Model {
            snapshot: session.snapshot.clone(),
            settings: session.settings.clone(),
            profiles: session.profiles.iter().map(|profile| profile.name.clone()).collect(),
            recording: session.recording_action(),
            show_overview: session.show_overview,
            show_settings: session.show_settings,
            controls: session.dirty_controls || session.show_settings,
        };
        session.dirty_ui = false;
        session.dirty_controls = false;
        session.show_overview = false;
        session.show_settings = false;
        Some(model)
    }) else {
        return;
    };
    let Some(model) = model else {
        return;
    };
    UI.with(|cell| {
        let mut widgets = cell.borrow_mut();
        let Some(ui) = widgets.as_mut() else {
            return;
        };
        apply_model(ui, &model);
    });
}

struct Model {
    snapshot: DeskSnapshot,
    settings: Settings,
    profiles: Vec<String>,
    recording: Option<Action>,
    show_overview: bool,
    show_settings: bool,
    controls: bool,
}

fn apply_model(ui: &mut Widgets, model: &Model) {
    unsafe {
        let start = if model.snapshot.tiling { "Pause" } else { "Start tiling" };
        let _: () = msg_send![ui.start_item, setTitle: ns_string(start)];
        let _: () = msg_send![ui.status_item_label, setTitle: ns_string(&model.snapshot.status)];
        let _: () = msg_send![ui.status_label, setStringValue: ns_string(&model.snapshot.status)];
        let access = if model.snapshot.trusted { "Accessibility: Allowed" } else { "Allow Accessibility" };
        let input = if model.snapshot.input_monitoring { "Input Monitoring: Allowed" } else { "Allow Input Monitoring" };
        let _: () = msg_send![ui.access_button, setTitle: ns_string(access)];
        let _: () = msg_send![ui.input_button, setTitle: ns_string(input)];
        if model.controls {
            write_controls(ui, &model.settings, &model.profiles, model.recording);
        }
        place_border(ui, model.snapshot.focus_frame, model.settings.focus_border && model.snapshot.tiling);
        place_indicator(ui, &model.snapshot);
        place_dim(ui, model);
        redraw_overview(ui, &model.snapshot);
        if model.show_settings {
            let _: () = msg_send![ui.settings_window, makeKeyAndOrderFront: NIL];
            activate();
        }
        if model.show_overview {
            let _: () = msg_send![ui.overview_window, makeKeyAndOrderFront: NIL];
            activate();
        }
    }
}

fn write_controls(ui: &Widgets, settings: &Settings, profiles: &[String], recording: Option<Action>) {
    unsafe {
        let _: () = msg_send![ui.gap, setDoubleValue: settings.gap];
        let _: () = msg_send![ui.outer, setDoubleValue: settings.outer_gap];
        let _: () = msg_send![ui.width, setDoubleValue: settings.default_width * 100.0];
        let _: () = msg_send![ui.presets, setStringValue: ns_string(&store::preset_text(&settings.presets))];
        let _: () = msg_send![ui.mode, selectItemAtIndex: if settings.desk_mode == DeskMode::Unified { 1isize } else { 0 }];
        let center = match settings.center_mode {
            CenterMode::Never => 0,
            CenterMode::WhenOverflow => 1,
            CenterMode::Always => 2,
        };
        let _: () = msg_send![ui.center, selectItemAtIndex: center];
        let trackpad = match settings.trackpad {
            TrackpadMode::Off => 0,
            TrackpadMode::WithModifier => 1,
            TrackpadMode::Horizontal => 2,
        };
        let _: () = msg_send![ui.trackpad, selectItemAtIndex: trackpad];
        let flags = [
            settings.keep_original_height,
            settings.focus_border,
            settings.dim_inactive,
            settings.focus_follows_mouse,
            settings.mouse_follows_focus,
            settings.linked_scroll,
            settings.launch_at_login,
        ];
        for (button, enabled) in ui.flags.iter().zip(flags) {
            let _: () = msg_send![*button, setState: if enabled { 1isize } else { 0 }];
        }
        let _: () = msg_send![ui.profile_popup, removeAllItems];
        if profiles.is_empty() {
            let _: () = msg_send![ui.profile_popup, addItemWithTitle: ns_string("No profiles yet")];
        }
        for name in profiles {
            let _: () = msg_send![ui.profile_popup, addItemWithTitle: ns_string(name)];
        }
        let index: isize = msg_send![ui.action_popup, indexOfSelectedItem];
        let action = Action::all().get(index.max(0) as usize).copied();
        let chord = action
            .map(|action| {
                settings
                    .bindings
                    .iter()
                    .filter(|binding| binding.action == action)
                    .map(|binding| format!("{}{}", binding.modifiers.symbols(), key_title(binding.key_code)))
                    .collect::<Vec<_>>()
                    .join("  ")
            })
            .unwrap_or_default();
        let label = if let Some(action) = recording {
            format!("Press a shortcut for {}", action.title())
        } else if chord.is_empty() {
            "Choose an action and click Record.".into()
        } else {
            chord
        };
        let _: () = msg_send![ui.shortcut_label, setStringValue: ns_string(&label)];
    }
}

fn place_border(ui: &Widgets, frame: Option<Rect>, visible: bool) {
    unsafe {
        let Some(frame) = frame.filter(|_| visible) else {
            let _: () = msg_send![ui.border_window, orderOut: NIL];
            return;
        };
        if frame.width < 40.0 || frame.height < 40.0 {
            let _: () = msg_send![ui.border_window, orderOut: NIL];
            return;
        }
        let rect = NsRect::new(frame.x + 2.0, frame.y + 2.0, (frame.width - 4.0).max(20.0), (frame.height - 4.0).max(20.0));
        let _: () = msg_send![ui.border_window, setFrame: rect display: 1i8];
        let _: () = msg_send![ui.border_window, orderFrontRegardless];
    }
}

fn place_indicator(ui: &mut Widgets, snapshot: &DeskSnapshot) {
    unsafe {
        if !snapshot.indicator_visible || snapshot.indicator.is_empty() {
            let _: () = msg_send![ui.indicator_window, orderOut: NIL];
            return;
        }
        let signature = snapshot
            .indicator
            .iter()
            .map(|tile| format!("{}:{}", tile.column_id, tile.focused))
            .collect::<Vec<_>>()
            .join("|");
        if signature != ui.indicator_signature {
            ui.indicator_signature = signature;
            rebuild_indicator(ui, &snapshot.indicator);
        }
        place_viewport(ui, &snapshot.indicator);
        let (width, height, _, _, _, _) = indicator_metrics(snapshot.indicator.len());
        let anchor = snapshot
            .displays
            .iter()
            .find(|display| display.id == snapshot.indicator_display)
            .or_else(|| snapshot.displays.first());
        let Some(display) = anchor else {
            return;
        };
        let x = display.usable.x + (display.usable.width - width) / 2.0;
        let y = display.usable.y + 10.0;
        let _: () = msg_send![ui.indicator_window, setFrame: NsRect::new(x, y, width, height) display: 1i8];
        let _: () = msg_send![ui.indicator_host, setFrame: NsRect::new(0.0, 0.0, width, height)];
        let _: () = msg_send![ui.indicator_window, orderFrontRegardless];
    }
}

fn rebuild_indicator(ui: &mut Widgets, tiles: &[IndicatorTile]) {
    unsafe {
        let subviews: Id = msg_send![ui.indicator_host, subviews];
        let copy: Id = msg_send![subviews, copy];
        let count: usize = msg_send![copy, count];
        for index in 0..count {
            let view: Id = msg_send![copy, objectAtIndex: index];
            let _: () = msg_send![view, removeFromSuperview];
        }
        release(copy);
        let (width, height, cell_w, cell_h, gap, pad) = indicator_metrics(tiles.len());
        let _: () = msg_send![ui.indicator_host, setFrame: NsRect::new(0.0, 0.0, width, height)];
        let viewport: Id = msg_send![class!(NSView), alloc];
        let viewport: Id = msg_send![viewport, initWithFrame: NsRect::new(0.0, 0.0, cell_w, cell_h)];
        stroke_layer(viewport);
        let _: () = msg_send![ui.indicator_host, addSubview: viewport];
        ui.indicator_viewport = viewport;
        let cell_y = (height - cell_h) / 2.0;
        for (index, tile) in tiles.iter().enumerate() {
            let frame = NsRect::new(pad + index as f64 * (cell_w + gap), cell_y, cell_w, cell_h);
            let button: Id = msg_send![class!(NSButton), alloc];
            let button: Id = msg_send![button, initWithFrame: frame];
            let _: () = msg_send![button, setTitle: ns_string("")];
            let _: () = msg_send![button, setBordered: 0i8];
            let _: () = msg_send![button, setTarget: ui.controller];
            let _: () = msg_send![button, setAction: sel!(indicatorClick:)];
            let _: () = msg_send![button, setTag: index as isize];
            let _: () = msg_send![button, setAccessibilityLabel: ns_string(&tile.title)];
            if tile.focused {
                paint_layer(button, 0.93, 0.58, 0.24, 1.0, 4.0);
            } else {
                paint_layer(button, 0.62, 0.63, 0.66, 1.0, 4.0);
            }
            let _: () = msg_send![ui.indicator_host, addSubview: button];
        }
    }
}

fn place_viewport(ui: &Widgets, tiles: &[IndicatorTile]) {
    unsafe {
        if ui.indicator_viewport.is_null() {
            return;
        }
        let Some(frame) = viewport_frame(tiles) else {
            let _: () = msg_send![ui.indicator_viewport, setHidden: 1i8];
            return;
        };
        let _: () = msg_send![ui.indicator_viewport, setHidden: 0i8];
        let _: () = msg_send![ui.indicator_viewport, setFrame: frame];
    }
}

fn indicator_metrics(count: usize) -> (f64, f64, f64, f64, f64, f64) {
    let count = count.max(1) as f64;
    let gap = 5.0;
    let pad = 6.0;
    let height = 32.0;
    let cell_h = 16.0;
    let max_inner = 520.0 - pad * 2.0;
    let cell_w = ((max_inner - gap * (count - 1.0)) / count).clamp(12.0, 34.0);
    let width = pad * 2.0 + count * cell_w + (count - 1.0) * gap;
    (width, height, cell_w, cell_h, gap, pad)
}

fn viewport_frame(tiles: &[IndicatorTile]) -> Option<NsRect> {
    let first = tiles.iter().position(|tile| tile.on_screen)?;
    let last = tiles.iter().rposition(|tile| tile.on_screen).unwrap_or(first);
    let (_, height, cell_w, cell_h, gap, pad) = indicator_metrics(tiles.len());
    let x = pad + first as f64 * (cell_w + gap) - 3.0;
    let y = (height - cell_h) / 2.0 - 3.0;
    let width = (last - first) as f64 * (cell_w + gap) + cell_w + 6.0;
    Some(NsRect::new(x.max(2.0), y.max(2.0), width, cell_h + 6.0))
}

fn paint_layer(view: Id, red: f64, green: f64, blue: f64, alpha: f64, radius: f64) {
    unsafe {
        let _: () = msg_send![view, setWantsLayer: 1i8];
        let layer: Id = msg_send![view, layer];
        let ink = color(red, green, blue, alpha);
        let cg: Id = msg_send![ink, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];
        let _: () = msg_send![layer, setCornerRadius: radius];
    }
}

fn stroke_layer(view: Id) {
    unsafe {
        let _: () = msg_send![view, setWantsLayer: 1i8];
        let layer: Id = msg_send![view, layer];
        let clear: Id = msg_send![class!(NSColor), clearColor];
        let clear_cg: Id = msg_send![clear, CGColor];
        let ink = color(1.0, 1.0, 1.0, 0.96);
        let cg: Id = msg_send![ink, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: clear_cg];
        let _: () = msg_send![layer, setBorderColor: cg];
        let _: () = msg_send![layer, setBorderWidth: 2.0f64];
        let _: () = msg_send![layer, setCornerRadius: 6.0f64];
    }
}

fn place_dim(ui: &mut Widgets, model: &Model) {
    unsafe {
        if !model.settings.dim_inactive || !model.snapshot.tiling {
            for (_, window) in &ui.dim_windows {
                let _: () = msg_send![*window, orderOut: NIL];
            }
            return;
        }
        for display in &model.snapshot.displays {
            let window = dim_window(ui, display);
            let image = dim_image(display, model.snapshot.focus_frame);
            let view: Id = msg_send![window, contentView];
            let image_view: Id = msg_send![view, viewWithTag: 7isize];
            if !image_view.is_null() {
                let _: () = msg_send![image_view, setImage: image];
            }
            let _: () = msg_send![window, orderFrontRegardless];
        }
        let _: () = msg_send![ui.border_window, orderFrontRegardless];
    }
}

fn dim_window(ui: &mut Widgets, display: &DisplaySlice) -> Id {
    if let Some((_, window)) = ui.dim_windows.iter().find(|(id, _)| *id == display.id) {
        return *window;
    }
    unsafe {
        let window = borderless_window(NsRect::new(display.frame.x, display.frame.y, display.frame.width, display.frame.height), true);
        let view: Id = msg_send![window, contentView];
        let image_view: Id = msg_send![class!(NSImageView), alloc];
        let image_view: Id = msg_send![image_view, initWithFrame: NsRect::new(0.0, 0.0, display.frame.width, display.frame.height)];
        let _: () = msg_send![image_view, setTag: 7isize];
        let _: () = msg_send![image_view, setAutoresizingMask: 18usize];
        let _: () = msg_send![view, addSubview: image_view];
        ui.dim_windows.push((display.id, window));
        window
    }
}

fn dim_image(display: &DisplaySlice, focus: Option<Rect>) -> Id {
    unsafe {
        let size_w = display.frame.width;
        let size_h = display.frame.height;
        let image: Id = msg_send![class!(NSImage), alloc];
        let image: Id = msg_send![image, initWithSize: crate::platform::objc::NsSize { width: size_w, height: size_h }];
        let _: () = msg_send![image, lockFocus];
        let path: Id = msg_send![class!(NSBezierPath), bezierPath];
        let _: () = msg_send![path, appendBezierPathWithRect: NsRect::new(0.0, 0.0, size_w, size_h)];
        if let Some(focus) = focus {
            if let Some(hole) = focus.intersection(display.frame) {
                let _: () = msg_send![path, appendBezierPathWithRect: NsRect::new(hole.x - display.frame.x, hole.y - display.frame.y, hole.width, hole.height)];
            }
        }
        let _: () = msg_send![path, setWindingRule: 1isize];
        let shade = color(0.0, 0.0, 0.0, 0.22);
        let _: () = msg_send![shade, setFill];
        let _: () = msg_send![path, fill];
        let _: () = msg_send![image, unlockFocus];
        image
    }
}

fn redraw_overview(ui: &Widgets, snapshot: &DeskSnapshot) {
    unsafe {
        let bounds: NsRect = msg_send![ui.overview_view, bounds];
        let width = if bounds.size.width < 40.0 { 880.0 } else { bounds.size.width };
        let height = if bounds.size.height < 40.0 { 520.0 } else { bounds.size.height };
        let (image, hits) = draw_desk(snapshot, width, height);
        HITS.with(|cell| *cell.borrow_mut() = hits);
        let _: () = msg_send![ui.overview_view, setImage: image];
    }
}

fn draw_desk(snapshot: &DeskSnapshot, width: f64, height: f64) -> (Id, Vec<Hit>) {
    let mut hits = Vec::new();
    unsafe {
        let image: Id = msg_send![class!(NSImage), alloc];
        let image: Id = msg_send![image, initWithSize: crate::platform::objc::NsSize { width, height }];
        let _: () = msg_send![image, lockFocus];
        let background = color(0.11, 0.12, 0.16, 1.0);
        let _: () = msg_send![background, setFill];
        let backdrop: Id = msg_send![class!(NSBezierPath), bezierPathWithRect: NsRect::new(0.0, 0.0, width, height)];
        let _: () = msg_send![backdrop, fill];
        if snapshot.displays.is_empty() {
            draw_text("No displays detected", 24.0, height - 40.0, color(1.0, 1.0, 1.0, 0.8));
            let _: () = msg_send![image, unlockFocus];
            return (image, hits);
        }
        let min_x = snapshot.displays.iter().map(|display| display.frame.min_x()).fold(f64::MAX, f64::min);
        let min_y = snapshot.displays.iter().map(|display| display.frame.min_y()).fold(f64::MAX, f64::min);
        let max_x = snapshot.displays.iter().map(|display| display.frame.max_x()).fold(f64::MIN, f64::max);
        let max_y = snapshot.displays.iter().map(|display| display.frame.max_y()).fold(f64::MIN, f64::max);
        let span_x = (max_x - min_x).max(1.0);
        let span_y = (max_y - min_y).max(1.0);
        let scale = ((width - 48.0) / span_x).min((height - 72.0) / span_y);
        for display in &snapshot.displays {
            let rect = NsRect::new(
                24.0 + (display.frame.x - min_x) * scale,
                24.0 + (display.frame.y - min_y) * scale,
                display.frame.width * scale,
                display.frame.height * scale,
            );
            let path: Id = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: rect xRadius: 10.0 yRadius: 10.0];
            let fill = color(0.18, 0.2, 0.26, 1.0);
            let _: () = msg_send![fill, setFill];
            let _: () = msg_send![path, fill];
            draw_text(&display.name, rect.origin.x + 10.0, rect.origin.y + rect.size.height - 22.0, color(1.0, 1.0, 1.0, 0.72));
            hits.push(Hit { column: None, display: display.id, rect });
        }
        for column in &snapshot.columns {
            let rect = NsRect::new(
                24.0 + (column.frame.x - min_x) * scale,
                24.0 + (column.frame.y - min_y) * scale,
                (column.frame.width * scale).max(18.0),
                (column.frame.height * scale).max(18.0),
            );
            let path: Id = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: rect xRadius: 6.0 yRadius: 6.0];
            let fill = if column.focused {
                color(0.36, 0.52, 0.98, 1.0)
            } else if column.pinned {
                color(0.85, 0.55, 0.25, 1.0)
            } else {
                color(0.45, 0.52, 0.7, 1.0)
            };
            let _: () = msg_send![fill, setFill];
            let _: () = msg_send![path, fill];
            draw_text(&column.title, rect.origin.x + 6.0, rect.origin.y + 8.0, color(1.0, 1.0, 1.0, 0.95));
            hits.push(Hit { column: Some(column.id), display: column.display_id, rect });
        }
        let _: () = msg_send![image, unlockFocus];
        (image, hits)
    }
}

fn draw_text(text: &str, x: f64, y: f64, foreground: Id) {
    unsafe {
        let font: Id = msg_send![class!(NSFont), systemFontOfSize: 12.0];
        let keys = [NSFontAttributeName, NSForegroundColorAttributeName];
        let values = [font, foreground];
        let attrs: Id = msg_send![class!(NSDictionary), dictionaryWithObjects: values.as_ptr() forKeys: keys.as_ptr() count: 2usize];
        let label = ns_string(text);
        let _: () = msg_send![label, drawAtPoint: NsPoint { x, y } withAttributes: attrs];
    }
}

fn display_for_point<'a>(displays: &'a [DisplaySlice], x: f64, y: f64) -> Option<&'a DisplaySlice> {
    displays.iter().find(|display| display.frame.contains(x, y))
}

fn build_widgets(controller: Id) -> Widgets {
    unsafe {
        let menu: Id = msg_send![class!(NSMenu), new];
        let status_item_label = menu_item(menu, "openLayout", sel!(tick:), controller);
        let _: () = msg_send![status_item_label, setEnabled: 0i8];
        let start_item = menu_item(menu, "Start tiling", sel!(startStop:), controller);
        menu_item(menu, "All displays", sel!(openOverview:), controller);
        add_arrange_menu(menu, controller);
        menu_item(menu, "Settings…", sel!(openSettings:), controller);
        let separator: Id = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: separator];
        menu_item(menu, "Quit", sel!(quit:), controller);
        let bar: Id = msg_send![class!(NSStatusBar), systemStatusBar];
        let item: Id = msg_send![bar, statusItemWithLength: -1.0f64];
        let _: () = msg_send![item, retain];
        let button: Id = msg_send![item, button];
        let image: Id = msg_send![class!(NSImage), imageWithSystemSymbolName: ns_string("rectangle.split.3x1") accessibilityDescription: ns_string("openLayout")];
        let _: () = msg_send![button, setImage: image];
        let _: () = msg_send![item, setMenu: menu];

        let settings_window = titled_window("openLayout Settings", 680.0, 760.0);
        let (document, controls) = settings_form(controller);
        let scroll: Id = msg_send![class!(NSScrollView), alloc];
        let scroll: Id = msg_send![scroll, initWithFrame: NsRect::new(0.0, 0.0, 680.0, 760.0)];
        let _: () = msg_send![scroll, setHasVerticalScroller: 1i8];
        let _: () = msg_send![scroll, setDocumentView: document];
        let _: () = msg_send![settings_window, setContentView: scroll];

        let overview_window = titled_window("All displays", 920.0, 580.0);
        let overview_view: Id = msg_send![class!(OLDeskView), alloc];
        let overview_view: Id = msg_send![overview_view, initWithFrame: NsRect::new(0.0, 0.0, 920.0, 580.0)];
        let _: () = msg_send![overview_view, setAutoresizingMask: 18usize];
        let _: () = msg_send![overview_view, setImageScaling: 2isize];
        let _: () = msg_send![overview_window, setContentView: overview_view];

        let border_window = borderless_window(NsRect::new(0.0, 0.0, 100.0, 100.0), true);
        style_border(border_window);
        let indicator_window = borderless_window(NsRect::new(0.0, 0.0, 320.0, 32.0), false);
        let indicator_host: Id = msg_send![class!(OLFlippedView), alloc];
        let indicator_host: Id = msg_send![indicator_host, initWithFrame: NsRect::new(0.0, 0.0, 320.0, 32.0)];
        let _: () = msg_send![indicator_window, setContentView: indicator_host];
        paint_layer(indicator_host, 0.30, 0.30, 0.32, 0.94, 8.0);
        let clear: Id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![indicator_window, setBackgroundColor: clear];

        Widgets {
            controller,
            start_item,
            status_item_label,
            settings_window,
            overview_window,
            overview_view,
            gap: controls.gap,
            outer: controls.outer,
            mode: controls.mode,
            center: controls.center,
            trackpad: controls.trackpad,
            width: controls.width,
            presets: controls.presets,
            profile_name: controls.profile_name,
            profile_popup: controls.profile_popup,
            action_popup: controls.action_popup,
            shortcut_label: controls.shortcut_label,
            status_label: controls.status_label,
            access_button: controls.access_button,
            input_button: controls.input_button,
            flags: controls.flags,
            border_window,
            indicator_window,
            indicator_host,
            indicator_viewport: NIL,
            indicator_signature: String::new(),
            dim_windows: Vec::new(),
        }
    }
}

struct Controls {
    gap: Id,
    outer: Id,
    mode: Id,
    center: Id,
    trackpad: Id,
    width: Id,
    presets: Id,
    profile_name: Id,
    profile_popup: Id,
    action_popup: Id,
    shortcut_label: Id,
    status_label: Id,
    access_button: Id,
    input_button: Id,
    flags: [Id; 7],
}

fn settings_form(controller: Id) -> (Id, Controls) {
    unsafe {
        let document: Id = msg_send![class!(OLFlippedView), alloc];
        let document: Id = msg_send![document, initWithFrame: NsRect::new(0.0, 0.0, 680.0, 1400.0)];
        let _: () = msg_send![document, setWantsLayer: 1i8];
        let layer: Id = msg_send![document, layer];
        let background: Id = msg_send![class!(NSColor), windowBackgroundColor];
        let background_cg: Id = msg_send![background, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: background_cg];

        let mut y = 20.0;
        let (status_label, access_button, input_button) = section(document, &mut y, |y| {
            add_heading(document, "openLayout", 32.0, y, 600.0);
            add_hint(document, "Each screen keeps its own strip. Pausing or quitting saves that arrangement, and a screen you unplug keeps its windows until you plug it back in.", 32.0, y, 600.0, 36.0);
            let status = add_label(document, "Checking permissions…", 32.0, *y, 600.0);
            *y += 28.0;
            let access_button = add_button(document, "Allow Accessibility", controller, sel!(grantAccess:), 32.0, *y, 250.0);
            let input_button = add_button(document, "Allow Input Monitoring", controller, sel!(grantInput:), 294.0, *y, 250.0);
            *y += 36.0;
            (status, access_button, input_button)
        });
        let (gap, outer, width, presets) = section(document, &mut y, |y| {
            add_heading(document, "Layout", 32.0, y, 600.0);
            add_hint(document, "Space around the windows on every screen.", 32.0, y, 600.0, 18.0);
            let gap = labeled_slider(document, "Gap between windows", controller, sel!(gapChanged:), 0.0, 48.0, 32.0, y);
            let outer = labeled_slider(document, "Outer margin", controller, sel!(outerChanged:), 0.0, 64.0, 32.0, y);
            let width = labeled_slider(document, "Default width for a new window", controller, sel!(widthChanged:), 20.0, 100.0, 32.0, y);
            add_label(document, "Width presets, as percentages", 32.0, *y, 600.0);
            *y += 22.0;
            let presets = text_field(document, controller, sel!(presetsChanged:), 32.0, *y, 360.0);
            *y += 36.0;
            (gap, outer, width, presets)
        });
        let (mode, center, trackpad, linked) = section(document, &mut y, |y| {
            add_heading(document, "Screens", 32.0, y, 600.0);
            add_hint(document, "Hold Control and Option, then scroll up, down, or sideways. The strip under the pointer moves with your fingers.", 32.0, y, 600.0, 32.0);
            let mode = labeled_popup(document, "Desk", controller, sel!(modeChanged:), &["Each screen on its own", "One desk across every screen"], 32.0, y);
            let center = labeled_popup(document, "Focused window", controller, sel!(centerChanged:), &["Leave it where it is", "Center it when the strip overflows", "Always center it"], 32.0, y);
            let trackpad = labeled_popup(document, "Trackpad", controller, sel!(trackpadChanged:), &["Off", "Control-Option and scroll", "Any horizontal scroll"], 32.0, y);
            let linked = checkbox(document, "Scroll every screen together", controller, 6, 32.0, y);
            (mode, center, trackpad, linked)
        });
        let flags = section(document, &mut y, |y| {
            add_heading(document, "Behavior", 32.0, y, 600.0);
            let keep = checkbox(document, "Keep the height a window opened with", controller, 1, 32.0, y);
            let border = checkbox(document, "Focus border", controller, 2, 32.0, y);
            let dim = checkbox(document, "Dim windows that are not focused", controller, 3, 32.0, y);
            let focus_mouse = checkbox(document, "Focus follows the pointer", controller, 4, 32.0, y);
            let mouse_focus = checkbox(document, "Pointer follows focus", controller, 5, 32.0, y);
            let login = checkbox(document, "Open at login", controller, 7, 32.0, y);
            [keep, border, dim, focus_mouse, mouse_focus, linked, login]
        });
        let (profile_name, profile_popup) = section(document, &mut y, |y| {
            add_heading(document, "Profiles", 32.0, y, 600.0);
            add_hint(document, "A profile remembers the whole desk, one strip per screen.", 32.0, y, 600.0, 18.0);
            let profile_name = text_field(document, controller, sel!(saveProfile:), 32.0, *y, 240.0);
            add_button(document, "Save profile", controller, sel!(saveProfile:), 284.0, *y - 2.0, 140.0);
            *y += 40.0;
            let profile_popup = popup(document, controller, sel!(applyProfile:), &["No profiles yet"], 32.0, *y, 280.0);
            add_button(document, "Apply", controller, sel!(applyProfile:), 324.0, *y, 100.0);
            *y += 40.0;
            (profile_name, profile_popup)
        });
        let (action_popup, shortcut_label) = section(document, &mut y, |y| {
            add_heading(document, "Shortcuts", 32.0, y, 600.0);
            add_hint(document, "Control-Option. Arrows or H J K L move focus. Shift moves the window. S focuses the next screen.", 32.0, y, 600.0, 32.0);
            let titles = action_titles();
            let title_refs: Vec<&str> = titles.iter().map(String::as_str).collect();
            let action_popup = popup(document, controller, sel!(recordShortcut:), &title_refs, 32.0, *y, 280.0);
            add_button(document, "Record", controller, sel!(recordShortcut:), 324.0, *y, 100.0);
            add_button(document, "Reset", controller, sel!(resetShortcuts:), 432.0, *y, 100.0);
            *y += 36.0;
            let shortcut_label = add_label(document, "Choose an action, then click Record.", 32.0, *y, 600.0);
            *y += 28.0;
            (action_popup, shortcut_label)
        });
        let _: () = msg_send![document, setFrame: NsRect::new(0.0, 0.0, 680.0, y + 12.0)];
        (
            document,
            Controls {
                gap,
                outer,
                mode,
                center,
                trackpad,
                width,
                presets,
                profile_name,
                profile_popup,
                action_popup,
                shortcut_label,
                status_label,
                access_button,
                input_button,
                flags,
            },
        )
    }
}

fn section<T>(parent: Id, y: &mut f64, body: impl FnOnce(&mut f64) -> T) -> T {
    let top = *y;
    *y += 16.0;
    let value = body(y);
    *y += 12.0;
    card_behind(parent, 16.0, top, 648.0, *y - top);
    *y += 14.0;
    value
}

fn card_behind(parent: Id, x: f64, y: f64, width: f64, height: f64) {
    unsafe {
        let card: Id = msg_send![class!(OLFlippedView), alloc];
        let card: Id = msg_send![card, initWithFrame: NsRect::new(x, y, width, height)];
        let _: () = msg_send![card, setWantsLayer: 1i8];
        let layer: Id = msg_send![card, layer];
        let fill: Id = msg_send![class!(NSColor), controlBackgroundColor];
        let stroke: Id = msg_send![class!(NSColor), separatorColor];
        let fill_cg: Id = msg_send![fill, CGColor];
        let stroke_cg: Id = msg_send![stroke, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: fill_cg];
        let _: () = msg_send![layer, setBorderColor: stroke_cg];
        let _: () = msg_send![layer, setBorderWidth: 1.0f64];
        let _: () = msg_send![layer, setCornerRadius: 12.0f64];
        let _: () = msg_send![parent, addSubview: card positioned: -1isize relativeTo: NIL];
    }
}

fn add_heading(parent: Id, text: &str, x: f64, y: &mut f64, width: f64) {
    unsafe {
        let label: Id = msg_send![class!(NSTextField), labelWithString: ns_string(text)];
        let font: Id = msg_send![class!(NSFont), systemFontOfSize: 13.0f64 weight: 0.3f64];
        let _: () = msg_send![label, setFont: font];
        let _: () = msg_send![label, setFrame: NsRect::new(x, *y, width, 18.0)];
        let _: () = msg_send![parent, addSubview: label];
    }
    *y += 24.0;
}

fn add_hint(parent: Id, text: &str, x: f64, y: &mut f64, width: f64, height: f64) {
    unsafe {
        let label: Id = msg_send![class!(NSTextField), labelWithString: ns_string(text)];
        let font: Id = msg_send![class!(NSFont), systemFontOfSize: 12.0f64 weight: 0.0f64];
        let ink: Id = msg_send![class!(NSColor), secondaryLabelColor];
        let _: () = msg_send![label, setFont: font];
        let _: () = msg_send![label, setTextColor: ink];
        let _: () = msg_send![label, setFrame: NsRect::new(x, *y, width, height)];
        let _: () = msg_send![parent, addSubview: label];
    }
    *y += height + 6.0;
}

fn action_titles() -> Vec<String> {
    Action::all().iter().map(|action| action.title().to_string()).collect()
}

fn add_label(parent: Id, text: &str, x: f64, y: f64, width: f64) -> Id {
    unsafe {
        let label: Id = msg_send![class!(NSTextField), labelWithString: ns_string(text)];
        let _: () = msg_send![label, setFrame: NsRect::new(x, y, width, 36.0)];
        let _: () = msg_send![parent, addSubview: label];
        label
    }
}

fn add_button(parent: Id, title: &str, target: Id, action: Sel, x: f64, y: f64, width: f64) -> Id {
    unsafe {
        let button: Id = msg_send![class!(NSButton), buttonWithTitle: ns_string(title) target: target action: action];
        let _: () = msg_send![button, setFrame: NsRect::new(x, y, width, 28.0)];
        let _: () = msg_send![parent, addSubview: button];
        button
    }
}

fn checkbox(parent: Id, title: &str, target: Id, tag: isize, x: f64, y: &mut f64) -> Id {
    unsafe {
        let button: Id = msg_send![class!(NSButton), checkboxWithTitle: ns_string(title) target: target action: sel!(flagChanged:)];
        let _: () = msg_send![button, setTag: tag];
        let _: () = msg_send![button, setFrame: NsRect::new(x, *y, 560.0, 24.0)];
        let _: () = msg_send![parent, addSubview: button];
        *y += 28.0;
        button
    }
}

fn labeled_slider(parent: Id, title: &str, target: Id, action: Sel, min: f64, max: f64, x: f64, y: &mut f64) -> Id {
    add_label(parent, title, x, *y, 560.0);
    *y += 22.0;
    unsafe {
        let slider: Id = msg_send![class!(NSSlider), sliderWithTarget: target action: action];
        let _: () = msg_send![slider, setMinValue: min];
        let _: () = msg_send![slider, setMaxValue: max];
        let _: () = msg_send![slider, setFrame: NsRect::new(x, *y, 420.0, 24.0)];
        let _: () = msg_send![parent, addSubview: slider];
        *y += 34.0;
        slider
    }
}

fn labeled_popup(parent: Id, title: &str, target: Id, action: Sel, items: &[&str], x: f64, y: &mut f64) -> Id {
    add_label(parent, title, x, *y, 560.0);
    *y += 22.0;
    let popup = popup(parent, target, action, items, x, *y, 360.0);
    *y += 36.0;
    popup
}

fn popup(parent: Id, target: Id, action: Sel, items: &[&str], x: f64, y: f64, width: f64) -> Id {
    unsafe {
        let popup: Id = msg_send![class!(NSPopUpButton), alloc];
        let popup: Id = msg_send![popup, initWithFrame: NsRect::new(x, y, width, 26.0) pullsDown: 0i8];
        let _: () = msg_send![popup, setTarget: target];
        let _: () = msg_send![popup, setAction: action];
        for item in items {
            let _: () = msg_send![popup, addItemWithTitle: ns_string(item)];
        }
        let _: () = msg_send![parent, addSubview: popup];
        popup
    }
}

fn text_field(parent: Id, target: Id, action: Sel, x: f64, y: f64, width: f64) -> Id {
    unsafe {
        let field: Id = msg_send![class!(NSTextField), textFieldWithString: ns_string("")];
        let _: () = msg_send![field, setFrame: NsRect::new(x, y, width, 24.0)];
        let _: () = msg_send![field, setTarget: target];
        let _: () = msg_send![field, setAction: action];
        let _: () = msg_send![parent, addSubview: field];
        field
    }
}

fn menu_item(menu: Id, title: &str, action: Sel, target: Id) -> Id {
    unsafe {
        let item: Id = msg_send![class!(NSMenuItem), alloc];
        let item: Id = msg_send![item, initWithTitle: ns_string(title) action: action keyEquivalent: ns_string("")];
        let _: () = msg_send![item, setTarget: target];
        let _: () = msg_send![menu, addItem: item];
        item
    }
}

fn add_arrange_menu(menu: Id, controller: Id) {
    unsafe {
        let submenu: Id = msg_send![class!(NSMenu), new];
        let mut section = "";
        for (index, kind) in Arrange::all().iter().copied().enumerate() {
            if kind.section() != section {
                if !section.is_empty() {
                    let separator: Id = msg_send![class!(NSMenuItem), separatorItem];
                    let _: () = msg_send![submenu, addItem: separator];
                }
                section = kind.section();
                let header: Id = msg_send![class!(NSMenuItem), alloc];
                let header: Id = msg_send![header, initWithTitle: ns_string(section) action: NIL keyEquivalent: ns_string("")];
                let _: () = msg_send![header, setEnabled: 0i8];
                let _: () = msg_send![submenu, addItem: header];
            }
            let item = menu_item(submenu, kind.title(), sel!(arrange:), controller);
            let _: () = msg_send![item, setTag: index as isize];
            let _: () = msg_send![item, setImage: arrange_icon(kind)];
        }
        let parent: Id = msg_send![class!(NSMenuItem), alloc];
        let parent: Id = msg_send![parent, initWithTitle: ns_string("Arrange") action: NIL keyEquivalent: ns_string("")];
        let symbol: Id = msg_send![class!(NSImage), imageWithSystemSymbolName: ns_string("rectangle.split.2x1") accessibilityDescription: ns_string("Arrange")];
        if !symbol.is_null() {
            let _: () = msg_send![parent, setImage: symbol];
        }
        let _: () = msg_send![parent, setSubmenu: submenu];
        let _: () = msg_send![menu, addItem: parent];
    }
}

fn arrange_icon(kind: Arrange) -> Id {
    let width = 26.0;
    let height = 16.0;
    unsafe {
        let image: Id = msg_send![class!(NSImage), alloc];
        let image: Id = msg_send![image, initWithSize: NsSize { width, height }];
        let _: () = msg_send![image, lockFocus];
        let screen: Id = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: NsRect::new(0.4, 0.4, width - 0.8, height - 0.8) xRadius: 3.0 yRadius: 3.0];
        let wash = color(0.0, 0.0, 0.0, 0.2);
        let _: () = msg_send![wash, setFill];
        let _: () = msg_send![screen, fill];
        let glyphs = kind.glyphs();
        let pad = 1.7;
        let gap = if glyphs.len() > 1 { 1.15 } else { 0.0 };
        let inner_w = width - pad * 2.0;
        let inner_h = height - pad * 2.0;
        for glyph in glyphs {
            let cell_w = (glyph.w * inner_w - gap).max(1.2);
            let cell_h = (glyph.h * inner_h - gap).max(1.2);
            let x = pad + glyph.x * inner_w + gap * 0.5;
            let y = pad + (1.0 - glyph.y - glyph.h) * inner_h + gap * 0.5;
            let tile: Id = msg_send![class!(NSBezierPath), bezierPathWithRoundedRect: NsRect::new(x, y, cell_w, cell_h) xRadius: 1.7 yRadius: 1.7];
            let alpha = if glyph.primary { 0.96 } else { 0.4 };
            let ink = color(0.0, 0.0, 0.0, alpha);
            let _: () = msg_send![ink, setFill];
            let _: () = msg_send![tile, fill];
        }
        let _: () = msg_send![image, unlockFocus];
        let _: () = msg_send![image, setTemplate: 1i8];
        image
    }
}

fn titled_window(title: &str, width: f64, height: f64) -> Id {
    unsafe {
        let window: Id = msg_send![class!(NSWindow), alloc];
        let window: Id = msg_send![window, initWithContentRect: NsRect::new(0.0, 0.0, width, height)
            styleMask: 15usize
            backing: 2usize
            defer: 0i8];
        let _: () = msg_send![window, setTitle: ns_string(title)];
        let _: () = msg_send![window, setReleasedWhenClosed: 0i8];
        let _: () = msg_send![window, center];
        window
    }
}

fn borderless_window(frame: NsRect, ignore_mouse: bool) -> Id {
    unsafe {
        let window: Id = msg_send![class!(NSWindow), alloc];
        let window: Id = msg_send![window, initWithContentRect: frame styleMask: 0usize backing: 2usize defer: 0i8];
        let _: () = msg_send![window, setLevel: 25isize];
        let _: () = msg_send![window, setOpaque: 0i8];
        let _: () = msg_send![window, setHasShadow: 0i8];
        let clear: Id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![window, setBackgroundColor: clear];
        let _: () = msg_send![window, setIgnoresMouseEvents: if ignore_mouse { 1i8 } else { 0i8 }];
        let _: () = msg_send![window, setCollectionBehavior: 337usize];
        let _: () = msg_send![window, setHidesOnDeactivate: 0i8];
        let _: () = msg_send![window, setReleasedWhenClosed: 0i8];
        window
    }
}

fn style_border(window: Id) {
    unsafe {
        let view: Id = msg_send![window, contentView];
        let _: () = msg_send![view, setWantsLayer: 1i8];
        let layer: Id = msg_send![view, layer];
        let ink = color(0.36, 0.52, 0.98, 1.0);
        let cg: Id = msg_send![ink, CGColor];
        let _: () = msg_send![layer, setBorderWidth: 3.0f64];
        let _: () = msg_send![layer, setBorderColor: cg];
        let _: () = msg_send![layer, setCornerRadius: 8.0f64];
    }
}

fn register_classes() {
    unsafe {
        let object = class!(NSObject);
        let mut controller = ClassDecl::new("OLController", object).expect("OLController");
        controller.add_method(sel!(tick:), tick as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(activateApp:), activate_app as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(startStop:), start_stop as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(openSettings:), open_settings as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(openOverview:), open_overview as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(arrange:), arrange as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(quit:), quit as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(grantAccess:), grant_access as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(grantInput:), grant_input as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(gapChanged:), gap_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(outerChanged:), outer_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(widthChanged:), width_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(modeChanged:), mode_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(centerChanged:), center_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(trackpadChanged:), trackpad_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(presetsChanged:), presets_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(flagChanged:), flag_changed as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(saveProfile:), save_profile as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(applyProfile:), apply_profile as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(recordShortcut:), record_shortcut as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(resetShortcuts:), reset_shortcuts as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(indicatorClick:), indicator_click as extern "C" fn(&Object, Sel, Id));
        controller.add_method(sel!(onCommand:), on_command as extern "C" fn(&Object, Sel, Id));
        controller.add_method(
            sel!(applicationWillTerminate:),
            will_terminate as extern "C" fn(&Object, Sel, Id),
        );
        controller.add_method(
            sel!(applicationShouldTerminateAfterLastWindowClosed:),
            should_terminate as extern "C" fn(&Object, Sel, Id) -> i8,
        );
        controller.add_method(
            sel!(application:openURLs:),
            open_urls as extern "C" fn(&Object, Sel, Id, Id),
        );
        controller.register();

        let view = class!(NSView);
        let mut flipped = ClassDecl::new("OLFlippedView", view).expect("OLFlippedView");
        flipped.add_method(sel!(isFlipped), is_flipped as extern "C" fn(&Object, Sel) -> i8);
        flipped.register();

        let image = class!(NSImageView);
        let mut desk = ClassDecl::new("OLDeskView", image).expect("OLDeskView");
        desk.add_method(sel!(mouseDown:), mouse_down as extern "C" fn(&Object, Sel, Id));
        desk.add_method(sel!(mouseUp:), mouse_up as extern "C" fn(&Object, Sel, Id));
        desk.register();
    }
}

extern "C" {
    static NSFontAttributeName: Id;
    static NSForegroundColorAttributeName: Id;
}

extern "C" fn is_flipped(_: &Object, _: Sel) -> i8 {
    1
}

extern "C" fn tick(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.tick());
    sync();
}

extern "C" fn activate_app(_: &Object, _: Sel, note: Id) {
    let pid = activated_pid(note);
    with_session(|session| {
        if let Some(pid) = pid {
            session.reveal_activated(pid);
        }
        session.tick();
    });
    sync();
}

fn activated_pid(note: Id) -> Option<i32> {
    unsafe {
        if note.is_null() {
            return None;
        }
        let info: Id = msg_send![note, userInfo];
        if info.is_null() {
            return None;
        }
        let app: Id = msg_send![info, objectForKey: ns_string("NSWorkspaceApplicationKey")];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        if pid <= 0 { None } else { Some(pid) }
    }
}

extern "C" fn start_stop(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.perform(Action::TogglePause));
    sync();
}

extern "C" fn open_settings(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.show_settings = true);
    sync();
}

extern "C" fn open_overview(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.show_overview = true);
    sync();
}

extern "C" fn arrange(_: &Object, _: Sel, sender: Id) {
    let tag: isize = unsafe { msg_send![sender, tag] };
    if let Some(kind) = Arrange::from_index(tag) {
        with_session(|session| session.arrange(kind));
    }
    sync();
}

extern "C" fn quit(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.save());
    unsafe {
        let _: () = msg_send![shared_app(), terminate: NIL];
    }
}

extern "C" fn grant_access(_: &Object, _: Sel, _: Id) {
    open_privacy("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
}

extern "C" fn grant_input(_: &Object, _: Sel, _: Id) {
    crate::platform::ax::request_input_monitoring();
    open_privacy("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent");
    with_session(|session| session.note_permissions());
    sync();
}

fn open_privacy(url: &str) {
    unsafe {
        let workspace: Id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let url: Id = msg_send![class!(NSURL), URLWithString: ns_string(url)];
        let _: i8 = msg_send![workspace, openURL: url];
    }
}

extern "C" fn gap_changed(_: &Object, _: Sel, sender: Id) {
    let value: f64 = unsafe { msg_send![sender, doubleValue] };
    with_session(|session| session.set_gap(value));
    sync();
}

extern "C" fn outer_changed(_: &Object, _: Sel, sender: Id) {
    let value: f64 = unsafe { msg_send![sender, doubleValue] };
    with_session(|session| session.set_outer_gap(value));
    sync();
}

extern "C" fn width_changed(_: &Object, _: Sel, sender: Id) {
    let value: f64 = unsafe { msg_send![sender, doubleValue] };
    with_session(|session| session.set_default_width(value / 100.0));
    sync();
}

extern "C" fn mode_changed(_: &Object, _: Sel, sender: Id) {
    let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
    with_session(|session| session.set_mode(if index == 1 { DeskMode::Unified } else { DeskMode::Independent }));
    sync();
}

extern "C" fn center_changed(_: &Object, _: Sel, sender: Id) {
    let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
    let mode = match index {
        0 => CenterMode::Never,
        2 => CenterMode::Always,
        _ => CenterMode::WhenOverflow,
    };
    with_session(|session| session.set_center_mode(mode));
    sync();
}

extern "C" fn trackpad_changed(_: &Object, _: Sel, sender: Id) {
    let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
    let mode = match index {
        0 => TrackpadMode::Off,
        2 => TrackpadMode::Horizontal,
        _ => TrackpadMode::WithModifier,
    };
    with_session(|session| session.set_trackpad(mode));
    sync();
}

extern "C" fn presets_changed(_: &Object, _: Sel, sender: Id) {
    let value = from_ns(unsafe { msg_send![sender, stringValue] });
    with_session(|session| session.set_presets_text(&value));
    sync();
}

extern "C" fn flag_changed(_: &Object, _: Sel, sender: Id) {
    let tag: isize = unsafe { msg_send![sender, tag] };
    let state: isize = unsafe { msg_send![sender, state] };
    let flag = match tag {
        1 => "height",
        2 => "border",
        3 => "dim",
        4 => "focus-mouse",
        5 => "mouse-focus",
        6 => "linked",
        7 => "login",
        _ => return,
    };
    with_session(|session| session.set_flag(flag, state == 1));
    sync();
}

extern "C" fn save_profile(_: &Object, _: Sel, _: Id) {
    let name = UI.with(|cell| {
        cell.borrow().as_ref().map(|ui| from_ns(unsafe { msg_send![ui.profile_name, stringValue] })).unwrap_or_default()
    });
    with_session(|session| session.save_profile(name));
    sync();
}

extern "C" fn apply_profile(_: &Object, _: Sel, _: Id) {
    let name = UI.with(|cell| {
        cell.borrow().as_ref().map(|ui| from_ns(unsafe { msg_send![ui.profile_popup, titleOfSelectedItem] })).unwrap_or_default()
    });
    with_session(|session| session.apply_profile_named(&name));
    sync();
}

extern "C" fn record_shortcut(_: &Object, _: Sel, _: Id) {
    let index = UI.with(|cell| {
        cell.borrow().as_ref().map(|ui| unsafe { msg_send![ui.action_popup, indexOfSelectedItem] }).unwrap_or(0)
    });
    if let Some(action) = Action::all().get(index as usize).copied() {
        with_session(|session| session.begin_recording(action));
    }
    sync();
}

extern "C" fn reset_shortcuts(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.reset_bindings());
    sync();
}

extern "C" fn indicator_click(_: &Object, _: Sel, sender: Id) {
    let index: isize = unsafe { msg_send![sender, tag] };
    with_session(|session| {
        if let Some(tile) = session.snapshot.indicator.get(index as usize) {
            let id = tile.column_id;
            session.focus_column(id);
        }
    });
    sync();
}

extern "C" fn on_command(_: &Object, _: Sel, note: Id) {
    let text = from_ns(unsafe { msg_send![note, object] });
    events::enqueue(events::BusEvent::Command(text));
}

extern "C" fn will_terminate(_: &Object, _: Sel, _: Id) {
    with_session(|session| session.save());
}

extern "C" fn should_terminate(_: &Object, _: Sel, _: Id) -> i8 {
    0
}

extern "C" fn open_urls(_: &Object, _: Sel, _: Id, urls: Id) {
    unsafe {
        let count: usize = msg_send![urls, count];
        for index in 0..count {
            let url: Id = msg_send![urls, objectAtIndex: index];
            let text = from_ns(msg_send![url, absoluteString]);
            if let Some(command) = openlayout::engine::parse_url(&text) {
                with_session(|session| session.handle_command(command));
            }
        }
    }
    sync();
}

extern "C" fn mouse_down(_: &Object, _: Sel, event: Id) {
    let point = event_point(event);
    let column = HITS.with(|cell| {
        cell.borrow().iter().rev().find(|hit| hit.column.is_some() && hit.rect.contains(point)).and_then(|hit| hit.column)
    });
    DRAG.with(|cell| *cell.borrow_mut() = column);
}

extern "C" fn mouse_up(this: &Object, _: Sel, event: Id) {
    let point = event_point_in_view(this, event);
    let dragged = DRAG.with(|cell| cell.borrow_mut().take());
    let Some(column) = dragged else {
        return;
    };
    let target = HITS.with(|cell| {
        cell.borrow().iter().rev().find(|hit| hit.rect.contains(point)).map(|hit| (hit.column, hit.display))
    });
    if let Some((before, display)) = target {
        if before == Some(column) {
            return;
        }
        with_session(|session| session.move_column(column, display, before));
        sync();
    }
}

fn event_point(event: Id) -> NsPoint {
    unsafe { msg_send![event, locationInWindow] }
}

fn event_point_in_view(view: &Object, event: Id) -> NsPoint {
    let location = event_point(event);
    unsafe { msg_send![view, convertPoint: location fromView: NIL] }
}

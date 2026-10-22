use super::objc::{ns_string, Id, NIL};
use crate::store::TrackpadMode;
use objc::{class, msg_send};
use openlayout::engine::{Action, Binding, Mod};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub enum BusEvent {
    Action(Action),
    Scroll { delta: f64, x: f64, y: f64 },
    Flags(bool),
    Capture(u16, u8),
    Mouse { x: f64, y: f64, kind: MouseKind },
    Command(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Down,
    Up,
    Moved,
}

struct HotState {
    bindings: Vec<Binding>,
    recording: bool,
    trackpad: TrackpadMode,
    watch_mouse: bool,
    modifier_held: bool,
    tap: usize,
}

impl HotState {
    fn new() -> Self {
        Self {
            bindings: Binding::defaults(),
            recording: false,
            trackpad: TrackpadMode::WithModifier,
            watch_mouse: false,
            modifier_held: false,
            tap: 0,
        }
    }
}

static HOT: Mutex<HotState> = Mutex::new(HotState {
    bindings: Vec::new(),
    recording: false,
    trackpad: TrackpadMode::WithModifier,
    watch_mouse: false,
    modifier_held: false,
    tap: 0,
});

static QUEUE: Mutex<Vec<BusEvent>> = Mutex::new(Vec::new());
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static MONITOR: AtomicU64 = AtomicU64::new(0);

const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS: u32 = 12;
const SCROLL: u32 = 22;
const MOUSE_DOWN: u32 = 1;
const MOUSE_UP: u32 = 2;
const MOUSE_MOVED: u32 = 5;
const TAP_DISABLED: u32 = 0xFFFF_FFFE;

const CONTROL: u64 = 1 << 18;
const OPTION: u64 = 1 << 19;
const SHIFT: u64 = 1 << 17;
const COMMAND: u64 = 1 << 20;

extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events: u64,
        callback: extern "C" fn(*mut std::ffi::c_void, u32, *mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void,
        user: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    fn CGEventGetFlags(event: *mut std::ffi::c_void) -> u64;
    fn CGEventGetIntegerValueField(event: *mut std::ffi::c_void, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: *mut std::ffi::c_void, field: u32) -> f64;
    fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const std::ffi::c_void,
        port: *mut std::ffi::c_void,
        order: isize,
    ) -> *mut std::ffi::c_void;
    fn CFRunLoopGetMain() -> *mut std::ffi::c_void;
    fn CFRunLoopAddSource(run_loop: *mut std::ffi::c_void, source: *mut std::ffi::c_void, mode: *const std::ffi::c_void);
    static kCFRunLoopCommonModes: *const std::ffi::c_void;
    static _dispatch_main_q: u8;
    fn dispatch_async_f(
        queue: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct ScrollDescriptor {
    reserved: usize,
    size: usize,
    signature: *const i8,
    layout: *const i8,
}

unsafe impl Sync for ScrollDescriptor {}

#[repr(C)]
struct ScrollBlock {
    isa: *const u8,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*mut ScrollBlock, Id),
    descriptor: *const ScrollDescriptor,
}

static SCROLL_SIGNATURE: &[u8] = b"v16@?0@8\0";

static SCROLL_DESCRIPTOR: ScrollDescriptor = ScrollDescriptor {
    reserved: 0,
    size: std::mem::size_of::<ScrollBlock>(),
    signature: SCROLL_SIGNATURE.as_ptr().cast(),
    layout: std::ptr::null(),
};

extern "C" {
    static _NSConcreteGlobalBlock: u8;
}

unsafe extern "C" fn on_global_scroll(_: *mut ScrollBlock, event: Id) {
    if event.is_null() {
        return;
    }
    if HOT.lock().map(|hot| hot.tap != 0).unwrap_or(false) {
        return;
    }
    let kind: u64 = msg_send![event, type];
    let cg: *mut std::ffi::c_void = msg_send![event, CGEvent];
    let (dx, dy) = if kind == 31 {
        let dx: f64 = msg_send![event, deltaX];
        let dy: f64 = msg_send![event, deltaY];
        (dx * 280.0, dy * 280.0)
    } else if !cg.is_null() {
        raw_scroll(cg)
    } else {
        let precise: i8 = msg_send![event, hasPreciseScrollingDeltas];
        let mut dx: f64 = msg_send![event, scrollingDeltaX];
        let mut dy: f64 = msg_send![event, scrollingDeltaY];
        if precise == 0 {
            dx *= 16.0;
            dy *= 16.0;
        }
        (dx, dy)
    };
    let Some(delta) = pan_delta(dx, dy, vertical_scroll_allowed()) else {
        return;
    };
    let flags: u64 = msg_send![event, modifierFlags];
    if !scroll_wanted(flags) {
        return;
    };
    let (x, y) = if cg.is_null() {
        (f64::NAN, f64::NAN)
    } else {
        let point = CGEventGetLocation(cg);
        (point.x, point.y)
    };
    enqueue(BusEvent::Scroll { delta, x, y });
}

fn scroll_wanted(flags: u64) -> bool {
    let Ok(hot) = HOT.lock() else {
        return false;
    };
    match hot.trackpad {
        TrackpadMode::Off => false,
        TrackpadMode::WithModifier => (flags & CONTROL != 0 && flags & OPTION != 0) || hot.modifier_held,
        TrackpadMode::Horizontal => true,
    }
}

fn vertical_scroll_allowed() -> bool {
    HOT.lock().map(|hot| hot.trackpad != TrackpadMode::Horizontal).unwrap_or(true)
}

fn raw_scroll(event: *mut std::ffi::c_void) -> (f64, f64) {
    unsafe {
        let mut dx = CGEventGetDoubleValueField(event, 97);
        let mut dy = CGEventGetDoubleValueField(event, 96);
        if dx.abs() < 0.5 && dy.abs() < 0.5 {
            let ix = CGEventGetIntegerValueField(event, 97) as f64;
            let iy = CGEventGetIntegerValueField(event, 96) as f64;
            if ix.abs() >= 1.0 || iy.abs() >= 1.0 {
                dx = ix;
                dy = iy;
            }
        }
        if dx.abs() < 0.5 && dy.abs() < 0.5 {
            dx = CGEventGetIntegerValueField(event, 94) as f64 / 65536.0;
            dy = CGEventGetIntegerValueField(event, 93) as f64 / 65536.0;
        }
        if dx.abs() < 0.5 && dy.abs() < 0.5 {
            dx = CGEventGetIntegerValueField(event, 12) as f64 * 16.0;
            dy = CGEventGetIntegerValueField(event, 11) as f64 * 16.0;
        }
        (dx, dy)
    }
}

fn pan_delta(dx: f64, dy: f64, allow_vertical: bool) -> Option<f64> {
    if dx.abs() >= 0.5 && dx.abs() >= dy.abs() {
        return Some(dx);
    }
    if allow_vertical && dy.abs() >= 0.5 {
        return Some(dy);
    }
    None
}

pub fn install() -> bool {
    install_scroll_monitor();
    if HOT.lock().map(|hot| hot.tap != 0).unwrap_or(false) {
        return true;
    }
    let mask = (1u64 << KEY_DOWN)
        | (1u64 << KEY_UP)
        | (1u64 << FLAGS)
        | (1u64 << SCROLL)
        | (1u64 << MOUSE_DOWN)
        | (1u64 << MOUSE_UP)
        | (1u64 << MOUSE_MOVED);
    unsafe {
        let tap = CGEventTapCreate(1, 0, 0, mask, tap_callback, std::ptr::null_mut());
        if tap.is_null() {
            eprintln!("openlayout: event tap unavailable until Accessibility permission is granted");
            return false;
        }
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        if let Ok(mut hot) = HOT.lock() {
            hot.tap = tap as usize;
        }
        true
    }
}

pub fn install_scroll_monitor() {
    if MONITOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        let block = Box::leak(Box::new(ScrollBlock {
            isa: std::ptr::addr_of!(_NSConcreteGlobalBlock),
            flags: (1 << 28) | (1 << 30),
            reserved: 0,
            invoke: on_global_scroll,
            descriptor: &SCROLL_DESCRIPTOR,
        }));
        let mask = (1u64 << SCROLL) | (1u64 << 31);
        let monitor: Id = msg_send![class!(NSEvent), addGlobalMonitorForEventsMatchingMask: mask handler: block as *mut ScrollBlock];
        if !monitor.is_null() {
            let retained: Id = msg_send![monitor, retain];
            MONITOR.store(retained as u64, Ordering::SeqCst);
        }
    }
}

pub fn update(bindings: &[Binding], recording: bool, trackpad: TrackpadMode, watch_mouse: bool) {
    if let Ok(mut hot) = HOT.lock() {
        hot.bindings = bindings.to_vec();
        hot.recording = recording;
        hot.trackpad = trackpad;
        hot.watch_mouse = watch_mouse;
    }
}

pub fn post_command(command: &str) {
    unsafe {
        let pool: Id = msg_send![class!(NSAutoreleasePool), new];
        let center: Id = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let _: () = msg_send![center,
            postNotificationName: ns_string("com.openlayout.command")
            object: ns_string(command)
            userInfo: NIL
            deliverImmediately: 1i8
        ];
        let _: () = msg_send![pool, drain];
    }
    println!("sent `{command}` to openLayout");
}

pub fn enqueue(event: BusEvent) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(event);
    }
    unsafe {
        dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q).cast_mut().cast(), std::ptr::null_mut(), drain);
    }
}

extern "C" fn drain(_: *mut std::ffi::c_void) {
    let events = QUEUE.lock().map(|mut queue| queue.drain(..).collect::<Vec<_>>()).unwrap_or_default();
    if events.is_empty() {
        return;
    }
    crate::app::handle_events(events);
}

extern "C" fn tap_callback(
    _proxy: *mut std::ffi::c_void,
    event_type: u32,
    event: *mut std::ffi::c_void,
    _user: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    if event_type == TAP_DISABLED {
        let tap = HOT.lock().map(|hot| hot.tap as *mut std::ffi::c_void).unwrap_or(std::ptr::null_mut());
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    let Ok(mut hot) = HOT.lock() else {
        return event;
    };
    if hot.bindings.is_empty() && !hot.recording {
        // Still allow the defaults installed at startup. An empty list means
        // install() raced ahead of the first update, so keep listening.
    }
    let flags = unsafe { CGEventGetFlags(event) };
    let mods = mods_from(flags);
    match event_type {
        KEY_DOWN => {
            let code = unsafe { CGEventGetIntegerValueField(event, 9) } as u16;
            if hot.recording {
                drop(hot);
                enqueue(BusEvent::Capture(code, mods.bits()));
                return std::ptr::null_mut();
            }
            if let Some(action) = hot
                .bindings
                .iter()
                .find(|binding| binding.key_code == code && binding.modifiers == mods)
                .map(|binding| binding.action)
            {
                drop(hot);
                enqueue(BusEvent::Action(action));
                return std::ptr::null_mut();
            }
        }
        FLAGS => {
            let held = flags & CONTROL != 0 && flags & OPTION != 0;
            hot.modifier_held = held;
            drop(hot);
            enqueue(BusEvent::Flags(held));
            return event;
        }
        SCROLL => {
            let allow_vertical = hot.trackpad != TrackpadMode::Horizontal;
            let consume = match hot.trackpad {
                TrackpadMode::Off => false,
                TrackpadMode::WithModifier => (flags & CONTROL != 0 && flags & OPTION != 0) || hot.modifier_held,
                TrackpadMode::Horizontal => true,
            };
            drop(hot);
            if !consume {
                return event;
            }
            let (dx, dy) = raw_scroll(event);
            let Some(delta) = pan_delta(dx, dy, allow_vertical) else {
                return event;
            };
            let point = unsafe { CGEventGetLocation(event) };
            enqueue(BusEvent::Scroll { delta, x: point.x, y: point.y });
            return std::ptr::null_mut();
        }
        MOUSE_DOWN | MOUSE_UP | MOUSE_MOVED => {
            if event_type == MOUSE_MOVED && !hot.watch_mouse {
                return event;
            }
            let point = unsafe { CGEventGetLocation(event) };
            let kind = match event_type {
                MOUSE_DOWN => MouseKind::Down,
                MOUSE_UP => MouseKind::Up,
                _ => MouseKind::Moved,
            };
            drop(hot);
            enqueue(BusEvent::Mouse { x: point.x, y: point.y, kind });
            return event;
        }
        KEY_UP => {}
        _ => {}
    }
    event
}

fn mods_from(flags: u64) -> Mod {
    let mut bits = 0u8;
    if flags & CONTROL != 0 {
        bits |= Mod::CONTROL;
    }
    if flags & OPTION != 0 {
        bits |= Mod::OPTION;
    }
    if flags & SHIFT != 0 {
        bits |= Mod::SHIFT;
    }
    if flags & COMMAND != 0 {
        bits |= Mod::COMMAND;
    }
    Mod::new(bits)
}

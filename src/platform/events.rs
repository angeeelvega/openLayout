use super::objc::{ns_string, Id, NIL};
use crate::store::TrackpadMode;
use objc::{class, msg_send};
use openlayout::engine::{Action, Binding, Mod};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub enum BusEvent {
    Action(Action),
    Scroll(f64),
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
    tap: usize,
}

impl HotState {
    fn new() -> Self {
        Self {
            bindings: Binding::defaults(),
            recording: false,
            trackpad: TrackpadMode::WithModifier,
            watch_mouse: false,
            tap: 0,
        }
    }
}

static HOT: Mutex<HotState> = Mutex::new(HotState {
    bindings: Vec::new(),
    recording: false,
    trackpad: TrackpadMode::WithModifier,
    watch_mouse: false,
    tap: 0,
});

static QUEUE: Mutex<Vec<BusEvent>> = Mutex::new(Vec::new());

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

pub fn install() {
    if HOT.lock().map(|hot| hot.tap != 0).unwrap_or(false) {
        return;
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
            return;
        }
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        if let Ok(mut hot) = HOT.lock() {
            hot.tap = tap as usize;
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
    let Ok(hot) = HOT.lock() else {
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
            drop(hot);
            enqueue(BusEvent::Flags(held));
            return event;
        }
        SCROLL => {
            let dx = unsafe { CGEventGetDoubleValueField(event, 97) };
            let dy = unsafe { CGEventGetDoubleValueField(event, 96) };
            if dx.abs() < 0.5 || dx.abs() < dy.abs() {
                return event;
            }
            let consume = match hot.trackpad {
                TrackpadMode::Off => false,
                TrackpadMode::WithModifier => flags & CONTROL != 0 && flags & OPTION != 0,
                TrackpadMode::Horizontal => true,
            };
            if consume {
                drop(hot);
                enqueue(BusEvent::Scroll(dx));
                return std::ptr::null_mut();
            }
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

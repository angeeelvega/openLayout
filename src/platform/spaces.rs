use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::sync::OnceLock;

const RTLD_LAZY: i32 = 1;
const SINT32: i32 = 3;
const SINT64: i32 = 4;

type ConnectionFn = unsafe extern "C" fn() -> i32;
type ActiveSpaceFn = unsafe extern "C" fn(i32) -> i32;
type SpacesForWindowsFn = unsafe extern "C" fn(i32, i32, *const c_void) -> *const c_void;
type MoveWindowFn = unsafe extern "C" fn(i32, u32, *const c_void) -> i32;

struct Sky {
    connection: ConnectionFn,
    active_space: ActiveSpaceFn,
    spaces_for_windows: SpacesForWindowsFn,
    move_window: MoveWindowFn,
}

fn sky() -> Option<&'static Sky> {
    static SKY: OnceLock<Option<Sky>> = OnceLock::new();
    SKY.get_or_init(|| unsafe {
        let path = CString::new("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight").ok()?;
        let handle = dlopen(path.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let connection = std::mem::transmute(symbol(handle, "CGSMainConnectionID")?);
        let active_space = std::mem::transmute(symbol(handle, "CGSGetActiveSpace")?);
        let spaces_for_windows = std::mem::transmute(symbol(handle, "CGSCopySpacesForWindows")?);
        let move_window = std::mem::transmute(symbol(handle, "SLSMoveWindow")?);
        Some(Sky { connection, active_space, spaces_for_windows, move_window })
    })
    .as_ref()
}

fn symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
    let name = CString::new(name).ok()?;
    let pointer = unsafe { dlsym(handle, name.as_ptr()) };
    if pointer.is_null() { None } else { Some(pointer) }
}

/// Move a window through the window server. `x` and `y` are the Cocoa origin
/// (bottom-left of the window, Y growing upward from the primary display).
pub fn move_window(window: u32, x: f64, y: f64) -> bool {
    let Some(sky) = sky() else {
        return false;
    };
    if window == 0 {
        return false;
    }
    let point = [x, y];
    unsafe {
        (sky.move_window)((sky.connection)(), window, point.as_ptr().cast()) == 0
    }
}

pub fn active_space() -> u64 {
    let Some(sky) = sky() else {
        return 0;
    };
    unsafe {
        let id = (sky.active_space)((sky.connection)());
        if id <= 0 { 0 } else { id as u64 }
    }
}

pub fn spaces_for(window_ids: &[u32]) -> HashMap<u32, u64> {
    let mut found = HashMap::new();
    let Some(sky) = sky() else {
        return found;
    };
    unsafe {
        let connection = (sky.connection)();
        for id in window_ids {
            if let Some(space) = space_of(sky, connection, *id) {
                found.insert(*id, space);
            }
        }
    }
    found
}

unsafe fn space_of(sky: &Sky, connection: i32, window: u32) -> Option<u64> {
    let mut number = window as i32;
    let cf_number = CFNumberCreate(std::ptr::null(), SINT32, &mut number as *mut i32 as *const c_void);
    if cf_number.is_null() {
        return None;
    }
    let values = [cf_number];
    let array = CFArrayCreate(std::ptr::null(), values.as_ptr(), 1, &kCFTypeArrayCallBacks as *const _ as *const c_void);
    CFRelease(cf_number);
    if array.is_null() {
        return None;
    }
    let spaces = (sky.spaces_for_windows)(connection, 7, array);
    CFRelease(array);
    if spaces.is_null() || CFArrayGetCount(spaces) < 1 {
        if !spaces.is_null() {
            CFRelease(spaces);
        }
        return None;
    }
    let first = CFArrayGetValueAtIndex(spaces, 0);
    let mut raw: i64 = 0;
    let ok = CFNumberGetValue(first, SINT64, &mut raw as *mut i64 as *mut c_void);
    if !ok {
        let mut narrow: i32 = 0;
        if CFNumberGetValue(first, SINT32, &mut narrow as *mut i32 as *mut c_void) {
            raw = narrow as i64;
        }
    }
    CFRelease(spaces);
    if raw <= 0 { None } else { Some(raw as u64) }
}

extern "C" {
    fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    fn CFRelease(value: *const c_void);
    fn CFNumberCreate(allocator: *const c_void, kind: i32, value: *const c_void) -> *const c_void;
    fn CFNumberGetValue(number: *const c_void, kind: i32, value: *mut c_void) -> bool;
    fn CFArrayCreate(allocator: *const c_void, values: *const *const c_void, count: isize, callbacks: *const c_void) -> *const c_void;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
    static kCFTypeArrayCallBacks: u8;
}

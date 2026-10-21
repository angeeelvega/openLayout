use super::objc::{from_ns, ns_string, Id, NIL};
use objc::{class, msg_send};
use openlayout::engine::Rect;
use std::ffi::CString;

type CFTypeRef = *const std::ffi::c_void;

const UTF8: u32 = 0x0800_0100;
const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, idx: isize) -> *const std::ffi::c_void;
    fn CFStringCreateWithCString(alloc: *const std::ffi::c_void, cstr: *const i8, encoding: u32) -> CFTypeRef;
    fn CFStringGetCString(string: CFTypeRef, buffer: *mut i8, size: isize, encoding: u32) -> u8;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    static kCFBooleanTrue: CFTypeRef;
    static kCFBooleanFalse: CFTypeRef;

    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(element: CFTypeRef, attribute: CFTypeRef, value: CFTypeRef) -> i32;
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFTypeRef) -> i32;
    fn AXValueCreate(kind: u32, value: *const std::ffi::c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, out: *mut std::ffi::c_void) -> bool;
    fn AXValueGetTypeID() -> usize;
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    fn _AXUIElementGetWindow(element: CFTypeRef, wid: *mut u32) -> i32;
}

pub struct AxElement(pub usize);

impl Drop for AxElement {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { CFRelease(self.0 as CFTypeRef) }
        }
    }
}

impl AxElement {
    pub fn raw(&self) -> usize {
        self.0
    }
}

pub struct RawWindow {
    pub pid: i32,
    pub number: u32,
    pub element: AxElement,
    pub bundle_id: String,
    pub app_name: String,
    pub title: String,
    pub frame: Rect,
    pub standard: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub resizable: bool,
}

pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn prompt_trust() {
    unsafe {
        let key = ns_string("AXTrustedCheckOptionPrompt");
        let value: Id = msg_send![class!(NSNumber), numberWithBool: 1i8];
        let dict: Id = msg_send![class!(NSDictionary), dictionaryWithObject: value forKey: key];
        let _ = AXIsProcessTrustedWithOptions(dict as CFTypeRef);
    }
}

pub fn list_windows(primary_height: f64) -> Vec<RawWindow> {
    if !trusted() {
        return Vec::new();
    }
    let own = std::process::id() as i32;
    unsafe {
        let workspace: Id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let apps: Id = msg_send![workspace, runningApplications];
        if apps.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![apps, count];
        let mut windows = Vec::new();
        for index in 0..count {
            let app: Id = msg_send![apps, objectAtIndex: index];
            let pid: i32 = msg_send![app, processIdentifier];
            if pid == own {
                continue;
            }
            let policy: isize = msg_send![app, activationPolicy];
            if policy == 2 {
                continue;
            }
            let bundle = from_ns(msg_send![app, bundleIdentifier]);
            let name = from_ns(msg_send![app, localizedName]);
            if is_system_agent(&bundle) {
                continue;
            }
            collect_app(pid, &bundle, &name, primary_height, &mut windows);
        }
        windows
    }
}

pub fn set_frame(element: usize, frame: Rect, primary_height: f64) {
    if element == 0 {
        return;
    }
    let origin = CGPoint {
        x: frame.x,
        y: primary_height - frame.y - frame.height,
    };
    let size = CGSize {
        width: frame.width.max(40.0),
        height: frame.height.max(40.0),
    };
    unsafe {
        let element = element as CFTypeRef;
        if let Some(size_value) = ax_value(AX_SIZE, &size as *const CGSize as *const _) {
            let _ = AXUIElementSetAttributeValue(element, cf_string("AXSize"), size_value);
            CFRelease(size_value);
        }
        if let Some(point_value) = ax_value(AX_POINT, &origin as *const CGPoint as *const _) {
            let _ = AXUIElementSetAttributeValue(element, cf_string("AXPosition"), point_value);
            CFRelease(point_value);
        }
    }
}

pub fn read_frame(element: usize, primary_height: f64) -> Option<Rect> {
    if element == 0 {
        return None;
    }
    unsafe {
        let element = element as CFTypeRef;
        let origin = ax_point(element, "AXPosition")?;
        let size = ax_size(element, "AXSize")?;
        Some(Rect::new(
            origin.x,
            primary_height - origin.y - size.height,
            size.width,
            size.height,
        ))
    }
}

pub fn focus(element: usize, pid: i32) {
    if element == 0 {
        return;
    }
    unsafe {
        let element = element as CFTypeRef;
        let _ = AXUIElementSetAttributeValue(element, cf_string("AXMain"), kCFBooleanTrue);
        let _ = AXUIElementSetAttributeValue(element, cf_string("AXFocused"), kCFBooleanTrue);
        let _ = AXUIElementPerformAction(element, cf_string("AXRaise"));
        let running: Id = msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if !running.is_null() {
            let _: i8 = msg_send![running, activateWithOptions: 2usize];
        }
    }
}

pub fn release_element(element: usize) {
    if element != 0 {
        unsafe { CFRelease(element as CFTypeRef) }
    }
}

pub fn warp_cursor(frame: Rect, primary_height: f64) {
    let point = CGPoint {
        x: frame.mid_x(),
        y: primary_height - frame.mid_y(),
    };
    unsafe {
        CGWarpMouseCursorPosition(point);
    }
}

extern "C" {
    fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
}

fn collect_app(pid: i32, bundle: &str, name: &str, primary_height: f64, out: &mut Vec<RawWindow>) {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return;
        }
        let mut value: CFTypeRef = std::ptr::null();
        let error = AXUIElementCopyAttributeValue(app, cf_string("AXWindows"), &mut value);
        CFRelease(app);
        if error != 0 || value.is_null() {
            return;
        }
        let count = CFArrayGetCount(value);
        for index in 0..count {
            let item = CFArrayGetValueAtIndex(value, index);
            if item.is_null() {
                continue;
            }
            let retained = CFRetain(item);
            if let Some(window) = describe(pid, bundle, name, retained, primary_height) {
                out.push(window);
            }
        }
        CFRelease(value);
    }
}

fn describe(pid: i32, bundle: &str, name: &str, element: CFTypeRef, primary_height: f64) -> Option<RawWindow> {
    let subrole = copy_string(element, "AXSubrole").unwrap_or_default();
    let standard = subrole.is_empty() || subrole == "AXStandardWindow";
    if !standard && !subrole.is_empty() {
        unsafe { CFRelease(element) };
        return None;
    }
    let minimized = copy_bool(element, "AXMinimized");
    let fullscreen = copy_bool(element, "AXFullScreen");
    let resizable = copy_bool(element, "AXResizable");
    let title = copy_string(element, "AXTitle").unwrap_or_default();
    let Some(origin) = ax_point(element, "AXPosition") else {
        unsafe { CFRelease(element) };
        return None;
    };
    let Some(size) = ax_size(element, "AXSize") else {
        unsafe { CFRelease(element) };
        return None;
    };
    if size.width < 80.0 || size.height < 60.0 {
        unsafe { CFRelease(element) };
        return None;
    }
    let mut number = 0u32;
    let status = unsafe { _AXUIElementGetWindow(element, &mut number) };
    if status != 0 || number == 0 {
        number = fallback_number(pid, &title, origin.x, origin.y);
    }
    Some(RawWindow {
        pid,
        number,
        element: AxElement(element as usize),
        bundle_id: bundle.to_string(),
        app_name: name.to_string(),
        title,
        frame: Rect::new(origin.x, primary_height - origin.y - size.height, size.width, size.height),
        standard,
        minimized,
        fullscreen,
        resizable,
    })
}

fn fallback_number(pid: i32, title: &str, x: f64, y: f64) -> u32 {
    let mut hash = pid as u32;
    for byte in title.bytes() {
        hash = hash.wrapping_mul(16777619) ^ byte as u32;
    }
    hash ^= (x as i32 as u32).wrapping_mul(31);
    hash ^ (y as i32 as u32)
}

fn is_system_agent(bundle: &str) -> bool {
    matches!(
        bundle,
        "com.apple.dock"
            | "com.apple.controlcenter"
            | "com.apple.notificationcenterui"
            | "com.apple.SystemUIServer"
            | "com.apple.WindowManager"
            | "com.apple.Spotlight"
            | "com.apple.loginwindow"
    )
}

fn cf_string(text: &str) -> CFTypeRef {
    let c = CString::new(text).unwrap_or_else(|_| CString::new("").unwrap());
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}

fn copy_string(element: CFTypeRef, attribute: &str) -> Option<String> {
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let attr = cf_string(attribute);
        let error = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr);
        if error != 0 || value.is_null() {
            return None;
        }
        let mut buffer = vec![0i8; 1024];
        let ok = CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8);
        CFRelease(value);
        if ok == 0 {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn copy_bool(element: CFTypeRef, attribute: &str) -> bool {
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let attr = cf_string(attribute);
        let error = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr);
        if error != 0 || value.is_null() {
            return false;
        }
        let flag = CFBooleanGetValue(value) != 0;
        CFRelease(value);
        flag
    }
}

fn ax_point(element: CFTypeRef, attribute: &str) -> Option<CGPoint> {
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let attr = cf_string(attribute);
        let error = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr);
        if error != 0 || value.is_null() || CFGetTypeID(value) != AXValueGetTypeID() {
            if !value.is_null() {
                CFRelease(value);
            }
            return None;
        }
        let mut point = CGPoint { x: 0.0, y: 0.0 };
        let ok = AXValueGetValue(value, AX_POINT, &mut point as *mut CGPoint as *mut _);
        CFRelease(value);
        if ok { Some(point) } else { None }
    }
}

fn ax_size(element: CFTypeRef, attribute: &str) -> Option<CGSize> {
    unsafe {
        let mut value: CFTypeRef = std::ptr::null();
        let attr = cf_string(attribute);
        let error = AXUIElementCopyAttributeValue(element, attr, &mut value);
        CFRelease(attr);
        if error != 0 || value.is_null() || CFGetTypeID(value) != AXValueGetTypeID() {
            if !value.is_null() {
                CFRelease(value);
            }
            return None;
        }
        let mut size = CGSize { width: 0.0, height: 0.0 };
        let ok = AXValueGetValue(value, AX_SIZE, &mut size as *mut CGSize as *mut _);
        CFRelease(value);
        if ok { Some(size) } else { None }
    }
}

fn ax_value(kind: u32, value: *const std::ffi::c_void) -> Option<CFTypeRef> {
    let created = unsafe { AXValueCreate(kind, value) };
    if created.is_null() { None } else { Some(created) }
}

#[allow(dead_code)]
pub fn bool_ref(value: bool) -> CFTypeRef {
    unsafe { if value { kCFBooleanTrue } else { kCFBooleanFalse } }
}

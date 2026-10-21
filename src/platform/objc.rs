use objc::runtime::Object;
use objc::{class, msg_send, sel_impl};
use objc::runtime::Sel;
use std::ffi::{CStr, CString};
use std::ptr;

pub type Id = *mut Object;

pub const NIL: Id = ptr::null_mut();

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NsPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NsSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NsRect {
    pub origin: NsPoint,
    pub size: NsSize,
}

impl NsRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: NsPoint { x, y },
            size: NsSize { width, height },
        }
    }

    pub fn contains(self, point: NsPoint) -> bool {
        point.x >= self.origin.x
            && point.x < self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y < self.origin.y + self.size.height
    }
}

pub fn ns_string(text: &str) -> Id {
    let owned = text.replace('\0', "");
    let c = CString::new(owned).unwrap_or_else(|_| CString::new("").unwrap());
    unsafe {
        let object: Id = msg_send![class!(NSString), alloc];
        msg_send![object, initWithUTF8String: c.as_ptr()]
    }
}

pub fn from_ns(object: Id) -> String {
    if object.is_null() {
        return String::new();
    }
    unsafe {
        let ptr: *const i8 = msg_send![object, UTF8String];
        if ptr.is_null() {
            return String::new();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

pub fn shared_app() -> Id {
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

pub fn activate() {
    unsafe {
        let app = shared_app();
        let _: () = msg_send![app, activateIgnoringOtherApps: 1i8];
    }
}

pub fn color(red: f64, green: f64, blue: f64, alpha: f64) -> Id {
    unsafe {
        msg_send![class!(NSColor), colorWithCalibratedRed: red green: green blue: blue alpha: alpha]
    }
}

pub fn release(object: Id) {
    if object.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![object, release];
    }
}

pub fn selector(name: &str) -> Sel {
    sel_impl!(name)
}

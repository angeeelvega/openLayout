use super::objc::{from_ns, ns_string, Id, NsRect};
use objc::{class, msg_send};
use openlayout::engine::{DisplaySlice, Rect};

pub struct ScreenInfo {
    pub id: u32,
    pub frame: Rect,
    pub visible: Rect,
    pub name: String,
}

pub fn screens() -> Vec<ScreenInfo> {
    unsafe {
        let list: Id = msg_send![class!(NSScreen), screens];
        if list.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![list, count];
        let mut screens = Vec::with_capacity(count);
        for index in 0..count {
            let screen: Id = msg_send![list, objectAtIndex: index];
            let frame: NsRect = msg_send![screen, frame];
            let visible: NsRect = msg_send![screen, visibleFrame];
            let description: Id = msg_send![screen, deviceDescription];
            let number: Id = msg_send![description, objectForKey: ns_string("NSScreenNumber")];
            let id: u32 = if number.is_null() {
                0
            } else {
                msg_send![number, unsignedIntValue]
            };
            let name = from_ns(msg_send![screen, localizedName]);
            screens.push(ScreenInfo {
                id,
                frame: to_rect(frame),
                visible: to_rect(visible),
                name: if name.is_empty() { format!("Display {id}") } else { name },
            });
        }
        screens
    }
}

pub fn primary_height() -> f64 {
    screens()
        .into_iter()
        .find(|screen| screen.frame.x == 0.0 && screen.frame.y == 0.0)
        .map(|screen| screen.frame.height)
        .unwrap_or(0.0)
}

pub fn display_slices(outer_gap: f64) -> Vec<DisplaySlice> {
    screens()
        .into_iter()
        .filter(|screen| screen.id != 0)
        .map(|screen| {
            let usable = Rect::new(
                screen.visible.x + outer_gap,
                screen.visible.y + outer_gap,
                (screen.visible.width - outer_gap * 2.0).max(120.0),
                (screen.visible.height - outer_gap * 2.0).max(120.0),
            );
            DisplaySlice {
                id: screen.id,
                frame: screen.frame,
                usable,
                name: screen.name,
            }
        })
        .collect()
}

fn to_rect(rect: NsRect) -> Rect {
    Rect::new(rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    unsafe {
        let service: Id = msg_send![class!(SMAppService), mainApp];
        if service.is_null() {
            return Err("Launch at login is unavailable on this Mac.".into());
        }
        let mut error: Id = super::objc::NIL;
        let ok: i8 = if enabled {
            msg_send![service, registerAndReturnError: &mut error]
        } else {
            msg_send![service, unregisterAndReturnError: &mut error]
        };
        if ok == 0 {
            let message = if error.is_null() {
                "Could not update the login item.".to_string()
            } else {
                from_ns(msg_send![error, localizedDescription])
            };
            Err(message)
        } else {
            Ok(())
        }
    }
}

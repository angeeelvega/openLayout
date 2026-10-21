use crate::platform::events::BusEvent;
use crate::session::Session;
use std::cell::RefCell;

thread_local! {
    static APP: RefCell<Option<Session>> = RefCell::new(None);
}

pub fn install(session: Session) {
    APP.with(|cell| *cell.borrow_mut() = Some(session));
}

pub fn with_session<T>(body: impl FnOnce(&mut Session) -> T) -> Option<T> {
    APP.with(|cell| cell.borrow_mut().as_mut().map(body))
}

pub fn handle_events(events: Vec<BusEvent>) {
    with_session(|session| {
        for event in events {
            session.handle_bus(event);
        }
    });
    crate::ui::sync();
}

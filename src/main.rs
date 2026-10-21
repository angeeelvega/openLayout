mod app;
mod platform;
mod session;
mod store;
mod ui;

#[macro_use]
extern crate objc;

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(head) = args.next() {
        let mut command = head;
        for arg in args {
            command.push(' ');
            command.push_str(&arg);
        }
        platform::events::post_command(&command);
        return;
    }
    ui::run();
}

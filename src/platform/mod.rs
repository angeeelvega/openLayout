pub mod ax;
pub mod events;
pub mod objc;
pub mod screens;
pub mod spaces;

#[link(name = "AppKit", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "IOKit", kind = "framework")]
#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

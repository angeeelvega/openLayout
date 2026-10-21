//! Layout engine for openLayout.
//!
//! Window placement is pure and allocation-light enough to run on every frame.
//! Talking to macOS Accessibility lives in the application binary, not here.

pub mod engine;

//! macOS и Linux: тему ведёт winit, акцент — `#0078D4`, шрифты — встроенные egui.
//! macOS (`controlAccentColor`, системный шрифт через Core Text) — в пассиве,
//! docs/macos-notes.md.

use std::sync::mpsc;

use super::{BackdropCaps, FontFiles, Look};

pub(super) fn look() -> Look {
    Look::default()
}

pub(super) fn backdrop() -> BackdropCaps {
    BackdropCaps::default()
}

pub(super) fn fonts() -> FontFiles {
    FontFiles::default()
}

pub(super) fn watch(_ctx: egui::Context, _sink: mpsc::Sender<Look>) {}

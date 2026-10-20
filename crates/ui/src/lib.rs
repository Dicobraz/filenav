//! Urfin — UI-фронтенд. Переезд на GPUI (ADR-0018, `docs/TASKS-GPUI.md`):
//! [`gpui_shell`] — новый интерфейс (пока пустое окно, `urfin --ui=gpui`);
//! прежний egui-интерфейс — в `crates/ui/legacy` до G-06 и по умолчанию,
//! его API реэкспортирован из корня крейта.

#[path = "../legacy/mod.rs"]
mod legacy;

pub use legacy::*;

pub mod gpui_shell;

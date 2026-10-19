//! Интерфейс на GPUI (ADR-0018). G-01 — фундамент: пустое окно по `urfin --ui=gpui`.
//! Core, панели и ввод — G-02…G-05.

use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, px, size,
};

/// Пустой корневой view.
struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// Цикл приложения GPUI с одним пустым окном; возвращает управление после закрытия.
pub fn run() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        if let Err(e) = gpui_kit::open_window(options, cx, |_window, cx| cx.new(|_| Shell)) {
            tracing::error!(error = %e, "gpui window");
            cx.quit();
        }
        cx.activate(true);
    });
}

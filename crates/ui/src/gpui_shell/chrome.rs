//! Каркас окна на GPUI (G-05, S2 §1.4, §2.12): заголовок 40 px с табами каждой панели
//! над своей карточкой и кнопками «─ □ ✕» (системный заголовок скрыт, зоны —
//! `WindowControlArea`: перетаскивание, Snap Layouts на «□»), cmdline и строка F-клавиш.

use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Div, Hsla, IntoElement, Render, SharedString, Stateful, WeakEntity,
    Window, WindowControlArea, div, px, rgb,
};
use urfin_core::{AppState, Command, Side, Tab};

use super::CoreHost;
use super::panel::Listen;
use super::theme::Theme;
use crate::keybar::{Item, Mods};
use crate::tokens::{Metric, Role};

/// Шрифт иконок ОС (S2 §1.6).
const ICON_FONT: &str = "Segoe Fluent Icons";
const W_CAPTION: f32 = 46.0;
const W_TAB_MAX: f32 = 220.0;
/// «Закрыть» при наведении — красный Windows.
const CLOSE_HOVER: u32 = 0x00c4_2b1c;

/// Имя таба: последний компонент пути (у корня — сам путь).
pub(super) fn tab_title(t: &Tab) -> String {
    t.loc.path.file_name().map_or_else(
        || t.loc.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Табы панели: активный сливается с карточкой (фон панели, скругление сверху).
fn tabs(
    s: &AppState,
    side: Side,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
) -> Div {
    let p = s.panel(side);
    let mut row = div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_end()
        .gap_1()
        .overflow_hidden();
    for (i, t) in p.tabs.iter().enumerate() {
        let active = i == p.active;
        let id = SharedString::from(format!("tab-{}-{}", side_name(side), t.id.0));
        let tab = div()
            .id(id)
            .h(px(theme.m(Metric::HTab)))
            .max_w(px(W_TAB_MAX))
            .min_w_0()
            .flex()
            .items_center()
            .px(px(theme.m(Metric::PadRowX)))
            .rounded_t(px(theme.m(Metric::RadiusOverlay)))
            .text_color(theme.c(if active {
                Role::TextPrimary
            } else {
                Role::TextSecondary
            }))
            .when(active, |d| d.bg(theme.c(Role::LayerPanel)))
            .when(!active, |d| {
                d.hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
            })
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(tab_title(t))),
            )
            .on_click(click(
                Command::new("tab.goto")
                    .arg("side", side_name(side))
                    .arg("n", i64::try_from(i + 1).unwrap_or(1)),
            ));
        row = row.child(tab);
    }
    row.child(
        div()
            .id(SharedString::from(format!("tab-new-{}", side_name(side))))
            .h(px(theme.m(Metric::HTab)))
            .w(px(theme.m(Metric::HTab)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(theme.m(Metric::RadiusControl)))
            .font_family(ICON_FONT)
            .text_color(theme.c(Role::TextSecondary))
            .hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
            .child(SharedString::from("\u{E710}"))
            .on_click(click(Command::new("tab.new").arg("side", side_name(side)))),
    )
}

pub(super) fn side_name(side: Side) -> &'static str {
    if side == Side::Left { "left" } else { "right" }
}

fn caption(
    id: &'static str,
    glyph: char,
    area: WindowControlArea,
    theme: &Theme,
    hover: Hsla,
    fg_hover: Hsla,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(W_CAPTION))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(ICON_FONT)
        .text_size(px(10.0))
        .text_color(theme.c(Role::TextPrimary))
        .hover(move |d| d.bg(hover).text_color(fg_hover))
        .window_control_area(area)
        .child(SharedString::from(glyph.to_string()))
}

/// Табы обеих панелей (содержимое заголовка, когда меню F9 закрыто).
pub(super) fn panel_tabs(
    s: &AppState,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
) -> Div {
    let gap = px(theme.m(Metric::GapPanels));
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_row()
        .gap(gap)
        .child(tabs(s, Side::Left, theme, click))
        .child(tabs(s, Side::Right, theme, click))
}

/// Заголовок окна: [содержимое][место для перетаскивания][─ □ ✕].
pub(super) fn titlebar(content: Div, theme: &Theme, maximized: bool) -> Div {
    let gap = px(theme.m(Metric::GapPanels));
    let hover = theme.c(Role::FillSubtleHover);
    let primary = theme.c(Role::TextPrimary);
    let white: Hsla = rgb(0x00ff_ffff).into();
    div()
        .h(px(theme.m(Metric::HTitlebar)))
        .flex_none()
        .flex()
        .flex_row()
        .pl(gap)
        .gap(gap)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_row()
                .gap(gap)
                .child(content)
                .child(
                    div()
                        .id("drag")
                        .w(px(48.0))
                        .flex_none()
                        .h_full()
                        .window_control_area(WindowControlArea::Drag),
                ),
        )
        .child(caption(
            "min",
            '\u{E921}',
            WindowControlArea::Min,
            theme,
            hover,
            primary,
        ))
        .child(caption(
            "max",
            if maximized { '\u{E923}' } else { '\u{E922}' },
            WindowControlArea::Max,
            theme,
            hover,
            primary,
        ))
        .child(caption(
            "close",
            '\u{E8BB}',
            WindowControlArea::Close,
            theme,
            rgb(CLOSE_HOVER).into(),
            white,
        ))
}

/// Строка приглашения: `путь>` активной панели (ввод команд — S6).
pub(super) fn cmdline(s: &AppState, theme: &Theme) -> Div {
    let path = s.panel(s.active).tab().loc.path.display().to_string();
    div()
        .h(px(theme.m(Metric::HCmdline)))
        .flex_none()
        .flex()
        .items_center()
        .px(px(theme.m(Metric::GapPanels) + theme.m(Metric::PadRowX)))
        .font_family("Cascadia Mono")
        .text_size(px(theme.m(Metric::TypeMono)))
        .text_color(theme.c(Role::TextSecondary))
        .child(SharedString::from(format!("{path}>")))
}

/// Строка F-клавиш: 10 равных слотов «номер + подпись»; клик — команда слота.
pub(super) fn keybar(
    items: &[Item],
    mods: Mods,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
) -> Div {
    let mut bar = div()
        .h(px(theme.m(Metric::HKeybar)))
        .flex_none()
        .flex()
        .flex_row()
        .px(px(theme.m(Metric::GapPanels)))
        .text_size(px(theme.m(Metric::TypeCaption)));
    for n in 1..=10u8 {
        let item = items.iter().find(|i| i.n == n);
        let mut slot = div()
            .id(SharedString::from(format!("fkey-{n}-{mods:?}")))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded(px(theme.m(Metric::RadiusControl)))
            .child(
                div()
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(n.to_string())),
            );
        if let Some(it) = item {
            slot = slot
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(theme.c(if it.enabled {
                            Role::TextPrimary
                        } else {
                            Role::TextDisabled
                        }))
                        .child(SharedString::from(it.label.clone())),
                )
                .when(it.enabled, |d| {
                    d.hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
                        .on_click(click(it.cmd.clone()))
                });
        }
        bar = bar.child(slot);
    }
    bar
}

/// Кэшируемые части каркаса (G-07): табы в заголовке и низ окна (cmdline + F-клавиши).
/// Сам заголовок с «─ □ ✕» не кэшируется: gpui не повторяет `WindowControlArea` из кэша.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Tabs,
    Bottom,
}

pub(super) struct ChromeView {
    pub host: WeakEntity<CoreHost>,
    pub part: Part,
}

impl Render for ChromeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(host) = self.host.upgrade() else {
            return div();
        };
        let h = host.read(cx);
        let Some(core) = h.core.as_ref() else {
            return div();
        };
        let s = core.state();
        let l = Listen(self.host.clone());
        let click = |cmd: Command| -> super::overlays::Handler {
            Box::new(l.listener(move |h: &mut CoreHost, _: &ClickEvent, w, cx| {
                h.run(cmd.clone(), w, cx);
            }))
        };
        match self.part {
            Part::Tabs => panel_tabs(s, &h.theme, &click),
            Part::Bottom => div()
                .size_full()
                .flex()
                .flex_col()
                .child(cmdline(s, &h.theme))
                .child(keybar(h.keybar.items(h.mods), h.mods, &h.theme, &click)),
        }
    }
}

/// Отпечаток табов: имена и активные табы обеих панелей, тема, язык.
pub(super) fn tabs_print(s: &AppState, dark: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    (dark, urfin_i18n::epoch()).hash(&mut h);
    for side in [Side::Left, Side::Right] {
        let p = s.panel(side);
        p.active.hash(&mut h);
        for t in &p.tabs {
            (t.id.0, tab_title(t)).hash(&mut h);
        }
    }
    h.finish()
}

/// Отпечаток низа: путь активной панели, подписи F-клавиш, модификаторы, тема.
pub(super) fn bottom_print(s: &AppState, items: &[Item], mods: Mods, dark: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    (dark, urfin_i18n::epoch(), format!("{mods:?}")).hash(&mut h);
    s.panel(s.active).tab().loc.path.hash(&mut h);
    for it in items {
        (it.n, &it.label, it.enabled).hash(&mut h);
    }
    h.finish()
}

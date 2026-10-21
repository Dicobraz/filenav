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
/// Таб (D-02): поля и скругление сверху.
const TAB_PAD_X: f32 = 14.0;
const TAB_RADIUS: f32 = 7.0;
/// Отступ заголовка слева.
const TITLE_PAD_L: f32 = 12.0;
/// «Закрыть» при наведении — красный Windows.
const CLOSE_HOVER: u32 = 0x00c4_2b1c;

/// Имя таба: последний компонент пути (у корня — сам путь).
pub(super) fn tab_title(t: &Tab) -> String {
    t.loc.path.file_name().map_or_else(
        || t.loc.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Набор табов панели (D-02): таб 32, поля 14, скругление 7 сверху. Активный таб
/// активной панели — фон `panel`, текст и крестик; активный таб другой панели — фон
/// `panel`, `secondary`, без крестика; остальные — без фона, `secondary`. Свой «+».
/// `width` — ширина набора (по панели); `None` — до конца заголовка.
fn tabs(
    s: &AppState,
    side: Side,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
    width: Option<f32>,
) -> Div {
    let p = s.panel(side);
    let focus = s.active == side;
    let name = side_name(side);
    let mut row = div()
        .min_w_0()
        .h_full()
        .flex()
        .items_end()
        .gap(px(2.0))
        .overflow_hidden();
    row = match width {
        Some(w) => row.w(px(w)).flex_none(),
        None => row.flex_1(),
    };
    for (i, t) in p.tabs.iter().enumerate() {
        let active = i == p.active;
        let id = SharedString::from(format!("tab-{name}-{}", t.id.0));
        let tab = div()
            .id(id)
            .h(px(theme.m(Metric::HTab)))
            .max_w(px(W_TAB_MAX))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(TAB_PAD_X))
            .rounded_t(px(TAB_RADIUS))
            .text_color(theme.c(if active && focus {
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
            .when(active && focus, |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("tab-close-{name}")))
                        .text_size(px(11.0))
                        .text_color(theme.c(Role::TextSecondary))
                        .child(SharedString::new_static("×"))
                        .on_click(click(Command::new("tab.close").arg("side", name))),
                )
            })
            .on_click(click(
                Command::new("tab.goto")
                    .arg("side", name)
                    .arg("n", i64::try_from(i + 1).unwrap_or(1)),
            ));
        row = row.child(tab);
    }
    row.child(
        div()
            .id(SharedString::from(format!("tab-new-{name}")))
            .h(px(theme.m(Metric::HTab)))
            .px(px(12.0))
            .flex()
            .items_center()
            .text_color(theme.c(Role::TextSecondary))
            .hover(|d| d.text_color(theme.c(Role::TextPrimary)))
            .child(SharedString::new_static("+"))
            .on_click(click(Command::new("tab.new").arg("side", name))),
    )
    // Пустое место справа от табов — заголовок окна: перетаскивание, двойной клик — развернуть.
    .child(
        div()
            .id(SharedString::from(format!("drag-{name}")))
            .flex_1()
            .min_w(px(8.0))
            .h_full()
            .window_control_area(WindowControlArea::Drag),
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
        .h(px(theme.m(Metric::HTab)))
        .flex()
        .items_center()
        .justify_center()
        .font_family(ICON_FONT)
        .text_size(px(10.0))
        .text_color(theme.c(Role::TextSecondary))
        .hover(move |d| d.bg(hover).text_color(fg_hover))
        .window_control_area(area)
        .child(SharedString::from(glyph.to_string()))
}

/// Наборы табов (содержимое заголовка, когда меню F9 закрыто): у каждой панели над ней,
/// граница — по сплиттеру (линия 1×16). Скрытая панель — без набора; в терминале
/// (Ctrl+O) табов нет (D-02).
pub(super) fn panel_tabs(
    s: &AppState,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
    widths: [Option<f32>; 2],
) -> Div {
    let row = div().flex_1().min_w_0().h_full().flex().flex_row();
    match widths {
        [Some(l), Some(_)] => row
            .child(tabs(s, Side::Left, theme, click, Some(l - TITLE_PAD_L)))
            .child(
                div()
                    .w(px(1.0))
                    .h(px(16.0))
                    .mt(px(8.0))
                    .flex_none()
                    .self_center()
                    .bg(theme.c(Role::StrokeDivider)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl(px(TITLE_PAD_L))
                    .flex()
                    .child(tabs(s, Side::Right, theme, click, None)),
            ),
        [Some(_), None] => row.child(tabs(s, Side::Left, theme, click, None)),
        [None, Some(_)] => row.child(tabs(s, Side::Right, theme, click, None)),
        [None, None] => row.child(
            div()
                .id("drag-term")
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        ),
    }
}

/// Заголовок окна (D-02): [наборы табов][метка контекста][─ □ ✕]. Кнопки 46×32 у верха,
/// значки `secondary`; метка повышенных прав/удалённого сеанса — рамкой цвета контекста.
pub(super) fn titlebar(
    content: Div,
    theme: &Theme,
    maximized: bool,
    badge: Option<(Hsla, String)>,
) -> Div {
    let hover = theme.c(Role::FillSubtleHover);
    let primary = theme.c(Role::TextPrimary);
    let white: Hsla = rgb(0x00ff_ffff).into(); // colors: allow «Закрыть» Windows: белый на красном
    div()
        .h(px(theme.m(Metric::HTitlebar)))
        .flex_none()
        .flex()
        .flex_row()
        .pl(px(TITLE_PAD_L))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_row()
                .child(content),
        )
        .child(
            div()
                .flex_none()
                .self_start()
                .h(px(theme.m(Metric::HTab)))
                .flex()
                .items_center()
                .children(badge.map(|(color, label)| {
                    div()
                        .mr(px(12.0))
                        .px(px(8.0))
                        .py(px(2.0))
                        .border_1()
                        .border_color(color)
                        .rounded(px(4.0))
                        .text_color(color)
                        .text_size(px(12.0))
                        .child(SharedString::from(label))
                }))
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
                )),
        )
}

/// Командная строка (D-02): `путь>` активной панели и каретка-блок `accent`; фон `panel`,
/// линия сверху, моно 12,5. Ввод команд — S6.
pub(super) fn cmdline(s: &AppState, theme: &Theme) -> Div {
    let path = s.panel(s.active).tab().loc.path.display().to_string();
    div()
        .h(px(theme.m(Metric::HCmdline)))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(theme.m(Metric::PadRowX)))
        .bg(theme.c(Role::LayerPanel))
        .border_t_1()
        .border_color(theme.c(Role::StrokeDivider))
        .font_family("Cascadia Mono")
        .text_size(px(CLI_TEXT))
        .text_color(theme.c(Role::TextPrimary))
        .child(SharedString::from(format!("{path}>")))
        .child(div().w(px(8.0)).h(px(16.0)).bg(theme.c(Role::AccentFill)))
}

/// Статус-строка (D-02, №61): свойства файла · выделение · свободно; фон `header`.
pub(super) fn status(zones: &[String; 3], theme: &Theme) -> Div {
    div()
        .h(px(theme.m(Metric::HStatus)))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(theme.m(Metric::PadRowX)))
        .bg(theme.c(Role::Header))
        .border_t_1()
        .border_color(theme.c(Role::StrokeDivider))
        .text_size(px(SMALL_TEXT))
        .text_color(theme.c(Role::TextSecondary))
        .children(zones.iter().map(|z| {
            div()
                .whitespace_nowrap()
                .child(SharedString::from(z.clone()))
        }))
}

/// Строка клавиш (D-02): «F1 Помощь» — клавиша цветом текста, подпись `secondary`;
/// 10 клавиш — по ширине, меньше — слева с шагом 28. Клик — команда клавиши.
pub(super) fn keybar(
    items: &[Item],
    mods: Mods,
    theme: &Theme,
    click: &impl Fn(Command) -> super::overlays::Handler,
) -> Div {
    let mut shown: Vec<&Item> = items.iter().filter(|i| (1..=10).contains(&i.n)).collect();
    shown.sort_by_key(|i| i.n);
    let full = shown.len() >= 10;
    let mut bar = div()
        .h(px(theme.m(Metric::HKeybar)))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(KEYBAR_GAP))
        .px(px(theme.m(Metric::PadRowX)))
        .when(full, Styled::justify_between)
        .text_size(px(SMALL_TEXT))
        .text_color(theme.c(Role::TextSecondary));
    for it in shown {
        let n = it.n;
        bar = bar.child(
            div()
                .id(SharedString::from(format!("fkey-{n}-{mods:?}")))
                .flex()
                .gap(px(4.0))
                .whitespace_nowrap()
                .child(
                    div()
                        .text_color(theme.c(Role::TextPrimary))
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child(SharedString::from(format!("F{n}"))),
                )
                .child(
                    div()
                        .when(!it.enabled, |d| d.text_color(theme.c(Role::TextDisabled)))
                        .child(SharedString::from(it.label.clone())),
                )
                .when(it.enabled, |d| {
                    d.hover(|d| d.text_color(theme.c(Role::TextPrimary)))
                        .on_click(click(it.cmd.clone()))
                }),
        );
    }
    bar
}

/// Текст командной строки и мелкий текст низа окна (D-02), px.
const CLI_TEXT: f32 = 12.5;
const SMALL_TEXT: f32 = 12.0;
/// Шаг клавиш, когда их меньше десяти.
const KEYBAR_GAP: f32 = 28.0;

/// Низ окна (cmdline + F-клавиши) — кэшируемая view (G-07). Заголовок и табы не кэшируются:
/// gpui не повторяет `WindowControlArea` из кэша (перетаскивание, «─ □ ✕», Snap).
pub(super) struct ChromeView {
    pub host: WeakEntity<CoreHost>,
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
        let zones = crate::status::zones(s, &h.theme.fmt);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(cmdline(s, &h.theme))
            .child(status(&zones, &h.theme))
            .child(keybar(h.keybar.items(h.mods), h.mods, &h.theme, &click))
    }
}

/// Отпечаток низа: путь активной панели, статус-строка, подписи F-клавиш,
/// модификаторы, тема.
pub(super) fn bottom_print(
    s: &AppState,
    zones: &[String; 3],
    items: &[Item],
    mods: Mods,
    look: u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    (look, urfin_i18n::epoch(), format!("{mods:?}"), zones).hash(&mut h);
    s.panel(s.active).tab().loc.path.hash(&mut h);
    for it in items {
        (it.n, &it.label, it.enabled).hash(&mut h);
    }
    h.finish()
}

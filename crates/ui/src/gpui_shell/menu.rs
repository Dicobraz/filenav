//! Меню F9 на GPUI (S2 §2.6, G-05): строка меню в заголовке вместо табов и выпадающий
//! список под заголовком открытого меню. Пункты — `menu.toml` (вшитый или из каталога
//! конфига), подписи — Fluent, подсказки клавиш — [`KeyHints`]. Клавиши списка (↑↓,
//! Enter, Esc, ←/→, буква пункта) разбирает [`Menu::key`].

use gpui_kit::prelude::*;
use gpui_kit::{Div, Keystroke, SharedString, div, px};
use urfin_core::{Command, CommandRegistry};

use super::overlays::{Click, ClickAll};
use super::theme::Theme;
use crate::keybar::KeyHints;
use crate::menu::{Item, MenuDef, label, letter};
use crate::tokens::{Metric, Role};

const W_FLYOUT: f32 = 320.0;
const W_TITLE: f32 = 110.0;

/// Меню и выбор в открытом списке.
pub(super) struct Menu {
    pub menus: Vec<MenuDef>,
    /// (id открытого меню, выбранный пункт).
    sel: Option<(String, usize)>,
}

impl Menu {
    pub(super) fn new(registry: &CommandRegistry, user: Option<&str>) -> Menu {
        let parsed = user
            .and_then(|t| crate::menu::parse(t, registry).ok())
            .or_else(|| crate::menu::parse(crate::menu::MENU_TOML, registry).ok());
        Menu {
            menus: parsed.map(|(m, _)| m).unwrap_or_default(),
            sel: None,
        }
    }

    fn def(&self, open: &str) -> Option<&MenuDef> {
        self.menus.iter().find(|m| m.id == open)
    }

    /// Выбранный пункт открытого меню (новое меню — первый пункт с командой).
    fn selected(&mut self, open: &str) -> usize {
        if self.sel.as_ref().is_none_or(|(id, _)| id != open) {
            let first = self
                .def(open)
                .and_then(|d| d.items.iter().position(|i| matches!(i, Item::Cmd { .. })))
                .unwrap_or(0);
            self.sel = Some((open.to_owned(), first));
        }
        self.sel.as_ref().map_or(0, |(_, i)| *i)
    }

    /// Команды пункта: у меню панели — сначала `panel.focus`, в конце — закрыть меню.
    fn run(def: &MenuDef, i: usize) -> Vec<Command> {
        let Some(Item::Cmd { cmd, .. }) = def.items.get(i) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(3);
        out.push(Command::new("ui.cancel"));
        if let Some(side) = def.side {
            out.push(Command::new("panel.focus").arg("side", super::chrome::side_name(side)));
        }
        out.push((**cmd).clone());
        out
    }

    /// Клавиша при открытом меню: команды для Core (пусто — только смена выбора).
    pub(super) fn key(&mut self, open: &str, ks: &Keystroke) -> Vec<Command> {
        let sel = self.selected(open);
        let Some(def) = self.def(open).cloned() else {
            return vec![Command::new("ui.cancel")];
        };
        let n = def.items.len();
        let step = |from: usize, dir: isize| -> usize {
            let mut i = from;
            for _ in 0..n {
                i = usize::try_from(
                    (isize::try_from(i).unwrap_or(0) + dir)
                        .rem_euclid(isize::try_from(n.max(1)).unwrap_or(1)),
                )
                .unwrap_or(0);
                if matches!(def.items[i], Item::Cmd { .. }) {
                    return i;
                }
            }
            from
        };
        let mut next = sel;
        match ks.key.as_str() {
            "escape" | "f9" => return vec![Command::new("ui.cancel")],
            "enter" => return Self::run(&def, sel),
            "up" => next = step(sel, -1),
            "down" => next = step(sel, 1),
            "home" => next = step(n.saturating_sub(1), 1),
            "end" => next = step(0, -1),
            "left" | "right" => {
                let dir = if ks.key == "left" { -1 } else { 1 };
                if let Some(id) = crate::menu::neighbor(&self.menus, open, dir) {
                    return vec![Command::new("ui.menu").arg("menu", id.as_str())];
                }
            }
            k => {
                let i18n = urfin_i18n::current();
                let typed = ks.key_char.as_deref().unwrap_or(k).to_lowercase();
                let hit = def.items.iter().position(|it| match it {
                    Item::Cmd { label: l, .. } => letter(&i18n, l)
                        .is_some_and(|c| c.to_string() == typed || c.to_string() == k),
                    Item::Sep => false,
                });
                if let Some(i) = hit {
                    return Self::run(&def, i);
                }
            }
        }
        self.sel = Some((open.to_owned(), next));
        Vec::new()
    }

    /// Строка меню в заголовке (вместо табов).
    pub(super) fn bar(&self, open: &str, theme: &Theme, click: Click<'_>) -> Div {
        let i18n = urfin_i18n::current();
        let mut row = div().flex_1().h_full().flex().items_center().gap_1();
        for m in &self.menus {
            let on = m.id == open;
            row = row.child(
                div()
                    .id(SharedString::from(format!("menu-{}", m.id)))
                    .w(px(W_TITLE))
                    .h(px(theme.m(Metric::HTab)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme.m(Metric::RadiusControl)))
                    .when(on, |d| d.bg(theme.c(Role::FillSubtlePressed)))
                    .hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
                    .child(SharedString::from(label(&i18n, &m.title)))
                    .on_click(click(Command::new("ui.menu").arg("menu", m.id.as_str()))),
            );
        }
        row
    }

    /// Выпадающий список открытого меню под его заголовком.
    pub(super) fn flyout(
        &mut self,
        open: &str,
        hints: &dyn KeyHints,
        theme: &Theme,
        click: ClickAll<'_>,
    ) -> Option<Div> {
        let sel = self.selected(open);
        let idx = self.menus.iter().position(|m| m.id == open)?;
        let def = self.menus.get(idx)?.clone();
        let i18n = urfin_i18n::current();
        #[allow(clippy::cast_precision_loss, reason = "номер меню")]
        let left = theme.m(Metric::GapPanels) + idx as f32 * (W_TITLE + 4.0);
        let mut list = div()
            .absolute()
            .top(px(theme.m(Metric::HTitlebar)))
            .left(px(left))
            .w(px(W_FLYOUT))
            .p_1()
            .flex()
            .flex_col()
            .rounded(px(theme.m(Metric::RadiusOverlay)))
            .border_1()
            .border_color(theme.c(Role::StrokeCard))
            .bg(theme.c(Role::LayerOverlay))
            .shadow_lg();
        for (i, it) in def.items.iter().enumerate() {
            list = match it {
                Item::Sep => list.child(div().my_1().h(px(1.0)).bg(theme.c(Role::StrokeDivider))),
                Item::Cmd { cmd, label: l } => {
                    let hint = hints.hint(cmd).map(|h| h.to_string()).unwrap_or_default();
                    let cmds = Self::run(&def, i);
                    let mut item = div()
                        .id(SharedString::from(format!("mi-{open}-{i}")))
                        .h(px(theme.m(Metric::HMenuItem)))
                        .flex()
                        .items_center()
                        .px_3()
                        .rounded(px(theme.m(Metric::RadiusControl)))
                        .hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
                        .when(i == sel, |d| d.bg(theme.c(Role::FillSubtlePressed)))
                        .child(div().flex_1().child(SharedString::from(label(&i18n, l))))
                        .child(
                            div()
                                .text_size(px(theme.m(Metric::TypeCaption)))
                                .text_color(theme.c(Role::TextSecondary))
                                .child(SharedString::from(hint)),
                        );
                    if !cmds.is_empty() {
                        item = item.on_click(click(cmds));
                    }
                    list.child(item)
                }
            };
        }
        Some(list)
    }
}

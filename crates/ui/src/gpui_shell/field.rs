//! Поля ввода окна GPUI (G-05): черновик команды (`ui.draft`: назначение копирования
//! и переноса, имя каталога, S2-15) и переименование на месте (`ui.rename`, S2-16).
//! Поле — `InputState` gpui-kit (IME, выделение, буфер обмена); Enter и Esc разбирает
//! хост, остальные клавиши — поле.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Div, Entity, SharedString, Window, div, px};
use urfin_core::{AppState, Command, Row, Tab};
use urfin_i18n::FluentArgs;

use super::theme::Theme;
use super::{CoreHost, overlays};
use crate::UiState;
use crate::tokens::{Metric, Role};

/// Что сейчас редактируется.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Key {
    /// Черновик команды (`ops.copy`, `ops.mkdir`…).
    Draft(String),
    /// Переименование в табе (id таба, старое имя).
    Rename(u32, String),
    /// Правка пути панели (`ui.path_edit`, ADR-0014).
    Path(urfin_core::Side),
}

pub(super) struct Field {
    pub key: Key,
    pub input: Entity<InputState>,
}

/// Черновик с полем ввода (у удаления поля нет — кнопки).
fn has_field(cmd: &str) -> bool {
    cmd != "ops.delete"
}

/// Путь таба для поля: локальный — путь ОС, иначе uri.
fn path_text(t: &Tab) -> String {
    if t.loc.is_local() {
        t.loc.path.display().to_string()
    } else {
        t.loc.to_uri()
    }
}

/// Нужное поле и его начальный текст по состоянию UI.
pub(super) fn wanted(s: &AppState) -> Option<(Key, String)> {
    let st = s.ext.get::<UiState>()?;
    if let Some(d) = &st.draft {
        if !has_field(&d.cmd) {
            return None;
        }
        let init = if d.text.is_empty() && matches!(d.cmd.as_str(), "ops.copy" | "ops.move") {
            path_text(s.panel(s.active.other()).tab())
        } else {
            d.text.to_string()
        };
        return Some((Key::Draft(d.cmd.to_string()), init));
    }
    if let Some(side) = st.path_edit {
        return Some((Key::Path(side), path_text(s.panel(side).tab())));
    }
    let (tab, name) = st.rename.as_ref()?;
    Some((Key::Rename(tab.0, name.to_string()), name.to_string()))
}

impl CoreHost {
    /// Поле — под текущее состояние: новое получает фокус и выделенный текст; без поля
    /// фокус возвращается окну.
    pub(super) fn sync_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.core.as_ref().and_then(|c| wanted(c.state()));
        match (&self.field, want) {
            (Some(f), Some((k, _))) if f.key == k => {}
            (_, Some((key, init))) => {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(init));
                input.update(cx, |s, cx| {
                    s.focus(window, cx);
                    s.select_all(window, cx);
                });
                self.field = Some(Field { key, input });
            }
            (Some(_), None) => {
                self.field = None;
                window.focus(&self.focus, cx);
            }
            (None, None) => {}
        }
    }

    /// Enter в поле или в черновике удаления: команды и закрытие.
    pub(super) fn field_commit(&mut self, shift: bool, cx: &mut Context<Self>) -> Vec<Command> {
        let st = self
            .core
            .as_ref()
            .and_then(|c| c.state().ext.get::<UiState>());
        let text = self
            .field
            .as_ref()
            .map(|f| f.input.read(cx).value().to_string())
            .unwrap_or_default();
        let mut out = Vec::with_capacity(2);
        if let Some(d) = st.and_then(|u| u.draft.as_ref()) {
            if d.cmd == "ops.delete" {
                let mut c = Command::new("ops.delete").arg("confirmed", true);
                if shift || self.ops_permanent {
                    c = c.arg("permanent", true);
                }
                out.push(c);
            } else if let Ok(c) = Command::parse(&d.cmd) {
                out.push(c.arg(&d.arg, text.as_str()));
            }
        } else if let Some(Key::Path(side)) = self.field.as_ref().map(|f| &f.key) {
            // Проверку и переход делает handler; он же закрывает поле.
            return vec![
                Command::new("ui.path_edit")
                    .arg("side", super::chrome::side_name(*side))
                    .arg("path", text.trim()),
            ];
        } else if let Some(Key::Rename(_, old)) = self.field.as_ref().map(|f| &f.key) {
            let to = text.trim();
            if !to.is_empty() && to != old {
                out.push(Command::new("ops.rename").arg("to", to));
            }
        }
        out.push(Command::new("ui.cancel"));
        out
    }
}

/// Поле переименования для строки курсора таба `t` (если оно открыто для него).
pub(super) fn rename_input(field: Option<&Field>, t: &Tab) -> Option<Input> {
    let f = field?;
    match &f.key {
        Key::Rename(tab, _) if *tab == t.id.0 => Some(Input::new(&f.input)),
        _ => None,
    }
}

/// Сколько и какого объёма объектов затронет команда: выделение или строка курсора.
fn targets(t: &Tab) -> (u64, u64) {
    if !t.selection.is_empty() {
        return (u64::from(t.selection.len()), t.selection.bytes());
    }
    match t.current() {
        Some(Row::Entry { entry, .. }) => (1, entry.size),
        _ => (0, 0),
    }
}

/// Строка черновика над cmdline: текст, поле (если есть), кнопки.
#[allow(clippy::too_many_lines, reason = "строка черновика слева направо")]
pub(super) fn draft_bar(
    s: &AppState,
    field: Option<&Field>,
    permanent: bool,
    theme: &Theme,
    click: overlays::ClickAll<'_>,
) -> Option<Div> {
    let d = s.ext.get::<UiState>()?.draft.as_ref()?;
    let i18n = urfin_i18n::current();
    let (count, bytes) = targets(s.active_tab());
    let mut a = FluentArgs::new();
    a.set("count", count);
    a.set("size", urfin_i18n::fmt::bytes_with(&i18n, bytes));
    let fallback = || {
        let key = format!("ui-draft-{}", d.cmd.replace('.', "-"));
        if i18n.has(&key) {
            i18n.get(&key)
        } else {
            i18n.get(&format!("cmd-{}", d.cmd.replace('.', "-")))
        }
    };
    let delete = d.cmd == "ops.delete";
    let (text, tone) = match d.cmd.as_str() {
        "ops.delete" if permanent => (i18n.fmt("ui-draft-delete-permanent", &a), Role::CriticalFg),
        "ops.delete" => (i18n.fmt("ui-draft-delete", &a), Role::CautionFg),
        "ops.copy" => (i18n.fmt("ui-draft-copy", &a), Role::AccentFill),
        "ops.move" => (i18n.fmt("ui-draft-move", &a), Role::AccentFill),
        _ => (fallback(), Role::AccentFill),
    };
    let button = |label: String, chip: &str, accent: bool, cmds: Vec<Command>| {
        div()
            .id(SharedString::from(format!("draft-{chip}")))
            .h(px(theme.m(Metric::HControl) + 4.0))
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(theme.m(Metric::RadiusControl)))
            .border_1()
            .border_color(theme.c(Role::StrokeControl))
            .bg(theme.c(if accent {
                Role::AccentFill
            } else {
                Role::FillControl
            }))
            .text_color(theme.c(if accent {
                Role::AccentOn
            } else {
                Role::TextPrimary
            }))
            .child(SharedString::from(label))
            .child(
                div()
                    .text_size(px(theme.m(Metric::TypeCaption)))
                    .opacity(0.7)
                    .child(SharedString::from(chip.to_owned())),
            )
            .on_click(click(cmds))
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap_3()
        .child(div().flex_none().child(SharedString::from(text)));
    if let Some(f) = field.filter(|f| matches!(f.key, Key::Draft(_))) {
        row = row.child(div().flex_1().min_w_0().child(Input::new(&f.input)));
        if d.cmd == "ops.mkdir" {
            row = row.child(
                div()
                    .text_size(px(theme.m(Metric::TypeCaption)))
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(i18n.get("ui-draft-mkdir-hint"))),
            );
        }
    } else {
        row = row.child(div().flex_1());
    }
    if delete {
        let confirm = |perm: bool| {
            let mut c = Command::new("ops.delete").arg("confirmed", true);
            if perm {
                c = c.arg("permanent", true);
            }
            vec![c, Command::new("ui.cancel")]
        };
        row = row.child(button(
            i18n.get("ui-button-delete"),
            "Enter",
            true,
            confirm(permanent),
        ));
        if !permanent {
            row = row.child(button(
                i18n.get("ui-button-permanent"),
                "⇧Enter",
                false,
                confirm(true),
            ));
        }
    }
    row = row.child(button(
        i18n.get("ui-button-cancel"),
        "Esc",
        false,
        vec![Command::new("ui.cancel")],
    ));
    Some(
        div()
            .flex_none()
            .mx(px(theme.m(Metric::GapPanels)))
            .mt(px(theme.m(Metric::GapPanels)))
            .p_3()
            .rounded(px(theme.m(Metric::RadiusOverlay)))
            .border_1()
            .border_color(theme.c(tone))
            .bg(theme.c(Role::LayerPanel))
            .child(row),
    )
}

/// Поле правки пути панели `side` (если оно открыто).
pub(super) fn path_input(field: Option<&Field>, side: urfin_core::Side) -> Option<Input> {
    let f = field?;
    match &f.key {
        Key::Path(s) if *s == side => Some(Input::new(&f.input)),
        _ => None,
    }
}

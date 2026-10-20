//! Меню F9 (S2 §2.6, S2-14): меню-бар в заголовке вместо табов и выпадающий список
//! (Fluent `MenuFlyout`). Немодальное: панели и job-бар живут, фокус —
//! `Focus::Overlay(Menu)`; клик мимо закрывает меню и не уходит в панель.
//!
//! Пункты — из вшитого `menu.toml` (переопределяется в каталоге конфига); подпись —
//! ключ Fluent, буква — его атрибут `.menu-key`, хоткей — [`KeyHints`].
//! Клавиатура: ← → — меню, ↑ ↓ — пункт, буква — выбор, Enter — выполнить; Esc
//! закрывает в app (`ui.cancel`).

use compact_str::CompactString;
use egui::{Align2, Pos2, Rect, Sense, pos2, vec2};
use serde::Deserialize;
use urfin_core::{Command, CommandRegistry, Side};
use urfin_i18n::I18n;

use crate::legacy::canvas::Canvas;
use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::keybar::{self, KeyHints};
use crate::legacy::panel::side_arg;
use crate::legacy::text::Tx;
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Вшитое меню.
pub const MENU_TOML: &str = include_str!("menu.toml");

/// Высота пункта и ширина выпадающего списка (S2 §2.6).
const ITEM_H: f32 = 28.0;
const FLYOUT_W: f32 = 320.0;
const SEP_H: f32 = 9.0;

/// Меню (заголовок меню-бара и его пункты).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuDef {
    pub id: CompactString,
    pub title: CompactString,
    /// Меню панели: перед командой — `panel.focus`.
    pub side: Option<Side>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Cmd {
        cmd: Box<Command>,
        label: CompactString,
    },
    Sep,
}

#[derive(Deserialize)]
struct File {
    menu: Vec<RawMenu>,
}

#[derive(Deserialize)]
struct RawMenu {
    id: String,
    title: String,
    side: Option<String>,
    #[serde(default)]
    items: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    cmd: Option<String>,
    label: Option<String>,
    #[serde(default)]
    sep: bool,
}

/// Разбор `menu.toml`. Команда пункта проверяется по реестру (docs/COMMANDS.md): id
/// известен, аргументы описаны и нужного типа, обязательные есть. Иначе пункт скрыт,
/// предупреждение — во втором значении (S2-14).
pub fn parse(
    text: &str,
    registry: &CommandRegistry,
) -> Result<(Vec<MenuDef>, Vec<String>), String> {
    let file: File = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    let menus = file
        .menu
        .into_iter()
        .map(|m| {
            let side = match m.side.as_deref() {
                Some("left") => Some(Side::Left),
                Some("right") => Some(Side::Right),
                _ => None,
            };
            let mut items: Vec<Item> = Vec::new();
            for raw in m.items {
                if raw.sep {
                    // Подряд и по краям разделители не нужны.
                    if items.last().is_some_and(|i| *i != Item::Sep) {
                        items.push(Item::Sep);
                    }
                    continue;
                }
                let Some(text) = raw.cmd else {
                    warnings.push(format!("menu {}: item without cmd", m.id));
                    continue;
                };
                let cmd = match Command::parse(&text) {
                    Ok(c) => c,
                    Err(e) => {
                        warnings.push(format!("menu {}: {text}: {e}", m.id));
                        continue;
                    }
                };
                if let Err(e) = registry.validate(&cmd) {
                    warnings.push(format!("menu {}: {text}: {e}", m.id));
                    continue;
                }
                let label = raw
                    .label
                    .unwrap_or_else(|| format!("cmd-{}", keybar::key_part(cmd.id.as_str())));
                items.push(Item::Cmd {
                    cmd: Box::new(cmd),
                    label: label.into(),
                });
            }
            while items.last() == Some(&Item::Sep) {
                items.pop();
            }
            MenuDef {
                id: m.id.into(),
                title: m.title.into(),
                side,
                items,
            }
        })
        .collect();
    Ok((menus, warnings))
}

/// Копия реестра (описания команд статические): UI держит её для проверки
/// `menu.toml`, загруженного после старта.
pub fn copy_registry(r: &CommandRegistry) -> CommandRegistry {
    let mut copy = CommandRegistry::new();
    for m in r.iter() {
        // Id уже проверены при первой регистрации.
        let _ = copy.register(std::slice::from_ref(m));
    }
    copy
}

/// Меню F9 открыто: id меню (`ui.menu menu=`), не меню breadcrumb.
pub fn open_id(st: &crate::legacy::state::UiState) -> Option<&str> {
    st.menu
        .as_deref()
        .filter(|m| st.menu_side.is_none() && *m != "subdirs")
}

/// Состояние меню между кадрами.
#[derive(Debug, Default)]
pub(crate) struct MenuMemo {
    /// Меню, к которому относится `sel`.
    open: Option<CompactString>,
    /// Текущий пункт (индекс в `items`).
    pub sel: Option<usize>,
    /// Левые края заголовков меню-бара (куда прижать список).
    pub anchors: Vec<(CompactString, Pos2)>,
}

/// Подпись пункта.
pub(crate) fn label(i18n: &I18n, key: &str) -> String {
    i18n.get(key)
}

/// Буква выбора: атрибут `.menu-key`, если эта буква есть в подписи (атрибут из
/// запасного языка — латиница у русской подписи — не годится), иначе первая буква.
pub(crate) fn letter(i18n: &I18n, key: &str) -> Option<char> {
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let text: String = label(i18n, key).chars().map(lower).collect();
    let attr = i18n
        .has(key)
        .then(|| i18n.attr(key, "menu-key"))
        .and_then(|a| a.chars().next().map(lower))
        .filter(|c| text.contains(*c));
    attr.or_else(|| text.chars().find(|c| c.is_alphanumeric()))
}

/// Одинаковые буквы выбора внутри меню (текущий язык): предупреждения. Такие пункты
/// выбираются по кругу, но это неудобно — буквы стоит развести (`.menu-key`).
pub fn letter_conflicts(menus: &[MenuDef], i18n: &I18n) -> Vec<String> {
    let mut out = Vec::new();
    for m in menus {
        let mut seen: Vec<(char, Vec<String>)> = Vec::new();
        for item in &m.items {
            let Item::Cmd { label: key, .. } = item else {
                continue;
            };
            let Some(ch) = letter(i18n, key) else {
                continue;
            };
            let name = label(i18n, key);
            match seen.iter_mut().find(|(c, _)| *c == ch) {
                Some((_, names)) => names.push(name),
                None => seen.push((ch, vec![name])),
            }
        }
        for (ch, names) in seen.into_iter().filter(|(_, n)| n.len() > 1) {
            out.push(format!(
                "menu {}: letter '{ch}' is shared by {}",
                m.id,
                names.join(", ")
            ));
        }
    }
    out
}

/// Подчёркивание буквы выбора (как мнемоника Windows): `x` начала и конца буквы от
/// левого края подписи и `y` линии от центра строки. Первое вхождение буквы.
fn underline(
    ctx: &egui::Context,
    fonts: &FontSet,
    size: f32,
    text: &str,
    letter: Option<char>,
) -> Option<(f32, f32, f32)> {
    let ch = letter?;
    let lower = |c: char| c.to_lowercase().next().unwrap_or(c);
    let idx = text.chars().position(|c| lower(c) == ch)?;
    let font = fonts.id(Font::Ui, size);
    let g = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE));
    let at = |i: usize| g.pos_from_cursor(egui::text::CCursor::new(i));
    let (a, b) = (at(idx), at(idx + 1));
    Some((
        a.left(),
        b.left().max(a.left() + 2.0),
        g.size().y / 2.0 - 1.0,
    ))
}

/// Команды пункта: `ui.cancel`, для меню панели — `panel.focus`, сама команда.
fn execute(def: &MenuDef, cmd: &Command, out: &mut Vec<Command>) {
    out.push(Command::new("ui.cancel"));
    if let Some(side) = def.side {
        out.push(Command::new("panel.focus").arg("side", side_arg(side)));
    }
    out.push(cmd.clone());
}

fn selectable(def: &MenuDef, i: usize) -> bool {
    matches!(def.items.get(i), Some(Item::Cmd { .. }))
}

/// Следующий пункт в направлении `dir` (по кругу, разделители пропускаются).
fn step(def: &MenuDef, from: Option<usize>, dir: isize) -> Option<usize> {
    let n = def.items.len();
    if n == 0 {
        return None;
    }
    let n_i = isize::try_from(n).ok()?;
    let mut i = from.map_or(if dir > 0 { -1 } else { n_i }, |f| {
        isize::try_from(f).unwrap_or(0)
    });
    for _ in 0..n {
        i = (i + dir).rem_euclid(n_i);
        let u = usize::try_from(i).ok()?;
        if selectable(def, u) {
            return Some(u);
        }
    }
    None
}

/// Меню-бар в полосе заголовка `area`: заголовки меню, текущее — подсветка и пилюля.
#[allow(
    clippy::too_many_arguments,
    reason = "полоса заголовка и состояние меню"
)]
pub(crate) fn bar(
    c: &mut dyn Canvas,
    ui: &mut egui::Ui,
    i18n: &I18n,
    tokens: &Tokens,
    fonts: &FontSet,
    menus: &[MenuDef],
    open: &str,
    memo: &mut MenuMemo,
    area: Rect,
    out: &mut Vec<Command>,
    interactive: &mut Vec<Rect>,
) {
    let size = tokens.m(Metric::TypeBody);
    let pad = tokens.m(Metric::SpaceM);
    let radius = tokens.radius(Metric::RadiusControl);
    let mut x = area.left() + pad;
    memo.anchors.clear();
    for m in menus {
        let title = label(i18n, &m.title);
        let font = fonts.id(Font::Ui, size);
        let w = ui
            .ctx()
            .fonts_mut(|f| f.layout_no_wrap(title.clone(), font, egui::Color32::WHITE))
            .size()
            .x
            + 2.0 * pad;
        let rect = Rect::from_min_max(pos2(x, area.top() + 4.0), pos2(x + w, area.bottom() - 4.0));
        let resp = ui.interact(
            rect,
            ui.scope_id().with(("menu-title", m.id.as_str())),
            Sense::click(),
        );
        interactive.push(rect);
        let current = m.id == open;
        if current || resp.hovered() {
            c.rect(rect, Role::FillSubtleHover, radius);
        }
        if current {
            // Пилюля снизу — признак текущего меню (S2 §2.6).
            let pill =
                Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 2.0), vec2(16.0, 3.0));
            c.rect(pill, Role::AccentFill, egui::CornerRadius::same(2));
        }
        c.text(
            rect.center(),
            Align2::CENTER_CENTER,
            &Tx::user(title.as_str()),
            Font::Ui,
            size,
            Role::TextPrimary,
        );
        // Клик или наведение при открытом меню — переключить.
        if !current && (resp.clicked() || resp.hovered()) {
            out.push(Command::new("ui.menu").arg("menu", m.id.as_str()));
        }
        memo.anchors.push((m.id.clone(), rect.left_bottom()));
        x += w + 2.0;
    }
}

/// Выпадающий список меню `def` от точки `at`: возвращает его прямоугольник.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "список, подсказки и ввод — пункты по порядку"
)]
pub(crate) fn flyout(
    c: &mut dyn Canvas,
    ui: &mut egui::Ui,
    i18n: &I18n,
    tokens: &Tokens,
    fonts: &FontSet,
    hints: &dyn KeyHints,
    def: &MenuDef,
    memo: &mut MenuMemo,
    at: Pos2,
    out: &mut Vec<Command>,
) -> Rect {
    if memo.open.as_deref() != Some(def.id.as_str()) {
        memo.open = Some(def.id.clone());
        memo.sel = step(def, None, 1);
    }
    let size = tokens.m(Metric::TypeBody);
    let caption = tokens.m(Metric::TypeCaption);
    let pad = tokens.m(Metric::SpaceM);
    #[allow(clippy::cast_precision_loss, reason = "пунктов — десятки")]
    let h: f32 = def
        .items
        .iter()
        .map(|i| if *i == Item::Sep { SEP_H } else { ITEM_H })
        .sum::<f32>()
        + 8.0;
    let panel = Rect::from_min_size(at, vec2(FLYOUT_W, h));
    c.rect(
        panel.translate(vec2(0.0, 2.0)),
        Role::ShadowOverlay,
        tokens.radius(Metric::RadiusOverlay),
    );
    c.rect(
        panel,
        Role::LayerOverlay,
        tokens.radius(Metric::RadiusOverlay),
    );
    c.stroke(
        panel,
        Role::StrokeDivider,
        tokens.radius(Metric::RadiusOverlay),
        1.0,
    );
    keys(ui, i18n, def, memo, out);
    let mut y = panel.top() + 4.0;
    for (i, item) in def.items.iter().enumerate() {
        match item {
            Item::Sep => {
                c.rect(
                    Rect::from_min_size(
                        pos2(panel.left() + 8.0, y + SEP_H / 2.0),
                        vec2(FLYOUT_W - 16.0, 1.0),
                    ),
                    Role::StrokeDivider,
                    egui::CornerRadius::ZERO,
                );
                y += SEP_H;
            }
            Item::Cmd { cmd, label: key } => {
                let row =
                    Rect::from_min_size(pos2(panel.left() + 4.0, y), vec2(FLYOUT_W - 8.0, ITEM_H));
                let resp = ui.interact(
                    row,
                    ui.scope_id().with(("menu-item", def.id.as_str(), i)),
                    Sense::click(),
                );
                if resp.hovered() {
                    memo.sel = Some(i);
                }
                if memo.sel == Some(i) {
                    c.rect(
                        row,
                        Role::FillSubtleHover,
                        tokens.radius(Metric::RadiusControl),
                    );
                    let pill = Rect::from_min_size(
                        pos2(row.left(), row.center().y - 8.0),
                        vec2(3.0, 16.0),
                    );
                    c.rect(pill, Role::AccentFill, egui::CornerRadius::same(2));
                }
                let hint = hints.hint(cmd);
                let hint_w = hint.as_ref().map_or(0.0, |h| {
                    c.text(
                        pos2(row.right() - pad, row.center().y),
                        Align2::RIGHT_CENTER,
                        &Tx::user(h.as_str()),
                        Font::Caption,
                        caption,
                        Role::TextSecondary,
                    )
                    .width()
                });
                let text = label(i18n, key);
                let left = row.left() + pad + 16.0;
                c.text_clipped(
                    pos2(left, row.center().y),
                    row.width() - 2.0 * pad - 16.0 - hint_w - 8.0,
                    &Tx::user(text.as_str()),
                    Font::Ui,
                    size,
                    Role::TextPrimary,
                );
                if let Some(u) = underline(ui.ctx(), fonts, size, &text, letter(i18n, key)) {
                    let base = row.center().y + u.2;
                    c.rect(
                        Rect::from_min_max(pos2(left + u.0, base), pos2(left + u.1, base + 1.0)),
                        Role::TextPrimary,
                        egui::CornerRadius::ZERO,
                    );
                }
                if resp.clicked() {
                    execute(def, cmd, out);
                }
                y += ITEM_H;
            }
        }
    }
    panel
}

/// Клавиатура меню: ↑ ↓, буква, Enter; ← → — соседнее меню (команда, текущий пункт
/// сбрасывается в первом кадре нового меню).
fn keys(ui: &egui::Ui, i18n: &I18n, def: &MenuDef, memo: &mut MenuMemo, out: &mut Vec<Command>) {
    let (up, down, enter, typed) = ui.input(|i| {
        let typed: Option<char> = i.events.iter().find_map(|e| match e {
            egui::Event::Text(t) => t.chars().next(),
            _ => None,
        });
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            typed,
        )
    });
    if up {
        memo.sel = step(def, memo.sel, -1);
    }
    if down {
        memo.sel = step(def, memo.sel, 1);
    }
    if enter && let Some(Item::Cmd { cmd, .. }) = memo.sel.and_then(|i| def.items.get(i)) {
        execute(def, cmd, out);
        return;
    }
    if let Some(ch) = typed.map(|c| c.to_lowercase().next().unwrap_or(c)) {
        let hits: Vec<usize> = def
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::Cmd { label, .. } if letter(i18n, label) == Some(ch)))
            .map(|(i, _)| i)
            .collect();
        match hits.as_slice() {
            [] => {}
            // Одна буква — сразу выполнить.
            [i] => {
                if let Some(Item::Cmd { cmd, .. }) = def.items.get(*i) {
                    execute(def, cmd, out);
                }
            }
            // Буква повторяется — по кругу между её пунктами, Enter выполняет (как в Windows).
            many => {
                let next = many
                    .iter()
                    .find(|&&i| memo.sel.is_some_and(|s| i > s))
                    .unwrap_or(&many[0]);
                memo.sel = Some(*next);
            }
        }
    }
}

/// Соседнее меню для ← → (по кругу).
pub(crate) fn neighbor(menus: &[MenuDef], open: &str, dir: isize) -> Option<CompactString> {
    let i = menus.iter().position(|m| m.id == open)?;
    let n = isize::try_from(menus.len()).ok()?;
    let j = usize::try_from((isize::try_from(i).ok()? + dir).rem_euclid(n)).ok()?;
    Some(menus[j].id.clone())
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;

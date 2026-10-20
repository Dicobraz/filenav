//! Строка F-клавиш (S2 §2.12, S2-14) и подсказки клавиш [`KeyHints`].
//!
//! Привязки — заглушка пресета Far из S3 §3 ([`FarStub`]), пока keymap S3 (S3-04,
//! `Keymap::keybar`) не реализует `KeyHints`. Зажатые Shift / Ctrl / Alt меняют
//! подписи в том же кадре. Подпись — ключ слота (`keybar-<id>[-<вариант>]`), запасная —
//! `cmd-<id>` (S0 v3 №14). Слот с незарегистрированной командой — серый, без клика.

use compact_str::CompactString;
use urfin_core::{Command, CommandRegistry};
use urfin_i18n::I18n;

use crate::legacy::canvas::Canvas;
use crate::legacy::fonts::Font;
use crate::legacy::text::Tx;
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Слотов в строке (S2 §2.12: F1…F10).
pub const SLOTS: u8 = 10;

/// Набор модификаторов для строки F-клавиш.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Mods {
    #[default]
    None,
    Shift,
    Ctrl,
    Alt,
}

impl Mods {
    /// Один зажатый модификатор — его строка; иначе — без модификаторов.
    pub fn from_egui(m: egui::Modifiers) -> Self {
        match (m.shift, m.ctrl || m.command, m.alt) {
            (true, false, false) => Mods::Shift,
            (false, true, false) => Mods::Ctrl,
            (false, false, true) => Mods::Alt,
            _ => Mods::None,
        }
    }

    fn idx(self) -> usize {
        self as usize
    }

    fn prefix(self) -> &'static str {
        match self {
            Mods::None => "",
            Mods::Shift => "Shift+",
            Mods::Ctrl => "Ctrl+",
            Mods::Alt => "Alt+",
        }
    }
}

/// F-клавиша: команда (с аргументами) и ключ подписи.
#[derive(Clone, Debug, PartialEq)]
pub struct FKey {
    pub cmd: Command,
    pub label_key: CompactString,
}

/// Подсказки клавиш для keybar и меню (реализует keymap S3, до него — [`FarStub`]).
///
/// Отличие от S2 §9: слот — [`FKey`] с командой и аргументами, а не только id: в Far
/// Ctrl+F3…F7 — один `panel.sort` с разными `key`, и подписи у них разные.
pub trait KeyHints: std::fmt::Debug {
    /// F1…F12 для набора модификаторов.
    fn fkeys(&self, mods: Mods) -> [Option<FKey>; 12];
    /// Сочетание для команды (`F5`, `Shift+F6`, `Ctrl+1`), если есть.
    fn hint(&self, cmd: &Command) -> Option<CompactString>;
}

/// Пресет Far (S3 §3): команда с аргументами и ключ подписи по F-клавишам.
const FAR: [[Option<(&str, &str)>; 12]; 4] = [
    [
        Some(("ui.help", "keybar-ui-help")),
        Some(("ui.user_menu", "keybar-ui-user_menu")),
        Some(("viewer.open", "keybar-viewer-open")),
        Some(("editor.open", "keybar-editor-open")),
        Some(("ops.copy", "keybar-ops-copy")),
        Some(("ops.move", "keybar-ops-move")),
        Some(("ops.mkdir", "keybar-ops-mkdir")),
        Some(("ops.delete", "keybar-ops-delete")),
        Some(("ui.menu", "keybar-ui-menu")),
        Some(("app.quit", "keybar-app-quit")),
        Some(("ui.palette", "cmd-ui-palette")),
        Some(("ui.screens", "cmd-ui-screens")),
    ],
    [
        Some(("arc.add", "keybar-arc-add")),
        Some(("arc.extract", "keybar-arc-extract")),
        Some(("arc.commands", "keybar-arc-commands")),
        None,
        Some(("ops.copy current=true", "keybar-ops-copy-current")),
        Some(("ops.rename", "keybar-ops-rename")),
        None,
        Some(("ops.delete current=true", "keybar-ops-delete-current")),
        Some(("app.save_session", "keybar-app-save_session")),
        None,
        None,
        None,
    ],
    [
        Some(("panel.toggle side=left", "keybar-panel-toggle-left")),
        Some(("panel.toggle side=right", "keybar-panel-toggle-right")),
        Some(("panel.sort key=name", "keybar-panel-sort-name")),
        Some(("panel.sort key=ext", "keybar-panel-sort-ext")),
        Some(("panel.sort key=mtime", "keybar-panel-sort-mtime")),
        Some(("panel.sort key=size", "keybar-panel-sort-size")),
        Some(("panel.sort key=unsorted", "keybar-panel-sort-unsorted")),
        None,
        None,
        None,
        None,
        Some(("ui.menu.sort", "cmd-ui-menu-sort")),
    ],
    [
        Some(("ui.drives side=left", "keybar-ui-drives-left")),
        Some(("ui.drives side=right", "keybar-ui-drives-right")),
        None,
        None,
        None,
        Some(("ops.link", "keybar-ops-link")),
        Some(("search.files", "keybar-search-files")),
        None,
        Some(("ui.window.maximize", "keybar-ui-window-maximize")),
        Some(("search.dirs", "keybar-search-dirs")),
        None,
        Some(("ui.history.dirs", "keybar-ui-history-dirs")),
    ],
];

/// Сочетания без F-клавиш, которые показывает меню (S3 §3, пресет Far; временные
/// клавиши app до keymap S3).
const FAR_EXTRA: &[(&str, &str)] = &[
    ("panel.view_mode mode=brief", "Ctrl+1"),
    ("panel.view_mode mode=medium", "Ctrl+2"),
    ("panel.view_mode mode=full", "Ctrl+3"),
    ("panel.view_mode mode=wide", "Ctrl+4"),
    ("ui.panels.toggle_all", "Ctrl+O"),
    ("panel.switch", "Tab"),
    ("sel.toggle", "Ins"),
    ("sel.invert", "Gray *"),
    ("ui.draft command=sel.add arg=mask", "Gray +"),
    ("ui.draft command=sel.remove arg=mask", "Gray −"),
    ("tab.new", "Ctrl+Shift+T"),
    ("tab.close", "Ctrl+Shift+W"),
    ("panel.swap", "Ctrl+U"),
];

/// Заглушка keymap: пресет Far (S0-decisions №21) до S3-04.
#[derive(Debug, Default)]
pub struct FarStub;

impl KeyHints for FarStub {
    fn fkeys(&self, mods: Mods) -> [Option<FKey>; 12] {
        FAR[mods.idx()].map(|slot| {
            let (cmd, label) = slot?;
            Some(FKey {
                cmd: Command::parse(cmd).expect("stub command parses"),
                label_key: label.into(),
            })
        })
    }

    fn hint(&self, cmd: &Command) -> Option<CompactString> {
        let text = cmd.to_string();
        for mods in [Mods::None, Mods::Shift, Mods::Ctrl, Mods::Alt] {
            if let Some(n) = FAR[mods.idx()]
                .iter()
                .position(|s| s.is_some_and(|(c, _)| c == text))
            {
                return Some(format!("{}F{}", mods.prefix(), n + 1).into());
            }
        }
        FAR_EXTRA
            .iter()
            .find(|(c, _)| *c == text)
            .map(|(_, k)| CompactString::from(*k))
    }
}

impl FarStub {
    /// Все привязки заглушки: команда и сочетание (для реестра команд).
    pub fn bindings() -> Vec<(Command, String)> {
        let mut out = Vec::new();
        for mods in [Mods::None, Mods::Shift, Mods::Ctrl, Mods::Alt] {
            for (n, slot) in FAR[mods.idx()].iter().enumerate() {
                if let Some((cmd, _)) = slot {
                    let c = Command::parse(cmd).expect("stub command parses");
                    out.push((c, format!("{}F{}", mods.prefix(), n + 1)));
                }
            }
        }
        for (cmd, key) in FAR_EXTRA {
            out.push((
                Command::parse(cmd).expect("stub command parses"),
                (*key).to_owned(),
            ));
        }
        out
    }
}

/// Fluent-ключ команды: точки id → дефисы (`ops.copy` → `ops-copy`).
pub fn key_part(id: &str) -> String {
    id.replace('.', "-")
}

/// Подпись F-клавиши: `label_key`, иначе `cmd-<id>`, иначе id.
pub fn label(i18n: &I18n, label_key: &str, id: &str) -> String {
    let fallback = format!("cmd-{}", key_part(id));
    for key in [label_key, fallback.as_str()] {
        if i18n.has(key) {
            return i18n.get(key);
        }
    }
    id.to_owned()
}

/// Подготовленная строка: подписи всех наборов модификаторов пересчитываются при
/// смене языка (`epoch`), не каждый кадр.
#[derive(Debug, Default)]
pub struct Keybar {
    epoch: Option<u64>,
    rows: [Vec<Item>; 4],
}

/// F-клавиша строки.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// 1…10.
    pub n: u8,
    pub cmd: Command,
    pub label: String,
    /// Команда зарегистрирована (иначе — серая, без клика).
    pub enabled: bool,
}

impl Keybar {
    /// Обновить подписи, если сменился язык.
    pub fn refresh(
        &mut self,
        i18n: &I18n,
        epoch: u64,
        registry: &[&'static str],
        hints: &dyn KeyHints,
    ) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.epoch = Some(epoch);
        for mods in [Mods::None, Mods::Shift, Mods::Ctrl, Mods::Alt] {
            self.rows[mods.idx()] = hints
                .fkeys(mods)
                .into_iter()
                .zip(1u8..=SLOTS)
                .filter_map(|(slot, n)| {
                    let FKey { cmd, label_key } = slot?;
                    let id = cmd.id.as_str();
                    Some(Item {
                        n,
                        label: label(i18n, &label_key, id),
                        enabled: registry.contains(&id),
                        cmd,
                    })
                })
                .collect();
        }
    }

    /// Строка для зажатых модификаторов.
    pub fn items(&self, mods: Mods) -> &[Item] {
        &self.rows[mods.idx()]
    }
}

/// Id зарегистрированных команд.
pub fn registered(registry: &CommandRegistry) -> Vec<&'static str> {
    registry.iter().map(|m| m.id).collect()
}

/// Строка F-клавиш в `strip`: 10 равных слотов, номер и подпись, клик — команда.
pub(crate) fn show(
    c: &mut dyn Canvas,
    ui: &mut egui::Ui,
    kb: &Keybar,
    mods: Mods,
    tokens: &Tokens,
    out: &mut Vec<Command>,
) {
    let strip = ui.max_rect();
    let w = strip.width() / f32::from(SLOTS);
    let size = tokens.m(Metric::TypeCaption);
    let chip = tokens.radius(Metric::RadiusControl);
    for n in 1..=SLOTS {
        let x = strip.left() + w * f32::from(n - 1);
        let rect =
            egui::Rect::from_min_size(egui::pos2(x, strip.top()), egui::vec2(w, strip.height()))
                .shrink2(egui::vec2(tokens.m(Metric::SpaceXxs), 0.0));
        let item = kb.items(mods).iter().find(|i| i.n == n);
        let enabled = item.is_some_and(|i| i.enabled);
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let resp = ui.interact(rect, ui.scope_id().with(("fkey", n)), sense);
        let fill = if enabled && resp.hovered() {
            Role::FillSubtleHover
        } else {
            Role::FillControl
        };
        c.rect(rect, fill, chip);
        let num = c.text(
            rect.left_center() + egui::vec2(tokens.m(Metric::SpaceS), 0.0),
            egui::Align2::LEFT_CENTER,
            &Tx::user(n.to_string()),
            Font::Caption,
            size,
            Role::TextSecondary,
        );
        let Some(item) = item else {
            continue;
        };
        let room = rect.right() - num.right() - tokens.m(Metric::SpaceXs) - 2.0;
        c.text_clipped(
            egui::pos2(num.right() + tokens.m(Metric::SpaceXs), rect.center().y),
            room,
            &Tx::user(item.label.as_str()),
            Font::Caption,
            size,
            if enabled {
                Role::TextPrimary
            } else {
                Role::TextDisabled
            },
        );
        if resp.clicked() {
            out.push(item.cmd.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FarStub;

    /// У заглушки одно сочетание — одна команда (конфликты пользовательских keymap
    /// ловит компиляция keymap S3, §2.5 `Ambiguous`).
    #[test]
    fn stub_keys_are_unique() {
        let b = FarStub::bindings();
        let mut keys: Vec<&str> = b.iter().map(|(_, k)| k.as_str()).collect();
        keys.sort_unstable();
        let mut dups = keys.clone();
        dups.dedup();
        assert_eq!(keys.len(), dups.len(), "{keys:?}");
    }
}

//! Меню F9 (S2 §2.6, S2-14): меню-бар в заголовке вместо табов и выпадающий список
//! (Fluent `MenuFlyout`). Немодальное: панели и job-бар живут, фокус —
//! `Focus::Overlay(Menu)`; клик мимо закрывает меню и не уходит в панель.
//!
//! Пункты — из вшитого `menu.toml` (переопределяется в каталоге конфига); подпись —
//! ключ Fluent, буква — его атрибут `.menu-key`, хоткей — [`KeyHints`].
//! Клавиатура: ← → — меню, ↑ ↓ — пункт, буква — выбор, Enter — выполнить; Esc
//! закрывает в app (`ui.cancel`).

use compact_str::CompactString;
use serde::Deserialize;
use urfin_core::{Command, CommandRegistry, Side};
use urfin_i18n::I18n;

/// Вшитое меню.
pub const MENU_TOML: &str = include_str!("menu.toml");

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
                    .unwrap_or_else(|| format!("cmd-{}", crate::keybar::key_part(cmd.id.as_str())));
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
pub fn open_id(st: &crate::state::UiState) -> Option<&str> {
    st.menu
        .as_deref()
        .filter(|m| st.menu_side.is_none() && *m != "subdirs")
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

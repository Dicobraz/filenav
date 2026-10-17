//! Строка F-клавиш. Привязки — заглушка пресета Far из S3 §3, пока нет
//! `Keymap::keybar` (S3-04); подписи — `keybar-<id>`, запасная `cmd-<id>` (S0 v3 №14).

use urfin_core::CommandRegistry;
use urfin_i18n::I18n;

/// Команды F1…F12 без модификаторов (пресет Far, S3 §3).
pub const FAR_STUB: [Option<&str>; 12] = [
    Some("ui.help"),
    Some("ui.user_menu"),
    Some("viewer.open"),
    Some("editor.open"),
    Some("ops.copy"),
    Some("ops.move"),
    Some("ops.mkdir"),
    Some("ops.delete"),
    Some("ui.menu"),
    Some("app.quit"),
    None,
    None,
];

/// Команды без виджета: зарегистрированы, но в keybar пока серые (меню — S2-14).
const NO_WIDGET_YET: &[&str] = &["ui.menu"];

/// Fluent-ключ команды: точки id → дефисы (`ops.copy` → `ops-copy`).
pub fn key_part(id: &str) -> String {
    id.replace('.', "-")
}

/// Подпись F-клавиши: `keybar-<id>`, иначе `cmd-<id>`, иначе id.
pub fn label(i18n: &I18n, id: &str) -> String {
    let part = key_part(id);
    for key in [format!("keybar-{part}"), format!("cmd-{part}")] {
        if i18n.has(&key) {
            return i18n.get(&key);
        }
    }
    id.to_owned()
}

/// Подготовленная строка: подписи пересчитываются при смене языка (`epoch`).
#[derive(Debug, Default)]
pub struct Keybar {
    epoch: Option<u64>,
    pub items: Vec<Item>,
}

/// F-клавиша.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 1…12.
    pub n: u8,
    pub cmd: &'static str,
    pub label: String,
    /// Команда зарегистрирована (иначе — серая, без клика).
    pub enabled: bool,
}

impl Keybar {
    /// Обновить подписи, если сменился язык.
    pub fn refresh(&mut self, i18n: &I18n, epoch: u64, registry: &[&'static str]) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.epoch = Some(epoch);
        self.items = FAR_STUB
            .iter()
            .zip(1u8..)
            .filter_map(|(cmd, n)| {
                let cmd = (*cmd)?;
                Some(Item {
                    n,
                    cmd,
                    label: label(i18n, cmd),
                    enabled: registry.contains(&cmd) && !NO_WIDGET_YET.contains(&cmd),
                })
            })
            .collect();
    }
}

/// Id зарегистрированных команд.
pub fn registered(registry: &CommandRegistry) -> Vec<&'static str> {
    registry.iter().map(|m| m.id).collect()
}

//! Реестр команд для людей: `docs/COMMANDS.md` из живого реестра (core + ui).
//!
//! Команды — имена функций Urfin. Их принимают `menu.toml`, keymap (S3) и командная
//! строка в виде `id арг=значение …`; при загрузке команда проверяется по реестру
//! (`CommandRegistry::validate`). Документ сверяет тест `commands_doc_is_current`:
//! новая команда без обновления документа — красный тест.

use std::fmt::Write as _;

use urfin_core::{ArgSpec, Command, CommandRegistry, ValueKind};
use urfin_i18n::I18n;

use crate::legacy::keybar::{FarStub, key_part};
use crate::legacy::menu::{Item, MenuDef};

fn kind(k: ValueKind) -> &'static str {
    match k {
        ValueKind::Bool => "bool",
        ValueKind::Int => "int",
        ValueKind::Str => "str",
        ValueKind::Loc => "loc",
        ValueKind::List => "list",
    }
}

fn args(specs: &[ArgSpec]) -> String {
    if specs.is_empty() {
        return "—".to_owned();
    }
    specs
        .iter()
        .map(|a| {
            format!(
                "`{}{}:{}`",
                a.name,
                if a.required { "" } else { "?" },
                kind(a.kind)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Ячейка таблицы markdown: `|` экранируется.
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Текст документа.
pub fn render(registry: &CommandRegistry, en: &I18n, ru: &I18n, menus: &[MenuDef]) -> String {
    let bindings = FarStub::bindings();
    let mut out = String::new();
    out.push_str(HEADER);
    let mut categories: Vec<&str> = Vec::new();
    for m in registry.iter() {
        if !categories.contains(&m.category) {
            categories.push(m.category);
        }
    }
    for cat in categories {
        let metas: Vec<_> = registry.iter().filter(|m| m.category == cat).collect();
        let _ = writeln!(out, "\n## `{cat}` ({})\n", metas.len());
        out.push_str("| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |\n");
        out.push_str("|---|---|---|---|---|---|\n");
        for m in metas {
            let key = format!("cmd-{}", key_part(m.id));
            let title = |i: &I18n| {
                if i.has(&key) {
                    i.get(&key)
                } else {
                    m.title.to_owned()
                }
            };
            let keys: Vec<String> = bindings
                .iter()
                .filter(|(c, _)| c.id.as_str() == m.id)
                .map(|(c, k)| variant(c, k))
                .collect();
            let in_menus: Vec<String> = menus
                .iter()
                .filter(|d| {
                    d.items
                        .iter()
                        .any(|i| matches!(i, Item::Cmd { cmd, .. } if cmd.id.as_str() == m.id))
                })
                .map(|d| d.id.to_string())
                .collect();
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | {} | {} |",
                m.id,
                args(m.args),
                cell(&title(en)),
                cell(&title(ru)),
                dash(&keys.join(", ")),
                dash(&in_menus.join(", ")),
            );
        }
    }
    out
}

/// `Ctrl+F3 (key=name)` — сочетание и аргументы варианта.
fn variant(c: &Command, key: &str) -> String {
    let text = c.to_string();
    match text.split_once(' ') {
        Some((_, args)) => cell(&format!("{key} ({args})")),
        None => key.to_owned(),
    }
}

fn dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}

const HEADER: &str = r#"# Реестр команд Urfin

> Генерируется из реестра команд (core + ui + vfs) тестом `commands_doc_is_current`
> (`crates/ui/legacy/tests.rs`). Руками не править. Обновить:
> `URFIN_UPDATE_DOCS=1 cargo nextest run -p urfin-ui commands_doc`.

Команда — имя функции Urfin. Её принимают меню F9 (`menu.toml`), keymap (S3) и командная
строка в одном виде: `id арг=значение …`, например `panel.sort key=size` или
`panel.go path="C:\Program Files"`.

Значения аргументов:
- `bool` — `true` / `false`;
- `int` — целое;
- `str` — слово или строка в кавычках, кавычка внутри — `""`;
- `loc` — путь или uri (`local:C:\work`, архив — `local:C:\a.zip>>zip:/in`);
- `list` — `[a, b]`.

`?` — необязательный аргумент. `side` — `left | right | active | passive`, по умолчанию активная панель.

**Проверка в runtime.** При загрузке `menu.toml` каждая команда проверяется по этому реестру. Id должен быть известен, аргументы — описаны и нужного типа, обязательные — заданы. Иначе пункт скрыт, а предупреждение показывается тостом.

**Клавиши** — пресет Far (заглушка до keymap S3). **Меню F9** — где команда стоит во вшитом `menu.toml`.
"#;

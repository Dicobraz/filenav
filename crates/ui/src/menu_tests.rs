use egui::{Rect, pos2, vec2};
use urfin_core::Command;

use super::*;
use crate::canvas::RecordCanvas;
use crate::fonts::FontSet;
use crate::keybar::FarStub;
use crate::tokens::Tokens;

/// Реестр команд core и ui, как в приложении.
fn registered() -> urfin_core::CommandRegistry {
    crate::tests::registry()
}

fn i18n() -> urfin_i18n::I18n {
    urfin_i18n::Catalog::embedded()
        .build(&"ru".parse().unwrap())
        .0
}

#[test]
fn builtin_menu_has_only_registered_commands() {
    let (menus, warnings) = parse(MENU_TOML, &registered()).unwrap();
    assert_eq!(warnings, Vec::<String>::new());
    let ids: Vec<&str> = menus.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["left", "files", "commands", "options", "right"]);
    assert_eq!(menus[0].side, Some(Side::Left));
    // Подписи пунктов есть в обоих языках.
    let catalog = urfin_i18n::Catalog::embedded();
    for tag in ["en", "ru"] {
        let i18n = catalog.build(&tag.parse().unwrap()).0;
        for m in &menus {
            assert!(i18n.has(&m.title), "{tag}: {}", m.title);
            for item in &m.items {
                if let Item::Cmd { label, .. } = item {
                    assert!(i18n.has(label), "{tag}: {label}");
                }
            }
        }
    }
}

/// Во вшитом меню буквы выбора не повторяются ни в одном языке.
#[test]
fn builtin_menu_letters_are_unique() {
    let (menus, _) = parse(MENU_TOML, &registered()).unwrap();
    let catalog = urfin_i18n::Catalog::embedded();
    for tag in ["en", "ru"] {
        let i18n = catalog.build(&tag.parse().unwrap()).0;
        assert_eq!(
            letter_conflicts(&menus, &i18n),
            Vec::<String>::new(),
            "{tag}"
        );
    }
}

/// Повторяющаяся буква (своё меню) — пункты по кругу, без выполнения.
#[test]
fn shared_letter_cycles() {
    let text = r#"
[[menu]]
id = "x"
title = "menu-files"
items = [
    { cmd = "sel.all", label = "menu-sort-name" },
    { cmd = "sel.none", label = "menu-sort-name" },
]
"#;
    let menus = parse(text, &registered()).unwrap().0;
    assert_eq!(letter_conflicts(&menus, &i18n()).len(), 1);
    let mut fx = Fx::new();
    fx.menus = menus;
    fx.frame("x", Vec::new());
    assert_eq!(fx.memo.sel, Some(0));
    let (_, out) = fx.frame("x", vec![egui::Event::Text("и".into())]);
    assert_eq!(out.len(), 0);
    assert_eq!(fx.memo.sel, Some(1));
}

/// Неизвестный id — предупреждение, пункт скрыт; разделители по краям и подряд — нет.
#[test]
fn unknown_command_is_hidden_with_warning() {
    let text = r#"
[[menu]]
id = "x"
title = "menu-files"
items = [
    { sep = true },
    { cmd = "no.such" },
    { cmd = "sel.all" },
    { sep = true },
    { sep = true },
    { cmd = "sel.none" },
    { sep = true },
]
"#;
    let (menus, warnings) = parse(text, &registered()).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("no.such"));
    let items = &menus[0].items;
    assert_eq!(items.len(), 3, "{items:?}");
    assert!(matches!(items[1], Item::Sep));
    assert!(parse("[[menu]]\nid = 1", &registered()).is_err());
}

/// Аргументы проверяются по реестру: лишний, не того типа, без обязательного.
#[test]
fn bad_arguments_hide_the_item() {
    let text = r#"
[[menu]]
id = "x"
title = "menu-files"
items = [
    { cmd = "panel.sort bogus=1" },
    { cmd = "ui.zoom delta=big" },
    { cmd = "panel.view_mode" },
    { cmd = "panel.sort key=size" },
]
"#;
    let (menus, warnings) = parse(text, &registered()).unwrap();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert_eq!(menus[0].items.len(), 1);
}

#[test]
fn neighbor_and_step_wrap() {
    let (menus, _) = parse(MENU_TOML, &registered()).unwrap();
    assert_eq!(neighbor(&menus, "left", -1).as_deref(), Some("right"));
    assert_eq!(neighbor(&menus, "files", 1).as_deref(), Some("commands"));
    let files = &menus[1];
    // Первый пункт — rename, за ним разделитель: шаг вниз его пропускает.
    assert_eq!(step(files, Some(0), 1), Some(2));
    assert_eq!(step(files, Some(0), -1), Some(files.items.len() - 1));
}

struct Fx {
    ctx: egui::Context,
    i18n: urfin_i18n::I18n,
    tokens: Tokens,
    menus: Vec<MenuDef>,
    memo: MenuMemo,
}

impl Fx {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            i18n: i18n(),
            tokens: Tokens::default(),
            menus: parse(MENU_TOML, &registered()).unwrap().0,
            memo: MenuMemo::default(),
        }
    }

    /// Кадр выпадающего списка меню `id` от (20, 40).
    fn frame(&mut self, id: &str, events: Vec<egui::Event>) -> (String, Vec<Command>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let def = self.menus.iter().find(|m| m.id == id).unwrap().clone();
        let (mut yaml, mut out) = (String::new(), Vec::new());
        let ctx = self.ctx.clone();
        let mut full = ctx.run_ui(input, |root| {
            let mut c = RecordCanvas::new(&self.i18n);
            flyout(
                &mut c,
                root,
                &self.i18n,
                &self.tokens,
                &FontSet::default(),
                &FarStub,
                &def,
                &mut self.memo,
                pos2(20.0, 40.0),
                &mut out,
            );
            yaml = c.to_yaml();
        });
        full.textures_delta.clear();
        (yaml, out)
    }
}

fn key(k: egui::Key) -> egui::Event {
    egui::Event::Key {
        key: k,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn ids(out: &[Command]) -> Vec<String> {
    out.iter().map(ToString::to_string).collect()
}

/// Список §2.6: пункты 28 px, хоткеи справа (`KeyHints`), разделители, текущий —
/// подсветка и пилюля.
#[test]
fn snapshot_menu_left() {
    let mut fx = Fx::new();
    fx.frame("left", Vec::new());
    let (yaml, out) = fx.frame("left", Vec::new());
    assert_eq!(out.len(), 0);
    assert!(yaml.contains("\"Ctrl+1\""), "{yaml}");
    insta::assert_snapshot!(yaml);
}

#[test]
fn keyboard_moves_and_executes() {
    let mut fx = Fx::new();
    fx.frame("left", Vec::new());
    // ↓ ↓ Enter — третий пункт (Таблица) в левой панели.
    fx.frame("left", vec![key(egui::Key::ArrowDown)]);
    fx.frame("left", vec![key(egui::Key::ArrowDown)]);
    let (_, out) = fx.frame("left", vec![key(egui::Key::Enter)]);
    assert_eq!(
        ids(&out),
        [
            "ui.cancel",
            "panel.focus side=left",
            "panel.view_mode mode=full"
        ]
    );
    // Буква — пункт с этой буквой (`.menu-key`): «ш» — Широкий.
    let (_, out) = fx.frame("left", vec![egui::Event::Text("ш".into())]);
    assert_eq!(ids(&out).last().unwrap(), "panel.view_mode mode=wide");
}

use urfin_core::{ArgSpec, CommandMeta, CommandRegistry, ValueKind};

use super::*;

const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        required,
    }
}

const fn meta(id: &'static str, args: &'static [ArgSpec]) -> CommandMeta {
    CommandMeta {
        id,
        title: id,
        category: "test",
        args,
        repeatable: false,
    }
}

/// Команды примера S3 §2.2 и тестов.
static METAS: &[CommandMeta] = &[
    meta("ui.menu", &[arg("menu", ValueKind::Str, false)]),
    meta("app.quit", &[]),
    meta("panel.refresh", &[]),
    meta("panel.sort", &[arg("key", ValueKind::Str, true)]),
    meta("sel.toggle", &[]),
    meta("panel.cursor.down", &[]),
    meta("panel.cursor.up", &[]),
    meta("panel.hidden.toggle", &[]),
    meta("ui.split.width", &[arg("delta", ValueKind::Int, false)]),
    meta("panel.up", &[]),
];

fn reg() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    r.register(METAS).unwrap();
    r
}

fn flags() -> FlagRegistry {
    let mut f = FlagRegistry::with_builtin();
    // S6 регистрирует его сам.
    f.register("cmdline_empty", |_| true);
    f
}

fn load(layers: &[(&str, Layer)], os: Os) -> (Arc<Keymap>, Vec<KeymapWarning>) {
    let (km, w) = Keymap::from_str_layers(layers, &reg(), &flags(), os);
    (km, own(w))
}

/// Без предупреждений встроенных пресетов: их команды — в настоящем реестре, тестовый
/// реестр их не знает (пресеты против реестра проверяет app).
fn own(mut w: Vec<KeymapWarning>) -> Vec<KeymapWarning> {
    w.retain(|w| !matches!(w.file.as_str(), "base" | "far" | "dn"));
    w
}

fn keys(km: &Keymap) -> Vec<String> {
    km.bindings()
        .iter()
        .map(|b| {
            let cmds: Vec<String> = b.cmds.iter().map(ToString::to_string).collect();
            format!(
                "[{}] {} = {} ({:?})",
                b.src.context,
                b.chord,
                cmds.join(", "),
                b.layer
            )
        })
        .collect()
}

/// Пример S3 §2.2 дословно.
const EXAMPLE: &str = r#"
format = 1
name = "my"
extends = "far"              # base | far | dn | <пользовательский>; цепочка ≤4, цикл = ошибка

[context.global]             # ключ секции = when-выражение
"F10" = "ui.menu"
"Alt+X" = "app.quit"

[context.panel]
"Ctrl+R"  = "panel.refresh"                               # id команды
"Ctrl+F5" = { cmd = "panel.sort", key = "mtime" }          # форма S1: {cmd, args…}
"Shift+Down" = ["sel.toggle", "panel.cursor.down"]        # последовательность команд
"Ctrl+K Ctrl+H" = "panel.hidden.toggle"                   # аккорд: до 3 нажатий через пробел
"Up" = { cmd = "panel.cursor.up", _repeat = true }        # _repeat переопределяет CommandMeta
"Ctrl+O" = false                                          # снять унаследованную привязку

[context."panel && cmdline_empty"]
"Ctrl+Left" = { cmd = "ui.split.width", delta = -1 }

[context."*"]                # во всех контекстах
"Ctrl+Q" = false

[os.macos.context.panel]     # оверлей платформы: os.windows | os.macos | os.linux
"Cmd+Up" = "panel.up"
"#;

#[test]
fn spec_example_loads_without_warnings() {
    for os in [Os::Windows, Os::Macos, Os::Linux] {
        let (km, warnings) = load(&[(EXAMPLE, Layer::User)], os);
        let text: Vec<String> = warnings.iter().map(ToString::to_string).collect();
        if os == Os::Macos {
            // На macOS Ctrl+F1…F8 и Ctrl+стрелки — системные (S3 §4).
            assert_eq!(
                text,
                [
                    "my: [panel] Ctrl+F5: reserved by the OS: never reaches Urfin",
                    "my: [panel && cmdline_empty] Ctrl+Left: reserved by the OS: never reaches Urfin"
                ]
            );
        } else {
            assert_eq!(text, Vec::<String>::new(), "{os:?}");
        }
        let k = keys(&km);
        assert!(
            k.contains(&"[panel] Ctrl+F5 = panel.sort key=mtime (User)".to_owned()),
            "{k:#?}"
        );
        assert!(
            k.contains(&"[panel] Shift+Down = sel.toggle, panel.cursor.down (User)".to_owned())
        );
        assert!(k.contains(&"[panel] Ctrl+K Ctrl+H = panel.hidden.toggle (User)".to_owned()));
        let mac = k.iter().any(|l| l.contains("Cmd+Up"));
        assert_eq!(mac, os == Os::Macos, "оверлей только своей ОС");
        let up = km
            .bindings()
            .iter()
            .find(|b| b.chord.to_string() == "Up" && b.layer == Layer::User)
            .unwrap();
        assert_eq!(up.repeat, Some(true));
    }
}

/// Каждый вид предупреждения (S3 §2.5) — в одном снапшоте.
#[test]
fn every_warning_kind() {
    let bad = r#"
format = 1
name = "bad"
colour = "red"

[context.panel]
"Ctrl+Nope" = "app.quit"
"F2" = "no.such"
"F3" = { cmd = "panel.sort", bogus = 1 }
"F4" = { cmd = "panel.sort" }
"F5" = "panel.refresh"
"Alt+Tab" = "app.quit"
"Ctrl+Alt+Q" = "app.quit"
"Ctrl+K" = "app.quit"
"Ctrl+K Ctrl+C" = "panel.refresh"

[context."panel || cmdline"]
"F5" = "app.quit"

[context."panel &&"]
"F6" = "app.quit"

[context."panel && nope"]
"F7" = "app.quit"
"#;
    let layers = [
        ("this is { not toml", Layer::User),
        ("format = 2\nname = \"future\"", Layer::User),
        ("format = 1\nname = \"a\"\nextends = \"b\"", Layer::User),
        ("format = 1\nname = \"b\"\nextends = \"a\"", Layer::User),
        (
            "format = 1\nname = \"orphan\"\nextends = \"ghost\"",
            Layer::User,
        ),
        ("format = 1\nname = \"c1\"\nextends = \"c2\"", Layer::User),
        ("format = 1\nname = \"c2\"\nextends = \"c3\"", Layer::User),
        ("format = 1\nname = \"c3\"\nextends = \"c4\"", Layer::User),
        ("format = 1\nname = \"c4\"\nextends = \"c5\"", Layer::User),
        ("format = 1\nname = \"c5\"\nextends = \"c6\"", Layer::User),
        ("format = 1\nname = \"c6\"", Layer::User),
        (bad, Layer::User),
        ("format = 1\nname = \"bad\"", Layer::User),
    ];
    let (_, warnings) = load(&layers, Os::Windows);
    let text: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    insta::assert_snapshot!(text.join("\n"));
    let kinds: Vec<&WarnKind> = warnings.iter().map(|w| &w.kind).collect();
    let has = |f: fn(&WarnKind) -> bool| kinds.iter().any(|k| f(k));
    assert!(has(|k| matches!(k, WarnKind::BadFile(_))));
    assert!(has(|k| matches!(k, WarnKind::BadFormat(2))));
    assert!(has(|k| matches!(k, WarnKind::UnknownKey)));
    assert!(has(|k| matches!(k, WarnKind::ExtendsCycle(_))));
    assert!(has(|k| matches!(k, WarnKind::ExtendsTooDeep(_))));
    assert!(has(|k| matches!(k, WarnKind::ExtendsMissing(_))));
    assert!(has(|k| matches!(k, WarnKind::Duplicate(_))));
    assert!(has(|k| matches!(k, WarnKind::UnknownCommand(_))));
    assert!(has(|k| matches!(k, WarnKind::BadArgs(_))));
    assert!(has(|k| matches!(k, WarnKind::BadKey(_))));
    assert!(has(|k| matches!(k, WarnKind::BadWhen(_))));
    assert!(has(|k| matches!(k, WarnKind::UnknownFlag(_))));
    assert!(has(|k| matches!(k, WarnKind::Ambiguous(_))));
    assert!(has(|k| matches!(k, WarnKind::PrefixShadow(_))));
    assert!(has(|k| matches!(k, WarnKind::Reserved)));
    assert!(has(|k| matches!(k, WarnKind::AltGrRisk)));
}

/// Битый файл — предупреждение, слой пропущен, остальное загружено.
#[test]
fn broken_file_is_skipped() {
    let good = "format = 1\nname = \"good\"\n[context.panel]\n\"F5\" = \"panel.refresh\"";
    let (km, warnings) = load(&[(good, Layer::Preset), ("[[[", Layer::User)], Os::Windows);
    assert_eq!(warnings.len(), 1);
    assert!(matches!(warnings[0].kind, WarnKind::BadFile(_)));
    assert_eq!(keys(&km), ["[panel] F5 = panel.refresh (Preset)"]);
}

/// `"*" = false` снимает привязку во всех контекстах, `false` в контексте — только там.
#[test]
fn unbinding() {
    let base = r#"
format = 1
name = "lower"
[context.panel]
"Ctrl+Q" = "app.quit"
"F5" = "panel.refresh"
[context.cmdline]
"Ctrl+Q" = "app.quit"
"F5" = "panel.refresh"
"#;
    let all = "format = 1\nname = \"u1\"\nextends = \"lower\"\n[context.\"*\"]\n\"Ctrl+Q\" = false";
    let (km, w) = load(&[(base, Layer::Preset), (all, Layer::User)], Os::Windows);
    assert_eq!(w, Vec::new());
    assert!(
        !keys(&km).iter().any(|k| k.contains("Ctrl+Q")),
        "{:#?}",
        keys(&km)
    );
    let one = "format = 1\nname = \"u2\"\nextends = \"lower\"\n[context.panel]\n\"F5\" = false";
    let (km, _) = load(&[(base, Layer::Preset), (one, Layer::User)], Os::Windows);
    let k = keys(&km);
    assert!(!k.contains(&"[panel] F5 = panel.refresh (Preset)".to_owned()));
    assert!(k.contains(&"[cmdline] F5 = panel.refresh (Preset)".to_owned()));
    // `*` без `false` не привязывает.
    let star = "format = 1\nname = \"s\"\n[context.\"*\"]\n\"F9\" = \"app.quit\"";
    let (km, w) = load(&[(star, Layer::User)], Os::Windows);
    assert!(km.bindings().is_empty());
    assert!(matches!(w[0].kind, WarnKind::BadWhen(_)));
}

/// Оверлей ОС в пресете — слой `Os`, в пользовательском файле — `User`.
#[test]
fn os_overlay_layers() {
    let preset = "format = 1\nname = \"p\"\n[os.windows.context.panel]\n\"F5\" = \"panel.refresh\"";
    let (km, _) = load(&[(preset, Layer::Preset)], Os::Windows);
    assert_eq!(keys(&km), ["[panel] F5 = panel.refresh (Os)"]);
    let (km, _) = load(&[(preset, Layer::Preset)], Os::Linux);
    assert!(km.bindings().is_empty());
    let (km, _) = load(&[(preset, Layer::User)], Os::Windows);
    let user: Vec<String> = keys(&km)
        .into_iter()
        .filter(|k| k.ends_with("(User)"))
        .collect();
    assert_eq!(user, ["[panel] F5 = panel.refresh (User)"]);
}

/// `Keymap::load`: пресет far (с base) и `config_dir/keymaps/user.toml` с `extends = "dn"`.
#[test]
fn load_from_config_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        config_dir: tmp.path().to_path_buf(),
        state_dir: tmp.path().join("state"),
        log_dir: tmp.path().join("logs"),
        cache_dir: tmp.path().join("cache"),
        portable: true,
    };
    std::fs::create_dir(tmp.path().join("keymaps")).unwrap();
    std::fs::write(
        tmp.path().join("keymaps/user.toml"),
        "format = 1\nname = \"user\"\nextends = \"dn\"\n[context.panel]\n\"F5\" = \"panel.refresh\"",
    )
    .unwrap();
    let (km, warnings) = Keymap::load(&paths, &KeymapCfg::default(), &reg(), &flags(), Os::Windows);
    assert_eq!(own(warnings), Vec::new());
    let user: Vec<String> = keys(&km)
        .into_iter()
        .filter(|k| k.ends_with("(User)"))
        .collect();
    assert_eq!(user, ["[panel] F5 = panel.refresh (User)"]);
    // Нет пользовательского файла и неизвестный пресет — предупреждение, старт идёт.
    let cfg = KeymapCfg {
        preset: "nope".into(),
        user: None,
        ..KeymapCfg::default()
    };
    let (km, warnings) = Keymap::load(&paths, &cfg, &reg(), &flags(), Os::Windows);
    assert!(km.bindings().is_empty());
    assert!(matches!(warnings[0].kind, WarnKind::ExtendsMissing(_)));
}

/// Секция `[keymap]` S3 §2.1 (+ `logical` из v4 №5).
#[test]
fn keymap_section() {
    let text = r#"
preset = "dn"
user = "mine.toml"
match = "logical"
quick_search = "alt_tap"
alt_tap_ms = 300
chord_timeout_ms = 1000
mac_option_as_alt = "both"
"#;
    let cfg: KeymapCfg = toml::from_str(text).unwrap();
    assert_eq!(cfg.preset, "dn");
    assert_eq!(cfg.r#match, MatchMode::Logical);
    assert_eq!(cfg.quick_search, QsMode::AltTap);
    assert_eq!(cfg.mac_option_as_alt, OptionAlt::Both);
    let default: KeymapCfg = toml::from_str("").unwrap();
    assert_eq!(default, KeymapCfg::default());
    assert_eq!(default.preset, "far");
}

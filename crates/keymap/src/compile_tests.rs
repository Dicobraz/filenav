use urfin_core::{
    AppState, ArgSpec, Command, CommandMeta, CommandRegistry, Focus, Location, Panel, Side, Tab,
    TabId, ValueKind,
};

use crate::{ChordState, FlagRegistry, Keymap, Layer, Lookup, Mods, Os, Stroke};

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

static METAS: &[CommandMeta] = &[
    meta("a.one", &[]),
    meta("a.two", &[]),
    meta("a.three", &[]),
    meta("editor.new", &[]),
    meta("panel.sort", &[arg("key", ValueKind::Str, true)]),
    meta("ops.copy", &[arg("current", ValueKind::Bool, false)]),
];

pub(crate) fn reg() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    r.register(METAS).unwrap();
    r
}

pub(crate) fn flags() -> FlagRegistry {
    let mut f = FlagRegistry::with_builtin();
    f.register("right", |st| st.active == Side::Right);
    f
}

pub(crate) fn state(focus: Focus) -> AppState {
    let loc = Location::local(if cfg!(windows) { r"C:\w" } else { "/w" });
    let mut st = AppState::new(
        Panel::new(Tab::new(TabId(1), loc.clone())),
        Panel::new(Tab::new(TabId(2), loc)),
    );
    st.focus = focus;
    st
}

fn km(layers: &[(&str, Layer)]) -> std::sync::Arc<Keymap> {
    let (km, w) = Keymap::from_str_layers(layers, &reg(), &flags(), Os::Windows);
    assert_eq!(w, Vec::new());
    km
}

fn s(text: &str) -> Stroke {
    Stroke::parse(text, Os::Windows).unwrap()
}

fn hit(km: &Keymap, stroke: &str, st: &AppState) -> Option<String> {
    match km.lookup(None, s(stroke), st) {
        Lookup::Hit(cmds, _) => Some(
            cmds.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
    }
}

/// Одна клавиша в `panel` и `panel && <флаг>`: точнее — то, что сработало с флагом.
#[test]
fn more_specific_context_wins() {
    let k = km(&[(
        r#"format = 1
name = "p"
[context.panel]
"F5" = "a.one"
[context."panel && right"]
"F5" = "a.two"
[context.global]
"F6" = "a.one"
"#,
        Layer::Preset,
    )]);
    let mut st = state(Focus::Panel);
    assert_eq!(hit(&k, "F5", &st).as_deref(), Some("a.one"));
    st.active = Side::Right;
    assert_eq!(hit(&k, "F5", &st).as_deref(), Some("a.two"));
    // `global` действует в cmdline, `panel` — нет.
    assert_eq!(hit(&k, "F5", &state(Focus::Cmdline)), None);
    assert_eq!(
        hit(&k, "F6", &state(Focus::Cmdline)).as_deref(),
        Some("a.one")
    );
    assert_eq!(hit(&k, "F6", &state(Focus::Terminal)), None, "ADR-0015");
}

/// Пользовательский слой поверх пресета при равной специфичности.
#[test]
fn user_over_preset() {
    let preset = "format = 1\nname = \"far\"\n[context.panel]\n\"F7\" = \"a.one\"\n[context.global]\n\"F8\" = \"a.one\"";
    let user = "format = 1\nname = \"u\"\nextends = \"far\"\n[context.panel]\n\"F7\" = \"a.two\"\n[context.global]\n\"F8\" = \"a.two\"";
    let k = km(&[(preset, Layer::Preset), (user, Layer::User)]);
    let st = state(Focus::Panel);
    assert_eq!(hit(&k, "F7", &st).as_deref(), Some("a.two"));
    assert_eq!(hit(&k, "F8", &st).as_deref(), Some("a.two"));
}

/// Аккорд и одиночная привязка его начала в другом контексте.
#[test]
fn chords_and_prefix() {
    let k = km(&[(
        r#"format = 1
name = "p"
[context.panel]
"Ctrl+K Ctrl+C" = "a.one"
"Ctrl+K Ctrl+X Ctrl+Y" = "a.three"
[context.cmdline]
"Ctrl+K" = "a.two"
"#,
        Layer::Preset,
    )]);
    let st = state(Focus::Panel);
    assert_eq!(k.lookup(None, s("Ctrl+K"), &st), Lookup::Prefix);
    let mut p = ChordState::default();
    p.strokes.push(s("Ctrl+K"));
    assert!(matches!(
        k.lookup(Some(&p), s("Ctrl+C"), &st),
        Lookup::Hit(..)
    ));
    assert_eq!(k.lookup(Some(&p), s("Ctrl+X"), &st), Lookup::Prefix);
    assert_eq!(k.lookup(Some(&p), s("Ctrl+Z"), &st), Lookup::Miss);
    p.strokes.push(s("Ctrl+X"));
    assert!(matches!(
        k.lookup(Some(&p), s("Ctrl+Y"), &st),
        Lookup::Hit(..)
    ));
    // В cmdline аккорда нет — одиночная привязка.
    assert_eq!(
        hit(&k, "Ctrl+K", &state(Focus::Cmdline)).as_deref(),
        Some("a.two")
    );
}

/// Сторона модификатора точнее общего: `LCtrl+X` против `Ctrl+X`.
#[test]
fn modifier_sides() {
    let k = km(&[(
        "format = 1\nname = \"p\"\n[context.panel]\n\"Ctrl+X\" = \"a.one\"\n\"LCtrl+X\" = \"a.two\"",
        Layer::Preset,
    )]);
    let st = state(Focus::Panel);
    let x = Stroke::parse("X", Os::Windows).unwrap().key;
    let left = Stroke::new(x, Mods::CTRL | Mods::LCTRL);
    let right = Stroke::new(x, Mods::CTRL | Mods::RCTRL);
    let plain = Stroke::new(x, Mods::CTRL);
    let name = |l: Lookup<'_>| match l {
        Lookup::Hit(c, _) => c[0].to_string(),
        other => format!("{other:?}"),
    };
    assert_eq!(name(k.lookup(None, left, &st)), "a.two");
    assert_eq!(name(k.lookup(None, right, &st)), "a.one");
    assert_eq!(name(k.lookup(None, plain, &st)), "a.one");
}

/// `keybar(Shift)` в Far-пресете: `F4 = editor.new` (фрагмент пресета S3 §3; весь
/// пресет — S3-09). Подписи — ключи `keybar-*`, как в S2.
#[test]
fn keybar_rows() {
    let far = r#"format = 1
name = "far"
[context.panel]
"F5" = "ops.copy"
"Shift+F4" = "editor.new"
"Shift+F5" = { cmd = "ops.copy", current = true }
"Ctrl+F3" = { cmd = "panel.sort", key = "name" }
"#;
    let k = km(&[(far, Layer::Preset)]);
    let st = state(Focus::Panel);
    let shift = k.keybar(Mods::SHIFT, &st);
    let f4 = shift[3].as_ref().unwrap();
    assert_eq!(f4.cmd.to_string(), "editor.new");
    assert_eq!(f4.label_key, "keybar-editor-new");
    assert_eq!(
        shift[4].as_ref().unwrap().label_key,
        "keybar-ops-copy-current"
    );
    assert_eq!(
        k.keybar(Mods::CTRL, &st)[2].as_ref().unwrap().label_key,
        "keybar-panel-sort-name"
    );
    assert_eq!(
        k.keybar(Mods::empty(), &st)[4].as_ref().unwrap().label_key,
        "keybar-ops-copy"
    );
    assert!(k.keybar(Mods::ALT, &st).iter().all(Option::is_none));
}

/// `keys_for` — только сочетания, которые сейчас действительно вызовут команду.
#[test]
fn keys_for_winners_only() {
    let k = km(&[(
        r#"format = 1
name = "p"
[context.panel]
"F5" = "a.one"
"Ctrl+F5" = "a.one"
[context."panel && right"]
"F5" = "a.two"
"#,
        Layer::Preset,
    )]);
    let one = Command::parse("a.one").unwrap();
    let mut st = state(Focus::Panel);
    let keys: Vec<String> = k
        .keys_for(&one, &st)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(keys, ["F5", "Ctrl+F5"]);
    st.active = Side::Right;
    let keys: Vec<String> = k
        .keys_for(&one, &st)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(keys, ["Ctrl+F5"]);
    assert_eq!(k.effective().count(), 3);
}

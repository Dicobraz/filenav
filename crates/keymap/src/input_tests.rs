use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, ImeEvent, Key, Modifiers};
use urfin_core::{AppState, ArgSpec, CommandMeta, CommandRegistry, Focus, OverlayKind, ValueKind};

use super::vk;
use crate::compile::tests::state;
use crate::{FlagRegistry, KeyInput, KeyNote, Keymap, KeymapCfg, Layer, MatchMode, Mods, Os};

const TEXT: &[ArgSpec] = &[ArgSpec {
    name: "text",
    kind: ValueKind::Str,
    required: true,
}];
/// `cmdline.paste`: без `text` — из буфера обмена (S6).
const TEXT_OPT: &[ArgSpec] = &[ArgSpec {
    name: "text",
    kind: ValueKind::Str,
    required: false,
}];

const fn meta(id: &'static str, args: &'static [ArgSpec], repeatable: bool) -> CommandMeta {
    CommandMeta {
        id,
        title: id,
        category: "test",
        args,
        repeatable,
    }
}

static METAS: &[CommandMeta] = &[
    meta("panel.cursor.down", &[], true),
    meta("panel.switch", &[], false),
    meta("ops.copy", &[], false),
    meta("sel.all", &[], false),
    meta("sel.toggle", &[], false),
    meta("a.chord", &[], false),
    meta("a.gray", &[], false),
    meta("a.center", &[], false),
    meta("a.right", &[], false),
    meta("a.digit", &[], false),
    meta("ui.cancel", &[], false),
    meta("cmdline.paste", TEXT_OPT, false),
    meta("cmdline.insert", TEXT, false),
    meta("keymap.qs.start", TEXT, false),
    meta("keymap.qs.input", TEXT, false),
];

const PRESET: &str = r#"format = 1
name = "far"
[context.panel]
"Down" = "panel.cursor.down"
"Tab" = "panel.switch"
"F5" = "ops.copy"
"Ctrl+A" = "sel.all"
"Space" = "sel.toggle"
"Ctrl+K Ctrl+C" = "a.chord"
"GrayMinus" = "a.gray"
"Num5" = "a.center"
"RCtrl+1" = "a.right"
"Shift+1" = "a.digit"
"Ctrl+V" = "cmdline.paste"
"Shift+Insert" = "cmdline.paste"
[context.overlay]
"Esc" = "ui.cancel"
"#;

fn input_with(cfg: &KeymapCfg) -> KeyInput {
    let mut reg = CommandRegistry::new();
    reg.register(METAS).unwrap();
    let (km, w) = Keymap::from_str_layers(
        &[(PRESET, Layer::Preset)],
        &reg,
        &FlagRegistry::with_builtin(),
        Os::Windows,
    );
    assert_eq!(w, Vec::new());
    KeyInput::new(km, cfg)
}

fn input() -> KeyInput {
    input_with(&KeymapCfg::default())
}

fn key(key: Key, m: Modifiers, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: m,
    }
}

fn press(k: Key, m: Modifiers) -> Event {
    key(k, m, true)
}

struct Frame {
    events: Vec<Event>,
    cmds: Vec<String>,
    wake_at: Option<Instant>,
}

fn run_at(ki: &mut KeyInput, st: &AppState, now: Instant, events: Vec<Event>) -> Frame {
    let mut raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut out = Vec::new();
    let o = ki.process(&mut raw, st, now, &mut out);
    assert!(out.iter().all(|c| c.origin == urfin_core::Origin::Key));
    Frame {
        events: raw.events,
        cmds: out.iter().map(ToString::to_string).collect(),
        wake_at: o.wake_at,
    }
}

fn run(ki: &mut KeyInput, st: &AppState, events: Vec<Event>) -> Frame {
    run_at(ki, st, Instant::now(), events)
}

/// RU-раскладка: у «ф» нет латинской логической клавиши, egui даёт `key` = физическая A.
#[test]
fn ctrl_letter_on_russian_layout() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let f = run(&mut ki, &st, vec![press(Key::A, Modifiers::CTRL)]);
    assert_eq!(f.cmds, ["sel.all"]);
    assert_eq!(f.events, []);
}

/// `match = "physical"`: сочетание по позиции, даже если логическая клавиша другая.
#[test]
fn physical_match_mode() {
    let cfg = KeymapCfg {
        r#match: MatchMode::Physical,
        ..KeymapCfg::default()
    };
    let mut ki = input_with(&cfg);
    let st = state(Focus::Panel);
    // Dvorak: на позиции A — та же A, на позиции Q — «'».
    let ev = Event::Key {
        key: Key::Quote,
        physical_key: Some(Key::A),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::CTRL,
    };
    assert_eq!(run(&mut ki, &st, vec![ev]).cmds, ["sel.all"]);
}

/// Far: `Alt+ф` → `Key A` + `Text "ф"` → один `keymap.qs.start`, символ не печатается.
#[test]
fn alt_letter_starts_quick_search() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let f = run(
        &mut ki,
        &st,
        vec![
            Event::ModifiersChanged(Modifiers::ALT),
            press(Key::A, Modifiers::ALT),
            Event::Text("ф".into()),
        ],
    );
    assert_eq!(f.cmds, ["keymap.qs.start text=ф"]);
    assert!(!f.events.iter().any(|e| matches!(e, Event::Text(_))));
}

/// Режим `alt_tap` (DN): Alt+буква не начинает поиск — символ уходит в cmdline.
#[test]
fn alt_letter_without_far_mode() {
    let cfg = KeymapCfg {
        preset: "dn".into(),
        ..KeymapCfg::default()
    };
    let mut ki = input_with(&cfg);
    let st = state(Focus::Panel);
    let f = run(
        &mut ki,
        &st,
        vec![press(Key::A, Modifiers::ALT), Event::Text("ф".into())],
    );
    assert_eq!(f.cmds, ["cmdline.insert text=ф"]);
}

/// Внутри быстрого поиска печать → `keymap.qs.input`.
#[test]
fn quick_search_input() {
    let mut ki = input();
    let st = state(Focus::Overlay(OverlayKind::QuickSearch));
    let f = run(&mut ki, &st, vec![Event::Text("x".into())]);
    assert_eq!(f.cmds, ["keymap.qs.input text=x"]);
}

/// Удержание F5 (без автоповтора) → одна команда; удержание Down → по команде на повтор.
#[test]
fn repeat_policy() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let none = Modifiers::NONE;
    let f = run(
        &mut ki,
        &st,
        vec![
            press(Key::F5, none),
            press(Key::F5, none),
            press(Key::F5, none),
        ],
    );
    assert_eq!(f.cmds, ["ops.copy"]);
    let f = run(
        &mut ki,
        &st,
        vec![press(Key::F5, none), key(Key::F5, none, false)],
    );
    assert_eq!(f.cmds, Vec::<String>::new(), "still held");
    assert_eq!(f.events, [], "release of a consumed key is swallowed");
    assert_eq!(
        run(&mut ki, &st, vec![press(Key::F5, none)]).cmds,
        ["ops.copy"]
    );
    let f = run(
        &mut ki,
        &st,
        vec![
            press(Key::ArrowDown, none),
            press(Key::ArrowDown, none),
            press(Key::ArrowDown, none),
        ],
    );
    assert_eq!(f.cmds, ["panel.cursor.down"; 3]);
}

/// Tab в панели — команда, egui его не видит (фокус egui не двигается).
#[test]
fn tab_does_not_reach_egui() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let f = run(
        &mut ki,
        &st,
        vec![
            press(Key::Tab, Modifiers::NONE),
            key(Key::Tab, Modifiers::NONE, false),
        ],
    );
    assert_eq!(f.cmds, ["panel.switch"]);
    assert_eq!(f.events, []);
}

/// Пробел — команда, его `Text` не печатается; прочие символы в панели → cmdline.
#[test]
fn text_after_hit_is_suppressed() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let f = run(
        &mut ki,
        &st,
        vec![
            press(Key::Space, Modifiers::NONE),
            Event::Text(" ".into()),
            press(Key::B, Modifiers::NONE),
            Event::Text("b".into()),
        ],
    );
    assert_eq!(f.cmds, ["sel.toggle", "cmdline.insert text=b"]);
    assert_eq!(f.events, [press(Key::B, Modifiers::NONE)]);
}

/// В cmdline печать и непривязанные клавиши — виджету.
#[test]
fn cmdline_keeps_text() {
    let mut ki = input();
    let st = state(Focus::Cmdline);
    let evs = vec![press(Key::B, Modifiers::NONE), Event::Text("b".into())];
    let f = run(&mut ki, &st, evs.clone());
    assert_eq!(f.cmds, Vec::<String>::new());
    assert_eq!(f.events, evs);
}

/// Во время IME-композиции ничего не матчится; после Commit — снова.
#[test]
fn ime_preedit_passthrough() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let preedit = Event::Ime(ImeEvent::Preedit {
        text: "ㅎ".into(),
        active_range_chars: None,
    });
    let f = run(&mut ki, &st, vec![preedit, press(Key::F5, Modifiers::NONE)]);
    assert_eq!(f.cmds, Vec::<String>::new());
    assert_eq!(f.events.len(), 2);
    let f = run(
        &mut ki,
        &st,
        vec![Event::Ime(ImeEvent::Commit("하".into()))],
    );
    assert_eq!(f.events.len(), 1);
    let f = run(
        &mut ki,
        &st,
        vec![
            key(Key::F5, Modifiers::NONE, false),
            press(Key::F5, Modifiers::NONE),
        ],
    );
    assert_eq!(f.cmds, ["ops.copy"]);
}

/// Аккорд: начало поглощается, дедлайн будит UI; по таймауту сбрасывается.
#[test]
fn chord_and_timeout() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let ctrl = Modifiers::CTRL;
    let t0 = Instant::now();
    let f = run_at(
        &mut ki,
        &st,
        t0,
        vec![press(Key::K, ctrl), key(Key::K, ctrl, false)],
    );
    assert_eq!(f.events, []);
    assert_eq!(f.wake_at, Some(t0 + Duration::from_millis(1500)));
    assert_eq!(ki.pending().unwrap().to_string(), "Ctrl+K");
    let f = run_at(
        &mut ki,
        &st,
        t0 + Duration::from_millis(100),
        vec![press(Key::C, ctrl)],
    );
    assert_eq!(f.cmds, ["a.chord"]);
    assert!(ki.pending().is_none());

    run_at(
        &mut ki,
        &st,
        t0,
        vec![press(Key::K, ctrl), key(Key::K, ctrl, false)],
    );
    let late = t0 + Duration::from_millis(1600);
    let f = run_at(&mut ki, &st, late, vec![]);
    assert_eq!(f.wake_at, None);
    assert!(ki.pending().is_none());
    // Ctrl+C после таймаута — не аккорд (и не привязан: уходит в egui).
    let f = run_at(&mut ki, &st, late, vec![press(Key::C, ctrl)]);
    assert_eq!(f.cmds, Vec::<String>::new());
    assert_eq!(f.events.len(), 1);
}

/// Непривязанное продолжение аккорда сбрасывает его и поглощается.
#[test]
fn chord_miss_resets() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let ctrl = Modifiers::CTRL;
    run(&mut ki, &st, vec![press(Key::K, ctrl)]);
    // Повтор начала аккорда ничего не меняет.
    let f = run(&mut ki, &st, vec![press(Key::K, ctrl)]);
    assert_eq!(f.events, []);
    assert!(ki.pending().is_some());
    let f = run(&mut ki, &st, vec![press(Key::Z, ctrl)]);
    assert_eq!(f.cmds, Vec::<String>::new());
    assert_eq!(f.events, []);
    assert!(ki.pending().is_none());
}

/// Потеря фокуса окна (Alt+Tab) сбрасывает аккорд и нажатые клавиши.
#[test]
fn focus_loss_resets() {
    let mut ki = input();
    let st = state(Focus::Panel);
    run(
        &mut ki,
        &st,
        vec![
            press(Key::K, Modifiers::CTRL),
            press(Key::F5, Modifiers::NONE),
        ],
    );
    run(&mut ki, &st, vec![Event::WindowFocused(false)]);
    assert!(ki.pending().is_none());
    // F5 после возврата — новое нажатие, не повтор.
    let f = run(&mut ki, &st, vec![press(Key::F5, Modifiers::NONE)]);
    assert_eq!(f.cmds, ["ops.copy"]);
}

/// `Paste` без `Key`: сочетание `Ctrl+V`, текст — в аргумент `text`.
#[test]
fn paste_becomes_stroke_with_text() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let f = run(&mut ki, &st, vec![Event::Paste("a b".into())]);
    assert_eq!(f.cmds, [r#"cmdline.paste text="a b""#]);
    assert_eq!(f.events, []);
    // Copy не привязан — остаётся виджетам.
    let f = run(&mut ki, &st, vec![Event::Copy]);
    assert_eq!(f.events, [Event::Copy]);
}

fn note(vk: u16, mods: Mods) -> KeyNote {
    KeyNote {
        vk,
        extended: false,
        mods,
        down: true,
    }
}

/// Записки Windows: `Shift+Ins` с текстом и при пустом буфере (событий нет вовсе).
#[test]
fn notes_restore_shift_insert() {
    let mut ki = input();
    let st = state(Focus::Panel);
    ki.notes().push(note(vk::INSERT, Mods::SHIFT));
    let f = run(&mut ki, &st, vec![Event::Paste("x".into())]);
    assert_eq!(f.cmds, ["cmdline.paste text=x"]);
    ki.notes().push(note(vk::INSERT, Mods::SHIFT));
    let f = run(&mut ki, &st, vec![]);
    assert_eq!(f.cmds, ["cmdline.paste"]);
}

/// Серый минус по записке; `Num5` без `NumLock` — синтетический; без записки — обычный `-`.
#[test]
fn notes_numpad() {
    let mut ki = input();
    let st = state(Focus::Panel);
    ki.notes().push(note(vk::SUBTRACT, Mods::empty()));
    let f = run(
        &mut ki,
        &st,
        vec![press(Key::Minus, Modifiers::NONE), Event::Text("-".into())],
    );
    assert_eq!(f.cmds, ["a.gray"]);
    assert_eq!(f.events, []);
    let f = run(
        &mut ki,
        &st,
        vec![
            key(Key::Minus, Modifiers::NONE, false),
            press(Key::Minus, Modifiers::NONE),
            Event::Text("-".into()),
        ],
    );
    assert_eq!(f.cmds, ["cmdline.insert text=-"]);
    ki.notes().push(note(vk::CLEAR, Mods::empty()));
    assert_eq!(run(&mut ki, &st, vec![]).cmds, ["a.center"]);
}

/// Сторона модификатора — из записки хука: `RCtrl+1`.
#[test]
fn modifier_side_from_notes() {
    let mut ki = input();
    let st = state(Focus::Panel);
    ki.notes().push(KeyNote {
        vk: vk::CONTROL,
        extended: true,
        mods: Mods::CTRL | Mods::RCTRL,
        down: true,
    });
    let f = run(
        &mut ki,
        &st,
        vec![
            Event::ModifiersChanged(Modifiers::CTRL),
            press(Key::Num1, Modifiers::CTRL),
        ],
    );
    assert_eq!(f.cmds, ["a.right"]);
    // Ctrl отпущен — сторона забыта.
    run(&mut ki, &st, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    let f = run(&mut ki, &st, vec![press(Key::Num2, Modifiers::CTRL)]);
    assert_eq!(f.cmds, Vec::<String>::new());
}

/// Shift+цифра: символ (`!`) у egui — отдельная клавиша, сочетание — по цифре.
#[test]
fn shift_digit_by_position() {
    let mut ki = input();
    let st = state(Focus::Panel);
    let ev = Event::Key {
        key: Key::Exclamationmark,
        physical_key: Some(Key::Num1),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    };
    let f = run(&mut ki, &st, vec![ev, Event::Text("!".into())]);
    assert_eq!(f.cmds, ["a.digit"]);
    assert_eq!(f.events, []);
}

/// Оверлей: Esc → `ui.cancel`; прочее — виджетам оверлея.
#[test]
fn overlay_escape() {
    let mut ki = input();
    let st = state(Focus::Overlay(OverlayKind::Menu));
    let f = run(
        &mut ki,
        &st,
        vec![
            press(Key::Escape, Modifiers::NONE),
            press(Key::ArrowDown, Modifiers::NONE),
        ],
    );
    assert_eq!(f.cmds, ["ui.cancel"]);
    assert_eq!(f.events, [press(Key::ArrowDown, Modifiers::NONE)]);
}

/// Новая раскладка сбрасывает начатый аккорд.
#[test]
fn set_keymap_resets_chord() {
    let mut ki = input();
    let st = state(Focus::Panel);
    run(&mut ki, &st, vec![press(Key::K, Modifiers::CTRL)]);
    let other = input();
    ki.set_keymap(Arc::clone(&other.km));
    assert!(ki.pending().is_none());
}

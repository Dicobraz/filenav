//! Клавиатура GPUI → конвейер keymap S3 (G-03). Нажатия перехватываются на уровне
//! приложения (`intercept_keystrokes`, до привязок gpui-kit) и переводятся в события
//! `urfin_keymap::ev::RawInput`, которые разбирает [`KeyInput::process`]: пресеты, аккорды,
//! раскладка-независимость и быстрый поиск работают как в egui-окне.
//!
//! На Windows subclass окна (`sys::key_notes`) пишет записки `KeyNotes` в очередь этого
//! же `KeyInput` — их разбирает каждый `process`; записки без нажатия gpui (`Num5` без
//! `NumLock`) — [`Input::flush`] по будильнику subclass.

use std::time::Instant;

use gpui_kit::{Keystroke, Modifiers};
use urfin_core::{AppState, Command};
use urfin_keymap::ev::{Event, Key, Modifiers as EMods, RawInput};
use urfin_keymap::{ActiveKeymap, KeyInput, KeyNotes};

/// Состояние моста: конвейер keymap и последние модификаторы.
pub(super) struct Input {
    keys: KeyInput,
    /// Очередь записок `keys` (её же пишет subclass окна).
    notes: KeyNotes,
    mods: EMods,
}

/// Итог нажатия.
pub(super) struct Outcome {
    /// keymap забрал событие: дальше по gpui не пускать.
    pub consumed: bool,
    /// Дедлайн аккорда: к нему надо разбудить `pump`.
    pub wake_at: Option<Instant>,
}

impl Input {
    pub(super) fn new(keys: KeyInput) -> Self {
        Input {
            notes: keys.notes(),
            keys,
            mods: EMods::NONE,
        }
    }

    /// Очередь записок для subclass окна (`sys::key_notes`).
    pub(super) fn notes(&self) -> KeyNotes {
        self.notes.clone()
    }

    /// Нажатие разобрал не конвейер (поле ввода, меню, вопрос): его записки — сбросить.
    pub(super) fn skip(&self) {
        self.notes.clear();
    }

    /// Перечитанная раскладка (`keymap.reload`, `app.reload_config`) — в конвейер.
    fn sync(&mut self, st: &AppState) {
        if let Some(a) = st.ext.get::<ActiveKeymap>()
            && a.generation != self.keys.generation()
        {
            self.keys
                .apply(std::sync::Arc::clone(&a.keymap), &a.cfg, a.generation);
        }
    }

    fn run(&mut self, events: Vec<Event>, st: &AppState, out: &mut Vec<Command>) -> Outcome {
        self.sync(st);
        let n = events.len();
        let before = out.len();
        let mut raw = RawInput { events };
        let now = Instant::now();
        let o = self.keys.process(&mut raw, st, now, out);
        Outcome {
            // Без событий (Ctrl+Gray*) сочетание дала записка: поглощено, если есть команды.
            consumed: raw.events.len() < n || (n == 0 && out.len() > before),
            wake_at: o.wake_at,
        }
    }

    /// Записки subclass, не встретившие нажатия gpui, — в конвейер (синтетические
    /// сочетания `Num5`, стороны модификаторов).
    pub(super) fn flush(&mut self, st: &AppState, out: &mut Vec<Command>) -> Outcome {
        self.run(Vec::new(), st, out)
    }

    /// Нажатие: клавиша (логическая и физическая — из кода клавиши gpui, он не зависит от
    /// раскладки) и введённый символ.
    pub(super) fn key_down(
        &mut self,
        ks: &Keystroke,
        st: &AppState,
        out: &mut Vec<Command>,
    ) -> Outcome {
        self.mods = mods(ks.modifiers);
        let mut events = Vec::with_capacity(2);
        if let Some(key) = key_of(&ks.key) {
            events.push(Event::Key {
                key,
                physical_key: physical_of(key, down_vk()),
                pressed: true,
                repeat: false,
                modifiers: self.mods,
            });
        }
        // Символ — только без Ctrl/Alt (AltGr на Windows — Ctrl+Alt: это символ).
        let altgr = ks.modifiers.control && ks.modifiers.alt;
        if let Some(text) = ks.key_char.as_deref()
            && !text.chars().any(char::is_control)
            && (altgr || !(ks.modifiers.control || ks.modifiers.alt))
        {
            events.push(Event::Text(text.to_owned()));
        }
        // Пусто (Ctrl+Gray*: у gpui нет клавиши `*`, символа при Ctrl нет) — всё равно
        // `process`: сочетание даст записка subclass.
        self.run(events, st, out)
    }

    pub(super) fn key_up(&mut self, ks: &Keystroke, st: &AppState, out: &mut Vec<Command>) {
        self.mods = mods(ks.modifiers);
        if let Some(key) = key_of(&ks.key) {
            let ev = Event::Key {
                key,
                physical_key: Some(key),
                pressed: false,
                repeat: false,
                modifiers: self.mods,
            };
            self.run(vec![ev], st, out);
        }
    }

    /// Модификаторы без клавиши: одиночный Alt (меню F9), аккорды на Ctrl.
    pub(super) fn modifiers(
        &mut self,
        m: Modifiers,
        st: &AppState,
        out: &mut Vec<Command>,
    ) -> Outcome {
        self.mods = mods(m);
        self.run(vec![Event::ModifiersChanged(self.mods)], st, out)
    }

    /// Окно потеряло фокус: сбросить аккорд и зажатые модификаторы.
    pub(super) fn focus_lost(&mut self, st: &AppState, out: &mut Vec<Command>) {
        self.mods = EMods::NONE;
        self.run(vec![Event::WindowFocused(false)], st, out);
    }
}

fn mods(m: Modifiers) -> EMods {
    EMods {
        alt: m.alt,
        ctrl: m.control,
        shift: m.shift,
        mac_cmd: false,
        command: m.control,
    }
}

/// Код клавиши (`VK_*`) нажатия, которое сейчас разбирает gpui (Windows).
fn down_vk() -> Option<u16> {
    #[cfg(windows)]
    return crate::sys::key_notes::key_down_vk();
    #[cfg(not(windows))]
    None
}

/// Физическая клавиша для keymap. `Plus` в keymap — серый `+` (S3-07, `GrayPlus`);
/// gpui отдаёт `"+"` и для Shift+= основного ряда — его выдаёт код клавиши не `VK_ADD`.
/// Кода нет (subclass не стоит) — как раньше, по имени.
fn physical_of(key: Key, vk: Option<u16>) -> Option<Key> {
    match vk {
        Some(v) if key == Key::Plus && v != urfin_keymap::input::vk::ADD => None,
        _ => Some(key),
    }
}

/// Имя клавиши gpui (`"down"`, `"pagedown"`, `"a"`, `"f5"`, `"-"`) → клавиша keymap.
pub(super) fn key_of(name: &str) -> Option<Key> {
    let named = match name {
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "home" => "Home",
        "end" => "End",
        "enter" => "Enter",
        "escape" => "Escape",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "insert" => "Insert",
        "delete" => "Delete",
        "space" => "Space",
        _ => "",
    };
    if !named.is_empty() {
        return Key::from_name(named);
    }
    // F1…F35.
    if let Some(n) = name.strip_prefix('f')
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        return Key::from_name(&format!("F{n}"));
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            Key::from_name(&c.to_ascii_uppercase().to_string())
        }
        (Some(c), None) => Key::from_name(&c.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpui_names_map_to_egui_keys() {
        for (g, k) in [
            ("down", Key::ArrowDown),
            ("pagedown", Key::PageDown),
            ("enter", Key::Enter),
            ("tab", Key::Tab),
            ("insert", Key::Insert),
            ("space", Key::Space),
            ("f5", Key::F5),
            ("f10", Key::F10),
            ("a", Key::A),
            ("q", Key::Q),
            ("7", Key::Num7),
            ("-", Key::Minus),
            ("=", Key::Equals),
            ("[", Key::OpenBracket),
            ("]", Key::CloseBracket),
            ("\\", Key::Backslash),
        ] {
            assert_eq!(key_of(g), Some(k), "{g}");
        }
        assert_eq!(key_of("shift"), None);
    }

    /// Серые клавиши gpui называет как основной ряд; различает их записка subclass.
    /// Серый `+` — по коду клавиши: `"+"` основного ряда (Shift+=) не `GrayPlus`.
    #[test]
    fn numpad_names_and_gray_plus() {
        for (g, k) in [
            ("-", Key::Minus),
            ("/", Key::Slash),
            ("+", Key::Plus),
            ("enter", Key::Enter),
            ("delete", Key::Delete),
        ] {
            assert_eq!(key_of(g), Some(k), "{g}");
        }
        let add = urfin_keymap::input::vk::ADD;
        assert_eq!(physical_of(Key::Plus, Some(add)), Some(Key::Plus));
        assert_eq!(physical_of(Key::Plus, Some(0xBB)), None, "VK_OEM_PLUS");
        assert_eq!(physical_of(Key::Plus, None), Some(Key::Plus));
        assert_eq!(physical_of(Key::Minus, Some(0xBD)), Some(Key::Minus));
    }
}

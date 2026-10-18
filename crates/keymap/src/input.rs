//! Конвейер ввода (S3 §1.2): события `RawInput` кадра → сочетания → команды.
//!
//! [`KeyInput::process`] вызывается в `App::raw_input_hook`: поглощённые события
//! удаляются, egui видит только остаток. Повторы считаем сами: egui помечает `repeat`
//! уже после хука. Записки [`KeyNotes`] (Windows, S3-07) уточняют то, что egui-winit
//! теряет: цифровой блок, `Ctrl+Ins`/`Shift+Ins`/`Shift+Del`, стороны модификаторов.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Event, ImeEvent, Key};
use smallvec::SmallVec;
use urfin_core::{AppState, Command, Focus, Origin, OverlayKind};

use crate::{
    Chord, ChordState, KeyCode, Keymap, KeymapCfg, Lookup, MatchMode, Mods, Os, QsMode, Stroke,
};

/// Коды клавиш Windows, которые пишет хук (S3 §0, S3-07).
pub mod vk {
    pub const CLEAR: u16 = 0x0C;
    pub const RETURN: u16 = 0x0D;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const MULTIPLY: u16 = 0x6A;
    pub const ADD: u16 = 0x6B;
    pub const SUBTRACT: u16 = 0x6D;
    pub const DIVIDE: u16 = 0x6F;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4;
    pub const RMENU: u16 = 0xA5;
}

/// Записка хука: виртуальная клавиша, флаг extended и модификаторы со сторонами.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyNote {
    pub vk: u16,
    pub extended: bool,
    pub mods: Mods,
    pub down: bool,
}

/// Clone + Send. Пишет хук Windows, читает [`KeyInput`].
#[derive(Clone, Debug, Default)]
pub struct KeyNotes(Arc<Mutex<VecDeque<KeyNote>>>);

impl KeyNotes {
    /// Очередь ограничена: если кадры не идут, старые записки не копятся.
    const CAP: usize = 64;

    pub fn push(&self, n: KeyNote) {
        let mut q = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if q.len() == Self::CAP {
            q.pop_front();
        }
        q.push_back(n);
    }

    fn drain_into(&self, out: &mut Vec<(KeyNote, bool)>) {
        let mut q = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        out.extend(q.drain(..).map(|n| (n, false)));
    }
}

/// Итог кадра.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct InputOutcome {
    /// Когда разбудить UI: дедлайн аккорда.
    pub wake_at: Option<Instant>,
}

/// Модификаторы: общие — из egui, стороны (`LCtrl`, `RAlt`…) — из записок хука.
#[derive(Debug, Default)]
struct ModTracker {
    held: Mods,
    sides: Mods,
}

impl ModTracker {
    fn set(&mut self, m: egui::Modifiers) {
        self.held = mods_of(m);
        if !self.held.contains(Mods::CTRL) {
            self.sides.remove(Mods::LCTRL | Mods::RCTRL);
        }
        if !self.held.contains(Mods::ALT) {
            self.sides.remove(Mods::LALT | Mods::RALT);
        }
    }

    /// Записка модификатора обновляет стороны; `false` — записка не о модификаторе.
    fn note(&mut self, n: KeyNote) -> bool {
        let side = match (n.vk, n.extended) {
            (vk::LCONTROL, _) | (vk::CONTROL, false) => Mods::LCTRL,
            (vk::RCONTROL, _) | (vk::CONTROL, true) => Mods::RCTRL,
            (vk::LMENU, _) | (vk::MENU, false) => Mods::LALT,
            (vk::RMENU, _) | (vk::MENU, true) => Mods::RALT,
            _ => return false,
        };
        self.sides.set(side, n.down);
        true
    }

    fn with_sides(&self, m: Mods) -> Mods {
        let mut sides = self.sides;
        if !m.contains(Mods::CTRL) {
            sides.remove(Mods::LCTRL | Mods::RCTRL);
        }
        if !m.contains(Mods::ALT) {
            sides.remove(Mods::LALT | Mods::RALT);
        }
        m | sides
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn mods_of(m: egui::Modifiers) -> Mods {
    let mut out = Mods::empty();
    out.set(Mods::CTRL, m.ctrl);
    out.set(Mods::ALT, m.alt);
    out.set(Mods::SHIFT, m.shift);
    out.set(Mods::CMD, m.mac_cmd);
    out
}

/// Нажатая клавиша: прошло ли хоть одно её нажатие в egui (тогда и отпускание — туда).
#[derive(Copy, Clone, Debug)]
struct Down {
    key: Key,
    passed: bool,
}

#[derive(Debug)]
/// Конвейер ввода: keymap, модификаторы, аккорд, IME, `suppress_text`, записки.
pub struct KeyInput {
    km: Arc<Keymap>,
    os: Os,
    r#match: MatchMode,
    /// Быстрый поиск Far: `Alt+буква` при фокусе на панели.
    qs_alt: bool,
    chord_timeout: Duration,
    mods: ModTracker,
    pending: Option<(ChordState, Instant)>,
    ime: bool,
    suppress_text: bool,
    down: SmallVec<[Down; 8]>,
    notes: KeyNotes,
    frame_notes: Vec<(KeyNote, bool)>,
}

impl KeyInput {
    pub fn new(km: Arc<Keymap>, cfg: &KeymapCfg) -> Self {
        let qs_alt = match cfg.quick_search {
            QsMode::Alt => true,
            QsMode::Auto => cfg.preset != "dn",
            QsMode::AltTap | QsMode::CtrlTap | QsMode::Off => false,
        };
        KeyInput {
            os: km.os(),
            km,
            r#match: cfg.r#match,
            qs_alt,
            chord_timeout: Duration::from_millis(u64::from(cfg.chord_timeout_ms)),
            mods: ModTracker::default(),
            pending: None,
            ime: false,
            suppress_text: false,
            down: SmallVec::new(),
            notes: KeyNotes::default(),
            frame_notes: Vec::new(),
        }
    }

    /// Очередь записок для хука Windows.
    pub fn notes(&self) -> KeyNotes {
        self.notes.clone()
    }

    /// Новая раскладка (`keymap.reload`): незавершённый аккорд сбрасывается.
    pub fn set_keymap(&mut self, km: Arc<Keymap>) {
        self.os = km.os();
        self.km = km;
        self.pending = None;
    }

    /// Начатый аккорд (строка статуса, `KeymapView.pending` — S3-06).
    pub fn pending(&self) -> Option<Chord> {
        self.pending.as_ref().map(|(c, _)| Chord(c.strokes.clone()))
    }

    /// Разбирает события кадра. Поглощённые удаляются из `raw.events`, команды
    /// (`Origin::Key`) добавляются в `out`.
    pub fn process(
        &mut self,
        raw: &mut egui::RawInput,
        st: &AppState,
        now: Instant,
        out: &mut Vec<Command>,
    ) -> InputOutcome {
        self.frame_notes.clear();
        self.notes.drain_into(&mut self.frame_notes);
        for (n, used) in &mut self.frame_notes {
            // Отпускание модификатора тоже обновляет стороны.
            let modifier = self.mods.note(*n);
            *used = modifier || !n.down;
        }
        if self.pending.as_ref().is_some_and(|(_, d)| now >= *d) {
            self.pending = None;
        }
        let events = std::mem::take(&mut raw.events);
        raw.events.reserve(events.len());
        for ev in events {
            if self.event(&ev, st, now, out) {
                raw.events.push(ev);
            }
        }
        self.unpaired_notes(raw, st, now, out);
        InputOutcome {
            wake_at: self.pending.as_ref().map(|(_, d)| *d),
        }
    }

    /// `true` — событие остаётся для egui.
    fn event(&mut self, ev: &Event, st: &AppState, now: Instant, out: &mut Vec<Command>) -> bool {
        match ev {
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                self.ime = !text.is_empty();
                return true;
            }
            Event::Ime(ImeEvent::Commit(_)) => {
                self.ime = false;
                return true;
            }
            Event::WindowFocused(false) => {
                self.mods.reset();
                self.pending = None;
                self.suppress_text = false;
                self.down.clear();
                return true;
            }
            Event::ModifiersChanged(m) => {
                self.mods.set(*m);
                return true;
            }
            _ => {}
        }
        if self.ime {
            return true;
        }
        match ev {
            Event::Key {
                key,
                physical_key,
                pressed,
                modifiers,
                ..
            } => {
                self.mods.set(*modifiers);
                if *pressed {
                    self.press(*key, *physical_key, *modifiers, st, now, out)
                } else {
                    self.release(*key)
                }
            }
            Event::Copy | Event::Cut | Event::Paste(_) => {
                self.suppress_text = false;
                let (vk, letter) = match ev {
                    Event::Copy => (vk::INSERT, Key::C),
                    Event::Cut => (vk::DELETE, Key::X),
                    _ => (vk::INSERT, Key::V),
                };
                let stroke = if let Some(n) = self.pair(vk) {
                    let key = if vk == vk::INSERT {
                        Key::Insert
                    } else {
                        Key::Delete
                    };
                    Stroke::new(KeyCode::Key(key), n.mods)
                } else {
                    let m = if self.os == Os::Macos {
                        Mods::CMD
                    } else {
                        Mods::CTRL
                    };
                    Stroke::new(KeyCode::Key(letter), self.mods.with_sides(m))
                };
                let text = match ev {
                    Event::Paste(t) => Some(t.as_str()),
                    _ => None,
                };
                !self.feed(stroke, false, st, now, out, text)
            }
            Event::Text(t) => self.text(t, st, now, out),
            _ => true,
        }
    }

    fn press(
        &mut self,
        key: Key,
        physical: Option<Key>,
        modifiers: egui::Modifiers,
        st: &AppState,
        now: Instant,
        out: &mut Vec<Command>,
    ) -> bool {
        self.suppress_text = false;
        let i = self.down.iter().position(|d| d.key == key);
        let repeat = i.is_some();
        let code = self.code(key, physical);
        let stroke = Stroke::new(code, self.mods.with_sides(mods_of(modifiers)));
        let pass = !self.feed(stroke, repeat, st, now, out, None);
        match i {
            Some(i) => self.down[i].passed |= pass,
            None => self.down.push(Down { key, passed: pass }),
        }
        pass
    }

    fn release(&mut self, key: Key) -> bool {
        match self.down.iter().position(|d| d.key == key) {
            Some(i) => self.down.swap_remove(i).passed,
            None => true,
        }
    }

    /// Клавиша события, уточнённая запиской хука.
    fn code(&mut self, key: Key, physical: Option<Key>) -> KeyCode {
        if physical == Some(Key::Plus) {
            self.pair(vk::ADD);
            return KeyCode::GrayPlus;
        }
        let key = match (self.r#match, physical) {
            (MatchMode::Physical, Some(p)) => p,
            // Shift+цифра даёт символ (`!`, `№`): сочетание — по цифре (S3 §1.1 №2).
            (MatchMode::Auto, Some(p)) if is_digit(p) && !is_digit(key) => p,
            _ => key,
        };
        let own = match key {
            Key::Minus => (vk::SUBTRACT, KeyCode::GrayMinus),
            Key::Slash => (vk::DIVIDE, KeyCode::GraySlash),
            Key::Num5 => (vk::CLEAR, KeyCode::Num5),
            Key::Enter => (vk::RETURN, KeyCode::NumEnter),
            Key::Insert => (vk::INSERT, KeyCode::Key(key)),
            Key::Delete => (vk::DELETE, KeyCode::Key(key)),
            _ => return KeyCode::Key(key),
        };
        let found = if own.0 == vk::RETURN {
            self.pair_with(|n| n.vk == vk::RETURN && n.extended)
        } else {
            self.pair(own.0)
        };
        if found.is_some() {
            own.1
        } else {
            KeyCode::Key(key)
        }
    }

    fn pair(&mut self, vk: u16) -> Option<KeyNote> {
        self.pair_with(|n| n.vk == vk)
    }

    /// Первая неиспользованная записка нажатия, подходящая под `f`.
    fn pair_with(&mut self, f: impl Fn(&KeyNote) -> bool) -> Option<KeyNote> {
        let (n, used) = self
            .frame_notes
            .iter_mut()
            .find(|(n, used)| !*used && f(n))?;
        *used = true;
        Some(*n)
    }

    fn text(&mut self, t: &str, st: &AppState, now: Instant, out: &mut Vec<Command>) -> bool {
        if self.suppress_text {
            return false;
        }
        // Серая `*`: у egui нет `Key`, только `Text` — сочетание по записке.
        if t == "*"
            && let Some(n) = self.pair(vk::MULTIPLY)
            && self.feed(
                Stroke::new(KeyCode::GrayStar, n.mods),
                false,
                st,
                now,
                out,
                None,
            )
        {
            return false;
        }
        let id = match st.focus {
            Focus::Overlay(OverlayKind::QuickSearch) => "keymap.qs.input",
            Focus::Panel
                if self.qs_alt
                    && self.mods.held.contains(Mods::ALT)
                    && !self.mods.held.intersects(Mods::CTRL | Mods::CMD) =>
            {
                "keymap.qs.start"
            }
            Focus::Panel => "cmdline.insert",
            _ => return true,
        };
        if !self.km.has_command(id) {
            return true;
        }
        out.push(Command::new(id).arg("text", t).from(Origin::Key));
        false
    }

    /// Lookup сочетания. `true` — поглощено (команда, начало или сброс аккорда, повтор
    /// без автоповтора).
    fn feed(
        &mut self,
        stroke: Stroke,
        repeat: bool,
        st: &AppState,
        now: Instant,
        out: &mut Vec<Command>,
        text: Option<&str>,
    ) -> bool {
        // Повтор при начатом аккорде ничего не меняет (S3 §1.1 №11).
        if repeat && self.pending.is_some() {
            return true;
        }
        let pending = self.pending.as_ref().map(|(c, _)| c);
        match self.km.lookup(pending, stroke, st) {
            Lookup::Hit(cmds, repeatable) => {
                self.suppress_text = true;
                if repeat && !repeatable {
                    return true;
                }
                self.pending = None;
                for c in cmds {
                    let mut c = c.clone().from(Origin::Key);
                    if let Some(t) = text
                        && c.args.get("text").is_none()
                        && self.km.takes_text(c.id.as_str())
                    {
                        c = c.arg("text", t);
                    }
                    out.push(c);
                }
                true
            }
            Lookup::Prefix => {
                if !repeat {
                    let mut chord = pending.cloned().unwrap_or_default();
                    chord.strokes.push(stroke);
                    self.pending = Some((chord, now + self.chord_timeout));
                }
                true
            }
            Lookup::Miss => self.pending.take().is_some(),
        }
    }

    /// Записка без события egui (`Num5` без `NumLock`, `Shift+Ins` при пустом буфере,
    /// `Ctrl+GrayStar`) — синтетическое сочетание.
    fn unpaired_notes(
        &mut self,
        raw: &mut egui::RawInput,
        st: &AppState,
        now: Instant,
        out: &mut Vec<Command>,
    ) {
        if self.ime {
            return;
        }
        let mut i = 0;
        while i < self.frame_notes.len() {
            let (n, used) = self.frame_notes[i];
            i += 1;
            if used {
                continue;
            }
            let (code, ch) = match n.vk {
                vk::CLEAR => (KeyCode::Num5, None),
                vk::MULTIPLY => (KeyCode::GrayStar, Some("*")),
                vk::ADD => (KeyCode::GrayPlus, Some("+")),
                vk::SUBTRACT => (KeyCode::GrayMinus, Some("-")),
                vk::DIVIDE => (KeyCode::GraySlash, Some("/")),
                vk::RETURN if n.extended => (KeyCode::NumEnter, None),
                vk::INSERT => (KeyCode::Key(Key::Insert), None),
                vk::DELETE => (KeyCode::Key(Key::Delete), None),
                _ => continue,
            };
            if !self.feed(Stroke::new(code, n.mods), false, st, now, out, None)
                && let Some(ch) = ch
                && !n.mods.intersects(Mods::CTRL | Mods::ALT | Mods::CMD)
                && matches!(st.focus, Focus::Cmdline | Focus::Editor)
            {
                raw.events.push(Event::Text(ch.into()));
            }
        }
    }
}

fn is_digit(k: Key) -> bool {
    matches!(
        k,
        Key::Num0
            | Key::Num1
            | Key::Num2
            | Key::Num3
            | Key::Num4
            | Key::Num5
            | Key::Num6
            | Key::Num7
            | Key::Num8
            | Key::Num9
    )
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

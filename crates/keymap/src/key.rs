//! Клавиши, модификаторы, сочетания и аккорды (S3 §2.3, §5 «типы»).
//!
//! Грамматика: `[Mod+]*Key`, аккорд — сочетания через пробел, регистр не важен.
//! Каноническая форма для показа — `Ctrl+Alt+Shift+Cmd+Key`; `parse(display(x)) == x`.
//! Имена клавиш — `egui::Key::name()` (S0 v3 №13) и наши: `GrayPlus` (серый `+`
//! цифрового блока) отличается от `Plus`, `Num5` (центр цифрового блока) — от `5`.

use std::fmt;

use compact_str::CompactString;
use smallvec::SmallVec;

/// Клавиша: из egui или своя (цифровой блок, одиночное нажатие Alt/Ctrl).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum KeyCode {
    Key(egui::Key),
    GrayPlus,
    GrayMinus,
    GrayStar,
    GraySlash,
    Num5,
    NumEnter,
    /// Одиночное нажатие и отпускание Alt (S3 §1.4).
    AltTap,
    CtrlTap,
}

/// Свои клавиши: имя и код.
const OWN: [(&str, KeyCode); 8] = [
    ("GrayPlus", KeyCode::GrayPlus),
    ("GrayMinus", KeyCode::GrayMinus),
    ("GrayStar", KeyCode::GrayStar),
    ("GraySlash", KeyCode::GraySlash),
    ("Num5", KeyCode::Num5),
    ("NumEnter", KeyCode::NumEnter),
    ("AltTap", KeyCode::AltTap),
    ("CtrlTap", KeyCode::CtrlTap),
];

impl KeyCode {
    /// Каноническое имя.
    pub fn name(self) -> &'static str {
        match self {
            KeyCode::Key(egui::Key::Escape) => "Esc",
            KeyCode::Key(k) => k.name(),
            own => OWN.iter().find(|(_, c)| *c == own).map_or("?", |(n, _)| n),
        }
    }

    /// Имя без учёта регистра: свои, имена и синонимы egui (`Esc`, `Return`, `[`…).
    pub fn from_name(s: &str) -> Option<Self> {
        if let Some((_, c)) = OWN.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)) {
            return Some(*c);
        }
        if let Some(k) = egui::Key::from_name(s) {
            return Some(KeyCode::Key(k));
        }
        // egui сравнивает с учётом регистра: `esc`, `pageup`, `f5`.
        egui::Key::ALL
            .iter()
            .find(|k| k.name().eq_ignore_ascii_case(s))
            .or_else(|| {
                ALIASES
                    .iter()
                    .find(|(a, _)| a.eq_ignore_ascii_case(s))
                    .map(|(_, k)| k)
            })
            .map(|k| KeyCode::Key(*k))
    }
}

/// Синонимы egui, чьё каноническое имя другое (без учёта регистра).
const ALIASES: [(&str, egui::Key); 8] = [
    ("Esc", egui::Key::Escape),
    ("Escape", egui::Key::Escape),
    ("Return", egui::Key::Enter),
    ("ArrowUp", egui::Key::ArrowUp),
    ("ArrowDown", egui::Key::ArrowDown),
    ("ArrowLeft", egui::Key::ArrowLeft),
    ("ArrowRight", egui::Key::ArrowRight),
    ("Del", egui::Key::Delete),
];

bitflags::bitflags! {
    /// Модификаторы. Сторона (`LCTRL`…) — только если указана в привязке; без стороны —
    /// любая.
    #[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
    pub struct Mods: u16 {
        const CTRL = 1;
        const ALT = 2;
        const SHIFT = 4;
        const CMD = 8;
        const LCTRL = 16;
        const RCTRL = 32;
        const LALT = 64;
        const RALT = 128;
    }
}

/// ОС: `Mod` — Ctrl на Windows и Linux, Cmd на macOS.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum Os {
    Windows,
    Macos,
    Linux,
}

impl Os {
    /// ОС сборки.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Порядок модификаторов в канонической форме: группа и её флаги (без стороны —
/// первым).
const MOD_NAMES: [(Mods, &str); 8] = [
    (Mods::CTRL, "Ctrl"),
    (Mods::LCTRL, "LCtrl"),
    (Mods::RCTRL, "RCtrl"),
    (Mods::ALT, "Alt"),
    (Mods::LALT, "LAlt"),
    (Mods::RALT, "RAlt"),
    (Mods::SHIFT, "Shift"),
    (Mods::CMD, "Cmd"),
];

/// Модификатор по имени (без учёта регистра).
fn modifier(s: &str, os: Os) -> Option<Mods> {
    let m = match s.to_ascii_lowercase().as_str() {
        "mod" if os == Os::Macos => Mods::CMD,
        "ctrl" | "control" | "mod" => Mods::CTRL,
        "lctrl" => Mods::LCTRL,
        "rctrl" => Mods::RCTRL,
        "alt" | "option" => Mods::ALT,
        "lalt" => Mods::LALT,
        "ralt" => Mods::RALT,
        "shift" => Mods::SHIFT,
        "cmd" | "meta" | "super" => Mods::CMD,
        _ => return None,
    };
    Some(m)
}

/// Ошибка разбора с позицией (байт в исходной строке).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("at {pos}: {kind}")]
pub struct KeyError {
    pub pos: usize,
    pub kind: KeyErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyErrorKind {
    #[error("empty key")]
    Empty,
    #[error("unknown key '{0}'")]
    UnknownKey(CompactString),
    #[error("'{0}' is not a modifier (only the last part is a key)")]
    NotModifier(CompactString),
    #[error("modifier '{0}' repeats")]
    Repeated(CompactString),
    #[error("'{0}' conflicts with another side of the same modifier")]
    Sides(CompactString),
}

/// Сочетание: клавиша и модификаторы.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Stroke {
    pub key: KeyCode,
    pub mods: Mods,
}

impl Stroke {
    pub fn new(key: KeyCode, mods: Mods) -> Self {
        Stroke { key, mods }
    }

    /// `Ctrl+Shift+T`, `GrayPlus`, `Mod+O`.
    pub fn parse(s: &str, os: Os) -> Result<Self, KeyError> {
        parse_at(s, 0, os)
    }

    /// Каноническая форма. `os` — на будущее (имена клавиш Mac); сейчас одна форма.
    pub fn display(&self, _os: Os) -> CompactString {
        let mut out = CompactString::default();
        for (m, name) in MOD_NAMES {
            if self.mods.contains(m) {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(self.key.name());
        out
    }
}

impl fmt::Display for Stroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(Os::current()))
    }
}

/// Разбор сочетания `s`, начинающегося в исходной строке с байта `base`.
fn parse_at(s: &str, base: usize, os: Os) -> Result<Stroke, KeyError> {
    let err = |pos: usize, kind| KeyError {
        pos: base + pos,
        kind,
    };
    if s.trim().is_empty() {
        return Err(err(0, KeyErrorKind::Empty));
    }
    let parts: SmallVec<[(usize, &str); 6]> = split_plus(s);
    let (last_pos, last) = *parts.last().expect("non-empty");
    let mut mods = Mods::empty();
    for &(pos, part) in &parts[..parts.len() - 1] {
        if part.is_empty() {
            return Err(err(pos, KeyErrorKind::Empty));
        }
        let Some(m) = modifier(part, os) else {
            return Err(err(pos, KeyErrorKind::NotModifier(part.into())));
        };
        if mods.contains(m) {
            return Err(err(pos, KeyErrorKind::Repeated(part.into())));
        }
        let group = |g: Mods| !(mods & g).is_empty() && !(m & g).is_empty();
        if group(Mods::CTRL | Mods::LCTRL | Mods::RCTRL)
            || group(Mods::ALT | Mods::LALT | Mods::RALT)
        {
            return Err(err(pos, KeyErrorKind::Sides(part.into())));
        }
        mods |= m;
    }
    if last.is_empty() {
        return Err(err(last_pos, KeyErrorKind::Empty));
    }
    let key = KeyCode::from_name(last)
        .ok_or_else(|| err(last_pos, KeyErrorKind::UnknownKey(last.into())))?;
    Ok(Stroke { key, mods })
}

/// Части `a+b+c` с позициями. `+` как клавиша — `Plus` (S3 §2.3); одиночный `+` —
/// тоже `Plus`, чтобы `Ctrl+` было ошибкой, а `+` — клавишей.
fn split_plus(s: &str) -> SmallVec<[(usize, &str); 6]> {
    if s == "+" {
        let mut v = SmallVec::new();
        v.push((0, s));
        return v;
    }
    let mut out = SmallVec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == '+' {
            out.push((start, &s[start..i]));
            start = i + 1;
        }
    }
    out.push((start, &s[start..]));
    out
}

/// Аккорд: сочетания по очереди (`Ctrl+K Ctrl+C`).
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Chord(pub SmallVec<[Stroke; 3]>);

impl Chord {
    pub fn parse(s: &str, os: Os) -> Result<Self, KeyError> {
        let mut strokes = SmallVec::new();
        let mut i = 0;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i].is_ascii_whitespace() {
                i += 1;
                continue;
            }
            let end = s[i..]
                .find(|c: char| c.is_whitespace())
                .map_or(s.len(), |e| i + e);
            strokes.push(parse_at(&s[i..end], i, os)?);
            i = end;
        }
        if strokes.is_empty() {
            return Err(KeyError {
                pos: 0,
                kind: KeyErrorKind::Empty,
            });
        }
        Ok(Chord(strokes))
    }

    pub fn display(&self, os: Os) -> CompactString {
        let mut out = CompactString::default();
        for (i, s) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&s.display(os));
        }
        out
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display(Os::current()))
    }
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod tests;

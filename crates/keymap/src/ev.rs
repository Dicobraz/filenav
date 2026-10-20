//! Вход конвейера клавиш (S3 §1.2): события окна в форме, которую разбирает
//! [`crate::KeyInput::process`]. Раньше — типы egui; после перехода на GPUI (ADR-0018,
//! G-06) — свои, той же формы: окно GPUI переводит в них нажатия и текст.

pub use crate::ev_key::Key;

/// Зажатые модификаторы (`command` — Ctrl на Windows).
#[allow(clippy::struct_excessive_bools, reason = "флаги модификаторов")]
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub mac_cmd: bool,
    pub command: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        alt: false,
        ctrl: false,
        shift: false,
        mac_cmd: false,
        command: false,
    };
    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    pub const COMMAND: Self = Self {
        command: true,
        ..Self::NONE
    };

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    pub fn any(self) -> bool {
        !self.is_none()
    }

    /// Все модификаторы `query` зажаты (возможно, и другие).
    pub fn contains(self, query: Self) -> bool {
        (!query.alt || self.alt)
            && (!query.ctrl || self.ctrl)
            && (!query.shift || self.shift)
            && (!query.mac_cmd || self.mac_cmd)
            && (!query.command || self.command)
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self {
            alt: self.alt || rhs.alt,
            ctrl: self.ctrl || rhs.ctrl,
            shift: self.shift || rhs.shift,
            mac_cmd: self.mac_cmd || rhs.mac_cmd,
            command: self.command || rhs.command,
        }
    }
}

/// Кнопка мыши (нажатие закрывает быстрый поиск).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// Ввод метода ввода (IME).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImeEvent {
    /// Составление: пока текст не пуст, клавиши — IME.
    Preedit {
        text: String,
        active_range_chars: Option<(usize, usize)>,
    },
    Commit(String),
}

/// Событие окна.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Key {
        key: Key,
        /// Клавиша по положению (не зависит от раскладки).
        physical_key: Option<Key>,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Text(String),
    Copy,
    Cut,
    Paste(String),
    Ime(ImeEvent),
    ModifiersChanged(Modifiers),
    WindowFocused(bool),
    PointerButton {
        pos: (f32, f32),
        button: PointerButton,
        pressed: bool,
        modifiers: Modifiers,
    },
    MouseWheel {
        delta: (f32, f32),
        modifiers: Modifiers,
    },
    /// Указатель ушёл из окна (прочее событие, не клавиатура).
    PointerGone,
}

/// События кадра: поглощённые конвейер удаляет, остаток — окну.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawInput {
    pub events: Vec<Event>,
}

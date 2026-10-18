//! Хук сообщений Windows для keymap (S3 §0, S3-07): записки `KeyNotes` о клавишах,
//! которые egui-winit теряет — цифровой блок (`Gray-`, `Gray*`, `Gray/`, `Num5`,
//! `NumEnter`), `Ins`/`Del` (их Ctrl/Shift-сочетания egui превращает в
//! `Copy`/`Cut`/`Paste`), стороны Ctrl и Alt. Хук не поглощает сообщения.

use eframe::UserEvent;
use urfin_keymap::input::vk;
use urfin_keymap::{KeyNote, KeyNotes, Mods};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_LCONTROL, VK_LMENU, VK_RCONTROL, VK_RMENU, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MSG, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use winit::event_loop::EventLoopBuilder;
use winit::platform::windows::EventLoopBuilderExtWindows;

/// Ставит хук на цикл событий окна; записки читает `KeyInput::process`.
pub fn install(builder: &mut EventLoopBuilder<UserEvent>, notes: KeyNotes) {
    builder.with_msg_hook(move |msg| {
        // SAFETY: winit передаёт указатель на MSG из GetMessage/PeekMessage, он
        // действителен на время вызова.
        let m = unsafe { &*msg.cast::<MSG>() };
        if let Some(n) = note_of(m.message, m.wParam, m.lParam, held) {
            notes.push(n);
        }
        false
    });
}

/// Модификаторы со сторонами на момент сообщения (состояние очереди потока).
fn held() -> Mods {
    // SAFETY: GetKeyState без побочных эффектов, коды — константы.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    let mut m = Mods::empty();
    if down(VK_LCONTROL) {
        m |= Mods::CTRL | Mods::LCTRL;
    }
    if down(VK_RCONTROL) {
        m |= Mods::CTRL | Mods::RCTRL;
    }
    if down(VK_LMENU) {
        m |= Mods::ALT | Mods::LALT;
    }
    if down(VK_RMENU) {
        m |= Mods::ALT | Mods::RALT;
    }
    if down(VK_SHIFT) {
        m |= Mods::SHIFT;
    }
    m
}

/// Записка по сообщению клавиатуры; `None` — клавиша, которую egui передаёт сам.
/// `held` вызывается только для нужных клавиш.
fn note_of(
    message: u32,
    wparam: usize,
    lparam: isize,
    held: impl FnOnce() -> Mods,
) -> Option<KeyNote> {
    let down = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => true,
        WM_KEYUP | WM_SYSKEYUP => false,
        _ => return None,
    };
    let code = u16::try_from(wparam & 0xFFFF).ok()?;
    // Бит 24 lParam — расширенная клавиша: правые Ctrl/Alt, NumEnter, Ins/Del блока
    // навигации.
    let extended = (lparam >> 24) & 1 == 1;
    let wanted = match code {
        vk::RETURN => extended,
        vk::CLEAR
        | vk::CONTROL
        | vk::MENU
        | vk::INSERT
        | vk::DELETE
        | vk::MULTIPLY
        | vk::ADD
        | vk::SUBTRACT
        | vk::DIVIDE => true,
        _ => false,
    };
    wanted.then(|| KeyNote {
        vk: code,
        extended,
        mods: held(),
        down,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXT: isize = 1 << 24;

    fn n(message: u32, code: u16, lparam: isize) -> Option<KeyNote> {
        note_of(message, usize::from(code), lparam, || {
            Mods::CTRL | Mods::RCTRL
        })
    }

    #[test]
    fn numpad_and_ins_del_are_noted() {
        for code in [
            vk::SUBTRACT,
            vk::MULTIPLY,
            vk::DIVIDE,
            vk::ADD,
            vk::CLEAR,
            vk::INSERT,
        ] {
            let note = n(WM_KEYDOWN, code, 0).unwrap();
            assert_eq!((note.vk, note.down), (code, true));
            assert_eq!(note.mods, Mods::CTRL | Mods::RCTRL);
        }
        assert!(!n(WM_SYSKEYUP, vk::DELETE, EXT).unwrap().down);
    }

    /// Enter основного ряда egui различает сам; записка — только `NumEnter`.
    #[test]
    fn only_numpad_enter() {
        assert_eq!(n(WM_KEYDOWN, vk::RETURN, 0), None);
        assert!(n(WM_KEYDOWN, vk::RETURN, EXT).unwrap().extended);
    }

    /// Правый Ctrl — расширенный `VK_CONTROL`; прочие клавиши и сообщения — мимо.
    #[test]
    fn sides_and_ignored() {
        assert!(n(WM_KEYDOWN, vk::CONTROL, EXT).unwrap().extended);
        assert!(!n(WM_KEYUP, vk::MENU, 0).unwrap().extended);
        assert_eq!(n(WM_KEYDOWN, 0x41, 0), None, "A");
        assert_eq!(n(0x0102, vk::INSERT, 0), None, "WM_CHAR");
    }
}

//! Записки клавиш для keymap на окне GPUI (S3 §0, S3-07): subclass окна пишет в
//! [`KeyNotes`] то, что gpui теряет — цифровой блок (`Gray-`, `Gray*`, `Gray/`, `Num5`,
//! `NumEnter`), `Ins`/`Del` в сочетаниях, стороны Ctrl и Alt. Сообщения не поглощаются:
//! каждое уходит дальше через `DefSubclassProc`.
//!
//! Нажатия gpui разбирает не из `WM_KEYDOWN`: цикл сообщений сначала шлёт окну своё
//! `WM_GPUI_KEYDOWN` с теми же `wParam`/`lParam` и только если gpui его не обработал —
//! исходное `WM_KEYDOWN`. Поэтому записка нажатия пишется по `WM_GPUI_KEYDOWN`, а
//! следующее за ним такое же `WM_KEYDOWN` пропускается (иначе записка двоилась бы).
//! Мост клавиатуры (`gpui_shell::input`) получает нажатие синхронно, внутри
//! `WM_GPUI_KEYDOWN`: на это время [`key_down_vk`] отдаёт код клавиши.

#![allow(unsafe_code)]
#![allow(clippy::cast_sign_loss, reason = "Win32: биты lParam")]

use std::cell::Cell;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use urfin_keymap::input::vk;
use urfin_keymap::{KeyNote, KeyNotes, Mods};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_LCONTROL, VK_LMENU, VK_RCONTROL, VK_RMENU, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WM_KEYDOWN, WM_KEYUP, WM_NCDESTROY, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
};

const SUBCLASS_ID: usize = 0x4B4E;
/// Нажатие в gpui (`pets-gpui-windows`, `events.rs`: `WM_USER + 8`); версия gpui
/// зафиксирована в workspace — при обновлении сверить.
const WM_GPUI_KEYDOWN: u32 = WM_USER + 8;

thread_local! {
    /// Код клавиши нажатия, которое gpui разбирает прямо сейчас (поток окна).
    static KEY_DOWN: Cell<Option<u16>> = const { Cell::new(None) };
}

/// Код клавиши (`VK_*`) нажатия, которое сейчас разбирает gpui; `None` — вне
/// нажатия или subclass не стоит.
pub fn key_down_vk() -> Option<u16> {
    KEY_DOWN.with(Cell::get)
}

/// Данные subclass: живут, пока живёт окно (освобождаются в `WM_NCDESTROY`).
struct Ctx {
    notes: KeyNotes,
    /// Разобрать записки, которые не встретили своего события gpui (`Num5` без
    /// `NumLock`, отпускание одного из двух Ctrl).
    wake: Box<dyn Fn()>,
    /// Последнее `WM_GPUI_KEYDOWN`: за ним может прийти такое же `WM_KEYDOWN`.
    gpui_down: Cell<Option<(WPARAM, LPARAM)>>,
}

/// Ставит subclass на окно gpui; `false` — не вышло (окна нет или не Win32).
/// `wake` вызывается после каждого сообщения, по которому записана записка.
pub fn install(window: &impl HasWindowHandle, notes: KeyNotes, wake: Box<dyn Fn()>) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return false;
    };
    let hwnd = h.hwnd.get() as HWND;
    let data = Box::into_raw(Box::new(Ctx {
        notes,
        wake,
        gpui_down: Cell::new(None),
    }));
    // SAFETY: окно живо (вызов из `open_window`, поток окна); `data` живёт до
    // WM_NCDESTROY, где освобождается.
    unsafe {
        if SetWindowSubclass(hwnd, Some(proc), SUBCLASS_ID, data as usize) == 0 {
            drop(Box::from_raw(data));
            return false;
        }
    }
    true
}

/// Оконная процедура subclass.
unsafe extern "system" fn proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        // SAFETY: последний вызов для окна: снять subclass и освободить данные.
        unsafe {
            RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Ctx));
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
    }
    // SAFETY: `data` — `Box<Ctx>` из `install`, жив до WM_NCDESTROY.
    let c = unsafe { &*(data as *const Ctx) };
    let noted = match msg {
        WM_GPUI_KEYDOWN => {
            c.gpui_down.set(Some((wparam, lparam)));
            note_of(msg, wparam, lparam, held)
        }
        // Повтор того же нажатия после необработанного `WM_GPUI_KEYDOWN`.
        WM_KEYDOWN | WM_SYSKEYDOWN if c.gpui_down.take() == Some((wparam, lparam)) => None,
        _ => note_of(msg, wparam, lparam, held),
    };
    if let Some(n) = noted {
        c.notes.push(n);
    }
    let r = if msg == WM_GPUI_KEYDOWN {
        let code = u16::try_from(wparam & 0xFFFF).ok();
        let prev = KEY_DOWN.with(|k| k.replace(code));
        // SAFETY: штатная обработка (gpui).
        let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        KEY_DOWN.with(|k| k.set(prev));
        r
    } else {
        // SAFETY: штатная обработка.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    };
    if noted.is_some() {
        (c.wake)();
    }
    r
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

/// Записка по сообщению клавиатуры; `None` — клавиша, которую gpui передаёт сам.
/// `held` вызывается только для нужных клавиш.
fn note_of(
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    held: impl FnOnce() -> Mods,
) -> Option<KeyNote> {
    let down = match message {
        WM_GPUI_KEYDOWN | WM_KEYDOWN | WM_SYSKEYDOWN => true,
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

    const EXT: LPARAM = 1 << 24;

    fn n(message: u32, code: u16, lparam: LPARAM) -> Option<KeyNote> {
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
            let note = n(WM_GPUI_KEYDOWN, code, 0).unwrap();
            assert_eq!((note.vk, note.down), (code, true));
            assert_eq!(note.mods, Mods::CTRL | Mods::RCTRL);
        }
        assert!(!n(WM_SYSKEYUP, vk::DELETE, EXT).unwrap().down);
        assert!(n(WM_KEYDOWN, vk::DELETE, 0).unwrap().down);
    }

    /// Enter основного ряда gpui различает сам; записка — только `NumEnter`.
    #[test]
    fn only_numpad_enter() {
        assert_eq!(n(WM_GPUI_KEYDOWN, vk::RETURN, 0), None);
        assert!(n(WM_GPUI_KEYDOWN, vk::RETURN, EXT).unwrap().extended);
    }

    /// Правый Ctrl — расширенный `VK_CONTROL`; прочие клавиши и сообщения — мимо.
    #[test]
    fn sides_and_ignored() {
        assert!(n(WM_SYSKEYDOWN, vk::CONTROL, EXT).unwrap().extended);
        assert!(!n(WM_KEYUP, vk::MENU, 0).unwrap().extended);
        assert_eq!(n(WM_GPUI_KEYDOWN, 0x41, 0), None, "A");
        assert_eq!(n(0x0102, vk::INSERT, 0), None, "WM_CHAR");
    }

    /// Вне нажатия gpui кода клавиши нет.
    #[test]
    fn no_key_down_outside_message() {
        assert_eq!(key_down_vk(), None);
    }
}

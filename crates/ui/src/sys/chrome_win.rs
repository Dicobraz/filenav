//! Своя рамка окна на Windows (S2 §0, §1.4): subclass окна winit.
//!
//! - `WM_NCCALCSIZE` убирает системный заголовок: клиент начинается от верха окна, рамки
//!   ресайза слева, справа и снизу остаются за клиентом (невидимые, с тенью DWM); у
//!   развёрнутого окна клиент сжимается на толщину рамки — контент не уходит за экран.
//! - `WM_NCHITTEST` по раскладке UI ([`ChromeShared`]): заголовок → `HTCAPTION`
//!   (перетаскивание, двойной клик, Aero Snap), кнопка «развернуть» → `HTMAXBUTTON`
//!   (меню Snap Layouts Win11), верх → `HTTOP*`.
//! - События кнопки «развернуть» неклиентские: наведение и нажатие ведёт эта процедура.
//! - DWM: тёмная рамка по теме, Mica (`DWMWA_SYSTEMBACKDROP_TYPE`) или сплошной фон.
//! - С Mica у окна нет `WS_CAPTION`: рамка растянута на всего клиента, и DWM рисовал
//!   бы под прозрачным заголовком свои кнопки caption — глифы двоились с нашими. Без
//!   заголовка развёрнутое окно занимает весь монитор, поэтому `WM_GETMINMAXINFO`
//!   возвращает рабочую область (панель задач видна, автоскрытая — выезжает).

#![allow(unsafe_code)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Win32: координаты в LPARAM, коды HT* в LRESULT"
)]

use std::sync::Arc;
use std::sync::atomic::Ordering;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ScreenToClient,
};
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::Shell::{
    ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA, DefSubclassProc,
    RemoveWindowSubclass, SHAppBarMessage, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetClientRect, GetWindowLongPtrW, GetWindowRect, HTCAPTION, HTCLIENT, HTMAXBUTTON,
    HTTOP, HTTOPLEFT, HTTOPRIGHT, IsZoomed, MINMAXINFO, NCCALCSIZE_PARAMS, SM_CXFRAME,
    SM_CXPADDEDBORDER, SM_CYFRAME, STYLESTRUCT, SW_MAXIMIZE, SW_RESTORE, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WM_DWMCOMPOSITIONCHANGED, WM_GETMINMAXINFO, WM_MOUSEMOVE, WM_NCCALCSIZE,
    WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
    WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SETTINGCHANGE, WM_STYLECHANGING, WS_CAPTION,
};

use crate::chrome::{ChromeHandle, ChromeShared, Hit};

const SUBCLASS_ID: usize = 0xF11E;
/// `DWMWA_USE_IMMERSIVE_DARK_MODE` (Win10 20H1+, Win11).
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
/// `DWMWA_SYSTEMBACKDROP_TYPE` (Win11 22H2+).
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
const DWMSBT_MAINWINDOW: i32 = 2;

/// Данные subclass: живут, пока живёт окно (снимаются в `WM_NCDESTROY`).
struct Ctx {
    shared: Arc<ChromeShared>,
    egui: egui::Context,
    mica: bool,
}

/// Ставит subclass и атрибуты DWM. Клиентская область после снятия заголовка
/// возвращается к прежнему размеру: сессия хранит внутренний размер окна.
pub(super) fn install(hwnd: isize, egui: &egui::Context, mica: bool) -> Option<ChromeHandle> {
    let hwnd = hwnd as HWND;
    let shared = Arc::new(ChromeShared::default());
    let data = Box::into_raw(Box::new(Ctx {
        shared: Arc::clone(&shared),
        egui: egui.clone(),
        mica,
    }));
    let mut before = RECT::default();
    // SAFETY: окно живо (вызов из CreationContext, поток окна); `data` живёт до
    // WM_NCDESTROY, где освобождается.
    unsafe {
        GetClientRect(hwnd, &raw mut before);
        if SetWindowSubclass(hwnd, Some(proc), SUBCLASS_ID, data as usize) == 0 {
            drop(Box::from_raw(data));
            return None;
        }
    }
    if mica {
        // SAFETY: окно живо; рамку пересчитает `keep_client_size`.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_CAPTION as isize));
        }
    }
    apply_backdrop(hwnd, mica);
    keep_client_size(hwnd, &before);
    let raw = hwnd as isize;
    let set_dark = Arc::new(move |dark: bool| set_dark(raw, dark));
    Some(ChromeHandle::new(shared, mica, set_dark))
}

/// Пересчитать рамку и вернуть клиенту прежний размер (окно ещё скрыто cloak).
fn keep_client_size(hwnd: HWND, before: &RECT) {
    let mut window = RECT::default();
    let mut after = RECT::default();
    // SAFETY: окно живо; прямоугольники — выходные параметры.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED
                | SWP_NOMOVE
                | SWP_NOZORDER
                | SWP_NOOWNERZORDER
                | SWP_NOACTIVATE
                | 0x0001,
        );
        GetWindowRect(hwnd, &raw mut window);
        GetClientRect(hwnd, &raw mut after);
        let dw = (after.right - after.left) - (before.right - before.left);
        let dh = (after.bottom - after.top) - (before.bottom - before.top);
        if dw != 0 || dh != 0 {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                (window.right - window.left) - dw,
                (window.bottom - window.top) - dh,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

fn set_dark(hwnd: isize, dark: bool) {
    let value: i32 = i32::from(dark);
    // SAFETY: окно живо пока жив UI; атрибут — BOOL по значению.
    unsafe {
        DwmSetWindowAttribute(
            hwnd as HWND,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const value).cast(),
            size_of::<i32>() as u32,
        );
    }
}

/// Mica за клиентской областью (её прозрачные пиксели) или без подложки.
fn apply_backdrop(hwnd: HWND, mica: bool) {
    if !mica {
        return;
    }
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let kind = DWMSBT_MAINWINDOW;
    // SAFETY: окно живо; структуры — по указателю на стек.
    unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw const kind).cast(),
            size_of::<i32>() as u32,
        );
    }
}

/// Развёрнутое окно без заголовка — по рабочей области монитора, как окно с
/// заголовком: рамка ресайза уходит за край, клиент (`WM_NCCALCSIZE`) — ровно рабочая
/// область. При автоскрытой панели задач оставляем у её края 1 px, иначе Windows
/// считает окно полноэкранным и панель не выезжает.
fn max_to_work_area(hwnd: HWND, mm: &mut MINMAXINFO) {
    let mut mi = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..MONITORINFO::default()
    };
    // SAFETY: структура на стеке с cbSize.
    let ok = unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(mon, &raw mut mi) != 0
    };
    if !ok {
        return;
    }
    let (m, mut w) = (mi.rcMonitor, mi.rcWork);
    for edge in [ABE_LEFT, ABE_TOP, ABE_RIGHT, ABE_BOTTOM] {
        let mut abd = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            uEdge: edge,
            rc: m,
            ..APPBARDATA::default()
        };
        // SAFETY: структура на стеке с cbSize; ABM_GETAUTOHIDEBAREX только читает.
        if unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &raw mut abd) } == 0 {
            continue;
        }
        match edge {
            ABE_LEFT => w.left = w.left.max(m.left + 1),
            ABE_TOP => w.top = w.top.max(m.top + 1),
            ABE_RIGHT => w.right = w.right.min(m.right - 1),
            _ => w.bottom = w.bottom.min(m.bottom - 1),
        }
    }
    let (fx, fy) = frame(hwnd);
    // Позиция — от начала монитора окна (для другого монитора Windows пересчитает).
    mm.ptMaxPosition = POINT {
        x: w.left - m.left - fx,
        y: w.top - m.top - fy,
    };
    mm.ptMaxSize = POINT {
        x: w.right - w.left + 2 * fx,
        y: w.bottom - w.top + 2 * fy,
    };
}

/// Толщина невидимой рамки ресайза по DPI окна.
fn frame(hwnd: HWND) -> (i32, i32) {
    // SAFETY: без указателей.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        let pad = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        (
            GetSystemMetricsForDpi(SM_CXFRAME, dpi) + pad,
            GetSystemMetricsForDpi(SM_CYFRAME, dpi) + pad,
        )
    }
}

fn repaint(c: &Ctx) {
    c.egui.request_repaint();
}

fn set_hover(c: &Ctx, on: bool) {
    if c.shared.max_hover.swap(on, Ordering::Relaxed) != on {
        repaint(c);
    }
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
    // SAFETY: `data` — `Box<Ctx>` из `install`, жив до WM_NCDESTROY.
    let c = unsafe { &*(data as *const Ctx) };
    match msg {
        WM_NCCALCSIZE if wparam != 0 => {
            // SAFETY: при wParam=TRUE lParam — NCCALCSIZE_PARAMS.
            let p = unsafe { &mut *(lparam as *mut NCCALCSIZE_PARAMS) };
            let r = &mut p.rgrc[0];
            let (fx, fy) = frame(hwnd);
            // SAFETY: без указателей.
            let zoomed = unsafe { IsZoomed(hwnd) } != 0;
            c.shared.maximized.store(zoomed, Ordering::Relaxed);
            r.left += fx;
            r.right -= fx;
            r.bottom -= fy;
            if zoomed {
                r.top += fy;
            }
            0
        }
        WM_NCHITTEST => {
            // SAFETY: штатная обработка — рамки ресайза и т.п.
            let def = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if def != HTCLIENT as LRESULT {
                return def;
            }
            let mut pt = POINT {
                x: i32::from(lparam as u16 as i16),
                y: i32::from((lparam >> 16) as u16 as i16),
            };
            // SAFETY: точка на стеке.
            unsafe { ScreenToClient(hwnd, &raw mut pt) };
            (match c.shared.hit(pt.x, pt.y) {
                Hit::Client => HTCLIENT,
                Hit::Caption => HTCAPTION,
                Hit::MaxButton => HTMAXBUTTON,
                Hit::Top => HTTOP,
                Hit::TopLeft => HTTOPLEFT,
                Hit::TopRight => HTTOPRIGHT,
            }) as LRESULT
        }
        WM_NCMOUSEMOVE => {
            let over = wparam == HTMAXBUTTON as usize;
            set_hover(c, over);
            if over {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: структура на стеке с cbSize.
                unsafe { TrackMouseEvent(&raw mut tme) };
                // Иначе DefWindowProc рисует системную кнопку поверх нашей.
                return 0;
            }
            // SAFETY: штатная обработка.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSELEAVE | WM_MOUSEMOVE => {
            set_hover(c, false);
            if msg == WM_NCMOUSELEAVE {
                c.shared.max_pressed.store(false, Ordering::Relaxed);
            }
            // SAFETY: штатная обработка.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as usize => {
            c.shared.max_pressed.store(true, Ordering::Relaxed);
            repaint(c);
            0
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as usize => {
            if c.shared.max_pressed.swap(false, Ordering::Relaxed) {
                // SAFETY: без указателей.
                unsafe {
                    let zoomed = IsZoomed(hwnd) != 0;
                    ShowWindow(hwnd, if zoomed { SW_RESTORE } else { SW_MAXIMIZE });
                }
            }
            repaint(c);
            0
        }
        WM_STYLECHANGING if c.mica && wparam as i32 == GWL_STYLE => {
            // winit пересобирает стиль из своих флагов (разворот, ресайз) — заголовок
            // не возвращаем.
            // SAFETY: при GWL_STYLE lParam — STYLESTRUCT.
            let st = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
            st.styleNew &= !WS_CAPTION;
            0
        }
        WM_GETMINMAXINFO if c.mica => {
            // Сначала winit: минимальный размер окна.
            // SAFETY: штатная обработка.
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            // SAFETY: при WM_GETMINMAXINFO lParam — MINMAXINFO.
            let mm = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            max_to_work_area(hwnd, mm);
            r
        }
        WM_DWMCOMPOSITIONCHANGED | WM_SETTINGCHANGE => {
            apply_backdrop(hwnd, c.mica);
            repaint(c);
            // SAFETY: штатная обработка.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            // SAFETY: последний вызов для окна: снять subclass и освободить данные.
            unsafe {
                RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS_ID);
                drop(Box::from_raw(data as *mut Ctx));
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
        }
        // SAFETY: штатная обработка.
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

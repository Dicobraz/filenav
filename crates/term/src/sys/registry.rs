//! Чтение реестра для поиска shell (S6-09): строка значения и имена подразделов.

#![allow(unsafe_code)]

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};

/// Корень реестра.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    CurrentUser,
    LocalMachine,
}

impl Root {
    fn hkey(self) -> HKEY {
        match self {
            Root::CurrentUser => HKEY_CURRENT_USER,
            Root::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Строковое значение (`REG_SZ`, `REG_EXPAND_SZ` — переменные раскрываются).
pub fn string(root: Root, key: &str, value: &str) -> Option<String> {
    let (k, v) = (wide(key), wide(value));
    let mut len = 0u32;
    // SAFETY: первый вызов — размер; строки с NUL живут до конца вызова.
    let r = unsafe {
        RegGetValueW(
            root.hkey(),
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut len,
        )
    };
    if r != ERROR_SUCCESS || len == 0 {
        return None;
    }
    let mut buf = vec![0u16; (len as usize).div_ceil(2)];
    // SAFETY: буфер `len` байт.
    let r = unsafe {
        RegGetValueW(
            root.hkey(),
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Имена подразделов ключа (`None` — ключа нет).
pub fn subkeys(root: Root, key: &str) -> Option<Vec<String>> {
    let k = wide(key);
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: строка с NUL; выходной — локальный.
    if unsafe { RegOpenKeyExW(root.hkey(), k.as_ptr(), 0, KEY_READ, &raw mut h) } != ERROR_SUCCESS {
        return None;
    }
    let mut out = Vec::new();
    let mut name = [0u16; 256];
    for i in 0.. {
        let mut len = u32::try_from(name.len()).unwrap_or(0);
        // SAFETY: буфер имени своего размера; ключ открыт.
        let r = unsafe {
            RegEnumKeyExW(
                h,
                i,
                name.as_mut_ptr(),
                &raw mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    // SAFETY: ключ открыт выше.
    unsafe { RegCloseKey(h) };
    Some(out)
}

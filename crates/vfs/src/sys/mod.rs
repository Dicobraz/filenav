//! Вызовы ОС (S0 v4 №1, v5 №5: `unsafe` в vfs — только здесь).

#[cfg(windows)]
#[allow(unsafe_code, reason = "WinAPI/NT: листинг каталогов")]
pub(crate) mod windows;

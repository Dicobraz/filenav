//! Время CPU процесса и текущего потока (бенч G-07): Windows — `GetProcessTimes` /
//! `GetThreadTimes`, другие ОС — 0.

#![allow(unsafe_code)]

/// Секунды CPU процесса (ядро + пользователь).
pub fn process_secs() -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let mut t = [windows_sys::Win32::Foundation::FILETIME::default(); 4];
        // SAFETY: псевдодескриптор текущего процесса; структуры — на стеке.
        let ok = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &raw mut t[0],
                &raw mut t[1],
                &raw mut t[2],
                &raw mut t[3],
            )
        };
        if ok != 0 {
            return secs(t[2]) + secs(t[3]);
        }
    }
    0.0
}

/// Секунды CPU текущего потока (поток интерфейса, если зван из него).
pub fn thread_secs() -> f64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
        let mut t = [windows_sys::Win32::Foundation::FILETIME::default(); 4];
        // SAFETY: псевдодескриптор текущего потока; структуры — на стеке.
        let ok = unsafe {
            GetThreadTimes(
                GetCurrentThread(),
                &raw mut t[0],
                &raw mut t[1],
                &raw mut t[2],
                &raw mut t[3],
            )
        };
        if ok != 0 {
            return secs(t[2]) + secs(t[3]);
        }
    }
    0.0
}

#[cfg(windows)]
fn secs(f: windows_sys::Win32::Foundation::FILETIME) -> f64 {
    let ticks = (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    #[allow(clippy::cast_precision_loss, reason = "секунды CPU")]
    let s = ticks as f64 / 10_000_000.0;
    s
}

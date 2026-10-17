//! Локальное время на Windows (S0-decisions 2026-10-17): правила поясов берёт ОС —
//! без вшитой базы `tzdb` (`jiff` на Windows вшивает её, бинарь +0,5 мегабайта).
//! `SystemTimeToTzSpecificLocalTimeEx` с динамическими правилами учитывает историю
//! перехода на летнее время, как Проводник.

#![allow(unsafe_code)]

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, GetDynamicTimeZoneInformation,
    SystemTimeToTzSpecificLocalTimeEx,
};

use super::LocalTime;

/// 100-нс интервалов от 1601-01-01 до 1970-01-01.
const EPOCH_1601: i64 = 116_444_736_000_000_000;

/// Пояс ОС или UTC.
#[derive(Clone)]
pub struct Zone(Option<DYNAMIC_TIME_ZONE_INFORMATION>);

impl std::fmt::Debug for Zone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => f.write_str("Zone(UTC)"),
            Some(z) => {
                let name = String::from_utf16_lossy(&z.TimeZoneKeyName);
                write!(f, "Zone({})", name.trim_end_matches('\0'))
            }
        }
    }
}

impl Zone {
    pub fn utc() -> Self {
        Self(None)
    }

    /// Текущий пояс ОС (без файлового I/O).
    pub fn system() -> Self {
        let mut z = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: структура на стеке; функция только пишет в неё.
        let rc = unsafe { GetDynamicTimeZoneInformation(&raw mut z) };
        // TIME_ZONE_ID_INVALID = 0xFFFFFFFF.
        Self((rc != u32::MAX).then_some(z))
    }

    /// Наносекунды от Unix epoch → местное время; вне диапазона Windows — `None`.
    pub fn local(&self, unix_ns: i64) -> Option<LocalTime> {
        let ticks = unix_ns.div_euclid(100).checked_add(EPOCH_1601)?;
        let ticks = u64::try_from(ticks).ok()?;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        // SAFETY: структуры на стеке.
        if unsafe { FileTimeToSystemTime(&raw const ft, &raw mut utc) } == 0 {
            return None;
        }
        let st = match &self.0 {
            None => utc,
            Some(z) => {
                let mut local = SYSTEMTIME::default();
                // SAFETY: пояс и время — на стеке/в self; функция пишет в `local`.
                let ok = unsafe {
                    SystemTimeToTzSpecificLocalTimeEx(&raw const *z, &raw const utc, &raw mut local)
                };
                if ok == 0 {
                    return None;
                }
                local
            }
        };
        Some(LocalTime {
            year: i32::from(st.wYear),
            month: st.wMonth as u8,
            day: st.wDay as u8,
            hour: st.wHour as u8,
            minute: st.wMinute as u8,
            second: st.wSecond as u8,
        })
    }
}

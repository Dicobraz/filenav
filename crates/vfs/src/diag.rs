//! Диагностика для бенчей (S4-11): пик памяти процесса.

/// Пик рабочего набора и private-памяти процесса, байт (`None` — ОС не дала).
/// Linux: `VmHWM` из `/proc/self/status` (private — `None`); macOS — `None`.
pub fn peak_memory() -> Option<(u64, Option<u64>)> {
    #[cfg(windows)]
    {
        crate::sys::win_meta::peak_memory().map(|(ws, private)| (ws, Some(private)))
    }
    #[cfg(not(windows))]
    {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb: u64 = s
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        Some((kb * 1024, None))
    }
}

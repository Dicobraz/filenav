//! Плотное представление для сортировки (S4 §4.1): `SortRec` по 24 байта и имена
//! без отдельных аллокаций.

use urfin_core::Entry;

/// Запись сортировки: префикс имени (или расширения), вспомогательный ключ
/// (размер / время), индекс в `entries`, группа и флаги. Сравнение префиксов как
/// `u64` решает большинство случаев без чтения строк.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SortRec {
    /// 8 байт сложенного имени/расширения, big-endian ([`super::prefix_key`]).
    pub prefix: u64,
    /// Размер или mtime (со сдвигом знака, [`mtime_key`]); иначе 0.
    pub aux: u64,
    pub idx: u32,
    /// 0 — каталоги (при `dirs_first`), 1 — прочее.
    pub group: u8,
    pub flags: u8,
    _pad: u16,
}

impl SortRec {
    /// Скрытая с учётом настроек (решение владельца №8).
    pub const HIDDEN_EFFECTIVE: u8 = 1;
    /// Имя экранировано (`Attrs::ESCAPED_NAME`).
    pub const ESCAPED: u8 = 2;

    pub fn new(idx: u32, group: u8, prefix: u64, aux: u64, flags: u8) -> SortRec {
        SortRec {
            prefix,
            aux,
            idx,
            group,
            flags,
            _pad: 0,
        }
    }
}

/// mtime (нс, со знаком) → `u64` с тем же порядком.
pub fn mtime_key(mtime: i64) -> u64 {
    mtime.cast_unsigned() ^ (1 << 63)
}

/// Имена по индексу записи.
pub trait Names {
    fn name(&self, idx: u32) -> &str;
}

impl Names for [Entry] {
    fn name(&self, idx: u32) -> &str {
        &self[idx as usize].name
    }
}

impl Names for Vec<Entry> {
    fn name(&self, idx: u32) -> &str {
        &self[idx as usize].name
    }
}

/// Имена во время загрузки: один буфер байтов и концы — плотно для сортировки
/// на cpu-pool, пока записи стримятся в core.
#[derive(Debug, Default, Clone)]
pub struct NameArena {
    bytes: String,
    ends: Vec<u32>,
}

impl NameArena {
    pub fn with_capacity(names: usize, bytes: usize) -> NameArena {
        NameArena {
            bytes: String::with_capacity(bytes),
            ends: Vec::with_capacity(names),
        }
    }

    /// Добавить имя; возвращает его индекс.
    pub fn push(&mut self, name: &str) -> u32 {
        self.bytes.push_str(name);
        self.ends
            .push(u32::try_from(self.bytes.len()).expect("arena < 4 GiB"));
        u32::try_from(self.ends.len() - 1).expect("< 2^32 names")
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }
}

impl Names for NameArena {
    fn name(&self, idx: u32) -> &str {
        let i = idx as usize;
        let start = if i == 0 { 0 } else { self.ends[i - 1] as usize };
        &self.bytes[start..self.ends[i] as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_rec_is_24_bytes() {
        assert_eq!(std::mem::size_of::<SortRec>(), 24);
    }

    #[test]
    fn mtime_key_keeps_order() {
        let v = [i64::MIN, -5, -1, 0, 1, 1_700_000_000_000_000_000, i64::MAX];
        for w in v.windows(2) {
            assert!(mtime_key(w[0]) < mtime_key(w[1]), "{w:?}");
        }
    }

    #[test]
    fn arena_returns_names() {
        let mut a = NameArena::default();
        let names = ["a", "", "ёлка", "file-0001.txt"];
        let ids: Vec<u32> = names.iter().map(|n| a.push(n)).collect();
        assert_eq!(ids, [0, 1, 2, 3]);
        for (i, n) in names.iter().enumerate() {
            assert_eq!(a.name(u32::try_from(i).unwrap()), *n);
        }
        assert_eq!(a.len(), 4);
    }
}

//! Таблица стратегий копирования (S5 §0, S5-14): какой движок для какого
//! файла. Источник — `[ops.strategy]` поверх умолчаний, выбранных бенчами.
//!
//! Мелкие файлы (S5-14, `docs/perf/s5-14-small-files.md`): по умолчанию —
//! системная копия (`CopyFileExW`). Свой путь (`small = "own"`) на стенде с
//! Defender не быстрее (в 8 потоков — на 45 % медленнее): системная копия
//! сообщает антивирусу, что это копия уже проверенного файла. `CopyFile2` —
//! в пределах шума от `CopyFileExW`.

use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::api::Engine;

/// Выбор движков по размеру файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StrategyTable {
    /// Мелкий файл (< `small_file`): `Os` или `Small` (свой путь).
    pub small: Engine,
}

impl Default for StrategyTable {
    fn default() -> Self {
        StrategyTable { small: Engine::Os }
    }
}

impl StrategyTable {
    /// Из `[ops.strategy]`; неизвестные значения — умолчание и предупреждение
    /// в лог. Ключи вида `"ssd>ssd"` — S5-19, здесь не разбираются.
    pub(crate) fn from_config(map: &BTreeMap<CompactString, CompactString>) -> StrategyTable {
        let mut t = StrategyTable::default();
        if let Some(v) = map.get("small") {
            match v.as_str() {
                "os" => t.small = Engine::Os,
                "own" => t.small = Engine::Small,
                other => tracing::warn!(value = other, "[ops.strategy] small: os | own"),
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_engine_from_config() {
        let mut m = BTreeMap::new();
        assert_eq!(StrategyTable::from_config(&m).small, Engine::Os);
        m.insert("small".into(), "own".into());
        assert_eq!(StrategyTable::from_config(&m).small, Engine::Small);
        m.insert("small".into(), "turbo".into());
        assert_eq!(StrategyTable::from_config(&m).small, Engine::Os);
    }
}

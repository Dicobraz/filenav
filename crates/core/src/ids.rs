//! Идентификаторы и сторона панели.

use serde::{Deserialize, Serialize};

/// Сторона панели.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// Противоположная сторона.
    #[must_use]
    pub fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Индекс для массивов из двух панелей: `Left` → 0, `Right` → 1.
    pub fn idx(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }
}

/// Идентификатор таба.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct TabId(pub u32);

/// Идентификатор задачи.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct JobId(pub u64);

/// Идентификатор inline-запроса.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct PromptId(pub u64);

/// Поколение таба: растёт при каждой навигации, по нему отбрасываются устаревшие результаты.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd, Default)]
pub struct Gen(pub u64);

#[cfg(test)]
mod tests {
    use super::Side;

    #[test]
    fn side_other_and_idx() {
        assert_eq!(Side::Left.other(), Side::Right);
        assert_eq!(Side::Right.other(), Side::Left);
        assert_eq!(Side::Left.idx(), 0);
        assert_eq!(Side::Right.idx(), 1);
    }
}

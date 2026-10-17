//! Сортировка и фильтры (S4 §4): natural-компаратор, ключи `SortRec`, имена.
//! `arrange()` (фильтр → сортировка → порядок) — S4-05.

mod keys;
mod natural;

pub use keys::{NameArena, Names, SortRec, mtime_key};
pub use natural::{cmp_with, name_cmp, natural_cmp, prefix_key};

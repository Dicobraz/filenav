//! Сортировка и фильтры (S4 §4): natural-компаратор, ключи `SortRec`, имена.
//! `arrange()` — фильтр → сортировка → порядок.

mod arrange;
mod keys;
mod natural;

pub use arrange::{
    ArrangeOpts, arrange, compare, is_hidden, record, reverse_within_groups, sort_recs, visible,
};

pub use keys::{NameArena, Names, SortRec, mtime_key};
pub use natural::{cmp_with, name_cmp, natural_cmp, prefix_key};

//! Inline-запросы: описание запроса. Очередь `Prompts` и ответы — S1-09.

use compact_str::CompactString;
use smallvec::SmallVec;

use crate::entry::Entry;
use crate::ids::Side;

/// Где показывать запрос.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    Panel(Side),
    Cmdline,
    JobBar,
}

/// Вариант ответа.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: CompactString,
    pub label: CompactString,
    pub key: char,
}

/// Дополнительные данные запроса для UI (S0 v2 №16); расширение — через S0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDetail {
    Conflict {
        src: Entry,
        dst: Entry,
        index: u64,
        total: u64,
    },
}

/// Описание inline-запроса.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptSpec {
    pub anchor: Anchor,
    pub title: String,
    pub body: String,
    pub choices: SmallVec<[Choice; 4]>,
    /// Индекс варианта по умолчанию.
    pub default: u8,
    /// Поле ввода с начальным значением.
    pub input: Option<String>,
    /// Флажок «для всех».
    pub allow_all: bool,
    pub detail: Option<PromptDetail>,
    /// Ввод маскируется, не логируется и не попадает в историю и сессию (S0 v5 №7).
    pub secret: bool,
}

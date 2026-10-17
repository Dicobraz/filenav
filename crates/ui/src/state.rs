//! Состояние UI в `AppState.ext` (S2 §3, S0 v2 №10). Меняет только handler `ui`;
//! фронтенд читает. Поля дерева, инфо, превью, томов и ширин колонок добавят их задачи
//! (S2-18, S2-10).

use compact_str::CompactString;
use smallvec::SmallVec;
use urfin_core::{Location, OverlayKind, PromptId, Side, TabId};

/// Тема окна (`ui.theme`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Строка ввода аргумента команды (`ui.draft`): F7 → `ops.mkdir name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    /// Целевая команда.
    pub cmd: CompactString,
    /// Аргумент, который заполняет ввод.
    pub arg: CompactString,
    pub text: CompactString,
}

/// Меню подкаталогов breadcrumb (`ui.menu menu=subdirs`, ADR-0014).
#[derive(Clone, Debug, PartialEq)]
pub struct Subdirs {
    pub side: Side,
    /// Каталог, чьи подкаталоги в меню.
    pub loc: Location,
    /// `None` — читается; ошибка — текстом.
    pub items: Option<Result<Vec<CompactString>, CompactString>>,
    /// Поколение запроса: старый ответ не применяется.
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiState {
    /// Зеркало `Focus::Overlay` (S0 v3 №1): открытый оверлей.
    pub overlay: Option<OverlayKind>,
    /// Меню F9 и подменю (`ui.menu menu=`); `subdirs` и `crumbs` — меню breadcrumb.
    pub menu: Option<CompactString>,
    /// Панель меню breadcrumb (`ui.menu side=`).
    pub menu_side: Option<Side>,
    pub subdirs: Option<Subdirs>,
    pub draft: Option<Draft>,
    /// Rename по месту: таб и исходное имя.
    pub rename: Option<(TabId, CompactString)>,
    /// Быстрый поиск: панель и введённый текст.
    pub qsearch: Option<(Side, CompactString)>,
    pub path_edit: Option<Side>,
    /// Поколение проверки введённого пути (`ui.path_edit path=`), пока она идёт.
    pub path_check: Option<u64>,
    /// Счётчик поколений фоновых запросов UI.
    pub next_gen: u64,
    /// Свёрнутые вопросы (`ui.prompt.hide`).
    pub prompt_hidden: SmallVec<[PromptId; 4]>,
    pub jobs_expanded: bool,
    /// Тема командой `ui.theme`; `None` — из `[ui] theme`.
    pub theme: Option<ThemeMode>,
    /// Масштаб в процентах (`ui.zoom`), 100 — без масштаба.
    pub zoom: u16,
    /// Доля левой панели (`ui.split`), 0,5 — поровну.
    pub split: f32,
    /// Набор Custom columns (`ui.columns`, M2+).
    pub columns: Option<CompactString>,
    /// Ctrl+O: панели скрыты, виден вывод терминала (S0-decisions №27).
    pub panels_hidden: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        Self {
            zoom: 100,
            split: 0.5,
            overlay: None,
            menu: None,
            menu_side: None,
            subdirs: None,
            draft: None,
            rename: None,
            qsearch: None,
            path_edit: None,
            path_check: None,
            next_gen: 0,
            prompt_hidden: SmallVec::new(),
            jobs_expanded: false,
            theme: None,
            columns: None,
            panels_hidden: false,
        }
    }

    /// Закрыть оверлей и всё, что к нему относится.
    pub(crate) fn close_overlay(&mut self) {
        self.overlay = None;
        self.menu = None;
        self.menu_side = None;
        self.subdirs = None;
        self.draft = None;
        self.rename = None;
        self.qsearch = None;
        self.path_edit = None;
        self.path_check = None;
    }

    /// Новое поколение фонового запроса.
    pub(crate) fn bump(&mut self) -> u64 {
        self.next_gen += 1;
        self.next_gen
    }
}

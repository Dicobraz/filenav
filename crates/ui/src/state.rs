//! Состояние UI в `AppState.ext` (S2 §3, S0 v2 №10). Меняет только handler `ui`;
//! фронтенд читает. Поля дерева, инфо, превью, томов и ширин колонок добавят их задачи
//! (S2-18, S2-10).

use compact_str::CompactString;
use smallvec::SmallVec;
use urfin_core::{Location, OverlayKind, PromptId, Side, TabId};

/// Тема, заданная политикой организации (`HKLM\Software\Policies\Aahz\Urfin`, T-01-5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemePolicy {
    /// `Theme` (`REG_SZ`): тема по умолчанию или единственная при `locked`.
    pub theme: Option<CompactString>,
    /// `ThemeLocked` (DWORD ≠ 0): сменить тему нельзя.
    pub locked: bool,
}

/// Пользовательские темы (`themes/*.toml`): загружаются задачей, не в UI-потоке.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserThemes {
    pub themes: std::sync::Arc<Vec<crate::theme::Theme>>,
    /// Растёт при каждой загрузке — ключ пересборки токенов окна.
    pub generation: u64,
    /// mtime каталога при последней загрузке: реактивация окна грузит заново, только
    /// если он изменился (без файлового вотчера).
    pub mtime: Option<std::time::SystemTime>,
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
    pub path_edit: Option<Side>,
    /// Поколение проверки введённого пути (`ui.path_edit path=`), пока она идёт.
    pub path_check: Option<u64>,
    /// Счётчик поколений фоновых запросов UI.
    pub next_gen: u64,
    /// Свёрнутые вопросы (`ui.prompt.hide`).
    pub prompt_hidden: SmallVec<[PromptId; 4]>,
    pub jobs_expanded: bool,
    /// Выбор темы (№59): `auto` — как в ОС, или id темы; `[ui] theme` при загрузке
    /// конфига, затем `ui.theme.*`. Политика `ThemeLocked` его перекрывает.
    pub theme: CompactString,
    /// Высота строки списка, px (`ui.density.set`): 20 / 24 / 28.
    pub density: u8,
    /// Иконки файлов (`ui.icons.set`).
    pub icons: crate::config::IconMode,
    pub user_themes: UserThemes,
    pub policy: ThemePolicy,
    /// Каталог своих тем (`<config_dir>/themes`); `None` — без диска (тесты).
    pub themes_dir: Option<std::path::PathBuf>,
    /// Масштаб в процентах (`ui.zoom`), 100 — без масштаба.
    pub zoom: u16,
    /// Доля левой панели (`ui.split`), 0,5 — поровну.
    pub split: f32,
    /// Набор Custom columns (`ui.columns`, M2+).
    pub columns: Option<CompactString>,
    /// Ctrl+O: панели скрыты, виден вывод терминала (S0-decisions №27).
    pub panels_hidden: bool,
    /// Режимы панелей (S2-18) по сторонам: «Дерево», «Инфо», быстрый просмотр.
    pub tree: [Option<crate::views::TreeState>; 2],
    pub info: [Option<crate::views::InfoState>; 2],
    pub preview: [Option<crate::views::PreviewState>; 2],
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
            path_edit: None,
            path_check: None,
            next_gen: 0,
            prompt_hidden: SmallVec::new(),
            jobs_expanded: false,
            theme: crate::theme::AUTO.into(),
            density: 24,
            icons: crate::config::IconMode::Mono,
            user_themes: UserThemes::default(),
            policy: ThemePolicy::default(),
            themes_dir: None,
            columns: None,
            panels_hidden: false,
            tree: [None, None],
            info: [None, None],
            preview: [None, None],
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
        self.path_edit = None;
        self.path_check = None;
    }

    /// Новое поколение фонового запроса.
    pub(crate) fn bump(&mut self) -> u64 {
        self.next_gen += 1;
        self.next_gen
    }
}

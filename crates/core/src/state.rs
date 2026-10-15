//! Состояние приложения. Владеет им Core в UI-потоке; мутации — только в handlers
//! и `Msg` (ADR-0001).

use std::any::{Any, TypeId};
use std::fmt;

use compact_str::CompactString;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::cmd::{Args, Command, CommandId};
use crate::history::VisitedDirs;
use crate::ids::{PromptId, Side, TabId};
use crate::tab::Tab;

/// Куда идёт ввод (S0 v3 №1, v5 №1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Focus {
    #[default]
    Panel,
    Cmdline,
    Prompt(PromptId),
    Viewer,
    Editor,
    Terminal,
    Overlay(OverlayKind),
}

/// Inline-оверлей с фокусом.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    Menu,
    QuickSearch,
    InlineEdit,
    Draft,
    Palette,
    /// Поиск, замена и goto во вьювере и редакторе (S7).
    Find,
}

/// Панель: набор табов.
#[derive(Debug)]
pub struct Panel {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub visible: bool,
}

impl Panel {
    pub fn new(tab: Tab) -> Self {
        Panel {
            tabs: vec![tab],
            active: 0,
            visible: true,
        }
    }

    /// Активный таб. Инвариант: табов ≥ 1, `active < tabs.len()`.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

/// Сигнал фронтенду; забирается `Core::take_ui_events` раз в кадр.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    Toast {
        level: Level,
        text: String,
    },
    ScrollToCursor(TabId),
    FocusChanged(Focus),
    /// Сигналы для S2/S6/S7 без изменения core.
    Ext {
        id: CommandId,
        args: Args,
    },
}

/// Inline-баннер: «прошлый запуск упал», ошибки конфига.
#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub level: Level,
    pub text: String,
    /// Кнопки: подпись и команда (S0 v2 №15).
    pub actions: SmallVec<[(CompactString, Command); 3]>,
}

/// Состояние других крейтов в `AppState` (S0 v2 №10): ключ — тип, объявленный в
/// своём крейте. Мутации — только в Handler/Msg.
#[derive(Default)]
pub struct Extensions(FxHashMap<TypeId, Box<dyn Any>>);

impl Extensions {
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Возвращает прежнее значение этого типа.
    pub fn insert<T: 'static>(&mut self, v: T) -> Option<T> {
        self.0
            .insert(TypeId::of::<T>(), Box::new(v))
            .and_then(|old| old.downcast().ok().map(|b| *b))
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Состояние приложения.
///
/// Поля `jobs`, `prompts` (S1-09) и `config` (S1-10) добавят свои задачи.
#[derive(Debug)]
pub struct AppState {
    pub panels: [Panel; 2],
    pub active: Side,
    pub focus: Focus,
    /// Глобальный MRU каталогов (Alt+F12).
    pub visited: VisitedDirs,
    pub banner: Option<Banner>,
    pub ext: Extensions,
    /// Растёт при любой мутации — ключ кэшей UI.
    pub rev: u64,
    pub quit: bool,
    ui_events: Vec<UiEvent>,
}

impl AppState {
    pub fn new(left: Panel, right: Panel) -> Self {
        AppState {
            panels: [left, right],
            active: Side::Left,
            focus: Focus::Panel,
            visited: VisitedDirs::default(),
            banner: None,
            ext: Extensions::default(),
            rev: 0,
            quit: false,
            ui_events: Vec::new(),
        }
    }

    pub fn panel(&self, s: Side) -> &Panel {
        &self.panels[s.idx()]
    }

    pub fn panel_mut(&mut self, s: Side) -> &mut Panel {
        &mut self.panels[s.idx()]
    }

    pub fn active_panel(&self) -> &Panel {
        self.panel(self.active)
    }

    pub fn active_tab(&self) -> &Tab {
        self.active_panel().tab()
    }

    pub fn passive_tab(&self) -> &Tab {
        self.panel(self.active.other()).tab()
    }

    pub fn tab(&self, id: TabId) -> Option<(Side, &Tab)> {
        [Side::Left, Side::Right].into_iter().find_map(|s| {
            self.panel(s)
                .tabs
                .iter()
                .find(|t| t.id == id)
                .map(|t| (s, t))
        })
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.panels
            .iter_mut()
            .flat_map(|p| p.tabs.iter_mut())
            .find(|t| t.id == id)
    }

    /// Отметить мутацию.
    pub fn touch(&mut self) {
        self.rev += 1;
    }

    /// Событие для фронтенда. Тоже изменение: поднимает `rev`, чтобы UI перерисовался.
    pub(crate) fn push_ui(&mut self, e: UiEvent) {
        self.ui_events.push(e);
        self.touch();
    }

    pub(crate) fn take_ui_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.ui_events)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, Banner, Extensions, Focus, Level, OverlayKind, Panel, UiEvent};
    use crate::cmd::Command;
    use crate::ids::{Side, TabId};
    use crate::loc::Location;
    use crate::tab::Tab;
    use smallvec::smallvec;

    fn state() -> AppState {
        let l = |id, p| Tab::new(TabId(id), Location::local(p));
        let mut left = Panel::new(l(1, "/a"));
        left.tabs.push(l(2, "/b"));
        left.active = 1;
        AppState::new(left, Panel::new(l(3, "/c")))
    }

    #[test]
    fn navigation_helpers() {
        let mut s = state();
        assert_eq!(s.active_tab().id, TabId(2));
        assert_eq!(s.passive_tab().id, TabId(3));
        s.active = Side::Right;
        assert_eq!(s.active_tab().id, TabId(3));
        assert_eq!(s.passive_tab().id, TabId(2));
        assert_eq!(
            s.tab(TabId(1)).map(|(side, t)| (side, t.id)),
            Some((Side::Left, TabId(1)))
        );
        assert_eq!(s.tab(TabId(3)).map(|(side, _)| side), Some(Side::Right));
        assert!(s.tab(TabId(9)).is_none());
        s.tab_mut(TabId(1)).unwrap().show_hidden = true;
        assert!(s.panel(Side::Left).tabs[0].show_hidden);
        s.panel_mut(Side::Right).visible = false;
        assert!(!s.panel(Side::Right).visible);
    }

    #[test]
    fn ui_events_are_taken_once() {
        let mut s = state();
        s.push_ui(UiEvent::FocusChanged(Focus::Overlay(OverlayKind::Find)));
        s.push_ui(UiEvent::ScrollToCursor(TabId(2)));
        assert_eq!(s.take_ui_events().len(), 2);
        assert_eq!(s.take_ui_events(), []);
    }

    #[test]
    fn extensions_typemap() {
        #[derive(Debug, PartialEq)]
        struct OpsUi(u32);
        struct Other;
        let mut e = Extensions::default();
        assert!(e.get::<OpsUi>().is_none());
        assert_eq!(e.insert(OpsUi(1)), None);
        e.get_mut::<OpsUi>().unwrap().0 += 1;
        assert_eq!(e.get::<OpsUi>(), Some(&OpsUi(2)));
        assert_eq!(e.insert(OpsUi(5)), Some(OpsUi(2)));
        assert!(e.get::<Other>().is_none());
        e.insert(Other);
        assert!(e.get::<Other>().is_some());
    }

    #[test]
    fn banner_actions_and_focus_default() {
        let b = Banner {
            level: Level::Warn,
            text: "crashed".into(),
            actions: smallvec![
                ("continue".into(), Command::new("app.save_session")),
                ("clear".into(), Command::new("app.reload_config")),
            ],
        };
        assert_eq!(b.actions.len(), 2);
        assert!(!b.actions.spilled());
        assert_eq!(state().focus, Focus::Panel);
        assert_eq!(Focus::default(), Focus::Panel);
    }
}

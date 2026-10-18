//! Раскладка окна (S2 §2, S2-07): регионы в логических единицах на сетке 4 px.
//!
//! Сверху вниз: заголовок 40 → баннер core (48) → карточки панелей (зазоры 8,
//! сплиттер) → job-бар (32 на задачу) → баннер-вопрос или черновик (48, конфликт —
//! 48 + 3×24; S2 §2, §2.7–2.8) → cmdline 32 → keybar 24 (обязательна, «Принцип»). Слои (S0 v5 №3): терминал → панели → cmdline → вьювер/редактор →
//! оверлеи. Область терминала не зависит от Ctrl+O — размер PTY не меняется.

use egui::{Rect, pos2};
use urfin_core::{Anchor, AppState, Side};

use crate::state::UiState;
use crate::tokens::{Metric, Tokens};

/// Минимальный размер окна (логические единицы).
pub const MIN_WINDOW: [f32; 2] = [640.0, 400.0];
/// Уже — видна только активная панель.
pub const SINGLE_PANEL_BELOW: f32 = 720.0;
/// Шаг сетки.
pub const GRID: f32 = 4.0;
/// Не больше стольких строк job-бара (остальное — «ещё N», S2-17).
pub const JOB_ROWS_MAX: usize = 4;

/// Регионы окна.
#[derive(Clone, Debug, PartialEq)]
pub struct Regions {
    pub titlebar: Rect,
    /// Баннеры core (сверху).
    pub banners: Vec<Rect>,
    /// Баннер-вопрос или черновик над cmdline; у вопроса `Anchor::Panel` — в ширину
    /// его карточки.
    pub prompt: Option<Rect>,
    /// Карточки панелей; `None` — скрыта (`panel.toggle`, узкое окно, Ctrl+O).
    pub panels: [Option<Rect>; 2],
    /// Зазор между карточками — тянется мышью (S2-10), двойной клик — сброс.
    pub splitter: Option<Rect>,
    /// Строки job-бара.
    pub jobs: Vec<Rect>,
    pub cmdline: Rect,
    pub keybar: Rect,
    /// Слой терминала (вывод команд за панелями, S0-decisions №27): от заголовка до
    /// cmdline, Ctrl+O его не меняет.
    pub terminal: Rect,
}

impl Regions {
    pub fn panel(&self, side: Side) -> Option<Rect> {
        self.panels[side.idx()]
    }
}

/// Округление к сетке 4 px.
fn snap(v: f32) -> f32 {
    (v / GRID).round() * GRID
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
    Rect::from_min_max(pos2(left, top), pos2(right.max(left), bottom.max(top)))
}

#[derive(Debug)]
pub struct Layout;

impl Layout {
    /// Регионы для окна `screen`.
    pub fn compute(screen: Rect, t: &Tokens, s: &AppState, ui: &UiState) -> Regions {
        let jobs = s.jobs.iter().filter(|j| j.state.is_active()).count();
        Self::compute_with(screen, t, s, ui, jobs)
    }

    /// [`Layout::compute`] с заданным числом активных задач.
    #[allow(clippy::many_single_char_names, reason = "геометрия: l, r, m, s, t")]
    pub fn compute_with(
        screen: Rect,
        t: &Tokens,
        s: &AppState,
        ui: &UiState,
        active_jobs: usize,
    ) -> Regions {
        let m = |x| t.m(x);
        let gap = m(Metric::GapPanels);
        let (l, r) = (screen.left(), screen.right());
        let title_bottom = screen.top() + m(Metric::HTitlebar);
        let titlebar = rect(l, screen.top(), r, title_bottom);

        // Низ: keybar, cmdline, job-бар.
        let keybar = rect(
            l + gap,
            screen.bottom() - gap - m(Metric::HKeybar),
            r - gap,
            screen.bottom() - gap,
        );
        let cmdline = rect(
            l + gap,
            keybar.top() - m(Metric::HCmdline),
            r - gap,
            keybar.top(),
        );
        // Не больше JOB_ROWS_MAX строк — в u8 без потерь.
        let active_jobs = u8::try_from(active_jobs.min(JOB_ROWS_MAX)).unwrap_or(0);
        let row = m(Metric::HJobRow);
        let bar = crate::prompt_bar::bar(s, ui);
        let bar_h = bar.map_or(0.0, |b| crate::prompt_bar::height(t, b) + gap);
        let bar_bottom = cmdline.top() - gap;
        let jobs_top = cmdline.top() - bar_h - row * f32::from(active_jobs);
        let jobs = (0..active_jobs)
            .map(|i| {
                let top = jobs_top + row * f32::from(i);
                rect(l + gap, top, r - gap, top + row)
            })
            .collect();

        // Верх: баннеры под заголовком.
        let mut y = title_bottom;
        let mut banners = Vec::new();
        let mut banner = |h: f32, y: &mut f32| {
            banners.push(rect(l + gap, *y, r - gap, *y + h));
            *y += h + gap;
        };
        if s.banner.is_some() {
            banner(m(Metric::HBanner), &mut y);
        }

        // Карточки.
        let area = rect(l + gap, y, r - gap, jobs_top - gap);
        let terminal = rect(l + gap, title_bottom, r - gap, cmdline.top() - gap);
        let visible = [Side::Left, Side::Right].map(|side| s.panel(side).visible);
        let single = screen.width() < SINGLE_PANEL_BELOW;
        let mut panels = [None, None];
        let mut splitter = None;
        if !ui.panels_hidden {
            match (visible, single) {
                ([true, true], false) => {
                    let split = ui.split.clamp(0.1, 0.9);
                    let left_r = snap(area.left() + (area.width() - gap) * split);
                    panels[0] = Some(rect(area.left(), area.top(), left_r, area.bottom()));
                    panels[1] = Some(rect(left_r + gap, area.top(), area.right(), area.bottom()));
                    splitter = Some(rect(left_r, area.top(), left_r + gap, area.bottom()));
                }
                // Узкое окно — только активная; иначе — та, что видна.
                ([true, true], true) => panels[s.active.idx()] = Some(area),
                ([true, false], _) => panels[0] = Some(area),
                ([false, true], _) => panels[1] = Some(area),
                ([false, false], _) => {}
            }
        }
        let prompt = bar.map(|b| {
            let h = crate::prompt_bar::height(t, b);
            let (left, right) = match b {
                crate::prompt_bar::Bar::Prompt(p, _) => match p.spec.anchor {
                    Anchor::Panel(side) => {
                        panels[side.idx()].map_or((l + gap, r - gap), |c| (c.left(), c.right()))
                    }
                    Anchor::JobBar | Anchor::Cmdline => (l + gap, r - gap),
                },
                crate::prompt_bar::Bar::Draft(_) => (l + gap, r - gap),
            };
            rect(left, bar_bottom - h, right, bar_bottom)
        });
        Regions {
            titlebar,
            banners,
            prompt,
            panels,
            splitter,
            jobs,
            cmdline,
            keybar,
            terminal,
        }
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;

//! Тосты (№51), job-бар и inline-вопрос (S2 §2.7–2.9) на GPUI (G-05).

use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, Div, SharedString, Window, div, px, relative};
use urfin_core::{AppState, Command, JobState, Level, Prompt, Value};

use super::theme::Theme;
use crate::tokens::{Metric, Role};

/// Стек тостов: не больше трёх, лишние — «+N» (№51).
const TOASTS_MAX: usize = 3;
const W_TOAST: f32 = 360.0;

/// Обработчик клика (команда в Core).
pub(super) type Handler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub(super) type Click<'a> = &'a dyn Fn(Command) -> Handler;
/// Клик с цепочкой команд (меню: закрыть, панель, пункт).
pub(super) type ClickAll<'a> = &'a dyn Fn(Vec<Command>) -> Handler;

/// Тост с дедлайном.
pub(super) struct Toast {
    pub level: Level,
    pub text: String,
    pub until: Instant,
}

impl Toast {
    pub(super) fn new(level: Level, text: String, now: Instant) -> Toast {
        let ttl = match level {
            Level::Info => 4,
            Level::Warn => 6,
            Level::Error => 8,
        };
        Toast {
            level,
            text,
            until: now + Duration::from_secs(ttl),
        }
    }
}

/// Тосты справа сверху: не забирают фокус и ввод.
pub(super) fn toasts(list: &[Toast], theme: &Theme) -> Option<Div> {
    if list.is_empty() {
        return None;
    }
    let mut col = div()
        .absolute()
        .top(px(theme.m(Metric::HTitlebar) + 8.0))
        .right(px(16.0))
        .w(px(W_TOAST))
        .flex()
        .flex_col()
        .gap_2();
    for t in list.iter().rev().take(TOASTS_MAX) {
        let (bg, fg) = match t.level {
            Level::Info => (Role::InfoBg, Role::TextPrimary),
            Level::Warn => (Role::CautionBg, Role::CautionFg),
            Level::Error => (Role::CriticalBg, Role::CriticalFg),
        };
        col = col.child(
            div()
                .p_3()
                .rounded(px(theme.m(Metric::RadiusOverlay)))
                .border_1()
                .border_color(theme.c(Role::StrokeCard))
                .bg(theme.c(bg))
                .text_color(theme.c(fg))
                .shadow_md()
                .child(SharedString::from(t.text.clone())),
        );
    }
    if list.len() > TOASTS_MAX {
        col = col.child(
            div()
                .text_size(px(theme.m(Metric::TypeCaption)))
                .text_color(theme.c(Role::TextSecondary))
                .child(SharedString::from(format!("+{}", list.len() - TOASTS_MAX))),
        );
    }
    Some(col)
}

/// Идут ли задачи (job-бар виден, перерисовка ≤ 30 раз/с).
pub(super) fn jobs_running(s: &AppState) -> bool {
    s.jobs.iter().any(|j| matches!(j.state, JobState::Running))
}

/// Job-бар: строка на задачу — заголовок, прогресс-полоса, проценты.
pub(super) fn job_bar(s: &AppState, theme: &Theme) -> Option<Div> {
    let jobs: Vec<_> = s
        .jobs
        .iter()
        .filter(|j| matches!(j.state, JobState::Running))
        .collect();
    if jobs.is_empty() {
        return None;
    }
    let mut bar = div()
        .flex_none()
        .flex()
        .flex_col()
        .mx(px(theme.m(Metric::GapPanels)))
        .mt(px(theme.m(Metric::GapPanels)))
        .rounded(px(theme.m(Metric::RadiusOverlay)))
        .border_1()
        .border_color(theme.c(Role::StrokeCard))
        .bg(theme.c(Role::LayerPanel));
    for (n, j) in jobs.iter().enumerate() {
        let p = j.progress.snapshot();
        #[allow(clippy::cast_precision_loss, reason = "доля для полосы")]
        let frac = if p.bytes_total > 0 {
            (p.bytes_done as f64 / p.bytes_total as f64).clamp(0.0, 1.0)
        } else if p.items_total > 0 {
            (p.items_done as f64 / p.items_total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation, reason = "доля 0…1")]
        let frac = frac as f32;
        bar = bar.child(
            div()
                .h(px(theme.m(Metric::HJobRow)))
                .flex()
                .items_center()
                .gap_3()
                .px(px(theme.m(Metric::PadRowX)))
                .child(
                    div()
                        .text_color(theme.c(Role::TextSecondary))
                        .child(SharedString::from(format!("#{}", n + 1))),
                )
                .child(
                    div()
                        .w(px(240.0))
                        .flex_none()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(j.title.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(4.0))
                        .rounded(px(2.0))
                        .bg(theme.c(Role::FillControl))
                        .child(
                            div()
                                .h_full()
                                .w(relative(frac))
                                .rounded(px(2.0))
                                .bg(theme.c(Role::AccentFill)),
                        ),
                )
                .child(
                    div()
                        .w(px(48.0))
                        .flex()
                        .justify_end()
                        .text_color(theme.c(Role::TextSecondary))
                        .child(SharedString::from(format!("{:.0}%", frac * 100.0))),
                ),
        );
    }
    Some(bar)
}

pub(super) fn prompt_id(p: &Prompt) -> Value {
    Value::Int(i64::try_from(p.id.0).unwrap_or(i64::MAX))
}

/// Ответ вариантом `i`.
pub(super) fn reply(p: &Prompt, i: usize) -> Option<Command> {
    let ch = p.spec.choices.get(i)?;
    Some(
        Command::new("prompt.reply")
            .arg("id", prompt_id(p))
            .arg("choice", ch.id.as_str()),
    )
}

/// Отмена вопроса (Esc).
pub(super) fn cancel(p: &Prompt) -> Command {
    Command::new("prompt.cancel").arg("id", prompt_id(p))
}

/// Inline-вопрос над cmdline: заголовок, текст и варианты (выбранный — акцентом).
pub(super) fn prompt_bar(p: &Prompt, selected: usize, theme: &Theme, click: Click<'_>) -> Div {
    let mut choices = div().flex().flex_row().gap_2();
    for (i, ch) in p.spec.choices.iter().enumerate() {
        let on = i == selected;
        choices = choices.child(
            div()
                .id(SharedString::from(format!("choice-{i}")))
                .h(px(theme.m(Metric::HControl) + 8.0))
                .px_3()
                .flex()
                .items_center()
                .rounded(px(theme.m(Metric::RadiusControl)))
                .border_1()
                .border_color(theme.c(Role::StrokeControl))
                .bg(theme.c(if on {
                    Role::AccentFill
                } else {
                    Role::FillControl
                }))
                .text_color(theme.c(if on {
                    Role::AccentOn
                } else {
                    Role::TextPrimary
                }))
                .child(SharedString::from(ch.label.to_string()))
                .when_some(reply(p, i), |d, c| d.on_click(click(c))),
        );
    }
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap_2()
        .mx(px(theme.m(Metric::GapPanels)))
        .mt(px(theme.m(Metric::GapPanels)))
        .p_3()
        .rounded(px(theme.m(Metric::RadiusOverlay)))
        .border_1()
        .border_color(theme.c(Role::AccentFill))
        .bg(theme.c(Role::LayerPanel))
        .child(
            div()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child(SharedString::from(p.spec.title.clone())),
        )
        .when(!p.spec.body.is_empty(), |d| {
            d.child(
                div()
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(p.spec.body.clone())),
            )
        })
        .child(choices)
}

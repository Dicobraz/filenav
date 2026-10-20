//! Тосты (№51), job-бар и inline-вопрос (S2 §2.7–2.9) на GPUI (G-05).

use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, Div, SharedString, Window, div, px, relative};
use urfin_core::{AppState, Command, Entry, JobState, Level, Prompt, PromptDetail, Value};

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

/// Job-бар: строка на задачу — заголовок, прогресс-полоса, проценты, отмена.
pub(super) fn job_bar(s: &AppState, theme: &Theme, click: Click<'_>) -> Option<Div> {
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
                )
                // Отмена задачи (`job.cancel`): посреди файла — тоже (S5-06).
                .child(
                    div()
                        .id(SharedString::from(format!("job-cancel-{}", j.id.0)))
                        .w(px(24.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(theme.m(Metric::RadiusControl)))
                        .font_family("Segoe Fluent Icons")
                        .text_color(theme.c(Role::TextSecondary))
                        .hover(|d| d.bg(theme.c(Role::FillSubtleHover)))
                        .child(SharedString::from("\u{E711}"))
                        .on_click(click(
                            Command::new("job.cancel")
                                .arg("id", i64::try_from(j.id.0).unwrap_or(i64::MAX)),
                        )),
                ),
        );
    }
    Some(bar)
}

pub(super) fn prompt_id(p: &Prompt) -> Value {
    Value::Int(i64::try_from(p.id.0).unwrap_or(i64::MAX))
}

/// Выбор в видимом вопросе: вариант и флажок «для всех».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PromptSel {
    pub id: u64,
    pub sel: usize,
    pub all: bool,
}

impl PromptSel {
    pub(super) const NONE: PromptSel = PromptSel {
        id: u64::MAX,
        sel: 0,
        all: false,
    };

    /// Выбор для вопроса `p`: прежний или вариант по умолчанию.
    pub(super) fn of(self, p: &Prompt) -> PromptSel {
        if self.id == p.id.0 {
            return self;
        }
        let n = p.spec.choices.len();
        PromptSel {
            id: p.id.0,
            sel: usize::from(p.spec.default).min(n.saturating_sub(1)),
            all: false,
        }
    }
}

/// Ответ вариантом `i`; `all` — «для всех» (если вопрос это допускает).
pub(super) fn reply(p: &Prompt, i: usize, all: bool) -> Option<Command> {
    let ch = p.spec.choices.get(i)?;
    let c = Command::new("prompt.reply")
        .arg("id", prompt_id(p))
        .arg("choice", ch.id.as_str());
    Some(if all && p.spec.allow_all {
        c.arg("all", true)
    } else {
        c
    })
}

/// Отмена вопроса (Esc).
pub(super) fn cancel(p: &Prompt) -> Command {
    Command::new("prompt.cancel").arg("id", prompt_id(p))
}

/// Inline-вопрос над cmdline: заголовок, текст, для конфликта — новый и
/// существующий файл (размер, дата), варианты с клавишей (выбранный — акцентом)
/// и флажок «для всех» (Пробел; Shift+клавиша — ответ для всех).
pub(super) fn prompt_bar(
    p: &Prompt,
    s: PromptSel,
    theme: &Theme,
    click: Click<'_>,
    toggle_all: Handler,
) -> Div {
    let i18n = urfin_i18n::current();
    let mut choices = div().flex().flex_row().flex_wrap().gap_2();
    for (i, ch) in p.spec.choices.iter().enumerate() {
        let on = i == s.sel;
        let key = ch.key.to_uppercase().to_string();
        choices = choices.child(
            div()
                .id(SharedString::from(format!("choice-{i}")))
                .h(px(theme.m(Metric::HControl) + 8.0))
                .px_3()
                .flex()
                .items_center()
                .gap_2()
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
                .when(ch.key.is_alphanumeric(), |d| {
                    d.child(
                        div()
                            .text_size(px(theme.m(Metric::TypeCaption)))
                            .text_color(theme.c(if on {
                                Role::AccentOn
                            } else {
                                Role::TextTertiary
                            }))
                            .child(SharedString::from(key)),
                    )
                })
                .when_some(reply(p, i, s.all), |d, c| d.on_click(click(c))),
        );
    }
    let mut bar = div()
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
        });
    if let Some(PromptDetail::Conflict { src, dst, .. }) = &p.spec.detail {
        bar = bar.child(conflict(src, dst, theme));
    }
    bar = bar.child(choices);
    if p.spec.allow_all {
        let glyph = if s.all { "\u{E73A}" } else { "\u{E739}" };
        bar = bar.child(
            div()
                .id("prompt-all")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .font_family("Segoe Fluent Icons")
                        .text_color(theme.c(if s.all {
                            Role::AccentText
                        } else {
                            Role::TextSecondary
                        }))
                        .child(SharedString::from(glyph)),
                )
                .child(SharedString::from(i18n.get("ui-prompt-all")))
                .child(
                    div()
                        .text_size(px(theme.m(Metric::TypeCaption)))
                        .text_color(theme.c(Role::TextTertiary))
                        .child(SharedString::from(i18n.get("ui-prompt-all-hint"))),
                )
                .on_click(toggle_all),
        );
    }
    bar
}

/// Новый и существующий файл: размер и дата; что новее и больше — акцентом.
fn conflict(src: &Entry, dst: &Entry, theme: &Theme) -> Div {
    let i18n = urfin_i18n::current();
    let row = |label: String, e: &Entry, newer: bool, larger: bool| {
        let mut size = String::new();
        theme.fmt.write_size(&mut size, e.size);
        let mut date = crate::fmt_cell::Cell::default();
        theme.fmt.write_datetime(&mut date, e.mtime);
        let hi = |on: bool| {
            theme.c(if on {
                Role::AccentText
            } else {
                Role::TextPrimary
            })
        };
        div()
            .flex()
            .flex_row()
            .gap_3()
            .child(
                div()
                    .w(px(120.0))
                    .text_color(theme.c(Role::TextSecondary))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .w(px(100.0))
                    .text_color(hi(larger))
                    .child(SharedString::from(size)),
            )
            .child(
                div()
                    .text_color(hi(newer))
                    .child(SharedString::from(date.as_str().to_owned())),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(row(
            i18n.get("ui-conflict-new"),
            src,
            src.mtime > dst.mtime,
            src.size > dst.size,
        ))
        .child(row(
            i18n.get("ui-conflict-old"),
            dst,
            dst.mtime > src.mtime,
            dst.size > src.size,
        ))
}

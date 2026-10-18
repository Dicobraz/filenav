//! Job-бар (S2 §2.9, S2-17): строка на задачу над баннером-вопросом и cmdline.
//!
//! Глиф состояния, «#id заголовок», прогресс-бар 4 px, проценты (`≤`, пока итог не
//! посчитан), объём или число элементов (`≥` у итога), скорость (EMA за 3 с) или фаза,
//! ETA, отложенные вопросы, кнопки «пауза/продолжить» и «отмена». Завершённые видны
//! ещё 5 с. Больше трёх задач — одна свёрнутая строка, Ctrl+J (`ui.jobs.toggle`)
//! разворачивает до трёх.

use std::time::{Duration, Instant};

use egui::{Align2, CornerRadius, Rect, Sense, pos2};
use rustc_hash::FxHashMap;
use urfin_core::{AppState, Command, Job, JobId, JobState, Phase, ProgressSnap, PromptId, Value};
use urfin_i18n::{FluentArgs, I18n};

use crate::canvas::Canvas;
use crate::fonts::Font;
use crate::icons;
use crate::state::UiState;
use crate::text::Tx;
use crate::tokens::{Metric, Role, Tokens};

/// Сколько видна завершённая задача.
pub const DONE_VISIBLE: Duration = Duration::from_secs(5);
/// Строк задач до сворачивания (S2 §2.9).
pub const ROWS_MAX: usize = 3;
/// Постоянная времени EMA скорости.
const EMA_TAU: f64 = 3.0;
/// Кнопки строки (S2 §2.9).
const JOB_BUTTON: f32 = 24.0;
/// Окно снапшотов для мгновенной скорости.
const RING: usize = 16;

/// Скорость задачи: кольцо снапшотов фиксированного размера и EMA (S2-17).
#[derive(Clone, Debug, Default)]
pub struct JobSpeed {
    ring: [(Option<Instant>, u64); RING],
    head: usize,
    ema: Option<f64>,
    last: Option<Instant>,
}

impl JobSpeed {
    /// Снапшот `bytes` в момент `now` (раз в кадр).
    pub fn push(&mut self, now: Instant, bytes: u64) {
        // Мгновенная скорость — по самому старому снапшоту в кольце (сглаживает кадры).
        let oldest = self.ring[self.head];
        if let (Some(t0), Some(last)) = (oldest.0, self.last) {
            let span = now.duration_since(t0).as_secs_f64();
            let dt = now.duration_since(last).as_secs_f64();
            if span > 0.0 && dt > 0.0 {
                #[allow(clippy::cast_precision_loss, reason = "байты в f64 — для скорости")]
                let rate = bytes.saturating_sub(oldest.1) as f64 / span;
                let alpha = 1.0 - (-dt / EMA_TAU).exp();
                self.ema = Some(self.ema.map_or(rate, |e| e + alpha * (rate - e)));
            }
        }
        self.ring[self.head] = (Some(now), bytes);
        self.head = (self.head + 1) % RING;
        self.last = Some(now);
    }

    /// Байт в секунду.
    pub fn speed(&self) -> Option<f64> {
        self.ema.filter(|s| *s > 0.0)
    }

    /// Время до конца для `remaining` байт.
    pub fn eta(&self, remaining: u64) -> Option<Duration> {
        #[allow(clippy::cast_precision_loss, reason = "оценка")]
        let secs = remaining as f64 / self.speed()?;
        secs.is_finite()
            .then(|| Duration::from_secs_f64(secs.min(1e7)))
    }
}

/// Задачи на экране: идущие и завершённые не раньше `DONE_VISIBLE` назад.
pub(crate) fn visible(s: &AppState, now: Instant) -> Vec<&Job> {
    s.jobs
        .iter()
        .filter(|j| {
            j.state.is_active()
                || j.finished
                    .is_some_and(|f| now.saturating_duration_since(f) < DONE_VISIBLE)
        })
        .collect()
}

/// Строк job-бара: до трёх; больше — одна свёрнутая или три развёрнутых.
pub(crate) fn rows(n: usize, expanded: bool) -> usize {
    if n <= ROWS_MAX {
        n
    } else if expanded {
        ROWS_MAX
    } else {
        1
    }
}

/// Состояние строки.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Queued,
    Running,
    Paused,
    Waiting,
    Done,
    Failed,
    Cancelled,
}

/// Что рисует строка (из `Job` или в тестах — напрямую).
#[derive(Clone, Debug)]
pub(crate) struct RowView {
    pub id: JobId,
    pub title: String,
    pub kind: Kind,
    pub snap: ProgressSnap,
    /// Байт в секунду и ETA (`JobSpeed`).
    pub speed: Option<f64>,
    pub eta: Option<Duration>,
    /// Вопросов ждёт: отложенные и открытый.
    pub questions: u32,
    /// Свёрнутый вопрос задачи — клик по строке его разворачивает.
    pub hidden_prompt: Option<PromptId>,
}

impl RowView {
    pub(crate) fn of(j: &Job, st: &UiState, s: &AppState, speed: Option<&JobSpeed>) -> Self {
        let snap = j.progress.snapshot();
        let kind = match &j.state {
            JobState::Queued => Kind::Queued,
            JobState::Running => Kind::Running,
            JobState::Paused => Kind::Paused,
            JobState::Waiting(_) => Kind::Waiting,
            JobState::Done(_) => Kind::Done,
            JobState::Failed(_) => Kind::Failed,
            JobState::Cancelled => Kind::Cancelled,
        };
        let hidden_prompt = s
            .prompts
            .iter()
            .find(|p| p.job == Some(j.id) && st.prompt_hidden.contains(&p.id))
            .map(|p| p.id);
        let open = u32::from(matches!(kind, Kind::Waiting));
        let remaining = snap.bytes_total.saturating_sub(snap.bytes_done);
        RowView {
            id: j.id,
            title: j.title.clone(),
            kind,
            speed: speed.and_then(JobSpeed::speed),
            eta: speed.and_then(|sp| sp.eta(remaining)),
            questions: snap.parked + open,
            snap,
            hidden_prompt,
        }
    }

    fn active(&self) -> bool {
        matches!(
            self.kind,
            Kind::Queued | Kind::Running | Kind::Paused | Kind::Waiting
        )
    }
}

fn glyph(k: Kind, questions: u32) -> (char, Role) {
    match k {
        Kind::Waiting => ('?', Role::CautionFg),
        _ if questions > 0 => ('?', Role::CautionFg),
        Kind::Queued | Kind::Running => (icons::PLAY, Role::AccentFill),
        Kind::Paused => (icons::PAUSE, Role::TextSecondary),
        Kind::Done => (icons::ACCEPT, Role::SuccessFg),
        Kind::Failed => (icons::ERROR_BADGE, Role::CriticalFg),
        Kind::Cancelled => (icons::CANCEL, Role::TextSecondary),
    }
}

/// Доля сделанного (0..=1): по байтам, без них — по элементам.
fn fraction(s: &ProgressSnap) -> Option<f32> {
    #[allow(clippy::cast_precision_loss, reason = "доля для полосы")]
    let f = |done: u64, total: u64| (done as f64 / total as f64).clamp(0.0, 1.0) as f32;
    if s.bytes_total > 0 {
        Some(f(s.bytes_done, s.bytes_total))
    } else if s.items_total > 0 {
        Some(f(s.items_done, s.items_total))
    } else {
        None
    }
}

/// «71 %», «≤12 %».
fn percent(s: &ProgressSnap) -> Option<String> {
    let p = (fraction(s)? * 100.0).floor();
    Some(format!("{}{p} %", if s.total_final { "" } else { "≤" }))
}

/// «3,1 ГБ / 4,4 ГБ» или «14 210/≥118 000».
fn volume(i18n: &I18n, s: &ProgressSnap) -> String {
    let ge = if s.total_final { "" } else { "≥" };
    if s.bytes_total > 0 {
        format!(
            "{} / {ge}{}",
            urfin_i18n::fmt::bytes_with(i18n, s.bytes_done),
            urfin_i18n::fmt::bytes_with(i18n, s.bytes_total)
        )
    } else if s.items_total > 0 {
        format!(
            "{}/{ge}{}",
            i18n.num_fmt().int(s.items_done),
            i18n.num_fmt().int(s.items_total)
        )
    } else {
        String::new()
    }
}

/// Скорость, а в фазах без передачи — название фазы.
fn speed_text(i18n: &I18n, r: &RowView) -> String {
    let key = match r.snap.phase {
        Phase::Transfer => {
            return r.speed.map_or_else(String::new, |b| {
                let mut a = FluentArgs::new();
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "скорость положительна и меньше u64::MAX"
                )]
                a.set("size", urfin_i18n::fmt::bytes_with(i18n, b as u64));
                i18n.fmt("ui-job-speed", &a)
            });
        }
        Phase::Scan => "ui-job-phase-scan",
        Phase::Verify => "ui-job-phase-verify",
        Phase::Finalize => "ui-job-phase-finalize",
        Phase::Recover => "ui-job-phase-recover",
    };
    i18n.get(key)
}

/// «≈ 35 с», «≈ 4 мин», «≈ 1 ч 20 мин».
fn eta_text(i18n: &I18n, d: Duration) -> String {
    let secs = d.as_secs();
    let mut a = FluentArgs::new();
    let key = if secs < 60 {
        a.set("s", secs.max(1));
        "ui-job-eta-sec"
    } else if secs < 3600 {
        a.set("m", secs / 60);
        "ui-job-eta-min"
    } else {
        a.set("h", secs / 3600);
        a.set("m", secs % 3600 / 60);
        "ui-job-eta-hour"
    };
    i18n.fmt(key, &a)
}

fn id_value(id: u64) -> Value {
    Value::Int(i64::try_from(id).unwrap_or(i64::MAX))
}

/// Кадр job-бара в строках `rows`.
#[allow(clippy::too_many_arguments, reason = "холст, ui, токены, строки, вид")]
pub(crate) fn show(
    c: &mut dyn Canvas,
    ui: &egui::Ui,
    i18n: &I18n,
    t: &Tokens,
    rows: &[Rect],
    views: &[RowView],
    expanded: bool,
    out: &mut Vec<Command>,
) {
    if rows.is_empty() {
        return;
    }
    if views.len() > ROWS_MAX && !expanded {
        collapsed(c, ui, i18n, t, rows[0], views, out);
        return;
    }
    for (r, v) in rows.iter().zip(views) {
        row(c, ui, i18n, t, *r, v, out);
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "строка задачи слева направо; геометрия: c, t, r, v, g, y, b"
)]
fn row(
    c: &mut dyn Canvas,
    ui: &egui::Ui,
    i18n: &I18n,
    t: &Tokens,
    r: Rect,
    v: &RowView,
    out: &mut Vec<Command>,
) {
    c.region("job", r);
    let pad = t.m(Metric::PadRowX);
    let caption = t.m(Metric::TypeCaption);
    let body = t.m(Metric::TypeBody);
    let y = r.center().y;
    let (g, role) = glyph(v.kind, v.questions);
    let gx = r.left() + pad + caption / 2.0;
    if g == '?' {
        c.text(
            pos2(gx, y),
            Align2::CENTER_CENTER,
            &Tx::user("?"),
            Font::Strong,
            body,
            role,
        );
    } else {
        c.icon(pos2(gx, y), g, caption, role);
    }
    // Кнопки справа: пауза/продолжить и отмена — у идущих.
    let btn = JOB_BUTTON;
    let mut right = r.right() - pad;
    if v.active() {
        let radius = CornerRadius::from(t.m(Metric::RadiusControl));
        let buttons = [
            (
                icons::CANCEL,
                Command::new("job.cancel").arg("id", id_value(v.id.0)),
            ),
            if v.kind == Kind::Paused {
                (
                    icons::PLAY,
                    Command::new("job.resume").arg("id", id_value(v.id.0)),
                )
            } else {
                (
                    icons::PAUSE,
                    Command::new("job.pause").arg("id", id_value(v.id.0)),
                )
            },
        ];
        for (k, (icon, cmd)) in buttons.into_iter().enumerate() {
            let b =
                Rect::from_min_max(pos2(right - btn, y - btn / 2.0), pos2(right, y + btn / 2.0));
            let id = ui.scope_id().with(("job-btn", v.id.0, k));
            let resp = ui.interact(b, id, Sense::click());
            if resp.hovered() {
                c.rect(b, Role::FillSubtleHover, radius);
            }
            c.icon(b.center(), icon, caption, Role::TextSecondary);
            if resp.clicked() {
                out.push(cmd);
            }
            right = b.left() - t.m(Metric::SpaceXs);
        }
        right -= t.m(Metric::SpaceM);
    }
    // Справа налево: вопросы, ETA, скорость/фаза, объём, проценты.
    let mut tail: Vec<(String, Role)> = Vec::new();
    if v.questions > 0 {
        let mut a = FluentArgs::new();
        a.set("count", v.questions);
        tail.push((i18n.fmt("ui-job-questions", &a), Role::CautionFg));
    }
    if v.active()
        && v.snap.phase == Phase::Transfer
        && let Some(d) = v.eta
    {
        tail.push((eta_text(i18n, d), Role::TextSecondary));
    }
    if v.active() {
        let sp = speed_text(i18n, v);
        if !sp.is_empty() {
            tail.push((sp, Role::TextSecondary));
        }
    }
    let vol = volume(i18n, &v.snap);
    if !vol.is_empty() {
        tail.push((vol, Role::TextSecondary));
    }
    if let Some(p) = percent(&v.snap) {
        tail.push((p, Role::TextPrimary));
    }
    for (text, role) in &tail {
        let tr = c.text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            &Tx::user(text.as_str()),
            Font::Caption,
            caption,
            *role,
        );
        right = tr.left() - t.m(Metric::SpaceM);
    }
    // Полоса 4 px: не уже 120, тянется.
    let bar_w = 120.0_f32
        .max((right - r.left()) * 0.25)
        .min(right - r.left() - 160.0)
        .max(0.0);
    let bar = Rect::from_min_max(pos2(right - bar_w, y - 2.0), pos2(right, y + 2.0));
    if bar_w > 0.0 {
        let radius = CornerRadius::from(2.0);
        c.rect(bar, Role::StrokeDivider, radius);
        if let Some(f) = fraction(&v.snap) {
            c.rect(
                bar.with_max_x(bar.left() + bar.width() * f),
                Role::AccentFill,
                radius,
            );
        }
    }
    let title_x = gx + caption;
    c.text_clipped(
        pos2(title_x, y),
        (bar.left() - t.m(Metric::SpaceM) - title_x).max(0.0),
        &Tx::user(format!("#{} {}", v.id.0, v.title)),
        Font::Ui,
        body,
        Role::TextPrimary,
    );
    // Клик по строке: свёрнутый вопрос — развернуть, иначе — проблемы задачи (S5).
    let hit = r.with_max_x(bar.right());
    let id = ui.scope_id().with(("job-row", v.id.0));
    if ui.interact(hit, id, Sense::click()).clicked() {
        out.push(match v.hidden_prompt {
            Some(p) => Command::new("ui.prompt.show").arg("id", id_value(p.0)),
            None => Command::new("ops.issues").arg("job", id_value(v.id.0)),
        });
    }
}

/// «Задачи 5 · ▶ #1 71 % ≈ 35 с · ? #2 2 вопроса · ⏸ #3 · ✓ #4 · ✗ #5   Ctrl+J ˄».
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, a, p, y")]
fn collapsed(
    c: &mut dyn Canvas,
    ui: &egui::Ui,
    i18n: &I18n,
    t: &Tokens,
    r: Rect,
    views: &[RowView],
    out: &mut Vec<Command>,
) {
    c.region("jobs.collapsed", r);
    let pad = t.m(Metric::PadRowX);
    let caption = t.m(Metric::TypeCaption);
    let y = r.center().y;
    let mut a = FluentArgs::new();
    a.set("count", views.len());
    let mut parts = vec![i18n.fmt("ui-jobs-count", &a)];
    for v in views {
        let mark = match glyph(v.kind, v.questions).0 {
            '?' => "?",
            g if g == icons::PLAY => "▶",
            g if g == icons::PAUSE => "⏸",
            g if g == icons::ACCEPT => "✓",
            _ => "✗",
        };
        let mut p = format!("{mark} #{}", v.id.0);
        if v.questions > 0 {
            let mut a = FluentArgs::new();
            a.set("count", v.questions);
            p.push(' ');
            p.push_str(&i18n.fmt("ui-job-questions", &a));
        } else if v.kind == Kind::Running {
            if let Some(pc) = percent(&v.snap) {
                p.push(' ');
                p.push_str(&pc);
            }
            if let Some(d) = v.eta {
                p.push(' ');
                p.push_str(&eta_text(i18n, d));
            }
        }
        parts.push(p);
    }
    let tr = c.text(
        pos2(r.right() - pad, y),
        Align2::RIGHT_CENTER,
        &Tx::user("Ctrl+J ˄"),
        Font::Caption,
        caption,
        Role::TextSecondary,
    );
    c.text_clipped(
        pos2(r.left() + pad, y),
        tr.left() - t.m(Metric::SpaceL) - r.left() - pad,
        &Tx::user(parts.join("  ·  ")),
        Font::Caption,
        caption,
        Role::TextPrimary,
    );
    let id = ui.scope_id().with("jobs-collapsed");
    if ui.interact(r, id, Sense::click()).clicked() {
        out.push(Command::new("ui.jobs.toggle"));
    }
}

/// Скорости живых задач: снапшот на кадр, ушедшие — убрать.
pub(crate) fn track(speeds: &mut FxHashMap<JobId, JobSpeed>, jobs: &[&Job], now: Instant) {
    speeds.retain(|id, _| jobs.iter().any(|j| j.id == *id));
    for j in jobs {
        if j.state.is_active() {
            speeds
                .entry(j.id)
                .or_default()
                .push(now, j.progress.snapshot().bytes_done);
        }
    }
}

/// Когда перерисовать, чтобы убрать завершённые (иначе — ничего: 0% CPU в простое).
pub(crate) fn next_expiry(jobs: &[&Job], now: Instant) -> Option<Duration> {
    jobs.iter()
        .filter_map(|j| j.finished)
        .map(|f| DONE_VISIBLE.saturating_sub(now.saturating_duration_since(f)))
        .filter(|d| !d.is_zero())
        .min()
}

#[cfg(test)]
#[path = "job_bar_tests.rs"]
mod tests;

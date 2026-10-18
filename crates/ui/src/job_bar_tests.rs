use std::time::{Duration, Instant};

use egui::{Rect, pos2, vec2};
use urfin_core::{JobId, Phase, ProgressSnap};

use super::*;
use crate::canvas::RecordCanvas;

fn i18n() -> urfin_i18n::I18n {
    urfin_i18n::Catalog::embedded()
        .build(&"ru".parse().unwrap())
        .0
}

/// Запись холста и команды job-бара для строк `views`.
fn frame(views: &[RowView], expanded: bool) -> (String, Vec<String>) {
    let i18n = i18n();
    let t = Tokens::default();
    let n = rows(views.len(), expanded);
    #[allow(clippy::cast_precision_loss, reason = "строк — единицы")]
    let rects: Vec<Rect> = (0..n)
        .map(|i| Rect::from_min_size(pos2(8.0, 600.0 + 32.0 * i as f32), vec2(1264.0, 32.0)))
        .collect();
    let ctx = egui::Context::default();
    let mut yaml = String::new();
    let mut out = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
        ..Default::default()
    };
    let mut full = ctx.run_ui(input, |root| {
        let mut c = RecordCanvas::new(&i18n);
        show(&mut c, root, &i18n, &t, &rects, views, expanded, &mut out);
        yaml = c.to_yaml();
    });
    full.textures_delta.clear();
    (yaml, out.iter().map(ToString::to_string).collect())
}

const GB: u64 = 1 << 30;
const MB: u64 = 1 << 20;
const MBF: f64 = 1_048_576.0;

fn copy_view() -> RowView {
    RowView {
        id: JobId(1),
        title: r"Копирование · C:\iso → E:\".into(),
        kind: Kind::Running,
        snap: ProgressSnap {
            bytes_done: 3_329_000_000,
            bytes_total: 4_724_464_025,
            phase: Phase::Transfer,
            total_final: true,
            ..ProgressSnap::default()
        },
        speed: Some(38.0 * MBF),
        eta: Some(Duration::from_secs(35)),
        questions: 0,
        hidden_prompt: None,
    }
}

fn move_view() -> RowView {
    RowView {
        id: JobId(2),
        title: r"Перенос · D:\tmp\build → F:\old".into(),
        kind: Kind::Waiting,
        snap: ProgressSnap {
            items_done: 14_210,
            items_total: 118_000,
            phase: Phase::Scan,
            total_final: false,
            parked: 1,
            ..ProgressSnap::default()
        },
        speed: None,
        eta: None,
        questions: 2,
        hidden_prompt: None,
    }
}

/// §2.9: две задачи — копирование и перенос с вопросами и неокончательным итогом.
#[test]
fn snapshot_two_jobs() {
    let (yaml, _) = frame(&[copy_view(), move_view()], false);
    for needle in [
        "#1 Копирование",
        "70 %",
        "38,0 МБ/с",
        "≈ 35 с",
        "≤12 %",
        // Группировка — неразрывный пробел (в YAML — `\u{a0}`).
        r"14\u{a0}210/≥118\u{a0}000",
        "Подсчёт",
        "2 вопроса",
    ] {
        assert!(yaml.contains(needle), "{needle}: {yaml}");
    }
    insta::assert_snapshot!(yaml);
}

/// 4+ задач — одна свёрнутая строка; клик — `ui.jobs.toggle`; развёрнуто — три.
#[test]
fn collapsed_and_expanded() {
    let mut views = vec![copy_view(), move_view()];
    for (id, kind) in [(3, Kind::Paused), (4, Kind::Done), (5, Kind::Failed)] {
        let mut v = copy_view();
        v.id = JobId(id);
        v.kind = kind;
        v.eta = None;
        views.push(v);
    }
    assert_eq!(rows(views.len(), false), 1);
    assert_eq!(rows(views.len(), true), 3);
    assert_eq!(rows(2, false), 2);
    let (yaml, _) = frame(&views, false);
    assert!(yaml.contains("Задачи 5"), "{yaml}");
    assert!(yaml.contains("? #2 2 вопроса"), "{yaml}");
    insta::assert_snapshot!(yaml);
    let (yaml, _) = frame(&views, true);
    assert_eq!(yaml.matches("region: ").count(), 3, "{yaml}");
}

/// Завершённая — без кнопок, с глифом; пауза — кнопка «продолжить».
#[test]
fn finished_and_paused_rows() {
    let mut done = copy_view();
    done.kind = Kind::Done;
    let (yaml, _) = frame(&[done], false);
    assert!(yaml.contains("success.fg"), "{yaml}");
    assert!(!yaml.contains("38,0 МБ/с"), "no speed when done: {yaml}");
}

/// ETA на синтетике (10 МБ/с с шумом ±30 % по кадрам): предсказанное время конца
/// держится в ±10 % за 5 с.
#[test]
fn eta_is_stable() {
    let mut sp = JobSpeed::default();
    let t0 = Instant::now();
    let total: u64 = 400 * MB;
    let mut done: u64 = 0;
    let mut ends = Vec::new();
    // Детерминированный «шум»: пила ±30 %.
    for frame in 0..600u32 {
        let now = t0 + Duration::from_millis(33 * u64::from(frame));
        let noise = 0.7 + 0.6 * f64::from(frame % 7) / 6.0;
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "тест"
        )]
        let step = (10.0 * MBF * 0.033 * noise) as u64;
        done = (done + step).min(total);
        sp.push(now, done);
        if (150..300).contains(&frame) {
            let eta = sp.eta(total - done).expect("speed known");
            ends.push(now.duration_since(t0) + eta);
        }
    }
    let min = ends.iter().min().unwrap().as_secs_f64();
    let max = ends.iter().max().unwrap().as_secs_f64();
    assert!(
        max / min < 1.10,
        "predicted end drifts: {min:.1}..{max:.1} s"
    );
    assert!(sp.speed().unwrap() > 9.0 * MBF && sp.speed().unwrap() < 11.0 * MBF);
}

/// Без задач — ни строк, ни перерисовки.
#[test]
fn idle_has_no_rows_or_repaint() {
    assert_eq!(rows(0, false), 0);
    assert_eq!(next_expiry(&[], Instant::now()), None);
    let (yaml, out) = frame(&[], false);
    assert!(yaml.is_empty() && out.is_empty());
    let _ = GB;
}

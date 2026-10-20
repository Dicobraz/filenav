use std::sync::Arc;

use egui::{Event, Key, Modifiers, Rect, pos2, vec2};
use smallvec::SmallVec;
use urfin_core::{
    Anchor, AppState, Attrs, Choice, Entry, EntryKind, Gen, JobId, Listing, ListingStatus,
    Location, Panel, PromptDetail, PromptSpec, Tab, TabId,
};

use super::*;
use crate::legacy::canvas::RecordCanvas;
use crate::legacy::sys::Zone;

/// 14.10.26 09:12 UTC.
const T0: i64 = 1_791_969_120 * 1_000_000_000;
const DAY: i64 = 86_400 * 1_000_000_000;

fn mock_dir(path: &str) -> Location {
    Location {
        backend: "test".into(),
        path: path.into(),
        outer: None,
    }
}

fn file(name: &str, size: u64, mtime: i64) -> Entry {
    Entry {
        name: name.into(),
        size,
        mtime,
        attrs: Attrs::default(),
        kind: EntryKind::File,
    }
}

/// Слева три помеченных файла (49,6 КБ), справа — каталог назначения.
fn state() -> AppState {
    let entries = vec![
        file("Cargo.lock", 48_213, T0),
        file("Cargo.toml", 1_229, T0),
        file("deny.toml", 1_331, T0),
        file("README.md", 3_891, T0),
    ];
    let mut left = Tab::new(TabId(1), mock_dir("/work/urfin"));
    left.listing = Listing::with_entries(entries, (0..4).collect(), Gen(1));
    left.listing.status = ListingStatus::Complete;
    for idx in 0..3 {
        let e = left.listing.entries[idx as usize].clone();
        left.selection.set(idx, true, &e);
    }
    let right = Tab::new(TabId(2), mock_dir("/backup/reports"));
    AppState::new(Panel::new(left), Panel::new(right))
}

fn choice(id: &str, label: &str, key: char) -> Choice {
    Choice {
        id: id.into(),
        label: label.into(),
        key,
    }
}

/// Конфликт «файл уже существует» (S5 §2.2, S2 §2.8).
fn conflict_spec() -> PromptSpec {
    PromptSpec {
        anchor: Anchor::JobBar,
        title: "Копирование #2".into(),
        body: String::new(),
        choices: SmallVec::from_vec(vec![
            choice("overwrite", "Заменить", 'r'),
            choice("skip", "Пропустить", 's'),
            choice("rename_new", "Переименовать", 'n'),
            choice("new_name", "Другое имя…", 'e'),
            choice("abort", "Прервать задачу", 'q'),
        ]),
        default: 0,
        input: None,
        allow_all: true,
        detail: Some(PromptDetail::Conflict {
            src: file("report-q3.xlsx", 2_097_152, T0),
            dst: file("report-q3.xlsx", 11_980, T0 - DAY),
            index: 3,
            total: 37,
        }),
        secret: false,
    }
}

fn simple_spec(allow_all: bool) -> PromptSpec {
    PromptSpec {
        anchor: Anchor::JobBar,
        title: "Идут задачи".into(),
        body: "Выйти и отменить их?".into(),
        choices: SmallVec::from_vec(vec![
            choice("stay", "Остаться", 'o'),
            choice("quit", "Выйти", 'x'),
        ]),
        default: 0,
        input: None,
        allow_all,
        detail: None,
        secret: false,
    }
}

struct Fx {
    ctx: egui::Context,
    i18n: urfin_i18n::I18n,
    tokens: Tokens,
    fonts: FontSet,
    fmt: FmtCfg,
    memo: BarMemo,
}

impl Fx {
    fn new() -> Self {
        let i18n = urfin_i18n::Catalog::embedded()
            .build(&"ru".parse().unwrap())
            .0;
        let fmt = FmtCfg::new(&i18n, false, Zone::utc());
        Fx {
            ctx: egui::Context::default(),
            i18n,
            tokens: Tokens::default(),
            fonts: FontSet::default(),
            fmt,
            memo: BarMemo::default(),
        }
    }

    /// Кадр баннера 1264 в ширину: запись холста и команды.
    fn frame(&mut self, s: &AppState, st: &UiState, events: Vec<Event>) -> (String, Vec<String>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
            events,
            ..Default::default()
        };
        let mut yaml = String::new();
        let mut out = Vec::new();
        let ctx = self.ctx.clone();
        let mut full = ctx.run_ui(input, |root| {
            let b = bar(s, st).expect("bar");
            let r = Rect::from_min_size(pos2(8.0, 560.0), vec2(1264.0, height(&self.tokens, b)));
            let mut c = RecordCanvas::new(&self.i18n);
            show(
                &mut c,
                &mut BarCx {
                    ui: root,
                    i18n: &self.i18n,
                    tokens: &self.tokens,
                    fonts: &self.fonts,
                    fmt: &self.fmt,
                    memo: &mut self.memo,
                },
                r,
                b,
                s,
                &mut out,
            );
            yaml = c.to_yaml();
        });
        full.textures_delta.clear();
        (yaml, out.iter().map(ToString::to_string).collect())
    }
}

fn key(k: Key, m: Modifiers) -> Event {
    Event::Key {
        key: k,
        physical_key: Some(k),
        pressed: true,
        repeat: false,
        modifiers: m,
    }
}

fn press(k: Key) -> Event {
    key(k, Modifiers::NONE)
}

fn draft_state(cmd: &str, arg: &str) -> UiState {
    let mut st = UiState::new();
    st.draft = Some(Draft {
        cmd: cmd.into(),
        arg: arg.into(),
        text: compact_str::CompactString::default(),
    });
    st
}

/// §2.7: в корзину — тон caution, Удалить / Насовсем / Отмена.
#[test]
fn snapshot_delete_trash() {
    let mut fx = Fx::new();
    fx.memo.ops = Some(OpsDraft {
        command: "ops.delete".into(),
        permanent: false,
    });
    let (yaml, _) = fx.frame(&state(), &draft_state("ops.delete", "confirmed"), vec![]);
    assert!(yaml.contains("В корзину 3 объекта (49,6 КБ)?"), "{yaml}");
    insta::assert_snapshot!(yaml);
}

/// §2.7: насовсем — тон critical, без кнопки «Насовсем».
#[test]
fn snapshot_delete_permanent() {
    let mut fx = Fx::new();
    fx.memo.ops = Some(OpsDraft {
        command: "ops.delete".into(),
        permanent: true,
    });
    let (yaml, _) = fx.frame(&state(), &draft_state("ops.delete", "confirmed"), vec![]);
    assert!(yaml.contains("critical.bg"), "{yaml}");
    assert!(!yaml.contains("Насовсем"));
    insta::assert_snapshot!(yaml);
}

/// §2.7: копирование — поле пути, новая папка — поле имени и подсказка.
#[test]
fn snapshot_copy_and_mkdir_drafts() {
    let mut fx = Fx::new();
    let (copy, _) = fx.frame(&state(), &draft_state("ops.copy", "dest"), vec![]);
    assert!(copy.contains("Копировать 3 объекта (49,6 КБ) в"), "{copy}");
    insta::assert_snapshot!("copy", copy);
    let mut fx = Fx::new();
    let (mkdir, _) = fx.frame(&state(), &draft_state("ops.mkdir", "name"), vec![]);
    assert!(
        mkdir.contains("Новая папка") && mkdir.contains(r"можно a\\b\\c"),
        "{mkdir}"
    );
    insta::assert_snapshot!("mkdir", mkdir);
}

/// §2.8: конфликт — шапка, «ещё 2 вопроса», таблица, дифф, кнопки, «Для всех».
#[test]
fn snapshot_conflict_with_queue() {
    let mut fx = Fx::new();
    let mut s = state();
    let _ = s.prompts.open(conflict_spec(), Some(JobId(2)));
    let _ = s.prompts.open(simple_spec(false), None);
    let _ = s.prompts.open(simple_spec(false), None);
    let (yaml, _) = fx.frame(&s, &UiState::new(), vec![]);
    for needle in [
        "Конфликт 3 из 37 · Копирование #2",
        "ещё 2 вопроса",
        "▲ новее на 1 д · +2,0 МБ",
        "Для всех",
        "Позже",
    ] {
        assert!(yaml.contains(needle), "{needle}: {yaml}");
    }
    insta::assert_snapshot!(yaml);
}

/// Enter — `default`, буква — свой выбор, ← → — фокус; без `allow_all` нет «Для всех».
#[test]
fn prompt_keys_answer() {
    let mut s = state();
    let (id, _rx) = s.prompts.open(simple_spec(false), None);
    let st = UiState::new();
    let mut fx = Fx::new();
    let (yaml, out) = fx.frame(&s, &st, vec![press(Key::Enter)]);
    assert!(!yaml.contains("Для всех"));
    assert_eq!(out, [format!("prompt.reply id={} choice=stay", id.0)]);
    let (_, out) = fx.frame(&s, &st, vec![press(Key::X)]);
    assert_eq!(out, [format!("prompt.reply id={} choice=quit", id.0)]);
    let (_, out) = fx.frame(&s, &st, vec![press(Key::ArrowRight), press(Key::Enter)]);
    assert_eq!(out, [format!("prompt.reply id={} choice=quit", id.0)]);
    // Вопрос без задачи: Esc — отказ.
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Escape)]);
    assert_eq!(out, [format!("prompt.cancel id={}", id.0)]);
}

/// Конфликт: A — «для всех», Q — прервать, Esc — «Позже» (задача идёт).
#[test]
fn conflict_keys() {
    let mut s = state();
    let (id, _rx) = s.prompts.open(conflict_spec(), Some(JobId(2)));
    let st = UiState::new();
    let mut fx = Fx::new();
    let (_, out) = fx.frame(&s, &st, vec![press(Key::A), press(Key::Enter)]);
    assert_eq!(
        out,
        [format!(
            "prompt.reply id={} choice=overwrite all=true",
            id.0
        )]
    );
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Q)]);
    assert_eq!(
        out,
        [format!("prompt.reply id={} choice=abort all=true", id.0)]
    );
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Escape)]);
    assert_eq!(out, [format!("ui.prompt.hide id={}", id.0)]);
}

/// «Другое имя…» (E): поле вместо кнопок, Enter — ответ с `input`, Esc — назад.
#[test]
fn conflict_other_name() {
    let mut s = state();
    let (id, _rx) = s.prompts.open(conflict_spec(), Some(JobId(2)));
    let st = UiState::new();
    let mut fx = Fx::new();
    let (_, out) = fx.frame(&s, &st, vec![press(Key::E)]);
    assert_eq!(out, Vec::<String>::new());
    let (yaml, _) = fx.frame(&s, &st, vec![]);
    assert!(
        yaml.contains("Новое имя") && yaml.contains("Применить"),
        "{yaml}"
    );
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Escape)]);
    assert_eq!(out, Vec::<String>::new());
    let (yaml, _) = fx.frame(&s, &st, vec![]);
    assert!(yaml.contains("Заменить"), "buttons are back: {yaml}");
    fx.frame(&s, &st, vec![press(Key::E)]);
    fx.frame(&s, &st, vec![]);
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Enter)]);
    assert_eq!(
        out,
        [format!(
            "prompt.reply id={} choice=new_name input=report-q3.xlsx",
            id.0
        )]
    );
}

/// Удаление: Enter — в корзину, Shift+Enter — насовсем; черновик закрывается.
#[test]
fn delete_draft_keys() {
    let s = state();
    let st = draft_state("ops.delete", "confirmed");
    let mut fx = Fx::new();
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Enter)]);
    assert_eq!(out, ["ops.delete confirmed=true", "ui.cancel"]);
    let (_, out) = fx.frame(&s, &st, vec![key(Key::Enter, Modifiers::SHIFT)]);
    assert_eq!(
        out,
        ["ops.delete confirmed=true permanent=true", "ui.cancel"]
    );
}

/// Маска (`ui.draft command=sel.add arg=mask`): ввод и Enter — команда с аргументом.
#[test]
fn text_draft_submits() {
    let s = state();
    let st = draft_state("sel.add", "mask");
    let mut fx = Fx::new();
    let (yaml, _) = fx.frame(&s, &st, vec![]);
    assert!(yaml.contains("Пометить по маске"), "{yaml}");
    fx.frame(&s, &st, vec![Event::Text("*.rs".into())]);
    let (_, out) = fx.frame(&s, &st, vec![press(Key::Enter)]);
    assert_eq!(out, ["sel.add mask=*.rs", "ui.cancel"]);
}

/// Свёрнутый вопрос клавиш не берёт, черновик — показывается вместо вопроса.
#[test]
fn hidden_prompt_and_priority() {
    let mut s = state();
    let (id, _rx) = s.prompts.open(simple_spec(false), Some(JobId(1)));
    let mut st = UiState::new();
    assert!(shown_prompt(&s, &st).is_some());
    st.prompt_hidden.push(id);
    assert!(shown_prompt(&s, &st).is_none());
    assert!(bar(&s, &st).is_none());
    let st = draft_state("sel.add", "mask");
    assert!(matches!(bar(&s, &st), Some(Bar::Draft(_))));
    let _ = Arc::new(());
}

/// Баннер core (S2-17): текст и кнопки действий без чипов; клик — команда действия.
#[test]
fn app_banner_actions() {
    let mut fx = Fx::new();
    let banner = urfin_core::Banner {
        level: urfin_core::Level::Error,
        text: "Прошлый запуск завершился сбоем".into(),
        actions: SmallVec::from_vec(vec![
            ("Показать логи".into(), urfin_core::Command::new("panel.go")),
            (
                "Закрыть".into(),
                urfin_core::Command::new("app.dismiss_banner"),
            ),
        ]),
    };
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 768.0))),
        ..Default::default()
    };
    let mut yaml = String::new();
    let mut out = Vec::new();
    let ctx = fx.ctx.clone();
    let mut full = ctx.run_ui(input, |root| {
        let r = Rect::from_min_size(pos2(8.0, 48.0), vec2(1264.0, 48.0));
        let mut c = RecordCanvas::new(&fx.i18n);
        app_banner(
            &mut c,
            &mut BarCx {
                ui: root,
                i18n: &fx.i18n,
                tokens: &fx.tokens,
                fonts: &fx.fonts,
                fmt: &fx.fmt,
                memo: &mut fx.memo,
            },
            r,
            &banner,
            &mut out,
        );
        yaml = c.to_yaml();
    });
    full.textures_delta.clear();
    assert!(yaml.contains("Прошлый запуск завершился сбоем"), "{yaml}");
    assert!(
        yaml.contains("Показать логи") && yaml.contains("Закрыть"),
        "{yaml}"
    );
    assert!(yaml.contains("critical.fg"), "{yaml}");
    assert_eq!(out.len(), 0);
}

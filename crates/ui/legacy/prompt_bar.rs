//! Баннер над cmdline (S2 §2.7, §2.8, S2-15): вопрос core (`prompts.active()`),
//! конфликт копирования (`PromptDetail::Conflict`) и черновик команды (`ui.draft`,
//! `ops.draft`). Fluent `InfoBar`: тон, иконка, текст, справа кнопки с чипами клавиш;
//! кнопка по умолчанию — акцентная.
//!
//! Клавиши вопроса читает этот модуль (keymap при видимом вопросе не работает):
//! Enter — кнопка в фокусе (сначала `default`), буква — `Choice.key`, ← → Tab —
//! фокус, A — «для всех», Esc — «Позже» (вопрос задачи сворачивается, задача идёт)
//! или отмена. Кнопка с «…» в подписи («Другое имя…») открывает поле ввода.
//! В черновике Esc и печать идут через keymap (`Focus::Overlay(Draft)`).

use egui::{Align2, CornerRadius, Event, Key, Modifiers, Rect, Sense, pos2, vec2};
use urfin_core::{AppState, Command, Entry, Prompt, PromptDetail, PromptId, Row, Side, Tab, Value};
use urfin_i18n::{FluentArgs, I18n};

use crate::legacy::canvas::Canvas;
use crate::legacy::fmt_cell::{Cell, FmtCfg};
use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::state::{Draft, UiState};
use crate::legacy::text::Tx;
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Иконки статуса (Segoe Fluent Icons).
mod glyph {
    pub const INFO: char = '\u{E946}';
    pub const WARNING: char = '\u{E7BA}';
    pub const DELETE: char = '\u{E74D}';
    pub const COPY: char = '\u{E8C8}';
    pub const MOVE: char = '\u{E8DE}';
    pub const FOLDER: char = '\u{E8F4}';
    pub const FILTER: char = '\u{E71C}';
    pub const CLOSE: char = '\u{E711}';
}

/// Что показывает баннер.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Bar<'s> {
    /// Вопрос и сколько ещё ждут за ним.
    Prompt(&'s Prompt, usize),
    Draft(&'s Draft),
}

/// Черновик из `ops.draft` (S0 v3 №15): аргументы исходной команды.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OpsDraft {
    pub command: String,
    pub permanent: bool,
}

/// Состояние баннера между кадрами (не в `AppState`: только отрисовка и фокус).
#[derive(Debug, Default)]
pub(crate) struct BarMemo {
    prompt: Option<PromptId>,
    focus: usize,
    all: bool,
    /// Поле ввода: текст и открыто ли («Другое имя…» — по кнопке).
    input: Option<String>,
    editing: bool,
    /// Текст черновика и для какой команды он заведён.
    draft: Option<(String, String)>,
    pub ops: Option<OpsDraft>,
}

/// Вопрос, который сейчас на экране (не свёрнут в job-бар).
pub fn shown_prompt<'s>(s: &'s AppState, st: &UiState) -> Option<&'s Prompt> {
    s.prompts
        .active()
        .filter(|p| !st.prompt_hidden.contains(&p.id))
}

/// Есть видимый вопрос: клавиши разбирает баннер, а не keymap (app).
pub fn prompt_takes_keys(s: &AppState) -> bool {
    let fallback = UiState::new();
    let st = s.ext.get::<UiState>().unwrap_or(&fallback);
    shown_prompt(s, st).is_some()
}

/// Баннер этого кадра: открытый черновик, иначе видимый вопрос.
pub(crate) fn bar<'s>(s: &'s AppState, st: &'s UiState) -> Option<Bar<'s>> {
    if let Some(d) = &st.draft {
        return Some(Bar::Draft(d));
    }
    let p = shown_prompt(s, st)?;
    Some(Bar::Prompt(p, s.prompts.len().saturating_sub(1)))
}

/// Высота: 48, конфликт — 48 + 3×24 (S2 §2.8).
pub(crate) fn height(t: &Tokens, b: Bar<'_>) -> f32 {
    let base = t.m(Metric::HBanner);
    match b {
        Bar::Prompt(p, _) if matches!(p.spec.detail, Some(PromptDetail::Conflict { .. })) => {
            base + 3.0 * t.m(Metric::HRow)
        }
        _ => base,
    }
}

/// Тон `InfoBar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Info,
    Caution,
    Critical,
}

impl Tone {
    fn bg(self) -> Role {
        match self {
            Tone::Info => Role::InfoBg,
            Tone::Caution => Role::CautionBg,
            Tone::Critical => Role::CriticalBg,
        }
    }

    fn fg(self) -> Role {
        match self {
            Tone::Info => Role::AccentText,
            Tone::Caution => Role::CautionFg,
            Tone::Critical => Role::CriticalFg,
        }
    }
}

/// Кнопка: подпись, чип клавиши, команда.
struct Button {
    label: String,
    chip: String,
    accent: bool,
    focused: bool,
    action: Action,
}

#[derive(Clone, Debug)]
enum Action {
    Cmds(Vec<Command>),
    /// Выбор вопроса `i` (с полем ввода — сначала открыть поле).
    Choice(usize),
    /// Закрыть поле ввода вопроса.
    Back,
}

/// Всё, что нужно для кадра.
pub(crate) struct BarCx<'a> {
    pub ui: &'a mut egui::Ui,
    pub i18n: &'a I18n,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub fmt: &'a FmtCfg,
    pub memo: &'a mut BarMemo,
}

/// Кадр баннера в `r`: рисует на `c`, команды — в `out`.
#[allow(clippy::many_single_char_names, reason = "геометрия: c, r, b, s")]
pub(crate) fn show(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    r: Rect,
    b: Bar<'_>,
    s: &AppState,
    out: &mut Vec<Command>,
) {
    c.region("prompt", r);
    match b {
        Bar::Prompt(p, more) => prompt(c, cx, r, p, more, s, out),
        Bar::Draft(d) => draft(c, cx, r, d, s, out),
    }
}

fn args(pairs: &[(&'static str, String)]) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    for (k, v) in pairs {
        a.set(*k, v.clone());
    }
    a
}

/// Подложка, рамка и иконка; возвращает x, с которого идёт текст.
/// Конфликт в 48 + 3×24 (S2 §2.8): шапка, таблица из трёх строк, кнопки.
const CONFLICT_HEAD: f32 = 36.0;
const CONFLICT_ROW: f32 = 20.0;

/// Строка шапки: середина 48 px баннера, у конфликта — его шапки.
fn head_y(t: &Tokens, r: Rect, conflict: bool) -> f32 {
    if conflict {
        r.top() + CONFLICT_HEAD / 2.0
    } else {
        r.top() + t.m(Metric::HBanner) / 2.0
    }
}

#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, x, y")]
fn frame(c: &mut dyn Canvas, t: &Tokens, r: Rect, tone: Tone, icon: char, top_row: f32) -> f32 {
    let radius = CornerRadius::from(t.m(Metric::RadiusOverlay));
    c.rect(r, tone.bg(), radius);
    c.stroke(r, Role::StrokeCard, radius, t.m(Metric::StrokeThin));
    let pad = t.m(Metric::SpaceL);
    let icon_size = t.m(Metric::WIcon);
    c.icon(
        pos2(r.left() + pad + icon_size / 2.0, top_row),
        icon,
        icon_size,
        tone.fg(),
    );
    r.left() + pad + icon_size + t.m(Metric::SpaceM)
}

/// Кнопки справа налево от `right` на строке `y`; клики — в действия.
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, x, y")]
fn buttons(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    right: f32,
    y: f32,
    list: &[Button],
    salt: &str,
) -> (Option<Action>, f32) {
    let t = cx.tokens;
    let h = t.m(Metric::HControl);
    let pad = t.m(Metric::SpaceM);
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    let radius = CornerRadius::from(t.m(Metric::RadiusControl));
    let mut x = right;
    let mut clicked = None;
    for (i, b) in list.iter().enumerate().rev() {
        let label_w = text_width(cx, &b.label, Font::Ui, body);
        let chip_w = if b.chip.is_empty() {
            -pad
        } else {
            text_width(cx, &b.chip, Font::Caption, caption) + pad
        };
        let w = pad + label_w + pad + chip_w + pad;
        let rect = Rect::from_min_max(pos2(x - w, y - h / 2.0), pos2(x, y + h / 2.0));
        let (bg, fg) = if b.accent {
            (Role::AccentFill, Role::AccentOn)
        } else {
            (Role::FillControl, Role::TextPrimary)
        };
        c.rect(rect, bg, radius);
        c.stroke(
            rect,
            if b.focused {
                Role::StrokeStrong
            } else {
                Role::StrokeControl
            },
            radius,
            t.m(Metric::StrokeThin),
        );
        c.text(
            pos2(rect.left() + pad, y),
            Align2::LEFT_CENTER,
            &Tx::user(b.label.as_str()),
            Font::Ui,
            body,
            fg,
        );
        let chip = Rect::from_min_max(
            pos2(rect.right() - pad - chip_w, y - caption * 0.75),
            pos2(rect.right() - pad, y + caption * 0.75),
        );
        if !b.chip.is_empty() {
            c.stroke(chip, Role::StrokeControl, radius, t.m(Metric::StrokeThin));
            c.text(
                chip.center(),
                Align2::CENTER_CENTER,
                &Tx::user(b.chip.as_str()),
                Font::Caption,
                caption,
                fg,
            );
        }
        let id = cx.ui.scope_id().with(("prompt-btn", salt, i));
        if cx.ui.interact(rect, id, Sense::click()).clicked() {
            clicked = Some(b.action.clone());
        }
        x = rect.left() - pad;
    }
    (clicked, x)
}

/// Ширина текста по шрифту egui (в тестах — оценка).
fn text_width(cx: &BarCx<'_>, s: &str, font: Font, size: f32) -> f32 {
    let id = cx.fonts.id(font, size);
    cx.ui.fonts_mut(|f| {
        f.layout_no_wrap(s.to_owned(), id, egui::Color32::WHITE)
            .size()
            .x
    })
}

// ---- вопросы ----

fn reset(cx: &mut BarCx<'_>, p: &Prompt) {
    if cx.memo.prompt == Some(p.id) {
        return;
    }
    cx.memo.prompt = Some(p.id);
    cx.memo.focus = usize::from(p.spec.default).min(p.spec.choices.len().saturating_sub(1));
    cx.memo.all = false;
    cx.memo.input.clone_from(&p.spec.input);
    // Поле открыто сразу, если в вопросе нет кнопки, которая его открывает.
    cx.memo.editing = p.spec.input.is_some() && !p.spec.choices.iter().any(opens_input);
}

/// «Другое имя…»: выбор с полем ввода.
fn opens_input(c: &urfin_core::Choice) -> bool {
    c.label.ends_with('…') || c.label.ends_with("...")
}

fn id_value(p: &Prompt) -> Value {
    Value::Int(i64::try_from(p.id.0).unwrap_or(i64::MAX))
}

fn reply(p: &Prompt, i: usize, memo: &BarMemo) -> Option<Command> {
    let ch = p.spec.choices.get(i)?;
    let mut c = Command::new("prompt.reply")
        .arg("id", id_value(p))
        .arg("choice", ch.id.as_str());
    if let Some(text) = &memo.input
        && (memo.editing || opens_input(ch))
    {
        c = c.arg("input", text.as_str());
    }
    if memo.all {
        c = c.arg("all", true);
    }
    Some(c)
}

/// «Позже» у вопроса задачи (свернуть в `?`), иначе отмена.
fn later(p: &Prompt) -> Command {
    if p.job.is_some() {
        Command::new("ui.prompt.hide").arg("id", id_value(p))
    } else {
        Command::new("prompt.cancel").arg("id", id_value(p))
    }
}

/// Буква → клавиша egui (любая раскладка: egui даёт позицию для нелатиницы).
fn key_of(ch: char) -> Option<Key> {
    Key::from_name(&ch.to_ascii_uppercase().to_string())
}

/// Клавиши вопроса: забираются из ввода egui.
/// Нажатия кадра по порядку (модификаторы — точно): `f` возвращает `true`, если
/// клавиша взята, и `Some` — чтобы остановиться. Взятые события удаляются из ввода.
fn take_keys<T>(
    ui: &egui::Ui,
    mut f: impl FnMut(Key, Modifiers) -> (bool, Option<T>),
) -> Option<T> {
    ui.input_mut(|i| {
        let mut done = None;
        i.events.retain(|e| {
            if done.is_some() {
                return true;
            }
            let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = e
            else {
                return true;
            };
            let (taken, stop) = f(*key, *modifiers);
            done = stop;
            !taken
        });
        done
    })
}

/// Клавиши вопроса.
fn prompt_keys(cx: &mut BarCx<'_>, p: &Prompt) -> Option<Action> {
    let n = p.spec.choices.len();
    let editing = cx.memo.editing;
    let (mut focus, mut all) = (cx.memo.focus, cx.memo.all);
    let none = Modifiers::NONE;
    let action = take_keys(cx.ui, |key, m| {
        let plain = m == none;
        match key {
            Key::Escape if plain => (
                true,
                Some(if editing {
                    Action::Back
                } else {
                    Action::Cmds(vec![later(p)])
                }),
            ),
            Key::Enter if plain => (true, Some(Action::Choice(focus))),
            _ if editing => (false, None),
            Key::ArrowRight | Key::Tab if plain && n > 0 => {
                focus = (focus + 1) % n;
                (true, None)
            }
            Key::ArrowLeft if plain && n > 0 => {
                focus = (focus + n - 1) % n;
                (true, None)
            }
            Key::Tab if m == Modifiers::SHIFT && n > 0 => {
                focus = (focus + n - 1) % n;
                (true, None)
            }
            _ if plain => {
                if let Some(k) = p
                    .spec
                    .choices
                    .iter()
                    .position(|c| key_of(c.key) == Some(key))
                {
                    (true, Some(Action::Choice(k)))
                } else if key == Key::A && p.spec.allow_all {
                    all = !all;
                    (true, None)
                } else {
                    (false, None)
                }
            }
            _ => (false, None),
        }
    });
    cx.memo.focus = focus;
    cx.memo.all = all;
    action
}

fn act(cx: &mut BarCx<'_>, p: &Prompt, a: Action, out: &mut Vec<Command>) {
    match a {
        Action::Cmds(cmds) => out.extend(cmds),
        Action::Back => cx.memo.editing = false,
        Action::Choice(i) => {
            let Some(ch) = p.spec.choices.get(i) else {
                return;
            };
            if opens_input(ch) && !cx.memo.editing {
                cx.memo.focus = i;
                cx.memo.editing = true;
                if cx.memo.input.is_none() {
                    cx.memo.input = Some(conflict_name(p).unwrap_or_default());
                }
                return;
            }
            if let Some(c) = reply(p, i, cx.memo) {
                out.push(c);
            }
        }
    }
}

/// Имя `src` конфликта — начальное значение «Другое имя…».
fn conflict_name(p: &Prompt) -> Option<String> {
    p.spec.detail.as_ref().map(|d| match d {
        PromptDetail::Conflict { src, .. } => src.name.to_string(),
    })
}

#[allow(clippy::too_many_lines, reason = "раскладка баннера сверху вниз")]
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, x, y")]
fn prompt(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    r: Rect,
    p: &Prompt,
    more: usize,
    s: &AppState,
    out: &mut Vec<Command>,
) {
    reset(cx, p);
    let t = cx.tokens;
    let conflict = p.spec.detail.as_ref().map(|d| match d {
        PromptDetail::Conflict {
            src,
            dst,
            index,
            total,
        } => (src, dst, *index, *total),
    });
    let tone = if conflict.is_some() {
        Tone::Caution
    } else {
        Tone::Info
    };
    let head_y = head_y(t, r, conflict.is_some());
    let x = frame(
        c,
        t,
        r,
        tone,
        if conflict.is_some() {
            glyph::WARNING
        } else {
            glyph::INFO
        },
        head_y,
    );
    let pad = t.m(Metric::SpaceL);
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    let mut action = prompt_keys(cx, p);

    // Шапка: заголовок (у конфликта — «Конфликт i из N · задача»), тело, «ещё N», ✕.
    let title = match conflict {
        Some((_, _, index, total)) => cx.i18n.fmt(
            "ui-conflict-head",
            &args(&[
                ("index", index.to_string()),
                ("total", total.to_string()),
                ("job", p.spec.title.clone()),
            ]),
        ),
        None => p.spec.title.clone(),
    };
    let tr = c.text(
        pos2(x, head_y),
        Align2::LEFT_CENTER,
        &Tx::user(title.as_str()),
        Font::Strong,
        body,
        Role::TextPrimary,
    );
    if conflict.is_none() && !p.spec.body.is_empty() {
        c.text(
            pos2(tr.right() + t.m(Metric::SpaceM), head_y),
            Align2::LEFT_CENTER,
            &Tx::user(p.spec.body.as_str()),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
    }
    let mut right = r.right() - pad;
    if p.job.is_some() && conflict.is_some() {
        let close = Rect::from_center_size(
            pos2(right - caption, head_y),
            vec2(2.0 * caption, 2.0 * caption),
        );
        c.icon(close.center(), glyph::CLOSE, caption, Role::TextSecondary);
        let id = cx.ui.scope_id().with(("prompt-close", p.id.0));
        if cx.ui.interact(close, id, Sense::click()).clicked() {
            action = Some(Action::Cmds(vec![later(p)]));
        }
        right = close.left() - pad;
    }
    if more > 0 {
        let mut a = FluentArgs::new();
        a.set("count", more);
        c.text(
            pos2(right, head_y),
            Align2::RIGHT_CENTER,
            &Tx::Args("ui-prompt-more", a),
            Font::Caption,
            caption,
            Role::TextSecondary,
        );
    }

    // Кнопки: у конфликта — отдельная строка внизу, иначе — справа в шапке.
    let row_y = if conflict.is_some() {
        r.bottom() - t.m(Metric::HControl) / 2.0 - t.m(Metric::SpaceXs)
    } else {
        head_y
    };
    let mut list: Vec<Button> = Vec::new();
    if cx.memo.editing {
        list.push(Button {
            label: cx.i18n.get("ui-button-apply"),
            chip: "Enter".into(),
            accent: true,
            focused: false,
            action: Action::Choice(cx.memo.focus),
        });
        list.push(Button {
            label: cx.i18n.get("ui-button-back"),
            chip: "Esc".into(),
            accent: false,
            focused: false,
            action: Action::Back,
        });
    } else {
        for (i, ch) in p.spec.choices.iter().enumerate() {
            list.push(Button {
                label: ch.label.to_string(),
                chip: ch.key.to_uppercase().collect(),
                accent: i == usize::from(p.spec.default),
                focused: i == cx.memo.focus,
                action: Action::Choice(i),
            });
        }
        if p.job.is_some() {
            list.push(Button {
                label: cx.i18n.get("ui-button-later"),
                chip: "Esc".into(),
                accent: false,
                focused: false,
                action: Action::Cmds(vec![later(p)]),
            });
        }
    }
    let (clicked, left_of_buttons) = buttons(c, cx, r.right() - pad, row_y, &list, "prompt");
    action = action.or(clicked);

    // Поле ввода (секрет — маской) слева от кнопок.
    if cx.memo.editing {
        let label_x = if conflict.is_some() {
            r.left() + pad
        } else {
            x
        };
        let lr = c.text(
            pos2(label_x, row_y),
            Align2::LEFT_CENTER,
            &Tx::key("ui-prompt-new-name"),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        let field = Rect::from_min_max(
            pos2(
                if conflict.is_some() {
                    lr.right() + t.m(Metric::SpaceM)
                } else {
                    (tr.right() + t.m(Metric::SpaceL)).max(lr.right() + t.m(Metric::SpaceM))
                },
                row_y - t.m(Metric::HControl) / 2.0,
            ),
            pos2(left_of_buttons, row_y + t.m(Metric::HControl) / 2.0),
        );
        let text = cx.memo.input.get_or_insert_with(String::new);
        if text_field(c, cx.ui, t, cx.fonts, field, text, p.spec.secret, p.id.0) {
            action = Some(Action::Choice(cx.memo.focus));
        }
    } else if p.spec.allow_all {
        // «Для всех» — у конфликта слева на строке кнопок.
        let y = row_y;
        let bx = if conflict.is_some() {
            r.left() + pad
        } else {
            x
        };
        let box_r = Rect::from_center_size(pos2(bx + caption / 2.0, y), vec2(caption, caption));
        let radius = CornerRadius::from(t.m(Metric::RadiusControl));
        if cx.memo.all {
            c.rect(box_r, Role::AccentFill, radius);
        }
        c.stroke(box_r, Role::StrokeStrong, radius, t.m(Metric::StrokeThin));
        let lr = c.text(
            pos2(box_r.right() + t.m(Metric::SpaceS), y),
            Align2::LEFT_CENTER,
            &Tx::key("ui-prompt-all"),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        c.text(
            pos2(lr.right() + t.m(Metric::SpaceS), y),
            Align2::LEFT_CENTER,
            &Tx::user("A"),
            Font::Caption,
            caption,
            Role::TextSecondary,
        );
        let hit = box_r.union(lr);
        let id = cx.ui.scope_id().with(("prompt-all", p.id.0));
        if cx.ui.interact(hit, id, Sense::click()).clicked() {
            cx.memo.all = !cx.memo.all;
        }
    }

    // Таблица «Новый / Есть» с диффом.
    if let Some((src, dst, _, _)) = conflict {
        conflict_table(c, cx, r, x, src, dst);
    }
    let _ = s;
    if let Some(a) = action {
        act(cx, p, a, out);
    }
}

/// Строки «Новый» и «Есть»: имя, размер, дата с годом; справа — разница.
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, x, y")]
fn conflict_table(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    r: Rect,
    x: f32,
    src: &Entry,
    dst: &Entry,
) {
    let t = cx.tokens;
    let row = CONFLICT_ROW;
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    let top = r.top() + CONFLICT_HEAD - t.m(Metric::SpaceS);
    let cols = [x, x + 80.0, x + 320.0, x + 420.0, x + 580.0];
    let head = top + row / 2.0;
    for (i, key) in ["ui-conflict-name", "ui-conflict-size", "ui-conflict-mtime"]
        .into_iter()
        .enumerate()
    {
        c.text(
            pos2(cols[i + 1], head),
            Align2::LEFT_CENTER,
            &Tx::key(key),
            Font::Caption,
            caption,
            Role::TextSecondary,
        );
    }
    for (k, (label, e)) in [("ui-conflict-new", src), ("ui-conflict-old", dst)]
        .into_iter()
        .enumerate()
    {
        #[allow(clippy::cast_precision_loss, reason = "две строки")]
        let y = top + row * (k as f32 + 1.5);
        c.text(
            pos2(cols[0], y),
            Align2::LEFT_CENTER,
            &Tx::key(label),
            Font::Ui,
            body,
            Role::TextSecondary,
        );
        c.text_clipped(
            pos2(cols[1], y),
            cols[2] - cols[1] - t.m(Metric::SpaceM),
            &Tx::user(e.name.as_str()),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        let mut size = String::new();
        cx.fmt.write_size(&mut size, e.size);
        c.text(
            pos2(cols[2], y),
            Align2::LEFT_CENTER,
            &Tx::user(size),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        let mut date = Cell::new();
        cx.fmt.write_datetime(&mut date, e.mtime);
        c.text(
            pos2(cols[3], y),
            Align2::LEFT_CENTER,
            &Tx::user(date.as_str()),
            Font::Ui,
            body,
            Role::TextPrimary,
        );
        if k == 0 {
            let (text, role) = diff(cx.i18n, cx.fmt, src, dst);
            c.text(
                pos2(cols[4], y),
                Align2::LEFT_CENTER,
                &Tx::user(text),
                Font::Ui,
                body,
                role,
            );
        }
    }
}

/// «▲ новее на 1 д · +2,0 МБ» (`success.fg`) или «▼ старше на …» (`critical.fg`).
fn diff(i18n: &I18n, fmt: &FmtCfg, src: &Entry, dst: &Entry) -> (String, Role) {
    let dt = src.mtime.saturating_sub(dst.mtime);
    let mut parts = Vec::new();
    let role = if dt == 0 || src.mtime == i64::MIN || dst.mtime == i64::MIN {
        Role::TextSecondary
    } else {
        // mtime — наносекунды (S1 `Entry`).
        let span = span(i18n, dt.unsigned_abs() / 1_000_000_000);
        let key = if dt > 0 {
            "ui-conflict-newer"
        } else {
            "ui-conflict-older"
        };
        parts.push(i18n.fmt(key, &args(&[("span", span)])));
        if dt > 0 {
            Role::SuccessFg
        } else {
            Role::CriticalFg
        }
    };
    if src.size != dst.size {
        let mut s = String::new();
        fmt.write_size(&mut s, src.size.abs_diff(dst.size));
        parts.push(format!(
            "{}{s}",
            if src.size > dst.size { '+' } else { '−' }
        ));
    }
    (parts.join(" · "), role)
}

/// Промежуток в секундах: минуты, часы или дни.
fn span(i18n: &I18n, secs: u64) -> String {
    let (key, n) = if secs < 3600 {
        ("ui-span-min", (secs / 60).max(1))
    } else if secs < 86_400 {
        ("ui-span-hours", secs / 3600)
    } else {
        ("ui-span-days", secs / 86_400)
    };
    let mut a = FluentArgs::new();
    a.set("n", n);
    i18n.fmt(key, &a)
}

/// Однострочное поле egui поверх подложки `fill.control.input`; `true` — Enter.
#[allow(
    clippy::too_many_arguments,
    reason = "поле: холст, ui, токены, место, текст"
)]
fn text_field(
    c: &mut dyn Canvas,
    ui: &mut egui::Ui,
    t: &Tokens,
    fonts: &FontSet,
    r: Rect,
    text: &mut String,
    secret: bool,
    salt: u64,
) -> bool {
    let radius = CornerRadius::from(t.m(Metric::RadiusControl));
    c.rect(r, Role::FillControlInput, radius);
    c.rect(
        r.with_min_y(r.bottom() - 2.0 * t.m(Metric::StrokeThin)),
        Role::AccentFill,
        CornerRadius::ZERO,
    );
    // Сам текст рисует TextEdit; в записи холста — только поле.
    c.region("prompt.field", r);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(2.0)));
    let id = child.scope_id().with(("prompt-field", salt));
    let resp = child.add_sized(
        r.shrink(2.0).size(),
        egui::TextEdit::singleline(text)
            .id(id)
            .password(secret)
            .frame(egui::Frame::NONE)
            .font(fonts.id(Font::Ui, t.m(Metric::TypeBody))),
    );
    if !resp.has_focus() && !resp.lost_focus() {
        resp.request_focus();
    }
    resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

// ---- черновики ----

/// Что делает черновик: подпись, тон, иконка, поле.
struct DraftLook {
    tone: Tone,
    icon: char,
    text: String,
    field: bool,
    hint: Option<String>,
}

/// Помеченные или под курсором: число и байты (подписи удаления и копирования).
fn targets(t: &Tab) -> (u64, u64) {
    if !t.selection.is_empty() {
        return (u64::from(t.selection.len()), t.selection.bytes());
    }
    match t.current() {
        Some(Row::Entry { entry, .. }) => (1, entry.size),
        _ => (0, 0),
    }
}

fn look(cx: &BarCx<'_>, d: &Draft, s: &AppState) -> DraftLook {
    let (count, bytes) = targets(s.active_tab());
    let size = urfin_i18n::fmt::bytes_with(cx.i18n, bytes);
    let mut a = FluentArgs::new();
    a.set("count", count);
    a.set("size", size);
    let permanent = cx.memo.ops.as_ref().is_some_and(|o| o.permanent);
    let fallback = || {
        let key = format!("ui-draft-{}", d.cmd.replace('.', "-"));
        if cx.i18n.has(&key) {
            cx.i18n.get(&key)
        } else {
            cx.i18n.get(&format!("cmd-{}", d.cmd.replace('.', "-")))
        }
    };
    match d.cmd.as_str() {
        "ops.delete" if permanent => DraftLook {
            tone: Tone::Critical,
            icon: glyph::WARNING,
            text: cx.i18n.fmt("ui-draft-delete-permanent", &a),
            field: false,
            hint: None,
        },
        "ops.delete" => DraftLook {
            tone: Tone::Caution,
            icon: glyph::DELETE,
            text: cx.i18n.fmt("ui-draft-delete", &a),
            field: false,
            hint: None,
        },
        "ops.copy" | "ops.move" => DraftLook {
            tone: Tone::Info,
            icon: if d.cmd == "ops.copy" {
                glyph::COPY
            } else {
                glyph::MOVE
            },
            text: cx.i18n.fmt(
                if d.cmd == "ops.copy" {
                    "ui-draft-copy"
                } else {
                    "ui-draft-move"
                },
                &a,
            ),
            field: true,
            hint: None,
        },
        "ops.mkdir" => DraftLook {
            tone: Tone::Info,
            icon: glyph::FOLDER,
            text: fallback(),
            field: true,
            hint: Some(cx.i18n.get("ui-draft-mkdir-hint")),
        },
        _ => DraftLook {
            tone: Tone::Info,
            icon: glyph::FILTER,
            text: fallback(),
            field: true,
            hint: None,
        },
    }
}

/// Команда черновика с введённым текстом (`sel.add mask=*.rs`).
fn draft_command(d: &Draft, text: &str) -> Option<Command> {
    Command::parse(&d.cmd).ok().map(|c| c.arg(&d.arg, text))
}

#[allow(clippy::too_many_lines, reason = "раскладка черновика слева направо")]
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, x, y")]
fn draft(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    r: Rect,
    d: &Draft,
    s: &AppState,
    out: &mut Vec<Command>,
) {
    let key = (d.cmd.to_string(), d.arg.to_string());
    if cx.memo.draft.as_ref().map(|(k, _)| k) != Some(&key.0) {
        let init = if d.text.is_empty() && matches!(d.cmd.as_str(), "ops.copy" | "ops.move") {
            // Путь назначения — пассивная панель.
            crate::legacy::panel::path_text(s.panel(s.active.other()).tab())
        } else {
            d.text.to_string()
        };
        cx.memo.draft = Some((key.0.clone(), init));
    }
    let l = look(cx, d, s);
    let t = cx.tokens;
    let x = frame(c, t, r, l.tone, l.icon, head_y(t, r, false));
    let pad = t.m(Metric::SpaceL);
    let body = t.m(Metric::TypeBody);
    let caption = t.m(Metric::TypeCaption);
    let y = r.center().y;
    let tr = c.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        &Tx::user(l.text.as_str()),
        Font::Ui,
        body,
        Role::TextPrimary,
    );
    let cancel = Button {
        label: cx.i18n.get("ui-button-cancel"),
        chip: "Esc".into(),
        accent: false,
        focused: false,
        action: Action::Cmds(vec![Command::new("ui.cancel")]),
    };
    let mut list = Vec::new();
    let delete = d.cmd == "ops.delete";
    let permanent = cx.memo.ops.as_ref().is_some_and(|o| o.permanent);
    if delete {
        let confirm = |permanent: bool| {
            let mut c = Command::new("ops.delete").arg("confirmed", true);
            if permanent {
                c = c.arg("permanent", true);
            }
            Action::Cmds(vec![c, Command::new("ui.cancel")])
        };
        list.push(Button {
            label: cx.i18n.get("ui-button-delete"),
            chip: "Enter".into(),
            accent: true,
            focused: true,
            action: confirm(permanent),
        });
        if !permanent {
            list.push(Button {
                label: cx.i18n.get("ui-button-permanent"),
                chip: "⇧Enter".into(),
                accent: false,
                focused: false,
                action: confirm(true),
            });
        }
    }
    list.push(cancel);
    let (mut action, left) = buttons(c, cx, r.right() - pad, y, &list, "draft");
    if delete {
        // Enter — кнопка по умолчанию, Shift+Enter — «Насовсем».
        let pick = take_keys(cx.ui, |key, m| match key {
            Key::Enter if m == Modifiers::NONE => (true, Some(0)),
            Key::Enter if m == Modifiers::SHIFT && !permanent => (true, Some(1)),
            _ => (false, None),
        });
        if let Some(i) = pick {
            action = Some(list[i].action.clone());
        }
    }
    if l.field {
        let mut right = left;
        if let Some(hint) = &l.hint {
            let hr = c.text(
                pos2(right, y),
                Align2::RIGHT_CENTER,
                &Tx::user(hint.as_str()),
                Font::Caption,
                caption,
                Role::TextSecondary,
            );
            right = hr.left() - pad;
        }
        let field = Rect::from_min_max(
            pos2(
                tr.right() + t.m(Metric::SpaceM),
                y - t.m(Metric::HControl) / 2.0,
            ),
            pos2(right, y + t.m(Metric::HControl) / 2.0),
        );
        let text = &mut cx.memo.draft.as_mut().expect("set above").1;
        if text_field(c, cx.ui, t, cx.fonts, field, text, false, 0)
            && let Some(cmd) = draft_command(d, text)
        {
            action = Some(Action::Cmds(vec![cmd, Command::new("ui.cancel")]));
        }
    }
    if let Some(Action::Cmds(cmds)) = action {
        out.extend(cmds);
    }
}

/// Баннер core (`AppState.banner`: сбой прошлого запуска, конфиг, раскладка; S2-17):
/// иконка, текст и кнопки `actions` (≤3, v2 №15). Подложку и рамку рисует `frame`.
#[allow(clippy::many_single_char_names, reason = "геометрия: c, t, r, b, x, y")]
pub(crate) fn app_banner(
    c: &mut dyn Canvas,
    cx: &mut BarCx<'_>,
    r: Rect,
    b: &urfin_core::Banner,
    out: &mut Vec<Command>,
) {
    let t = cx.tokens;
    let (tone, icon) = match b.level {
        urfin_core::Level::Error => (Tone::Critical, crate::legacy::icons::ERROR_BADGE),
        urfin_core::Level::Warn => (Tone::Caution, glyph::WARNING),
        urfin_core::Level::Info => (Tone::Info, glyph::INFO),
    };
    let y = head_y(t, r, false);
    let pad = t.m(Metric::SpaceL);
    let icon_size = t.m(Metric::WIcon);
    c.icon(
        pos2(r.left() + pad + icon_size / 2.0, y),
        icon,
        icon_size,
        tone.fg(),
    );
    let x = r.left() + pad + icon_size + t.m(Metric::SpaceM);
    let list: Vec<Button> = b
        .actions
        .iter()
        .take(3)
        .map(|(label, cmd)| Button {
            label: label.to_string(),
            chip: String::new(),
            accent: false,
            focused: false,
            action: Action::Cmds(vec![cmd.clone()]),
        })
        .collect();
    let (clicked, left) = buttons(c, cx, r.right() - pad, y, &list, "banner");
    c.text_clipped(
        pos2(x, y),
        (left - t.m(Metric::SpaceM) - x).max(0.0),
        &Tx::user(b.text.as_str()),
        Font::Ui,
        t.m(Metric::TypeBody),
        Role::TextPrimary,
    );
    if let Some(Action::Cmds(cmds)) = clicked {
        out.extend(cmds);
    }
}

/// `Side` пассивной панели — для подсветки строки `dst` конфликта.
pub(crate) fn conflict_dst<'s>(s: &'s AppState, st: &UiState) -> Option<(Side, &'s str)> {
    let p = shown_prompt(s, st)?;
    match &p.spec.detail {
        Some(PromptDetail::Conflict { dst, .. }) => Some((s.active.other(), dst.name.as_str())),
        None => None,
    }
}

#[cfg(test)]
#[path = "prompt_bar_tests.rs"]
mod tests;

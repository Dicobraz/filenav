//! Breadcrumb (S2 §2.1, S2-12): путь панели сегментами `C: › work › urfin`.
//!
//! Клик по сегменту — `panel.go`, по `›` — меню подкаталогов (`ui.menu menu=subdirs`,
//! список читает handler на полосе Interactive, ADR-0014), по `…` — меню скрытых
//! начальных сегментов, в пустое место — ввод пути (`ui.path_edit`). Длинный путь
//! сворачивает начальные сегменты в `…`. UNC — один корневой сегмент `\\srv\share`,
//! архив (`>>`, ADR-0009) — имя архива сегментом, внутренний путь — дальше.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use compact_str::CompactString;
use egui::{Align2, Galley, Pos2, Rect, Sense, pos2, vec2};
use urfin_core::{Command, Location, Side};
use urfin_i18n::I18n;

use crate::legacy::canvas::Canvas;
use crate::legacy::fonts::{Font, FontSet};
use crate::legacy::panel::side_arg;
use crate::legacy::state::{Subdirs, UiState};
use crate::legacy::text::{self, Tx};
use crate::legacy::tokens::{Metric, Role, Tokens};

/// Сегмент пути: подпись и каталог, в который ведёт клик.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub label: CompactString,
    pub loc: Location,
}

/// Сегменты `loc` от корня. Контейнер (`outer`) — его сегменты, последний ведёт в
/// корень архива; затем — путь внутри.
pub fn segments(loc: &Location) -> Vec<Crumb> {
    let mut out = match loc.outer.as_deref() {
        Some(outer) => {
            let mut v = segments(outer);
            if let Some(archive) = v.last_mut() {
                archive.loc = with_path(loc, root_of(&loc.path));
            }
            v
        }
        None => Vec::new(),
    };
    let archived = loc.outer.is_some();
    let mut prefix = false;
    let mut acc = PathBuf::new();
    for comp in loc.path.components() {
        match comp {
            Component::Prefix(p) => {
                acc.push(p.as_os_str());
                prefix = true;
                out.push(Crumb {
                    label: p.as_os_str().to_string_lossy().as_ref().into(),
                    loc: with_path(loc, acc.clone()),
                });
            }
            Component::RootDir => {
                acc.push(Component::RootDir.as_os_str());
                if prefix {
                    // `C:` ведёт в `C:\`, `\\srv\share` — в корень шары.
                    if let Some(last) = out.last_mut() {
                        last.loc = with_path(loc, acc.clone());
                    }
                } else if !archived {
                    // Корень unix — свой сегмент; корень архива — сегмент архива.
                    out.push(Crumb {
                        label: "/".into(),
                        loc: with_path(loc, acc.clone()),
                    });
                }
            }
            Component::Normal(n) => {
                acc.push(n);
                out.push(Crumb {
                    label: n.to_string_lossy().as_ref().into(),
                    loc: with_path(loc, acc.clone()),
                });
            }
            Component::CurDir | Component::ParentDir => {}
        }
    }
    out
}

/// Корень пути: `/` архива, `C:\`, `\\srv\share\`.
fn root_of(p: &Path) -> PathBuf {
    let mut acc = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => acc.push(comp.as_os_str()),
            _ => break,
        }
    }
    if acc.as_os_str().is_empty() {
        acc.push("/");
    }
    acc
}

/// Тот же backend и контейнер, другой путь.
fn with_path(loc: &Location, path: PathBuf) -> Location {
    if loc.is_local() {
        Location::local(path)
    } else {
        Location {
            backend: loc.backend.clone(),
            path,
            outer: loc.outer.clone(),
        }
    }
}

/// Подкаталог `name` каталога `loc` (пункт меню `›`).
pub fn child(loc: &Location, name: &str) -> Location {
    with_path(loc, loc.path.join(name))
}

/// Разметка сегментов: запоминается до смены пути, шрифта, масштаба или поколения
/// стиля (атлас шрифтов пересоздан).
#[derive(Debug, Default)]
pub(crate) struct CrumbMemo {
    key: Option<(Location, u32, bool, u32, u64)>,
    pub crumbs: Vec<Crumb>,
    galleys: Vec<Arc<Galley>>,
    /// Текст поля ввода пути, пока оно открыто.
    pub input: Option<String>,
    /// Точка меню (`›` или `…`), куда его прижать.
    pub menu_at: Option<Pos2>,
    /// Сколько начальных сегментов свёрнуто в `…` (меню `…`).
    pub hidden: usize,
}

impl CrumbMemo {
    fn refresh(&mut self, ctx: &egui::Context, loc: &Location, font: &egui::FontId, strong: bool) {
        let key = (
            loc.clone(),
            font.size.to_bits(),
            strong,
            ctx.pixels_per_point().to_bits(),
            crate::legacy::style_gen(),
        );
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.crumbs = segments(loc);
        self.galleys = self
            .crumbs
            .iter()
            .map(|c| {
                ctx.fonts_mut(|f| {
                    f.layout_no_wrap(c.label.to_string(), font.clone(), egui::Color32::WHITE)
                })
            })
            .collect();
        self.key = Some(key);
    }
}

/// Входы breadcrumb.
pub(crate) struct CrumbCx<'a> {
    pub ui: &'a mut egui::Ui,
    pub tokens: &'a Tokens,
    pub fonts: &'a FontSet,
    pub st: &'a UiState,
    pub memo: &'a mut CrumbMemo,
}

/// Ширина `›` с зазорами.
const CHEVRON_W: f32 = 20.0;
/// Минимум места под последний сегмент при сворачивании.
const LAST_MIN: f32 = 48.0;

/// Что показать: `…` и с какого сегмента начинать.
fn fit(widths: &[f32], avail: f32, ellipsis: f32) -> (bool, usize) {
    #[allow(clippy::cast_precision_loss, reason = "сегментов — десятки")]
    let total: f32 = widths.iter().sum::<f32>() + CHEVRON_W * widths.len().saturating_sub(1) as f32;
    if total <= avail || widths.len() <= 1 {
        return (false, 0);
    }
    // Справа налево, пока помещаются вместе с `… ›`.
    let mut used = ellipsis + CHEVRON_W;
    let mut first = widths.len() - 1;
    used += widths[first].max(LAST_MIN).min(avail - used);
    while first > 0 {
        let w = widths[first - 1] + CHEVRON_W;
        if used + w > avail {
            break;
        }
        used += w;
        first -= 1;
    }
    (first > 0, first)
}

/// Путь панели в `r`. Поле ввода — пока открыт `ui.path_edit` этой панели.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "сегменты, `›`, `…` и поле ввода — одна строка; геометрия: c, r, g, w, x"
)]
pub(crate) fn show(
    c: &mut dyn Canvas,
    cx: &mut CrumbCx<'_>,
    r: Rect,
    loc: &Location,
    side: Side,
    active: bool,
    out: &mut Vec<Command>,
) {
    let tk = cx.tokens;
    let pad = tk.m(Metric::PadRowX);
    let body = tk.m(Metric::TypeBody);
    let font = if active { Font::Strong } else { Font::Ui };
    let font_id = cx.fonts.id(font, body);
    let ctx = cx.ui.ctx().clone();

    if cx.st.path_edit == Some(side) {
        edit(cx, r.shrink2(vec2(pad - 4.0, 4.0)), loc, side, out);
        return;
    }
    cx.memo.input = None;
    cx.memo.refresh(&ctx, loc, &font_id, active);

    let widths: Vec<f32> = cx.memo.galleys.iter().map(|g| g.size().x).collect();
    let ellipsis_w = 16.0;
    let avail = r.width() - 2.0 * pad;
    let (collapsed, first) = fit(&widths, avail, ellipsis_w);
    cx.memo.hidden = first;
    let mid = r.center().y;
    let mut x = r.left() + pad;
    let focus_first = |out: &mut Vec<Command>| {
        if !active {
            out.push(Command::new("panel.focus").arg("side", side_arg(side)));
        }
    };
    // Пустое место строки — ввод пути (S2 §2.1). Регистрируется первым: сегменты и
    // `›` поверх него и забирают свои клики.
    let blank = cx.ui.interact(
        r,
        cx.ui.scope_id().with(("crumb-blank", side_arg(side))),
        Sense::click(),
    );
    if blank.clicked() {
        focus_first(out);
        out.push(Command::new("ui.path_edit").arg("side", side_arg(side)));
    }
    let hover = Role::FillSubtleHover;
    let radius = tk.radius(Metric::RadiusControl);

    if collapsed {
        let cell = Rect::from_min_max(
            pos2(x - 2.0, r.top() + 4.0),
            pos2(x + ellipsis_w + 2.0, r.bottom() - 4.0),
        );
        let resp = cx.ui.interact(
            cell,
            cx.ui.scope_id().with(("crumb-more", side_arg(side))),
            Sense::click(),
        );
        if resp.hovered() {
            c.rect(cell, hover, radius);
        }
        c.text(
            pos2(x, mid),
            Align2::LEFT_CENTER,
            &Tx::user("…"),
            font,
            body,
            Role::TextSecondary,
        );
        if resp.clicked() {
            focus_first(out);
            cx.memo.menu_at = Some(cell.left_bottom());
            out.push(
                Command::new("ui.menu")
                    .arg("menu", "crumbs")
                    .arg("side", side_arg(side)),
            );
        }
        x += ellipsis_w;
        c.icon(
            pos2(x + CHEVRON_W / 2.0, mid),
            crate::legacy::icons::CHEVRON_RIGHT,
            12.0,
            Role::TextTertiary,
        );
        x += CHEVRON_W;
    }

    let last = cx.memo.crumbs.len().saturating_sub(1);
    for i in first..cx.memo.crumbs.len() {
        let g = Arc::clone(&cx.memo.galleys[i]);
        let room = r.right() - pad - x;
        let w = g.size().x.min(room.max(0.0));
        let cell = Rect::from_min_max(
            pos2(x - 2.0, r.top() + 4.0),
            pos2(x + w + 2.0, r.bottom() - 4.0),
        );
        let resp = cx.ui.interact(
            cell,
            cx.ui.scope_id().with(("crumb", side_arg(side), i)),
            Sense::click(),
        );
        if resp.hovered() {
            c.rect(cell, hover, radius);
        }
        let role = if i == last {
            Role::TextPrimary
        } else {
            Role::TextSecondary
        };
        if g.size().x <= room {
            c.galley(pos2(x, mid - g.size().y / 2.0), g, role);
        } else {
            // Последний сегмент шире места — многоточием.
            c.text_clipped(
                pos2(x, mid),
                room,
                &Tx::user(cx.memo.crumbs[i].label.as_str()),
                font,
                body,
                role,
            );
        }
        if resp.clicked() {
            focus_first(out);
            out.push(
                Command::new("panel.go")
                    .arg("side", side_arg(side))
                    .arg("path", cx.memo.crumbs[i].loc.clone()),
            );
        }
        x += w;
        if i == last {
            break;
        }
        let chev = Rect::from_min_max(
            pos2(x, r.top() + 4.0),
            pos2(x + CHEVRON_W, r.bottom() - 4.0),
        );
        let resp = cx.ui.interact(
            chev,
            cx.ui.scope_id().with(("crumb-chevron", side_arg(side), i)),
            Sense::click(),
        );
        let open = cx
            .st
            .subdirs
            .as_ref()
            .is_some_and(|s| s.side == side && s.loc == cx.memo.crumbs[i].loc);
        if resp.hovered() || open {
            c.rect(chev, hover, radius);
        }
        let glyph = if open {
            crate::legacy::icons::CHEVRON_DOWN
        } else {
            crate::legacy::icons::CHEVRON_RIGHT
        };
        c.icon(chev.center(), glyph, 12.0, Role::TextTertiary);
        if resp.clicked() {
            focus_first(out);
            if open {
                out.push(Command::new("ui.cancel"));
            } else {
                cx.memo.menu_at = Some(chev.left_bottom());
                out.push(
                    Command::new("ui.menu")
                        .arg("menu", "subdirs")
                        .arg("side", side_arg(side))
                        .arg("path", cx.memo.crumbs[i].loc.clone()),
                );
            }
        }
        x += CHEVRON_W;
    }
}

/// Поле ввода пути: Enter — `ui.path_edit path=` (проверка в handler), Esc — app.
fn edit(cx: &mut CrumbCx<'_>, r: Rect, loc: &Location, side: Side, out: &mut Vec<Command>) {
    let opened = cx.memo.input.is_none();
    let text = cx.memo.input.get_or_insert_with(|| {
        if loc.is_local() {
            loc.path.display().to_string()
        } else {
            loc.to_uri()
        }
    });
    let mut ui = cx.ui.new_child(egui::UiBuilder::new().max_rect(r));
    let id = ui.scope_id().with(("path-edit", side_arg(side)));
    let resp = ui.add_sized(
        r.size(),
        egui::TextEdit::singleline(text)
            .id(id)
            .font(cx.fonts.id(Font::Ui, cx.tokens.m(Metric::TypeBody))),
    );
    if opened {
        resp.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let all = egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(text.chars().count()),
            );
            state.cursor.set_char_range(Some(all));
            state.store(ui.ctx(), id);
        }
    }
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        out.push(
            Command::new("ui.path_edit")
                .arg("side", side_arg(side))
                .arg("path", text.as_str()),
        );
        resp.request_focus();
    }
}

/// Меню подкаталогов (`›`) или скрытых сегментов (`…`) поверх карточек.
pub(crate) fn menu(
    ctx: &egui::Context,
    i18n: &I18n,
    tokens: &Tokens,
    st: &UiState,
    crumbs: [(&CrumbMemo, Side); 2],
    out: &mut Vec<Command>,
) {
    let Some(side) = st.menu_side else {
        return;
    };
    let Some((memo, _)) = crumbs.iter().find(|(_, s)| *s == side) else {
        return;
    };
    let Some(at) = memo.menu_at else {
        return;
    };
    let items: Vec<(CompactString, Location)> = match (st.menu.as_deref(), &st.subdirs) {
        (
            Some("subdirs"),
            Some(Subdirs {
                loc,
                items: Some(Ok(names)),
                ..
            }),
        ) => names.iter().map(|n| (n.clone(), child(loc, n))).collect(),
        // Свёрнутые в `…` сегменты, ближний к видимым — первым.
        (Some("crumbs"), _) => memo
            .crumbs
            .iter()
            .take(memo.hidden)
            .rev()
            .map(|c| (c.label.clone(), c.loc.clone()))
            .collect(),
        _ => Vec::new(),
    };
    let note = match (st.menu.as_deref(), &st.subdirs) {
        (Some("subdirs"), Some(Subdirs { items: None, .. })) => Some(Tx::key("ui-menu-loading")),
        (
            Some("subdirs"),
            Some(Subdirs {
                items: Some(Err(e)),
                ..
            }),
        ) => Some(Tx::user(e.to_string())),
        (
            Some("subdirs"),
            Some(Subdirs {
                items: Some(Ok(v)), ..
            }),
        ) if v.is_empty() => Some(Tx::key("ui-menu-empty")),
        _ => None,
    };
    let area = egui::Area::new(egui::Id::unique(("crumb-menu", side_arg(side))))
        .fixed_pos(at + vec2(0.0, 2.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(160.0);
                ui.set_max_width(360.0);
                if let Some(n) = &note {
                    text::label(ui, i18n, n);
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(tokens.m(Metric::HRow) * 16.0)
                    .show(ui, |ui| {
                        for (label, loc) in &items {
                            let resp = text::button(ui, i18n, &Tx::user(label.as_str()));
                            if resp.clicked() {
                                out.push(Command::new("ui.cancel"));
                                out.push(
                                    Command::new("panel.go")
                                        .arg("side", side_arg(side))
                                        .arg("path", loc.clone()),
                                );
                            }
                        }
                    });
            });
        });
    // Клик мимо меню — закрыть (кнопки `›` закрывают сами).
    let outside = ctx.input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.contains(p))
    });
    if outside {
        out.push(Command::new("ui.cancel"));
    }
}

#[cfg(test)]
#[path = "breadcrumb_tests.rs"]
mod tests;

//! `sel.*`: выделение в активном табе (семантика Far).

use super::{active_tab_mut, arg, choice, meta, same_text};
use crate::bus::Handler;
use crate::cmd::{Args, CmdError, Command, CommandId, CommandMeta, ValueKind};
use crate::entry::Entry;
use crate::mask::Mask;
use crate::msg::Cx;
use crate::selection::Selection;
use crate::state::UiEvent;
use crate::tab::{Row, Tab};

pub(crate) struct SelHandler;

static COMMANDS: [CommandMeta; 9] = [
    meta(
        "sel.toggle",
        "Toggle selection",
        &[arg("advance", ValueKind::Bool, false)],
        true,
    ),
    meta(
        "sel.move",
        "Select while moving",
        &[arg("motion", ValueKind::Str, true)],
        true,
    ),
    meta("sel.all", "Select all files", &[], false),
    meta("sel.none", "Clear selection", &[], false),
    meta(
        "sel.invert",
        "Invert selection",
        &[arg("dirs", ValueKind::Bool, false)],
        false,
    ),
    meta("sel.restore", "Restore selection", &[], false),
    meta(
        "sel.add",
        "Select by mask",
        &[arg("mask", ValueKind::Str, false)],
        false,
    ),
    meta(
        "sel.remove",
        "Deselect by mask",
        &[arg("mask", ValueKind::Str, false)],
        false,
    ),
    meta(
        "sel.same",
        "Select same",
        &[
            arg("by", ValueKind::Str, true),
            arg("on", ValueKind::Bool, false),
        ],
        false,
    ),
];

impl Handler for SelHandler {
    fn namespace(&self) -> &'static str {
        "sel"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let a = &cmd.args;
        match cmd.id.as_str() {
            "sel.toggle" => {
                let t = active_tab_mut(cx);
                if let Some(Row::Entry { idx, entry }) = t.current() {
                    let e = entry.clone();
                    t.selection.toggle(idx, &e);
                }
                if a.bool("advance").unwrap_or(false) {
                    let r = t.cursor.row + 1;
                    t.set_cursor_row(r);
                }
                t.touch();
            }
            "sel.move" => select_moving(active_tab_mut(cx), a)?,
            "sel.all" => {
                let t = active_tab_mut(cx);
                remember(t);
                let l = t.listing.clone();
                t.selection
                    .select_where(&l, true, |e| !e.kind.is_dir_like());
            }
            "sel.none" => {
                let t = active_tab_mut(cx);
                remember(t);
                t.selection.clear();
            }
            "sel.invert" => {
                let t = active_tab_mut(cx);
                remember(t);
                let l = t.listing.clone();
                t.selection.invert(&l, !a.bool("dirs").unwrap_or(false));
            }
            "sel.restore" => {
                let t = active_tab_mut(cx);
                let Some(prev) = t.prev_selection.take() else {
                    cx.keep_rev();
                    return Ok(());
                };
                t.prev_selection = Some(t.selection.names(&t.listing));
                t.selection = Selection::restore(&prev, &t.listing);
            }
            "sel.add" | "sel.remove" => {
                let on = cmd.id.as_str() == "sel.add";
                let Some(mask) = a.str("mask") else {
                    cx.ui(UiEvent::Ext {
                        id: CommandId::from_static("sel.ask_mask"),
                        args: Args::default().with("on", on),
                    });
                    return Ok(());
                };
                let m = Mask::parse(mask).map_err(|e| CmdError::arg("mask", e.to_string()))?;
                let t = active_tab_mut(cx);
                remember(t);
                let l = t.listing.clone();
                // Как в Far по умолчанию: маска выделяет только файлы, снимает — со всех.
                t.selection.select_where(&l, on, |e| {
                    (!on || !e.kind.is_dir_like()) && m.matches(&e.name)
                });
            }
            "sel.same" => {
                let by = choice(a, "by", &[("ext", true), ("name", false)])?;
                let on = a.bool("on").unwrap_or(true);
                let t = active_tab_mut(cx);
                let Some(Row::Entry { entry, .. }) = t.current() else {
                    cx.keep_rev();
                    return Ok(());
                };
                let key: String = if by { ext(entry) } else { stem(entry) }.to_owned();
                remember(t);
                let l = t.listing.clone();
                t.selection.select_where(&l, on, |e| {
                    !e.kind.is_dir_like() && same_text(if by { ext(e) } else { stem(e) }, &key)
                });
            }
            _ => unreachable!("registered command without handling: {}", cmd.id),
        }
        Ok(())
    }
}

/// Запомнить выделение для `sel.restore` перед массовым изменением.
fn remember(t: &mut Tab) {
    t.prev_selection = Some(t.selection.names(&t.listing));
}

fn ext(e: &Entry) -> &str {
    match e.name.rfind('.') {
        Some(i) if i > 0 => &e.name[i + 1..],
        _ => "",
    }
}

fn stem(e: &Entry) -> &str {
    match e.name.rfind('.') {
        Some(i) if i > 0 => &e.name[..i],
        _ => &e.name,
    }
}

/// Shift+стрелки: элементы, через которые прошёл курсор (включая исходный, без
/// конечного), получают состояние, обратное исходному элементу.
fn select_moving(t: &mut Tab, a: &Args) -> Result<(), CmdError> {
    #[derive(Copy, Clone)]
    enum Motion {
        Up,
        Down,
        PageUp,
        PageDown,
        Home,
        End,
    }
    let m = choice(
        a,
        "motion",
        &[
            ("up", Motion::Up),
            ("down", Motion::Down),
            ("page_up", Motion::PageUp),
            ("page_down", Motion::PageDown),
            ("home", Motion::Home),
            ("end", Motion::End),
        ],
    )?;
    let rows = t.rows();
    if rows == 0 {
        return Ok(());
    }
    let cur = t.cursor.row.min(rows - 1);
    let page = t.cursor.page.max(1);
    let last = rows - 1;
    let target = match m {
        Motion::Up => cur.saturating_sub(1),
        Motion::Down => (cur + 1).min(last),
        Motion::PageUp => cur.saturating_sub(page),
        Motion::PageDown => (cur + page).min(last),
        Motion::Home => 0,
        Motion::End => last,
    };
    let on = match t.row(cur) {
        Some(Row::Entry { idx, .. }) => !t.selection.contains(idx),
        _ => true,
    };
    let span: Vec<u32> = match target.cmp(&cur) {
        std::cmp::Ordering::Greater => (cur..target).collect(),
        std::cmp::Ordering::Less => (target + 1..=cur).collect(),
        std::cmp::Ordering::Equal => vec![cur],
    };
    for r in span {
        if let Some(Row::Entry { idx, entry }) = t.row(r) {
            let e = entry.clone();
            t.selection.set(idx, on, &e);
        }
    }
    t.set_cursor_row(target);
    Ok(())
}

//! `tab.*`: табы панели.

use super::{SIDE, arg, meta, rejected, side_arg};
use crate::bus::Handler;
use crate::cmd::{CmdError, Command, CommandMeta, SideRef, ValueKind};
use crate::ids::Side;
use crate::msg::Cx;
use crate::nav;
use crate::services::TabEvent;
use crate::tab::Tab;

pub(crate) struct TabHandler;

static COMMANDS: [CommandMeta; 7] = [
    meta("tab.new", "New tab", &[SIDE], false),
    meta("tab.dup", "Duplicate tab", &[SIDE], false),
    meta("tab.close", "Close tab", &[SIDE], false),
    meta("tab.next", "Next tab", &[SIDE], false),
    meta("tab.prev", "Previous tab", &[SIDE], false),
    meta(
        "tab.goto",
        "Go to tab",
        &[arg("n", ValueKind::Int, true), SIDE],
        false,
    ),
    meta(
        "tab.move",
        "Move tab",
        &[arg("n", ValueKind::Int, true), SIDE],
        false,
    ),
];

impl Handler for TabHandler {
    fn namespace(&self) -> &'static str {
        "tab"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let side = side_arg(cx, &cmd.args, SideRef::Active);
        cx.state.mark_session_dirty();
        let len = cx.state.panel(side).tabs.len();
        let active = cx.state.panel(side).active;
        match cmd.id.as_str() {
            "tab.new" | "tab.dup" => open(cx, side, cmd.id.as_str() == "tab.dup"),
            "tab.close" => {
                if len == 1 {
                    return Err(rejected("core-last-tab", &[]));
                }
                let p = cx.state.panel_mut(side);
                let mut t = p.tabs.remove(active);
                p.active = active.min(p.tabs.len() - 1);
                let shown = p.tab().id;
                if let Some(c) = t.cancel.take() {
                    c.cancel();
                }
                cx.services.listing.tab_event(TabEvent::Closed(t.id));
                cx.services.listing.tab_event(TabEvent::Shown(shown));
            }
            "tab.next" | "tab.prev" => {
                if len == 1 {
                    cx.keep_rev();
                    return Ok(());
                }
                let to = if cmd.id.as_str() == "tab.next" {
                    (active + 1) % len
                } else {
                    (active + len - 1) % len
                };
                activate(cx, side, to);
            }
            "tab.goto" => activate(cx, side, position(cmd, len)?),
            "tab.move" => {
                let to = position(cmd, len)?;
                let p = cx.state.panel_mut(side);
                let t = p.tabs.remove(active);
                p.tabs.insert(to, t);
                p.active = to;
            }
            _ => unreachable!("registered command without handling: {}", cmd.id),
        }
        Ok(())
    }
}

/// `n` с единицы → индекс.
fn position(cmd: &Command, len: usize) -> Result<usize, CmdError> {
    let n = cmd.args.int("n").unwrap_or(0);
    usize::try_from(n)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|&i| i < len)
        .ok_or_else(|| rejected("core-no-tab", &[("n", &n.to_string())]))
}

fn activate(cx: &mut Cx<'_>, side: Side, to: usize) {
    let p = cx.state.panel_mut(side);
    if p.active == to {
        cx.keep_rev();
        return;
    }
    let hidden = p.tab().id;
    p.active = to;
    let shown = p.tab().id;
    cx.services.listing.tab_event(TabEvent::Hidden(hidden));
    cx.services.listing.tab_event(TabEvent::Shown(shown));
}

/// Новый таб после активного в том же каталоге; `dup` копирует вид и историю.
fn open(cx: &mut Cx<'_>, side: Side, dup: bool) {
    let id = cx.state.alloc_tab_id();
    let config = cx.state.config.clone();
    let p = cx.state.panel_mut(side);
    let cur = p.tab();
    let mut t = if dup {
        Tab::new(id, cur.loc.clone())
    } else {
        Tab::with_config(id, cur.loc.clone(), &config)
    };
    if dup {
        t.sort = cur.sort;
        t.filter.clone_from(&cur.filter);
        t.show_hidden = cur.show_hidden;
        t.view_mode = cur.view_mode;
        t.mode = cur.mode;
        t.history = cur.history.clone();
    }
    let (hidden, loc) = (cur.id, cur.loc.clone());
    let at = p.active + 1;
    p.tabs.insert(at, t);
    p.active = at;
    cx.services.listing.tab_event(TabEvent::Hidden(hidden));
    cx.services.listing.tab_event(TabEvent::Shown(id));
    nav::navigate(cx, id, loc, None, 0, false);
}

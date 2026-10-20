//! Handlers `ops` и `file` (S5 §2.5): UI-поток, без I/O. Без подтверждения
//! (`confirmed`, `dest`, `name`) — черновик в окне (`UiEvent::Ext{"ops.draft"}`,
//! S0 v3 №15); с ним — запрос операции. Движок job — S5-04, операции —
//! S5-06…S5-10: до них — «ещё не сделано» тостом. `file.open` — системным
//! открытием (`ShellExecuteExW`) на `Lane::Interactive`.

use compact_str::CompactString;
use urfin_core::{
    ArgSpec, Args, CmdError, Command, CommandId, CommandMeta, Cx, Handler, Lane, Level, Row,
    UiEvent, ValueKind,
};

use crate::api::{OpKind, OpRequest};
use crate::config::OpsConfig;
use crate::dest::{DestError, parse_dest};

const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        required,
    }
}

const fn meta(id: &'static str, title: &'static str, args: &'static [ArgSpec]) -> CommandMeta {
    CommandMeta {
        id,
        title,
        category: "ops",
        args,
        repeatable: false,
    }
}

/// Аргументы копирования и переноса.
const COPY_ARGS: [ArgSpec; 7] = [
    arg("dest", ValueKind::Str, false),
    arg("current", ValueKind::Bool, false),
    arg("confirmed", ValueKind::Bool, false),
    arg("conflict", ValueKind::Str, false),
    arg("verify", ValueKind::Str, false),
    arg("filter", ValueKind::Str, false),
    arg("queue", ValueKind::Str, false),
];

static OPS_COMMANDS: [CommandMeta; 15] = [
    meta("ops.copy", "Copy", &COPY_ARGS),
    meta("ops.move", "Move", &COPY_ARGS),
    meta(
        "ops.delete",
        "Delete",
        &[
            arg("permanent", ValueKind::Bool, false),
            arg("current", ValueKind::Bool, false),
            arg("confirmed", ValueKind::Bool, false),
        ],
    ),
    meta(
        "ops.mkdir",
        "Make folder",
        &[
            arg("name", ValueKind::Str, false),
            arg("current", ValueKind::Bool, false),
        ],
    ),
    meta("ops.rename", "Rename", &[arg("to", ValueKind::Str, false)]),
    meta(
        "ops.multi_rename",
        "Multi-rename",
        &[
            arg("pattern", ValueKind::Str, false),
            arg("regex", ValueKind::Bool, false),
            arg("apply", ValueKind::Bool, false),
        ],
    ),
    meta(
        "ops.attrs",
        "Attributes",
        &[
            arg("set", ValueKind::Str, false),
            arg("clear", ValueKind::Str, false),
        ],
    ),
    meta(
        "ops.touch",
        "Set time",
        &[arg("mtime", ValueKind::Str, false)],
    ),
    meta(
        "ops.link",
        "Make link",
        &[
            arg("kind", ValueKind::Str, false),
            arg("dest", ValueKind::Str, false),
        ],
    ),
    meta(
        "ops.compare",
        "Compare folders",
        &[arg("mode", ValueKind::Str, false)],
    ),
    meta(
        "ops.sync",
        "Synchronize folders",
        &[arg("dir", ValueKind::Str, false)],
    ),
    meta(
        "ops.retry",
        "Retry failed",
        &[arg("id", ValueKind::Int, true)],
    ),
    meta(
        "ops.priority",
        "Operation priority",
        &[
            arg("id", ValueKind::Int, true),
            arg("prio", ValueKind::Str, false),
        ],
    ),
    meta("ops.run_now", "Run now", &[arg("id", ValueKind::Int, true)]),
    meta(
        "ops.recover",
        "Recover operation",
        &[
            arg("journal", ValueKind::Str, true),
            arg("action", ValueKind::Str, true),
        ],
    ),
];

static FILE_COMMANDS: [CommandMeta; 6] = [
    CommandMeta {
        category: "file",
        ..meta(
            "file.open",
            "Open",
            &[arg("elevated", ValueKind::Bool, false)],
        )
    },
    CommandMeta {
        category: "file",
        ..meta("file.open_with", "Open with", &[])
    },
    CommandMeta {
        category: "file",
        ..meta("file.reveal", "Show in Explorer", &[])
    },
    CommandMeta {
        category: "file",
        ..meta("file.clip.copy", "Copy to clipboard", &[])
    },
    CommandMeta {
        category: "file",
        ..meta("file.clip.cut", "Cut to clipboard", &[])
    },
    CommandMeta {
        category: "file",
        ..meta(
            "file.clip.paste",
            "Paste from clipboard",
            &[arg("move", ValueKind::Bool, false)],
        )
    },
];

fn text(key: &str, args: &[(&str, &str)]) -> String {
    let mut a = urfin_i18n::FluentArgs::new();
    for (k, v) in args {
        a.set(*k, (*v).to_owned());
    }
    urfin_i18n::current().fmt(key, &a)
}

fn not_yet(cmd: &Command) -> CmdError {
    CmdError::Rejected(text("ops-not-yet", &[("what", cmd.id.as_str())]).into())
}

/// Handler `ops`.
#[derive(Debug)]
pub struct OpsHandler {
    cfg: OpsConfig,
}

impl OpsHandler {
    pub fn new(cfg: OpsConfig) -> OpsHandler {
        OpsHandler { cfg }
    }
}

impl Handler for OpsHandler {
    fn namespace(&self) -> &'static str {
        "ops"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &OPS_COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let a = &cmd.args;
        match cmd.id.as_str() {
            "ops.copy" | "ops.move" => {
                let kind = if cmd.id.as_str() == "ops.copy" {
                    OpKind::Copy
                } else {
                    OpKind::Move
                };
                // Цели проверяются до черновика: на `..` без выделения — сразу «нечего».
                let req = self.request(cx, kind, a.bool("current").unwrap_or(false))?;
                let Some(dest) = a.str("dest") else {
                    draft(cx, cmd, false);
                    return Ok(());
                };
                let dest = parse_dest(&req.base, dest).map_err(dest_error)?;
                tracing::info!(target: "ops", kind = ?req.kind, items = req.items.len(), dest = %dest.path.display(), "request");
                Err(not_yet(cmd))
            }
            "ops.delete" => {
                let permanent = a.bool("permanent").unwrap_or(false) || !self.cfg.delete_to_trash;
                let kind = if permanent {
                    OpKind::Delete
                } else {
                    OpKind::Trash
                };
                let req = self.request(cx, kind, a.bool("current").unwrap_or(false))?;
                if !a.bool("confirmed").unwrap_or(false) {
                    draft(cx, cmd, a.bool("permanent").unwrap_or(false));
                    return Ok(());
                }
                tracing::info!(target: "ops", kind = ?req.kind, items = req.items.len(), "request");
                Err(not_yet(cmd))
            }
            "ops.mkdir" => {
                if a.str("name").is_none() {
                    draft(cx, cmd, false);
                    return Ok(());
                }
                Err(not_yet(cmd))
            }
            "ops.rename" => {
                if a.str("to").is_none() {
                    // Без имени — поле на месте (Shift+F6).
                    cx.dispatch(Command::new("ui.rename"));
                    return Ok(());
                }
                Err(not_yet(cmd))
            }
            _ => Err(not_yet(cmd)),
        }
    }
}

fn dest_error(e: DestError) -> CmdError {
    match e {
        DestError::Empty => CmdError::Rejected(text("ops-dest-empty", &[]).into()),
        DestError::NotADirectory(p) => {
            CmdError::Rejected(text("ops-dest-file", &[("path", &p)]).into())
        }
    }
}

/// Черновик операции в окне (S0 v3 №15): строка с полем или кнопками.
fn draft(cx: &mut Cx<'_>, cmd: &Command, permanent: bool) {
    let mut args = Args::default().with("command", cmd.id.as_str());
    if permanent {
        args = args.with("permanent", true);
    }
    if cmd.args.bool("current").unwrap_or(false) {
        args = args.with("current", true);
    }
    cx.ui(UiEvent::Ext {
        id: CommandId::from_static("ops.draft"),
        args,
    });
}

impl OpsHandler {
    /// Запрос по выделению активного таба (или элементу под курсором при `current`).
    fn request(&self, cx: &Cx<'_>, kind: OpKind, current: bool) -> Result<OpRequest, CmdError> {
        let t = cx.state.panel(cx.state.active).tab();
        let items: Vec<CompactString> = if current {
            match t.current() {
                Some(Row::Entry { entry, .. }) => vec![entry.name.clone()],
                _ => Vec::new(),
            }
        } else {
            t.targets()
                .map(|tg| tg.items.iter().map(|e| e.name.clone()).collect())
                .unwrap_or_default()
        };
        if items.is_empty() {
            return Err(CmdError::Rejected(text("ops-nothing", &[]).into()));
        }
        Ok(OpRequest {
            kind,
            base: t.loc.clone(),
            items,
            dest: None,
            opts: self.cfg.defaults.clone(),
        })
    }
}

/// Handler `file`: открытие и показ в Проводнике; буфер обмена — S5 позже.
#[derive(Debug, Default)]
pub struct FileHandler;

impl Handler for FileHandler {
    fn namespace(&self) -> &'static str {
        "file"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &FILE_COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let how = match cmd.id.as_str() {
            "file.open" if cmd.args.bool("elevated").unwrap_or(false) => crate::sys::Open::Elevated,
            "file.open" => crate::sys::Open::Default,
            "file.open_with" => crate::sys::Open::With,
            "file.reveal" => crate::sys::Open::Reveal,
            _ => return Err(not_yet(cmd)),
        };
        let t = cx.state.panel(cx.state.active).tab();
        let Some(Row::Entry { entry, .. }) = t.current() else {
            return Err(CmdError::Rejected(text("ops-nothing", &[]).into()));
        };
        if !t.loc.is_local() {
            // Архивы и сеть — через VFS (S8).
            return Err(not_yet(cmd));
        }
        let path = t.loc.path.join(entry.name.as_str());
        let name = entry.name.clone();
        let sink = cx.rt.sink();
        cx.rt
            .spawn(Lane::Interactive, "file.open", cx.rt.token(), move |tcx| {
                if let Err(e) = crate::sys::open(&path, how) {
                    tracing::warn!(target: "ops", error = %e, path = %path.display(), "file.open");
                    let msg = text(
                        "ops-open-failed",
                        &[("name", name.as_str()), ("error", &e.to_string())],
                    );
                    let _ = sink.send(
                        urfin_core::msg("ops.open_failed", move |cx| cx.toast(Level::Error, msg)),
                        &tcx.cancel,
                    );
                }
            });
        Ok(())
    }
}

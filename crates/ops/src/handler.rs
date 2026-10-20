//! Handlers `ops` и `file` (S5 §2.5): UI-поток, без I/O. Без подтверждения
//! (`confirmed`, `dest`, `name`) — черновик в окне (`UiEvent::Ext{"ops.draft"}`,
//! S0 v3 №15); с ним — запрос операции. Копирование и перенос — задачей
//! движка (S5-06); остальные операции — S5-09, S5-10, до них — «ещё не
//! сделано» тостом. `file.open` — системным открытием (`ShellExecuteExW`) на
//! `Lane::Interactive`.

use compact_str::CompactString;
use urfin_core::{
    ArgSpec, Args, CmdError, Command, CommandId, CommandMeta, Cx, Handler, Lane, Level, Row,
    UiEvent, ValueKind,
};

use crate::api::{ConflictPolicy, OpKind, OpRequest, RenameScheme};
use crate::config::OpsConfig;
use crate::dest::{DestError, parse_dest};
use crate::engine::JobEngine;

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
    engine: JobEngine,
}

impl OpsHandler {
    pub fn new(cfg: OpsConfig) -> OpsHandler {
        let engine = JobEngine::new(cfg.max_threads);
        OpsHandler { cfg, engine }
    }

    /// Копия или перенос задачей движка (S5-06). Назначение проверяется на ФС
    /// в плане задачи, не здесь (UI-поток без I/O).
    fn transfer(
        &self,
        cx: &mut Cx<'_>,
        mut req: OpRequest,
        dest: urfin_core::Location,
        raw: String,
        a: &Args,
    ) -> Result<(), CmdError> {
        if !req.base.is_local() || !dest.is_local() {
            // Архивы и сеть через VFS — S8 (S0 v5 №16).
            return Err(CmdError::Rejected(
                text("ops-not-yet", &[("what", "vfs")]).into(),
            ));
        }
        if let Some(c) = a.str("conflict") {
            req.opts.conflict = parse_conflict(c).ok_or_else(|| {
                CmdError::Rejected(text("ops-conflict-bad", &[("what", c)]).into())
            })?;
        }
        if let Some(f) = a.str("filter") {
            req.opts.filter = Some(f.into());
        }
        let mask =
            match req.opts.filter.as_deref() {
                None | Some("") => None,
                Some(m) => Some(urfin_core::Mask::parse(m).map_err(|_| {
                    CmdError::Rejected(text("ops-filter-bad", &[("mask", m)]).into())
                })?),
            };
        let start = crate::ops::copy::Start {
            req,
            dest,
            dest_raw: raw,
            mask,
            small_file: u64::from(self.cfg.small_file),
        };
        crate::ops::copy::start(&self.engine, cx, start);
        Ok(())
    }
}

/// `conflict=…` команды: политика по имени.
fn parse_conflict(s: &str) -> Option<ConflictPolicy> {
    Some(match s {
        "ask" => ConflictPolicy::Ask,
        "overwrite" => ConflictPolicy::Overwrite,
        "skip" => ConflictPolicy::Skip,
        "newer" => ConflictPolicy::OverwriteIfNewer,
        "differs" => ConflictPolicy::OverwriteIfDiffers,
        "identical" => ConflictPolicy::SkipIdentical,
        "rename" => ConflictPolicy::RenameNew(RenameScheme::Counter),
        "rename_time" => ConflictPolicy::RenameNew(RenameScheme::Timestamp),
        "rename_existing" => ConflictPolicy::RenameExisting(RenameScheme::Counter),
        "rename_existing_time" => ConflictPolicy::RenameExisting(RenameScheme::Timestamp),
        _ => return None,
    })
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
                let raw = dest.to_owned();
                let dest = parse_dest(&req.base, dest).map_err(dest_error)?;
                tracing::info!(target: "ops", kind = ?req.kind, items = req.items.len(), dest = %dest.path.display(), "request");
                self.transfer(cx, req, dest, raw, a)
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
                delete(cx, req)
            }
            "ops.mkdir" => {
                let Some(name) = a.str("name") else {
                    draft(cx, cmd, false);
                    return Ok(());
                };
                mkdir(cx, name)
            }
            "ops.rename" => {
                let Some(to) = a.str("to") else {
                    // Без имени — поле на месте (Shift+F6).
                    cx.dispatch(Command::new("ui.rename"));
                    return Ok(());
                };
                rename(cx, to)
            }
            _ => Err(not_yet(cmd)),
        }
    }
}

/// Одиночная операция активного таба вне UI-потока (S5-10): по успеху курсор —
/// на `focus`, листинг перечитывается; ошибка — тостом.
fn quick(
    cx: &mut Cx<'_>,
    name: &'static str,
    work: impl FnOnce(&std::path::Path) -> Result<Option<CompactString>, String> + Send + 'static,
) -> Result<(), CmdError> {
    let t = cx.state.panel(cx.state.active).tab();
    if !t.loc.is_local() {
        return Err(CmdError::Rejected(
            text("ops-not-yet", &[("what", name)]).into(),
        ));
    }
    let (tab, dir) = (t.id, t.loc.path.clone());
    let sink = cx.rt.sink();
    cx.rt
        .spawn(Lane::Interactive, name, cx.rt.token(), move |tcx| {
            let result = work(&dir);
            let m = urfin_core::msg(name, move |cx| match result {
                Ok(focus) => {
                    if let Some(t) = cx.state.tab_mut(tab)
                        && focus.is_some()
                    {
                        t.pending_focus = focus;
                    }
                    cx.dispatch(Command::new("panel.refresh").arg("tab", i64::from(tab.0)));
                }
                Err(e) => cx.toast(Level::Error, e),
            });
            let _ = sink.send(m, &tcx.cancel);
        });
    Ok(())
}

/// `ops.mkdir {name}`: `a/b/c`, `x;y` (Far); курсор — на первый созданный.
fn mkdir(cx: &mut Cx<'_>, spec: &str) -> Result<(), CmdError> {
    let spec = spec.to_owned();
    quick(cx, "ops.mkdir", move |dir| {
        crate::ops::mkdir::mkdir(dir, &spec)
            .map(|made| made.into_iter().next())
            .map_err(|e| {
                text(
                    "ops-mkdir-failed",
                    &[("name", e.name.as_str()), ("error", &io_text(&e.error))],
                )
            })
    })
}

/// `ops.rename {to}`: элемент под курсором активного таба, в его каталоге.
fn rename(cx: &mut Cx<'_>, to: &str) -> Result<(), CmdError> {
    let t = cx.state.panel(cx.state.active).tab();
    let Some(Row::Entry { entry, .. }) = t.current() else {
        return Err(CmdError::Rejected(text("ops-nothing", &[]).into()));
    };
    let to = to.trim();
    if !crate::ops::rename::is_plain_name(to) {
        return Err(CmdError::Rejected(text("ops-rename-bad", &[]).into()));
    }
    let (from, to) = (entry.name.clone(), CompactString::from(to));
    quick(cx, "ops.rename", move |dir| {
        crate::ops::rename::rename(dir, &from, &to)
            .map(|()| Some(to.clone()))
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    text("ops-exists", &[("name", to.as_str())])
                } else {
                    text(
                        "ops-rename-failed",
                        &[("name", from.as_str()), ("error", &io_text(&e))],
                    )
                }
            })
    })
}

/// Удаление задачей (прогресс, отмена в job-баре): корзина одним системным
/// вызовом, насовсем — по элементам. По окончании листинг перечитывается.
fn delete(cx: &mut Cx<'_>, req: OpRequest) -> Result<(), CmdError> {
    if !req.base.is_local() {
        return Err(CmdError::Rejected(
            text("ops-not-yet", &[("what", "ops.delete")]).into(),
        ));
    }
    let trash = req.kind == OpKind::Trash;
    let tab = cx.state.panel(cx.state.active).tab().id;
    let job = cx.state.jobs.start(
        urfin_core::JobSpec {
            kind: if trash { "trash" } else { "delete" },
            title: text(
                if trash {
                    "ops-job-trash"
                } else {
                    "ops-job-delete"
                },
                &[],
            ),
            lane: Lane::Bulk,
            priority: 0,
            owner_ns: "ops",
        },
        cx.rt.sink(),
    );
    let sink = cx.rt.sink();
    let (dir, items) = (req.base.path, req.items);
    cx.rt
        .spawn_dedicated("ops.delete".into(), cx.rt.token(), move |tcx| {
            use std::sync::atomic::Ordering;
            let _span =
                tracing::info_span!("ops.job", kind = if trash { "trash" } else { "delete" })
                    .entered();
            let p = job.progress();
            let n = items.len() as u64;
            p.items_total.store(n, Ordering::Relaxed);
            p.total_final.store(true, Ordering::Relaxed);
            let errors: Vec<String> = if trash {
                let r = crate::ops::delete::delete_to_trash(&dir, &items);
                p.items_done.store(n, Ordering::Relaxed);
                r.err().into_iter().collect()
            } else {
                let stop = || tcx.cancel.is_cancelled() || job.checkpoint().is_err();
                let done = |name: &str| {
                    p.set_current(name);
                    p.items_done.fetch_add(1, Ordering::Relaxed);
                };
                crate::ops::delete::delete_permanent(&dir, &items, &stop, &done)
                    .into_iter()
                    .map(|e| format!("{}: {}", e.name, e.error))
                    .collect()
            };
            let failed = errors.len() as u64;
            let m = urfin_core::msg("ops.delete", move |cx| {
                if let Some(first) = errors.first() {
                    let more = if errors.len() > 1 {
                        format!(" (+{})", errors.len() - 1)
                    } else {
                        String::new()
                    };
                    cx.toast(
                        Level::Error,
                        text("ops-delete-failed", &[("error", &format!("{first}{more}"))]),
                    );
                }
                cx.dispatch(Command::new("panel.refresh").arg("tab", i64::from(tab.0)));
            });
            let _ = sink.send(m, &tcx.cancel);
            let cancelled = job.cancel_token().is_cancelled();
            let items = p.items_done.load(Ordering::Relaxed);
            job.finish(if cancelled {
                Err("cancelled".into())
            } else {
                Ok(urfin_core::JobSummary {
                    items,
                    bytes: 0,
                    skipped: 0,
                    errors: failed,
                })
            });
        });
    Ok(())
}

/// Текст ошибки ОС без «(os error N)».
fn io_text(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error") {
        Some(i) => s[..i].to_owned(),
        None => s,
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

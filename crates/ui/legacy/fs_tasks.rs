//! Фоновые чтения ФС для виджетов (ADR-0014): handler `ui` ставит задачу на полосу
//! Interactive, ответ — [`Msg`] с поколением: устаревший (меню закрыто, путь введён
//! заново) не применяется. Сам handler I/O не делает.

use std::path::Path;
use std::sync::Arc;

use compact_str::CompactString;
use urfin_core::{CancelToken, Command, Cx, Lane, Level, Location, Msg, Runtime, Side};
use urfin_vfs::{ListOpts, VfsRegistry};

use crate::legacy::handler::ui_state;
use crate::legacy::panel::side_arg;

/// Пунктов в меню подкаталогов не больше (S2 §2.9: меню, а не список).
pub const MENU_MAX: usize = 500;

/// Подкаталоги `loc`: только каталоги, натуральный порядок, не больше [`MENU_MAX`].
pub fn spawn_subdirs(
    rt: &Runtime,
    vfs: Arc<VfsRegistry>,
    loc: Location,
    generation: u64,
    cancel: CancelToken,
) {
    rt.spawn(Lane::Interactive, "ui.subdirs", cancel, move |t| {
        let items = list_dirs(&vfs, &loc, &t.cancel, MENU_MAX);
        let _ = t
            .sink
            .send(Box::new(SubdirsListed { generation, items }), &t.cancel);
    });
}

/// Подкаталогов узла дерева не больше (S2-18): дальше — без них.
pub const TREE_MAX: usize = 20_000;

/// Подкаталоги узла дерева `loc` (S2-18); ответ — с поколением дерева.
pub fn spawn_tree_children(
    rt: &Runtime,
    vfs: Arc<VfsRegistry>,
    side: Side,
    loc: Location,
    generation: u64,
    cancel: CancelToken,
) {
    rt.spawn(Lane::Interactive, "ui.tree", cancel, move |t| {
        let names = list_dirs(&vfs, &loc, &t.cancel, TREE_MAX);
        let msg = TreeListed {
            side,
            loc,
            generation,
            names,
        };
        let _ = t.sink.send(Box::new(msg), &t.cancel);
    });
}

struct TreeListed {
    side: Side,
    loc: Location,
    generation: u64,
    names: Result<Vec<CompactString>, CompactString>,
}

impl Msg for TreeListed {
    fn name(&self) -> &'static str {
        "ui.tree"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        crate::legacy::views::tree_children(cx, self.side, &self.loc, self.generation, self.names);
    }
}

fn list_dirs(
    vfs: &VfsRegistry,
    loc: &Location,
    cancel: &CancelToken,
    max: usize,
) -> Result<Vec<CompactString>, CompactString> {
    let err = |e: urfin_vfs::VfsError| CompactString::from(e.to_string());
    let fs = vfs.get(loc).map_err(err)?;
    let opts = ListOpts {
        resolve_links: true,
        ..ListOpts::default()
    };
    let mut stream = fs.list(loc, &opts).map_err(err)?;
    let mut batch = Vec::new();
    let mut names = Vec::new();
    loop {
        batch.clear();
        let more = stream.next_batch(&mut batch, cancel).map_err(err)?;
        names.extend(
            batch
                .iter()
                .filter(|e| e.kind.is_dir_like())
                .map(|e| e.name.clone()),
        );
        if !more || cancel.is_cancelled() {
            break;
        }
    }
    names.sort_by(|a, b| urfin_vfs::sort::name_cmp(a, b, true));
    names.truncate(max);
    Ok(names)
}

struct SubdirsListed {
    generation: u64,
    items: Result<Vec<CompactString>, CompactString>,
}

impl Msg for SubdirsListed {
    fn name(&self) -> &'static str {
        "ui.subdirs"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        match ui_state(cx).subdirs.as_mut() {
            Some(s) if s.generation == self.generation => s.items = Some(self.items),
            _ => cx.keep_rev(),
        }
    }
}

/// Введённый путь: относительный — от `base` (S1 §7.3), uri — как есть (ADR-0009).
pub fn resolve_input(text: &str, base: &Location) -> Option<Location> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((backend, _)) = text.split_once(':')
        && backend.len() >= 2
        && backend
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
    {
        return Location::parse_uri(text).ok();
    }
    let p = Path::new(text);
    // `D:` — корень диска, а не текущий каталог диска.
    if cfg!(windows) && text.len() == 2 && text.ends_with(':') {
        return Some(Location::local(format!("{text}\\")));
    }
    if p.is_absolute() || p.has_root() {
        return Some(Location::local(p));
    }
    base.is_local().then(|| Location::local(base.path.join(p)))
}

/// Проверка, что введённый каталог читается: первый батч `Vfs::list` или ошибка.
pub fn spawn_path_check(
    rt: &Runtime,
    vfs: Arc<VfsRegistry>,
    side: Side,
    loc: Location,
    generation: u64,
    cancel: CancelToken,
) {
    rt.spawn(Lane::Interactive, "ui.path_check", cancel, move |t| {
        let ok = vfs
            .get(&loc)
            .and_then(|fs| fs.list(&loc, &ListOpts::default()))
            .and_then(|mut s| s.next_batch(&mut Vec::new(), &t.cancel).map(|_| ()));
        let msg = PathChecked {
            side,
            loc,
            generation,
            ok: ok.map_err(|e| CompactString::from(e.to_string())),
        };
        let _ = t.sink.send(Box::new(msg), &t.cancel);
    });
}

struct PathChecked {
    side: Side,
    loc: Location,
    generation: u64,
    ok: Result<(), CompactString>,
}

impl Msg for PathChecked {
    fn name(&self) -> &'static str {
        "ui.path_check"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        let st = ui_state(cx);
        if st.path_check != Some(self.generation) || st.path_edit != Some(self.side) {
            cx.keep_rev();
            return;
        }
        st.path_check = None;
        match self.ok {
            Ok(()) => {
                cx.dispatch(Command::new("ui.cancel"));
                cx.dispatch(
                    Command::new("panel.go")
                        .arg("side", side_arg(self.side))
                        .arg("path", self.loc),
                );
            }
            Err(e) => {
                // Путь не меняется, поле остаётся открытым (ADR-0014).
                let mut args = urfin_i18n::FluentArgs::new();
                args.set("path", display(&self.loc));
                args.set("error", e.to_string());
                let text = urfin_i18n::current().fmt("ui-path-not-found", &args);
                tracing::info!(path = %self.loc.to_uri(), error = %e, "path input rejected");
                cx.toast(Level::Warn, text);
            }
        }
    }
}

fn display(loc: &Location) -> String {
    if loc.is_local() {
        loc.path.display().to_string()
    } else {
        loc.to_uri()
    }
}

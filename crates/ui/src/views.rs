//! Режимы панели «Дерево», «Инфо», «Быстрый просмотр» (S2 §2.4, §2.5, S2-18): данные.
//!
//! Панель в режиме показывает соседнюю: её каталог (Инфо, Дерево) или элемент под
//! курсором (просмотр). Handler `ui` в `on_pump` сверяет состояние с соседней панелью
//! и ставит чтения на полосу Interactive; ответ — `Msg` с ключом/поколением, устаревший
//! не применяется. UI-поток I/O не делает.
//!
//! Просмотр читает первые `64 КиБ` после паузы 50 мс, прежнее чтение отменяется: при
//! быстром движении курсора в очереди не больше одного чтения. Дерево ленивое:
//! подкаталоги узла читаются при раскрытии; движение курсора с паузой 150 мс ведёт
//! соседнюю панель (`panel.go`), Enter — сразу и с фокусом.

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use compact_str::CompactString;
use urfin_core::{
    AppState, CancelToken, Command, Cx, Gen, Lane, Location, Msg, PanelMode, Row, Runtime, Side,
    TabId,
};
use urfin_vfs::volume::{Media, VolCaps};
use urfin_vfs::{Space, VfsRegistry};

use crate::handler::ui_state;
use crate::side_arg;

/// Сколько читает быстрый просмотр (S2 §2.5).
pub const PREVIEW_BYTES: usize = 64 * 1024;
/// Пауза перед чтением просмотра.
pub const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(50);
/// Пауза, после которой курсор дерева ведёт соседнюю панель.
pub const TREE_FOLLOW: Duration = Duration::from_millis(150);

/// Чтений просмотра с начала процесса (тест «не больше одного чтения в очереди»).
pub static PREVIEW_READS: AtomicUsize = AtomicUsize::new(0);

// ---- Инфо ----

/// Том для «Инфо».
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeView {
    pub root: PathBuf,
    pub label: String,
    pub fs_name: String,
    pub media: Media,
    pub caps: VolCaps,
    pub space: Option<Space>,
}

/// «Инфо» на панели: каталог соседней и его том (`None` — читается).
#[derive(Clone, Debug, PartialEq)]
pub struct InfoState {
    pub loc: Location,
    pub volume: Option<Result<VolumeView, CompactString>>,
}

// ---- Просмотр ----

/// Кодировка текста просмотра.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Cp1251,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Cp1251 => "1251",
        }
    }
}

/// Что показывает просмотр.
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    Text {
        text: Arc<str>,
        encoding: Encoding,
        crlf: bool,
        /// Прочитано не всё (файл больше `PREVIEW_BYTES`).
        truncated: bool,
    },
    Hex(Arc<[u8]>),
    /// Каталог под курсором: содержимое — в самой панели.
    Dir,
    /// `..` или пустой каталог.
    Nothing,
    Error(CompactString),
}

/// Элемент под курсором соседней панели.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewKey {
    pub tab: TabId,
    pub generation: Gen,
    pub name: CompactString,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewState {
    pub key: Option<PreviewKey>,
    pub size: u64,
    /// `None` — читается.
    pub content: Option<Preview>,
}

// ---- Дерево ----

#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub loc: Location,
    pub name: CompactString,
    pub depth: u16,
    pub expanded: bool,
    /// Подкаталоги прочитаны.
    pub loaded: bool,
    /// Прочитаны и их нет.
    pub empty: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeState {
    /// Узлы в порядке показа (дети — сразу за родителем); свёрнутые поддеревья
    /// остаются в списке, но не показываются.
    pub nodes: Vec<TreeNode>,
    /// Узел под курсором — по пути: переживает вставку детей.
    pub cursor: Location,
    /// Каталог соседней панели, по которому дерево сверено.
    pub synced: Location,
    pub generation: u64,
}

impl TreeState {
    /// Индексы видимых узлов (дети свёрнутых скрыты).
    pub fn visible(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut hide_below: Option<u16> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(d) = hide_below {
                if n.depth > d {
                    continue;
                }
                hide_below = None;
            }
            out.push(i);
            if !n.expanded {
                hide_below = Some(n.depth);
            }
        }
        out
    }

    pub fn index_of(&self, loc: &Location) -> Option<usize> {
        self.nodes.iter().position(|n| &n.loc == loc)
    }

    /// Конец поддерева узла `i` (исключительно).
    fn subtree_end(&self, i: usize) -> usize {
        let d = self.nodes[i].depth;
        self.nodes[i + 1..]
            .iter()
            .position(|n| n.depth <= d)
            .map_or(self.nodes.len(), |k| i + 1 + k)
    }
}

/// Корень пути: `C:\` или `/`.
pub fn root_of(loc: &Location) -> Location {
    let root = loc
        .path
        .ancestors()
        .last()
        .map_or_else(|| loc.path.clone(), std::path::Path::to_path_buf);
    Location {
        path: root,
        ..loc.clone()
    }
}

fn node(loc: Location, depth: u16) -> TreeNode {
    let name = loc.path.file_name().map_or_else(
        || loc.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    TreeNode {
        loc,
        name: name.into(),
        depth,
        expanded: false,
        loaded: false,
        empty: false,
    }
}

/// Дерево с раскрытой цепочкой от корня до `loc`.
pub fn tree_for(loc: &Location, generation: u64) -> TreeState {
    let root = root_of(loc);
    let mut chain: Vec<Location> = loc
        .path
        .ancestors()
        .map(|p| Location {
            path: p.to_path_buf(),
            ..loc.clone()
        })
        .collect();
    chain.reverse();
    if chain.first() != Some(&root) {
        chain.insert(0, root);
    }
    let nodes = chain
        .iter()
        .enumerate()
        .map(|(d, l)| TreeNode {
            expanded: true,
            ..node(l.clone(), u16::try_from(d).unwrap_or(u16::MAX))
        })
        .collect();
    TreeState {
        nodes,
        cursor: loc.clone(),
        synced: loc.clone(),
        generation,
    }
}

/// Подкаталоги узла `parent` прочитаны: вставить, сохранив уже раскрытые поддеревья.
pub fn merge_children(t: &mut TreeState, parent: &Location, names: &[CompactString]) {
    let Some(i) = t.index_of(parent) else {
        return;
    };
    let depth = t.nodes[i].depth + 1;
    let end = t.subtree_end(i);
    // Уже известные дети (цепочка к текущему каталогу) — вместе с поддеревьями.
    let mut blocks: Vec<(CompactString, Vec<TreeNode>)> = Vec::new();
    let mut k = i + 1;
    while k < end {
        let e = t.subtree_end(k);
        blocks.push((t.nodes[k].name.clone(), t.nodes[k..e].to_vec()));
        k = e;
    }
    let mut children = Vec::new();
    for name in names {
        if let Some(pos) = blocks.iter().position(|(n, _)| n == name) {
            children.extend(blocks.swap_remove(pos).1);
        } else {
            let loc = Location {
                path: t.nodes[i].loc.path.join(name.as_str()),
                ..t.nodes[i].loc.clone()
            };
            children.push(node(loc, depth));
        }
    }
    // Каталоги, которых нет в ответе (удалены или скрыты), но на пути курсора, — в конец.
    for (_, b) in blocks {
        children.extend(b);
    }
    t.nodes.splice(i + 1..end, children);
    let n = &mut t.nodes[i];
    n.loaded = true;
    n.empty = names.is_empty();
}

// ---- Синхронизация (on_pump) ----

/// Задачи режимов панелей: отменяются, когда цель меняется.
#[derive(Debug, Default)]
pub struct ViewTasks {
    preview: [Option<CancelToken>; 2],
    follow: Option<CancelToken>,
}

/// Сверить «Инфо», просмотр и дерево с соседними панелями; запустить чтения.
pub fn sync(cx: &mut Cx<'_>, vfs: &Arc<VfsRegistry>, tasks: &mut ViewTasks) {
    for side in [Side::Left, Side::Right] {
        let mode = cx.state.panel(side).tab().mode;
        match mode {
            PanelMode::Info => sync_info(cx, side),
            PanelMode::QView => sync_preview(cx, vfs, tasks, side),
            PanelMode::Tree => sync_tree(cx, vfs, side),
            PanelMode::Files => {}
        }
        if mode != PanelMode::QView
            && let Some(t) = tasks.preview[side.idx()].take()
        {
            t.cancel();
        }
        clear_unused(cx, side, mode);
    }
}

/// Режим выключен — данные не держим.
fn clear_unused(cx: &mut Cx<'_>, side: Side, mode: PanelMode) {
    let i = side.idx();
    let st = ui_state(cx);
    let changed = (mode != PanelMode::Info && st.info[i].take().is_some())
        | (mode != PanelMode::QView && st.preview[i].take().is_some())
        | (mode != PanelMode::Tree && st.tree[i].take().is_some());
    if changed {
        cx.state.touch();
    }
}

fn sync_info(cx: &mut Cx<'_>, side: Side) {
    let loc = cx.state.panel(side.other()).tab().loc.clone();
    let i = side.idx();
    if ui_state(cx).info[i].as_ref().is_some_and(|s| s.loc == loc) {
        return;
    }
    ui_state(cx).info[i] = Some(InfoState {
        loc: loc.clone(),
        volume: None,
    });
    cx.state.touch();
    if !loc.is_local() {
        ui_state(cx).info[i] = Some(InfoState {
            loc,
            volume: Some(Err("—".into())),
        });
        return;
    }
    cx.rt
        .spawn(Lane::Interactive, "ui.volume", cx.rt.token(), move |t| {
            let volume = urfin_vfs::volume::of(&loc.path)
                .map(|v| VolumeView {
                    space: urfin_vfs::volume::space(&loc.path).ok(),
                    root: v.root,
                    label: v.label,
                    fs_name: v.fs_name,
                    media: v.media,
                    caps: v.caps,
                })
                .map_err(|e| CompactString::from(e.to_string()));
            let _ = t
                .sink
                .send(Box::new(VolumeRead { side, loc, volume }), &t.cancel);
        });
}

struct VolumeRead {
    side: Side,
    loc: Location,
    volume: Result<VolumeView, CompactString>,
}

impl Msg for VolumeRead {
    fn name(&self) -> &'static str {
        "ui.volume"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        match ui_state(cx).info[self.side.idx()].as_mut() {
            Some(s) if s.loc == self.loc => s.volume = Some(self.volume),
            _ => cx.keep_rev(),
        }
    }
}

/// Цель просмотра: элемент под курсором соседней панели.
fn preview_target(
    s: &AppState,
    side: Side,
) -> (Option<PreviewKey>, Option<Preview>, Option<Location>, u64) {
    let t = s.panel(side.other()).tab();
    match t.current() {
        Some(Row::Entry { entry, .. }) => {
            let key = PreviewKey {
                tab: t.id,
                generation: t.listing.generation,
                name: entry.name.clone(),
            };
            if entry.kind.is_dir_like() {
                (Some(key), Some(Preview::Dir), None, 0)
            } else {
                let loc = Location {
                    path: t.loc.path.join(entry.name.as_str()),
                    ..t.loc.clone()
                };
                (Some(key), None, Some(loc), entry.size)
            }
        }
        _ => (None, Some(Preview::Nothing), None, 0),
    }
}

fn sync_preview(cx: &mut Cx<'_>, vfs: &Arc<VfsRegistry>, tasks: &mut ViewTasks, side: Side) {
    let i = side.idx();
    let (key, ready, loc, bytes) = preview_target(cx.state, side);
    // Та же цель — уже показана или читается.
    if ui_state(cx).preview[i]
        .as_ref()
        .is_some_and(|p| p.key == key)
    {
        return;
    }
    if let Some(t) = tasks.preview[i].take() {
        t.cancel();
    }
    ui_state(cx).preview[i] = Some(PreviewState {
        key: key.clone(),
        size: bytes,
        content: ready,
    });
    cx.state.touch();
    let Some(loc) = loc else {
        return;
    };
    let cancel = cx.rt.token();
    tasks.preview[i] = Some(cancel.clone());
    let vfs = Arc::clone(vfs);
    cx.rt
        .spawn(Lane::Interactive, "ui.preview", cancel, move |t| {
            // Пауза: при быстром движении курсора чтение отменяется до открытия файла.
            let start = std::time::Instant::now();
            while start.elapsed() < PREVIEW_DEBOUNCE {
                if t.cancel.is_cancelled() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if t.cancel.is_cancelled() {
                return;
            }
            PREVIEW_READS.fetch_add(1, Ordering::Relaxed);
            let content = read_preview(&vfs, &loc);
            let _ = t
                .sink
                .send(Box::new(PreviewRead { side, key, content }), &t.cancel);
        });
}

fn read_preview(vfs: &VfsRegistry, loc: &Location) -> Preview {
    let r = vfs.get(loc).and_then(|fs| fs.open_read(loc));
    let mut f = match r {
        Ok(f) => f,
        Err(e) => return Preview::Error(e.to_string().into()),
    };
    let mut buf = vec![0u8; PREVIEW_BYTES + 1];
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) => return Preview::Error(e.to_string().into()),
        }
    }
    let truncated = n > PREVIEW_BYTES;
    buf.truncate(n.min(PREVIEW_BYTES));
    classify(&buf, truncated)
}

/// Текст (UTF-8/UTF-16 по BOM, UTF-8 без BOM, иначе 1251) или hex для двоичных.
pub fn classify(b: &[u8], truncated: bool) -> Preview {
    let text = |s: String, encoding| Preview::Text {
        crlf: s.contains("\r\n"),
        text: s.into(),
        encoding,
        truncated,
    };
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return text(
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        );
    }
    for (bom, enc) in [
        ([0xFF, 0xFE], Encoding::Utf16Le),
        ([0xFE, 0xFF], Encoding::Utf16Be),
    ] {
        if let Some(rest) = b.strip_prefix(&bom) {
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&c| {
                    if enc == Encoding::Utf16Le {
                        u16::from_le_bytes(c)
                    } else {
                        u16::from_be_bytes(c)
                    }
                })
                .collect();
            return text(String::from_utf16_lossy(&units), enc);
        }
    }
    // Двоичный: NUL в начале файла.
    if b.iter().take(8192).any(|&c| c == 0) {
        return Preview::Hex(b.into());
    }
    match std::str::from_utf8(b) {
        Ok(s) => text(s.to_owned(), Encoding::Utf8),
        // Обрезано посреди символа UTF-8 — всё равно UTF-8.
        Err(e) if truncated && e.error_len().is_none() => text(
            String::from_utf8_lossy(&b[..e.valid_up_to()]).into_owned(),
            Encoding::Utf8,
        ),
        Err(_) => text(cp1251(b), Encoding::Cp1251),
    }
}

/// Windows-1251 → строка.
fn cp1251(b: &[u8]) -> String {
    const HIGH: [char; 64] = [
        'Ђ', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', 'ђ', '‘',
        '’', '“', '”', '•', '–', '—', '\u{98}', '™', 'љ', '›', 'њ', 'ќ', 'ћ', 'џ', '\u{a0}', 'Ў',
        'ў', 'Ј', '¤', 'Ґ', '¦', '§', 'Ё', '©', 'Є', '«', '¬', '\u{ad}', '®', 'Ї', '°', '±', 'І',
        'і', 'ґ', 'µ', '¶', '·', 'ё', '№', 'є', '»', 'ј', 'Ѕ', 'ѕ', 'ї',
    ];
    b.iter()
        .map(|&c| match c {
            0..=0x7F => char::from(c),
            0x80..=0xBF => HIGH[usize::from(c - 0x80)],
            // А..я подряд.
            _ => char::from_u32(0x0410 + u32::from(c - 0xC0)).unwrap_or('?'),
        })
        .collect()
}

struct PreviewRead {
    side: Side,
    key: Option<PreviewKey>,
    content: Preview,
}

impl Msg for PreviewRead {
    fn name(&self) -> &'static str {
        "ui.preview"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        match ui_state(cx).preview[self.side.idx()].as_mut() {
            Some(p) if p.key == self.key => p.content = Some(self.content),
            _ => cx.keep_rev(),
        }
    }
}

// ---- Дерево ----

fn sync_tree(cx: &mut Cx<'_>, vfs: &Arc<VfsRegistry>, side: Side) {
    let i = side.idx();
    let loc = cx.state.panel(side.other()).tab().loc.clone();
    let st = ui_state(cx);
    let fresh = match &st.tree[i] {
        Some(t) if t.synced == loc => return,
        // Другой том — дерево заново.
        Some(t) => root_of(&t.synced) != root_of(&loc),
        None => true,
    };
    let generation = st.bump();
    if fresh {
        st.tree[i] = Some(tree_for(&loc, generation));
    } else if let Some(t) = st.tree[i].as_mut() {
        t.synced = loc.clone();
        t.cursor = loc.clone();
        t.generation = generation;
        ensure_path(t, &loc);
    }
    cx.state.touch();
    load_pending(cx, vfs, side);
}

/// Раскрыть узлы на пути к `loc`; недостающие — добавить к родителю (подгрузятся).
fn ensure_path(t: &mut TreeState, loc: &Location) {
    let mut chain: Vec<Location> = loc
        .path
        .ancestors()
        .map(|p| Location {
            path: p.to_path_buf(),
            ..loc.clone()
        })
        .collect();
    chain.reverse();
    for w in chain.windows(2) {
        let (parent, child) = (&w[0], &w[1]);
        let Some(pi) = t.index_of(parent) else {
            continue;
        };
        t.nodes[pi].expanded = true;
        if t.index_of(child).is_none() {
            let at = t.subtree_end(pi);
            t.nodes
                .insert(at, node(child.clone(), t.nodes[pi].depth + 1));
        }
    }
    if let Some(i) = t.index_of(loc) {
        t.nodes[i].expanded = true;
    }
}

/// Прочитать подкаталоги раскрытых, ещё не прочитанных узлов.
fn load_pending(cx: &mut Cx<'_>, vfs: &Arc<VfsRegistry>, side: Side) {
    let Some(t) = ui_state(cx).tree[side.idx()].clone() else {
        return;
    };
    for n in t.nodes.iter().filter(|n| n.expanded && !n.loaded) {
        crate::fs_tasks::spawn_tree_children(
            cx.rt,
            Arc::clone(vfs),
            side,
            n.loc.clone(),
            t.generation,
            cx.rt.token(),
        );
    }
}

/// Подкаталоги узла прочитаны (`fs_tasks`): вставить, если дерево того же поколения.
pub(crate) fn tree_children(
    cx: &mut Cx<'_>,
    side: Side,
    parent: &Location,
    generation: u64,
    names: Result<Vec<CompactString>, CompactString>,
) {
    let Some(t) = ui_state(cx).tree[side.idx()].as_mut() else {
        cx.keep_rev();
        return;
    };
    if t.generation != generation {
        cx.keep_rev();
        return;
    }
    // Ошибка чтения (нет прав) — узел без детей.
    merge_children(t, parent, &names.unwrap_or_default());
}

/// Команды дерева активной панели (`ui.tree.*`, контекст keymap `panel && tree`).
pub fn tree_command(
    cx: &mut Cx<'_>,
    vfs: &Arc<VfsRegistry>,
    tasks: &mut ViewTasks,
    id: &str,
    n: Option<i64>,
) {
    let side = cx.state.active;
    let page = 20usize;
    let Some(t) = ui_state(cx).tree[side.idx()].as_mut() else {
        cx.keep_rev();
        return;
    };
    let vis = t.visible();
    let Some(cur) = t
        .index_of(&t.cursor)
        .and_then(|i| vis.iter().position(|v| *v == i))
    else {
        cx.keep_rev();
        return;
    };
    let last = vis.len().saturating_sub(1);
    let mut load = None;
    let to = match id {
        "ui.tree.up" => cur.saturating_sub(1),
        "ui.tree.down" => (cur + 1).min(last),
        "ui.tree.page_up" => cur.saturating_sub(page),
        "ui.tree.page_down" => (cur + page).min(last),
        "ui.tree.home" => 0,
        "ui.tree.end" => last,
        "ui.tree.select" => usize::try_from(n.unwrap_or(0)).unwrap_or(0).min(last),
        "ui.tree.expand" => {
            let i = vis[cur];
            if t.nodes[i].expanded {
                // Уже раскрыт — к первому ребёнку.
                if vis
                    .get(cur + 1)
                    .is_some_and(|&k| t.nodes[k].depth > t.nodes[i].depth)
                {
                    cur + 1
                } else {
                    cur
                }
            } else {
                t.nodes[i].expanded = true;
                if !t.nodes[i].loaded {
                    load = Some((t.nodes[i].loc.clone(), t.generation));
                }
                cur
            }
        }
        "ui.tree.collapse" => {
            let i = vis[cur];
            if t.nodes[i].expanded {
                t.nodes[i].expanded = false;
                cur
            } else {
                // Свёрнут — к родителю.
                let d = t.nodes[i].depth;
                vis[..cur]
                    .iter()
                    .rposition(|&k| t.nodes[k].depth < d)
                    .unwrap_or(cur)
            }
        }
        // `ui.tree.enter` и прочие — курсор на месте.
        _ => cur,
    };
    let target = t.nodes[vis.get(to).copied().unwrap_or(vis[cur])]
        .loc
        .clone();
    t.cursor = target.clone();
    if let Some((loc, generation)) = load {
        crate::fs_tasks::spawn_tree_children(
            cx.rt,
            Arc::clone(vfs),
            side,
            loc,
            generation,
            cx.rt.token(),
        );
    }
    if let Some(f) = tasks.follow.take() {
        f.cancel();
    }
    let other = side_arg(side.other());
    if id == "ui.tree.enter" {
        cx.dispatch(
            Command::new("panel.go")
                .arg("side", other)
                .arg("path", target),
        );
        cx.dispatch(Command::new("panel.focus").arg("side", other));
        return;
    }
    if to != cur {
        tasks.follow = Some(follow_later(cx.rt, side, target));
    }
}

/// Через [`TREE_FOLLOW`] — `panel.go` соседней панели, если курсор дерева всё там же.
fn follow_later(rt: &Runtime, side: Side, loc: Location) -> CancelToken {
    let cancel = rt.token();
    rt.spawn(
        Lane::Interactive,
        "ui.tree.follow",
        cancel.clone(),
        move |t| {
            std::thread::sleep(TREE_FOLLOW);
            if t.cancel.is_cancelled() {
                return;
            }
            let _ = t.sink.send(Box::new(TreeFollow { side, loc }), &t.cancel);
        },
    );
    cancel
}

struct TreeFollow {
    side: Side,
    loc: Location,
}

impl Msg for TreeFollow {
    fn name(&self) -> &'static str {
        "ui.tree.follow"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        let still = ui_state(cx).tree[self.side.idx()]
            .as_ref()
            .is_some_and(|t| t.cursor == self.loc);
        if still {
            cx.dispatch(
                Command::new("panel.go")
                    .arg("side", side_arg(self.side.other()))
                    .arg("path", self.loc),
            );
        } else {
            cx.keep_rev();
        }
    }
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;

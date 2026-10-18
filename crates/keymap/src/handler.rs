//! Handler `keymap`: быстрый поиск (S3 §1.3, S3-06, ADR-0016) и начатый аккорд.
//!
//! Состояние — [`KeymapView`] в `AppState.ext`; меняет его только этот handler, ui
//! рисует. Ввод поиска приходит командами из [`crate::KeyInput`], курсор ставит
//! `panel.cursor.to` (имя найдено здесь же: нет совпадения — символ не принимается и
//! тоста нет).

use std::time::Instant;

use compact_str::CompactString;
use smallvec::SmallVec;
use urfin_core::{
    ArgSpec, CmdError, Command, CommandMeta, Core, Cx, Focus, Gen, Handler, Mask, OverlayKind, Row,
    Side, Tab, TabId, UiEvent, ValueKind,
};

/// Строка быстрого поиска на панели `side`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickSearch {
    pub side: Side,
    /// Введённое (без неявной `*` в конце).
    pub mask: CompactString,
    /// Совпадений с маской в табе.
    pub matches: u32,
    /// Последний не принятый символ: ui подсвечивает поле 300 мс (S2 §576).
    pub miss_at: Option<Instant>,
}

/// Состояние keymap для ui (S3 §5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeymapView {
    pub qs: Option<QuickSearch>,
    /// Начатый аккорд для строки статуса: `Ctrl+K`.
    pub pending: Option<CompactString>,
}

const fn arg(name: &'static str, kind: ValueKind, required: bool) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        required,
    }
}

const fn meta(
    id: &'static str,
    title: &'static str,
    args: &'static [ArgSpec],
    repeatable: bool,
) -> CommandMeta {
    CommandMeta {
        id,
        title,
        category: "keymap",
        args,
        repeatable,
    }
}

static COMMANDS: [CommandMeta; 7] = [
    meta(
        "keymap.qs.start",
        "Quick search",
        &[arg("text", ValueKind::Str, false)],
        false,
    ),
    meta(
        "keymap.qs.input",
        "Quick search: type",
        &[arg("text", ValueKind::Str, true)],
        false,
    ),
    meta("keymap.qs.next", "Quick search: next", &[], true),
    meta("keymap.qs.prev", "Quick search: previous", &[], true),
    meta("keymap.qs.backspace", "Quick search: erase", &[], true),
    meta("keymap.qs.cancel", "Quick search: close", &[], false),
    meta(
        "keymap.pending",
        "Pending chord",
        &[arg("chord", ValueKind::Str, false)],
        false,
    ),
];

/// Регистрирует handler `keymap` (S3 §5 `register`; загрузку раскладки и `KeyInput`
/// делает app, перезагрузка — S3-10).
pub fn register(core: &mut Core) {
    core.register(Box::new(KeymapHandler::default()));
}

#[derive(Default)]
struct KeymapHandler {
    /// Строки, подходящие под текущую маску: следующий символ проверяет только их.
    cache: Option<Cands>,
}

/// Кандидаты маски `mask` в табе `key`.
struct Cands {
    key: TabKey,
    mask: CompactString,
    rows: Vec<u32>,
}

/// Содержимое таба не менялось: тот же таб, загрузка, порядок.
#[derive(Clone, Copy, PartialEq, Eq)]
struct TabKey {
    tab: TabId,
    generation: Gen,
    arrange_seq: u32,
    rows: u32,
}

fn tab_key(t: &Tab) -> TabKey {
    TabKey {
        tab: t.id,
        generation: t.listing.generation,
        arrange_seq: t.listing.arrange_seq,
        rows: t.rows(),
    }
}

impl Handler for KeymapHandler {
    fn namespace(&self) -> &'static str {
        "keymap"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let a = &cmd.args;
        match cmd.id.as_str() {
            "keymap.qs.start" => {
                let side = cx.state.active;
                view(cx).qs = Some(QuickSearch {
                    side,
                    mask: CompactString::default(),
                    matches: 0,
                    miss_at: None,
                });
                set_focus(cx, Focus::Overlay(OverlayKind::QuickSearch));
                self.cache = None;
                if let Some(t) = a.str("text") {
                    self.input(cx, t);
                }
            }
            "keymap.qs.input" => {
                if qs(cx).is_some() {
                    self.input(cx, a.str("text").unwrap_or_default());
                } else {
                    cx.keep_rev();
                }
            }
            "keymap.qs.next" => step(cx, true),
            "keymap.qs.prev" => step(cx, false),
            "keymap.qs.backspace" => {
                let Some(q) = qs(cx) else {
                    cx.keep_rev();
                    return Ok(());
                };
                let mut mask = q.mask.clone();
                mask.pop();
                self.cache = None;
                let matches = if mask.is_empty() {
                    0
                } else {
                    self.scan(cx.state.active_tab(), &mask)
                        .map_or(0, |(_, n)| n)
                };
                if let Some(q) = &mut view(cx).qs {
                    q.mask = mask;
                    q.matches = matches;
                    q.miss_at = None;
                }
            }
            "keymap.qs.cancel" => {
                self.cache = None;
                view(cx).qs = None;
                if cx.state.focus == Focus::Overlay(OverlayKind::QuickSearch) {
                    set_focus(cx, Focus::Panel);
                }
            }
            "keymap.pending" => {
                let chord = a.str("chord").map(CompactString::from);
                let v = view(cx);
                if v.pending == chord {
                    cx.keep_rev();
                } else {
                    v.pending = chord;
                }
            }
            _ => return Err(CmdError::Unknown(cmd.id.as_str().into())),
        }
        Ok(())
    }

    /// Фокус ушёл из поиска (клик, меню, `ui.cancel`) — поиск закрыт.
    fn on_pump(&mut self, cx: &mut Cx<'_>) {
        let open = cx
            .state
            .ext
            .get::<KeymapView>()
            .is_some_and(|v| v.qs.is_some());
        if open && cx.state.focus != Focus::Overlay(OverlayKind::QuickSearch) {
            self.cache = None;
            view(cx).qs = None;
            cx.state.touch();
        }
    }
}

impl KeymapHandler {
    /// Строка для маски `text` — текущая, если подходит, иначе первая сверху — и число
    /// совпадений. Если `text` продолжает прошлую маску, проверяются только её
    /// кандидаты. Кэш меняется только при попадании.
    fn scan(&mut self, t: &Tab, text: &str) -> Option<(u32, u32)> {
        let m = Matcher::new(text)?;
        let key = tab_key(t);
        let names = Names::new(t);
        let rows: Vec<u32> = match &self.cache {
            Some(c) if c.key == key && text.starts_with(c.mask.as_str()) => c
                .rows
                .iter()
                .copied()
                .filter(|&r| names.get(r).is_some_and(|n| m.matches(n)))
                .collect(),
            _ => (0..key.rows)
                .filter(|&r| names.get(r).is_some_and(|n| m.matches(n)))
                .collect(),
        };
        let first = *rows.first()?;
        let cur = t.cursor.row;
        let row = if rows.binary_search(&cur).is_ok() {
            cur
        } else {
            first
        };
        let n = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        self.cache = Some(Cands {
            key,
            mask: text.into(),
            rows,
        });
        Some((row, n))
    }

    /// Символы по одному, пока есть совпадения (вставка — так же, Far).
    fn input(&mut self, cx: &mut Cx<'_>, text: &str) {
        let Some(q) = qs(cx) else {
            return;
        };
        let mut mask = q.mask.clone();
        let mut accepted = None;
        let mut miss = false;
        for c in text.chars() {
            let mut next = mask.clone();
            next.push(c);
            if let Some(hit) = self.scan(cx.state.active_tab(), &next) {
                mask = next;
                accepted = Some(hit);
            } else {
                miss = true;
                break;
            }
        }
        if let Some((row, _)) = accepted {
            go(cx, row);
        }
        if let Some(q) = &mut view(cx).qs {
            if let Some((_, n)) = accepted {
                q.mask = mask;
                q.matches = n;
                q.miss_at = None;
            }
            if miss {
                q.miss_at = Some(Instant::now());
            }
        }
    }
}

/// Сопоставление строки поиска: введённое + неявная `*`. Без `*`, `?` и `>` — префикс
/// без учёта регистра (быстрый путь); иначе — core `Mask`: `>txt` — расширение
/// начинается с `txt` (DN), кавычки защищают `,;|` в имени.
#[derive(Debug)]
pub enum Matcher {
    Prefix(SmallVec<[char; 16]>),
    Mask(Mask),
}

impl Matcher {
    pub fn new(text: &str) -> Option<Self> {
        if !text.contains(['*', '?']) && !text.starts_with('>') {
            return Some(Matcher::Prefix(text.chars().map(lower).collect()));
        }
        let text: String = text.chars().filter(|c| *c != '"').collect();
        let pat = match text.strip_prefix('>') {
            Some(ext) => format!("\"*.{ext}*\""),
            None => format!("\"{text}*\""),
        };
        Mask::parse(&pat).ok().map(Matcher::Mask)
    }

    pub fn matches(&self, name: &str) -> bool {
        match self {
            Matcher::Prefix(needle) => {
                let mut chars = name.chars();
                needle.iter().all(|&n| {
                    chars
                        .next()
                        .is_some_and(|c| c == n || (!c.is_ascii_lowercase() && lower(c) == n))
                })
            }
            Matcher::Mask(m) => m.matches(name),
        }
    }
}

/// Строчная буква, если она одна (`İ` остаётся как есть).
fn lower(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// Имена строк таба без `Tab::row` на каждую строку (там проверка `..` по пути).
struct Names<'t> {
    /// 1, если первая строка — `..`.
    off: u32,
    t: &'t Tab,
}

impl<'t> Names<'t> {
    fn new(t: &'t Tab) -> Self {
        let order = u32::try_from(t.listing.order.len()).unwrap_or(u32::MAX);
        Names {
            off: t.rows() - order,
            t,
        }
    }

    fn get(&self, r: u32) -> Option<&'t str> {
        let i = r.checked_sub(self.off)? as usize;
        let idx = *self.t.listing.order.get(i)? as usize;
        self.t.listing.entries.get(idx).map(|e| e.name.as_str())
    }
}

fn name_at(t: &Tab, r: u32) -> Option<&str> {
    match t.row(r) {
        Some(Row::Entry { entry, .. }) => Some(entry.name.as_str()),
        _ => None,
    }
}

fn view<'a>(cx: &'a mut Cx<'_>) -> &'a mut KeymapView {
    if cx.state.ext.get::<KeymapView>().is_none() {
        cx.state.ext.insert(KeymapView::default());
    }
    cx.state
        .ext
        .get_mut::<KeymapView>()
        .expect("inserted above")
}

fn qs(cx: &Cx<'_>) -> Option<QuickSearch> {
    cx.state.ext.get::<KeymapView>()?.qs.clone()
}

fn set_focus(cx: &mut Cx<'_>, focus: Focus) {
    if cx.state.focus != focus {
        cx.state.focus = focus;
        cx.ui(UiEvent::FocusChanged(focus));
    }
}

/// Соседнее совпадение по кругу.
fn step(cx: &mut Cx<'_>, forward: bool) {
    let (rows, cur) = {
        let t = cx.state.active_tab();
        (t.rows(), t.cursor.row)
    };
    let m = qs(cx)
        .filter(|q| !q.mask.is_empty() && rows > 0)
        .and_then(|q| Matcher::new(&q.mask));
    let Some(m) = m else {
        cx.keep_rev();
        return;
    };
    let names = Names::new(cx.state.active_tab());
    let hit = (1..rows)
        .map(|k| {
            if forward {
                (cur + k) % rows
            } else {
                (cur + rows - k) % rows
            }
        })
        .find(|&r| names.get(r).is_some_and(|name| m.matches(name)));
    match hit {
        Some(r) => go(cx, r),
        None => cx.keep_rev(),
    }
}

fn go(cx: &mut Cx<'_>, row: u32) {
    let t = cx.state.active_tab();
    if row == t.cursor.row {
        return;
    }
    if let Some(name) = name_at(t, row) {
        let c = Command::new("panel.cursor.to").arg("name", name);
        cx.dispatch(c);
    }
}

#[cfg(test)]
#[path = "handler_tests.rs"]
mod tests;

//! Групповое переименование (S5 §4.14, S5-23).
//!
//! Шаблон в стиле Total Commander / Far: `[N]` имя без расширения, `[N2-5]`,
//! `[N3]`, `[N2-]` — символы имени (с 1), `[E]` расширение (и `[E1-2]`…),
//! `[C]` / `[C:start:step:width]` счётчик, `[Y]` `[M]` `[D]` дата изменения
//! (местная), `[U]` / `[L]` / `[n]` — дальше прописными / строчными / как есть,
//! `[[` — сама `[`. Пустое расширение съедает точку перед `[E]`. Режим regex:
//! `найти/заменить` (по последней `/`; в имени `/` не бывает), `$1` — группы.
//!
//! План (`from → to`) строится без I/O по именам каталога — для превью на
//! каждое нажатие (10k — меньше 50 мс). Проверки: недопустимое имя, дубли среди
//! новых имён, занятое имя вне набора. Применение — job: цепочки и циклы
//! (`a → b`, `b → a`) — в две фазы через временные имена.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use compact_str::CompactString;
use urfin_core::{Cx, JobId, JobSpec, JobSummary, Lane, Level, Location};

use crate::api::{FailedItem, IoClass, ItemOp, RenameSpec, Report};
use crate::engine::{DirNode, ExecCx, ItemResult, JobEngine, Note, OpExec, Plan, PlanError, Work};
use crate::names::{check_name, name_key};

/// Элемент для переименования.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub name: CompactString,
    /// Наносекунды от эпохи Unix; `i64::MIN` — неизвестно.
    pub mtime: i64,
    pub dir: bool,
}

/// Что не так с новым именем.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Пустое или недопустимое на Windows.
    Invalid,
    /// Такое же новое имя у другого элемента.
    Duplicate,
    /// Имя занято элементом вне набора.
    Exists,
}

/// Строка плана.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub from: CompactString,
    pub to: CompactString,
    pub problem: Option<Problem>,
}

/// План: только изменяющиеся имена.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenamePlan {
    pub rows: Vec<Row>,
    pub problems: usize,
}

/// Ошибка шаблона (текст — для строки превью).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PatternError(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Lit(String),
    Name(usize, Option<usize>),
    Ext(usize, Option<usize>),
    Counter { start: i64, step: i64, width: usize },
    Year,
    Month,
    Day,
    Upper,
    Lower,
    Plain,
}

fn bad(what: &str) -> PatternError {
    PatternError(what.to_owned())
}

/// `N` / `N3` / `N2-5` / `N2-` → (с, по).
fn range(rest: &str) -> Result<(usize, Option<usize>), PatternError> {
    if rest.is_empty() {
        return Ok((1, None));
    }
    let num = |s: &str| s.parse::<usize>().ok().filter(|n| *n > 0);
    match rest.split_once('-') {
        None => {
            let n = num(rest).ok_or_else(|| bad(rest))?;
            Ok((n, Some(n)))
        }
        Some((a, "")) => Ok((num(a).ok_or_else(|| bad(a))?, None)),
        Some((a, b)) => {
            let (a, b) = (num(a).ok_or_else(|| bad(a))?, num(b).ok_or_else(|| bad(b))?);
            if b < a {
                return Err(bad(rest));
            }
            Ok((a, Some(b)))
        }
    }
}

fn parse(pattern: &str) -> Result<Vec<Tok>, PatternError> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = pattern;
    while let Some(i) = rest.find('[') {
        lit.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix('[') {
            lit.push('[');
            rest = r;
            continue;
        }
        let Some(end) = rest.find(']') else {
            return Err(bad(&format!("[{rest}")));
        };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if !lit.is_empty() {
            out.push(Tok::Lit(std::mem::take(&mut lit)));
        }
        let mut chars = tag.chars();
        let tok = match (chars.next(), chars.as_str()) {
            (Some('N'), r) => {
                let (a, b) = range(r)?;
                Tok::Name(a, b)
            }
            (Some('E'), r) => {
                let (a, b) = range(r)?;
                Tok::Ext(a, b)
            }
            (Some('C'), r) => {
                let mut parts = r.split(':').skip(usize::from(r.starts_with(':')));
                let mut next = |d: i64| -> Result<i64, PatternError> {
                    match parts.next() {
                        None | Some("") => Ok(d),
                        Some(p) => p.parse().map_err(|_| bad(p)),
                    }
                };
                let (start, step, width) = (next(1)?, next(1)?, next(1)?);
                Tok::Counter {
                    start,
                    step,
                    width: usize::try_from(width.clamp(1, 12)).unwrap_or(1),
                }
            }
            (Some('Y'), "") => Tok::Year,
            (Some('M'), "") => Tok::Month,
            (Some('D'), "") => Tok::Day,
            (Some('U'), "") => Tok::Upper,
            (Some('L'), "") => Tok::Lower,
            (Some('n'), "") => Tok::Plain,
            _ => return Err(bad(&format!("[{tag}]"))),
        };
        out.push(tok);
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        out.push(Tok::Lit(lit));
    }
    Ok(out)
}

/// `report.final.pdf` → (`report.final`, `pdf`); каталог и `.gitignore` — без расширения.
fn split(name: &str, dir: bool) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && !dir => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

fn slice(s: &str, from: usize, to: Option<usize>) -> String {
    let n = s.chars().count();
    let to = to.unwrap_or(n).min(n);
    s.chars()
        .skip(from - 1)
        .take(to.saturating_sub(from - 1))
        .collect()
}

#[derive(Clone, Copy)]
enum Case {
    Plain,
    Upper,
    Lower,
}

fn push(out: &mut String, s: &str, case: Case) {
    match case {
        Case::Plain => out.push_str(s),
        Case::Upper => out.extend(s.chars().flat_map(char::to_uppercase)),
        Case::Lower => out.extend(s.chars().flat_map(char::to_lowercase)),
    }
}

/// Новое имя `target` (номер `index` в наборе — для счётчика).
fn apply(
    toks: &[Tok],
    target: &Target,
    index: usize,
    date: &dyn Fn(i64) -> Option<(u16, u8, u8)>,
) -> String {
    let (stem, ext) = split(&target.name, target.dir);
    let mut out = String::with_capacity(target.name.len() + 8);
    let mut case = Case::Plain;
    for (k, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Lit(s) => push(&mut out, s, case),
            Tok::Name(from, to) => push(&mut out, &slice(stem, *from, *to), case),
            Tok::Ext(from, to) => {
                let part = slice(ext, *from, *to);
                if part.is_empty()
                    && out.ends_with('.')
                    && matches!(toks.get(k.wrapping_sub(1)), Some(Tok::Lit(_)))
                {
                    out.pop();
                }
                push(&mut out, &part, case);
            }
            Tok::Counter { start, step, width } => {
                let n = start + step * i64::try_from(index).unwrap_or(i64::MAX);
                let _ = write!(out, "{n:0width$}");
            }
            Tok::Year | Tok::Month | Tok::Day => {
                if let Some((y, m, d)) = date(target.mtime) {
                    let _ = match tok {
                        Tok::Year => write!(out, "{y:04}"),
                        Tok::Month => write!(out, "{m:02}"),
                        _ => write!(out, "{d:02}"),
                    };
                }
            }
            Tok::Upper => case = Case::Upper,
            Tok::Lower => case = Case::Lower,
            Tok::Plain => case = Case::Plain,
        }
    }
    out
}

/// План по шаблону или regex; `existing` — есть ли имя в каталоге (по листингу).
pub fn plan(
    targets: &[Target],
    spec: &RenameSpec,
    existing: &dyn Fn(&str) -> bool,
    date: &dyn Fn(i64) -> Option<(u16, u8, u8)>,
) -> Result<RenamePlan, PatternError> {
    let mut news: Vec<CompactString> = Vec::with_capacity(targets.len());
    if spec.regex {
        let (find, repl) = spec
            .pattern
            .rsplit_once('/')
            .ok_or_else(|| bad(&urfin_i18n::current().get("ops-mr-regex-form")))?;
        let re = regex::Regex::new(find).map_err(|e| bad(&e.to_string()))?;
        for t in targets {
            news.push(re.replace_all(&t.name, repl).as_ref().into());
        }
    } else {
        let toks = parse(&spec.pattern)?;
        for (i, t) in targets.iter().enumerate() {
            news.push(apply(&toks, t, i, date).into());
        }
    }
    let from_keys: std::collections::HashSet<CompactString> =
        targets.iter().map(|t| name_key(&t.name)).collect();
    let mut counts: std::collections::HashMap<CompactString, usize> =
        std::collections::HashMap::with_capacity(news.len());
    for n in &news {
        *counts.entry(name_key(n)).or_default() += 1;
    }
    let mut out = RenamePlan::default();
    for (t, to) in targets.iter().zip(news) {
        if to == t.name {
            continue;
        }
        let key = name_key(&to);
        let problem = if check_name(&to).is_err() {
            Some(Problem::Invalid)
        } else if counts.get(&key).copied().unwrap_or(0) > 1 {
            Some(Problem::Duplicate)
        } else if key != name_key(&t.name) && !from_keys.contains(&key) && existing(&to) {
            Some(Problem::Exists)
        } else {
            None
        };
        out.problems += usize::from(problem.is_some());
        out.rows.push(Row {
            from: t.name.clone(),
            to,
            problem,
        });
    }
    Ok(out)
}

// --- применение ---

/// Job: строки плана без проблем.
#[derive(Debug)]
pub(crate) struct MultiRename {
    base: Location,
    rows: Vec<(CompactString, CompactString)>,
}

/// Из handler (UI-поток): job применения плана.
pub(crate) fn start(eng: &JobEngine, cx: &mut Cx<'_>, base: Location, plan: &RenamePlan) -> JobId {
    let rows: Vec<_> = plan
        .rows
        .iter()
        .filter(|r| r.problem.is_none())
        .map(|r| (r.from.clone(), r.to.clone()))
        .collect();
    let spec = JobSpec {
        kind: "rename",
        title: urfin_i18n::current().get("ops-job-multi-rename"),
        lane: Lane::Bulk,
        priority: 1,
        owner_ns: "ops",
    };
    eng.start(cx, spec, 1, MultiRename { base, rows })
}

fn failed(path: &Path, e: &std::io::Error) -> FailedItem {
    FailedItem {
        path: path.to_path_buf(),
        op: ItemOp::Rename,
        error: e.to_string().into(),
        class: IoClass::Other,
        pair: None,
    }
}

impl MultiRename {
    fn dir(&self) -> PathBuf {
        crate::names::os_path(&self.base.path)
    }

    fn one(from: &Path, to: &Path, case_only: bool) -> std::io::Result<()> {
        crate::sys::rename(from, to, case_only)
    }
}

impl OpExec for MultiRename {
    type Item = ();
    type Dir = ();

    fn plan(&self, p: &mut Plan<'_, Self>) -> Result<(), PlanError> {
        let dir = self.dir();
        let keys: std::collections::HashSet<CompactString> =
            self.rows.iter().map(|(f, _)| name_key(f)).collect();
        // Цепочка или цикл: новое имя — чьё-то старое (не своё) → две фазы.
        let chain = self
            .rows
            .iter()
            .any(|(f, t)| name_key(t) != name_key(f) && keys.contains(&name_key(t)));
        {
            let cx = p.cx();
            let pr = cx.progress();
            pr.items_total
                .store(self.rows.len() as u64, std::sync::atomic::Ordering::Relaxed);
            pr.total_final
                .store(true, std::sync::atomic::Ordering::Relaxed);
            pr.set_phase(urfin_core::Phase::Transfer);
        }
        let tag = format!("{:x}", std::process::id());
        let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
        for (i, (f, t)) in self.rows.iter().enumerate() {
            p.cx().checkpoint()?;
            let (from, to) = (dir.join(f.as_str()), dir.join(t.as_str()));
            let case_only = name_key(f) == name_key(t);
            if !chain || case_only {
                let r = Self::one(&from, &to, case_only);
                p.record(
                    None,
                    r.map_or_else(
                        |e| ItemResult::Failed(failed(&from, &e)),
                        |()| ItemResult::Done,
                    ),
                );
                continue;
            }
            let tmp = dir.join(format!(".fnv~mr{tag}-{i}"));
            match Self::one(&from, &tmp, false) {
                Ok(()) => staged.push((from, tmp, to)),
                Err(e) => p.record(None, ItemResult::Failed(failed(&from, &e))),
            }
        }
        for (from, tmp, to) in staged {
            match Self::one(&tmp, &to, false) {
                Ok(()) => p.record(None, ItemResult::Done),
                Err(e) => {
                    // Вернуть старое имя: файл не теряется под временным.
                    let _ = Self::one(&tmp, &from, false);
                    p.record(None, ItemResult::Failed(failed(&from, &e)));
                }
            }
        }
        p.cx().touch(&self.base);
        Ok(())
    }

    fn exec(&self, _work: &Work<Self>, _cx: &ExecCx<'_>) -> ItemResult {
        ItemResult::Done
    }

    fn finalize_dir(&self, _dir: &DirNode<()>, _cx: &ExecCx<'_>) -> ItemResult {
        ItemResult::Done
    }

    fn finished(&self, r: &Report, outcome: &Result<JobSummary, String>) -> Option<Note> {
        let s = outcome.as_ref().ok()?;
        let f = r.failed.first()?;
        let mut a = urfin_i18n::FluentArgs::new();
        a.set("n", s.errors);
        a.set("first", format!("{}: {}", f.path.display(), f.error));
        Some((
            Level::Error,
            urfin_i18n::current().fmt("ops-copy-errors", &a),
        ))
    }
}

#[cfg(test)]
#[path = "multi_rename_tests.rs"]
mod tests;

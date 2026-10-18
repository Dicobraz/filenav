//! Инкрементальный патч (S4 §5.4, S4-15): изменения debounce → `Patch` для core
//! без перечитывания каталога. Core применяет `update` → `remove` (по убыванию,
//! `swap_remove`) → `append` и берёт готовый `order`; здесь та же
//! последовательность моделируется, чтобы перенумеровать индексы и вставить
//! новые и изменённые записи в порядок бинарным поиском: O(n + k log n).

use std::cmp::Ordering;
use std::sync::Arc;

use compact_str::CompactString;
use rustc_hash::{FxHashMap, FxHashSet};
use urfin_core::{ArrangeSpec, Entry, Gen, Patch, SortKey};

use crate::sort::{ArrangeOpts, Names, compare, record, visible};

/// Имя → индекс записи. Кэш на `(generation, len)` (ADR-0010); каждое
/// попадание проверяется по имени записи, так что устаревший индекс (патч
/// отброшен core) не даёт неверных индексов — только перестройку.
#[derive(Debug, Default)]
pub struct DirIndex {
    pub generation: Gen,
    pub len: u32,
    map: FxHashMap<CompactString, u32>,
}

impl DirIndex {
    pub fn build(entries: &[Entry], generation: Gen) -> DirIndex {
        let mut map = FxHashMap::default();
        map.reserve(entries.len());
        for (i, e) in (0u32..).zip(entries) {
            map.insert(e.name.clone(), i);
        }
        DirIndex {
            generation,
            len: u32::try_from(entries.len()).unwrap_or(u32::MAX),
            map,
        }
    }

    /// Тот же листинг по поколению и длине (согласованность имён проверяет `get`).
    pub fn fits(&self, entries: &[Entry], generation: Gen) -> bool {
        self.generation == generation && self.len as usize == entries.len()
    }

    /// Индекс `name` в `entries`; `Err` — индекс не согласован с записями.
    fn get(&self, entries: &[Entry], name: &str) -> Result<Option<u32>, Stale> {
        match self.map.get(name) {
            Some(&i) if entries.get(i as usize).is_some_and(|e| e.name == name) => Ok(Some(i)),
            Some(_) => Err(Stale),
            None => Ok(None),
        }
    }
}

#[derive(Debug)]
struct Stale;

/// Изменения каталога с разрешёнными именами.
#[derive(Debug, Default)]
pub struct Changes {
    /// Новые и изменённые (`Entry` от backend'а или `stat_entry`).
    pub upsert: Vec<Entry>,
    pub removed: Vec<CompactString>,
}

/// Патч к `entries`/`order` или `None`, если менять нечего. Индекс обновляется
/// под результат (при отказе core он останется «впереди» — `get` это заметит).
pub fn build_patch(
    entries: &[Entry],
    order: &[u32],
    index: &mut DirIndex,
    changes: &Changes,
    spec: &ArrangeSpec,
    opts: ArrangeOpts,
    seq: u32,
) -> Option<Patch> {
    let lookup = |index: &DirIndex, name: &str| index.get(entries, name);
    let resolve = |index: &DirIndex| -> Result<_, Stale> {
        let mut update = Vec::new();
        let mut append = Vec::new();
        let mut remove = Vec::new();
        for e in &changes.upsert {
            match lookup(index, &e.name)? {
                Some(i) => update.push((i, e.clone())),
                None => append.push(e.clone()),
            }
        }
        for n in &changes.removed {
            if let Some(i) = lookup(index, n)? {
                remove.push(i);
            }
        }
        Ok((update, append, remove))
    };
    let (update, append, mut remove) = if let Ok(r) = resolve(index) {
        r
    } else {
        *index = DirIndex::build(entries, index.generation);
        resolve(index).ok()?
    };
    if update.is_empty() && append.is_empty() && remove.is_empty() {
        return None;
    }
    remove.sort_unstable_by(|a, b| b.cmp(a));
    remove.dedup();

    let n = u32::try_from(entries.len()).ok()?;
    let model = Model::new(n, &remove, &update, &append);
    let new_order = reorder(entries, order, &model, spec, opts);

    // Индекс — под записи после патча.
    for &old in &remove {
        index.map.remove(&entries[old as usize].name);
    }
    for (&old, &new) in &model.moved {
        index.map.insert(entries[old as usize].name.clone(), new);
    }
    for (k, e) in (model.kept..).zip(&append) {
        index.map.insert(e.name.clone(), k);
    }
    index.len = model.kept + u32::try_from(append.len()).ok()?;

    Some(Patch {
        base_len: n,
        seq,
        update,
        remove,
        append,
        order: Arc::new(new_order),
    })
}

/// Патч только из `update` по известным индексам (размеры каталогов, S4-17):
/// записи на местах, порядок — перестановкой изменённых.
pub fn update_patch(
    entries: &[Entry],
    order: &[u32],
    update: Vec<(u32, Entry)>,
    spec: &ArrangeSpec,
    opts: ArrangeOpts,
    seq: u32,
) -> Patch {
    let n = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    let new_order = {
        let model = Model::new(n, &[], &update, &[]);
        reorder(entries, order, &model, spec, opts)
    };
    Patch {
        base_len: n,
        seq,
        update,
        remove: Vec::new(),
        append: Vec::new(),
        order: Arc::new(new_order),
    }
}

/// Записи после патча без копирования: индекс → запись.
struct Model<'a> {
    /// Удалённые (старые индексы).
    removed: FxHashSet<u32>,
    /// Перенесённые `swap_remove`: старый → новый.
    moved: FxHashMap<u32, u32>,
    /// Позиции, занятые перенесёнными: новый → старый.
    slot: FxHashMap<u32, u32>,
    /// Обновлённые: старый индекс → запись.
    updated: FxHashMap<u32, &'a Entry>,
    append: &'a [Entry],
    /// Записей после удаления (первый индекс добавленных).
    kept: u32,
}

impl<'a> Model<'a> {
    fn new(n: u32, remove: &[u32], update: &'a [(u32, Entry)], append: &'a [Entry]) -> Model<'a> {
        let mut removed = FxHashSet::default();
        let mut slot: FxHashMap<u32, u32> = FxHashMap::default();
        let mut len = n;
        // Как core: по убыванию, `swap_remove`. Позиция `i` до своего удаления не
        // тронута (переносы идут только в большие, уже удалённые позиции).
        for &i in remove {
            let last = len - 1;
            removed.insert(i);
            let tail = slot.remove(&last).unwrap_or(last);
            if i != last {
                slot.insert(i, tail);
            }
            len -= 1;
        }
        let moved = slot.iter().map(|(&new, &old)| (old, new)).collect();
        Model {
            removed,
            moved,
            slot,
            updated: update.iter().map(|(i, e)| (*i, e)).collect(),
            append,
            kept: len,
        }
    }

    /// Новый индекс записи со старым индексом `old` (`None` — удалена).
    fn new_of(&self, old: u32) -> Option<u32> {
        if self.removed.contains(&old) {
            return None;
        }
        Some(self.moved.get(&old).copied().unwrap_or(old))
    }

    fn entry<'e>(&'e self, entries: &'e [Entry], new: u32) -> &'e Entry {
        if new >= self.kept {
            return &self.append[(new - self.kept) as usize];
        }
        let old = self.slot.get(&new).copied().unwrap_or(new);
        self.updated
            .get(&old)
            .copied()
            .unwrap_or(&entries[old as usize])
    }
}

/// Имена по новым индексам — для `compare`.
struct View<'a> {
    entries: &'a [Entry],
    model: &'a Model<'a>,
}

impl Names for View<'_> {
    fn name(&self, idx: u32) -> &str {
        &self.model.entry(self.entries, idx).name
    }
}

/// Новый порядок: старый без удалённых и переставляемых, с перенумерацией,
/// плюс вставка (бинарный поиск) изменённых, новых и — для `Unsorted`, где
/// порядок = индексы, — перенесённых.
fn reorder(
    entries: &[Entry],
    order: &[u32],
    model: &Model<'_>,
    spec: &ArrangeSpec,
    opts: ArrangeOpts,
) -> Vec<u32> {
    let unsorted = spec.sort.key == SortKey::Unsorted;
    let reinsert =
        |old: u32| model.updated.contains_key(&old) || (unsorted && model.moved.contains_key(&old));
    let mut base: Vec<u32> = Vec::with_capacity(order.len() + model.append.len());
    base.extend(order.iter().filter_map(|&old| {
        let new = model.new_of(old)?;
        (!reinsert(old)).then_some(new)
    }));
    let appended = u32::try_from(model.append.len()).unwrap_or(0);
    let mut ins: Vec<u32> = model
        .updated
        .keys()
        .filter_map(|&old| model.new_of(old))
        .collect();
    if unsorted {
        ins.extend(model.moved.values().copied());
    }
    ins.extend(model.kept..model.kept + appended);
    ins.retain(|&new| visible(model.entry(entries, new), spec, opts));
    ins.sort_unstable();
    ins.dedup();
    if ins.is_empty() {
        return base;
    }
    let view = View { entries, model };
    let cmp = |a: u32, b: u32| -> Ordering {
        if unsorted {
            return a.cmp(&b);
        }
        let ra = record(a, model.entry(entries, a), spec, opts);
        let rb = record(b, model.entry(entries, b), spec, opts);
        compare(&view, &ra, &rb, spec, opts)
    };
    ins.sort_unstable_by(|&a, &b| cmp(a, b));
    let mut out = Vec::with_capacity(base.len() + ins.len());
    let mut from = 0;
    for &x in &ins {
        let at = from + base[from..].partition_point(|&y| cmp(y, x) == Ordering::Less);
        out.extend_from_slice(&base[from..at]);
        out.push(x);
        from = at;
    }
    out.extend_from_slice(&base[from..]);
    out
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;

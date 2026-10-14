# S0 — изменения контрактов core v2 (ADR-0004)

Статус: принято S0, 2026-10-14. Источник: «Запросы к S0» из specs/S4.md и specs/S5.md. Имеет приоритет над specs/S1.md. Code реализует в указанных задачах S1.

| # | Изменение | Откуда | Задача S1 |
|---|---|---|---|
| 1 | `ListingUpdate::Chunk { entries: Vec<Entry>, visible: Vec<u32> }` — core дописывает `visible` в `order` через `Arc::make_mut` | S4-1 | S1-08 |
| 2 | `ListingUpdate::Patch(Patch { base_len, seq, update: Vec<(u32,Entry)>, remove: Vec<u32> /*desc, swap_remove*/, append: Vec<Entry>, order: Arc<Vec<u32>> })`; применяется при совпадении gen/len/seq, правит selection bits/bytes, stats, удерживает курсор, применяет pending_focus; иначе отбрасывается | S4-2 | S1-08 |
| 3 | `seq: u32` в `LoadReq`, `ArrangeReq`, `Order { order, for_len, seq }`; `Listing.arrange_seq` растёт на sort/filter/hidden.toggle; устаревшие отбрасываются | S4-3 | S1-08 |
| 4 | `ListingService::tab_event(&self, TabEvent)`; `TabEvent { Shown(TabId), Hidden(TabId), Closed(TabId), Left { tab, loc, entries: Arc<Vec<Entry>> } }` | S4-4 | S1-08 |
| 5 | `Attrs::ESCAPED_NAME = 1<<10`, `Attrs::SIZE_KNOWN = 1<<11`; пути для ОС — только через `vfs::name::{to_os, join_os}` | S4-5 | S1-03 |
| 6 | Нормализация `Location::local`: абсолютный; `\\?\C:\`→`C:\`, `\\?\UNC\s\sh`→`\\s\sh`; буква диска upper; на Windows `/`→`\`; `.`/`..` лексически; без хвостового разделителя кроме корня; регистр имён и symlink не трогать | S4-6 | S1-03 |
| 7 | `Lane::Bulk` — только короткие задачи. Долгие воркеры (ops, watcher, PTY) — `spawn_dedicated`; ops ограничивает свои потоки `ops.max_threads` | S5-1 | S1-05 (doc) |
| 8 | `JobSpec { kind, title, lane, priority: u8, owner_ns: &'static str }`; `JobHandle::set_state(Queued\|Running)`; `job.retry {id}`, `job.priority {id,p}` роутятся в handler `owner_ns`; `Waiting(PromptId)` = «есть открытый вопрос», прогресс может идти, переходы Waiting↔Running повторяемы | S5-2 | S1-09 |
| 9 | `Progress`: `phase: AtomicU8` (`Phase { Scan, Transfer, Verify, Finalize, Recover }`), `total_final: AtomicBool`, `parked: AtomicU32`; те же поля в `ProgressSnap` | S5-3 | S1-09 |
| 10 | `AppState.ext: Extensions` — typemap `get::<T>() / get_mut::<T>() / insert`; мутации только в Handler/Msg; ключи — типы своих крейтов | S5-4 | S1-06 |
| 11 | `Value::List(Arc<[Value]>)`; `Args::list(k)`, `Args::locs(k)`; текстовая форма `src=["a","b"]` | S5-5 | S1-04 |
| 12 | `VolumeInfo` (том, носитель SSD/HDD/NVMe/USB/Net, ФС, capabilities: reflink, case_sensitive, trash, free) — модуль `filenav_vfs::volume`; API — S4, sys-реализация — задача S5-02 | S5-6 | — (S4/S5) |
| 13 | `Handler::on_quit(&mut self, cx) -> QuitDecision { Allow, Veto(PromptSpec) }` (default Allow); `app.quit` опрашивает все handlers до `shutdown` | S5-7 | S1-07 |
| 14 | Core дедуплицирует `panel.refresh` по tab в пределах pump. Watcher: debounce 50 мс / max 250 мс (S4). Ops шлёт refresh ≤1/с на таб | S5-8 | S1-07 |
| 15 | `Banner.actions: SmallVec<[(CompactString, Command); 3]>` вместо `action` | S5-9 | S1-06 |
| 16 | `PromptSpec.detail: Option<PromptDetail>`; `enum PromptDetail { Conflict { src: Entry, dst: Entry, index: u64, total: u64 } }`, расширение через S0 | S5-10 | S1-09 |
| 17 | Датасеты B1–B6 (S5-11) — вход для S9, `xtask gen-dataset`, офлайн, фиксированные сиды | S5-11 | — (S9) |

Конфликтов между S1/S4/S5 после этих правок нет.

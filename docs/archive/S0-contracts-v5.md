# S0 — изменения контрактов v5 (ADR-0008)

Статус: принято S0, 2026-10-14. Источник: «Запросы к S0» specs/S7.md, S8.md, S9.md и вопросы пакета Code. Дополняет v2–v4.

## Core / app
| # | Изменение | Откуда |
|---|---|---|
| 1 | `OverlayKind::Find` (поиск/замена/goto во вьювере и редакторе, владелец S7, IME вкл) | S7-1 |
| 2 | `Focus::Editor`: IME вкл; keymap поглощает только `editor` + `global`, остальное — виджету. `Focus::Viewer`: IME выкл | S7-2 |
| 3 | Слои окна: терминал → панели → cmdline → вьювер/редактор (S7) → оверлеи. Quick view рисует `ui::view::quick`; `PreviewProvider` из S2-18 снимается | S7-3, уточняет v3 №11 |
| 4 | **Ввод редактора — `ViewInbox`** (очередь UI→handler, разбирается в начале pump). Набор текста не идёт через Command bus; `Value::Text` не вводим | S7-4 |
| 5 | `unsafe` дополнительно разрешён в `urfin-view::sys` (mmap, ReplaceFileW) и `urfin-app::sys` (хук `KeyNotes`, меню macOS). Итого sys-модули: vfs, ops, term, ui, view, app | S7-5, вопрос пакета |
| 6 | `MsgSink::poke()` и `Runtime::waker() -> Arc<dyn Waker>` — через общий `WakeFlag` (сигнатуры из TASKS-M0 утверждены); `Args::side(&self, k) -> Option<SideRef>` (`Left\|Right\|Active\|Passive`) — публичный API core | вопрос пакета, v3 №5/№7 |
| 7 | `PromptSpec.secret: bool` — ввод маскируется, значение никогда не логируется, не попадает в историю/сессию | S8-5 |
| 8 | `Location`: глубина `outer` ≤ 8; `>>` в компонентах экранируется в `to_uri`; схемы `search:` и `cmp:` в сессии не сохраняются (таб восстанавливается к последнему реальному каталогу) | S8-6 |
| 9 | Резолв открытия: `trait OpenResolver` + `Services.open_resolvers: Vec<Arc<dyn OpenResolver>>`; порядок S8 → пользовательские ассоциации → `term.open`. Уточняет v3 №9 | S8-9 |
| 10 | App: режимы `--bench-startup`, `--bench <scenario.toml>`; **стабильные имена tracing-спанов — контракт**: `startup.*`, `frame`, `list.*`, `ops.job`, `term.build`, `view.*` | S9-2 |
| 11 | Features `profile`, `dhat-heap`, `count-alloc`; CI запрещает их в release-сборке | S9-3 |
| 12 | Build-зависимость `winresource`; манифест exe: `longPathAware`, `PerMonitorV2`, `activeCodePage=UTF-8`. S2 сверяет DPI-логику | S9-4 |

## VFS / ops / новые крейты
| # | Изменение | Откуда |
|---|---|---|
| 13 | Новые крейты: `urfin-search`, `urfin-arc` (S8), `urfin-testdata` (dev, S9; `gen_tree` из S4-11 переезжает сюда). Фаза 3: `urfin-net` | S8-1, S9-1 |
| 14 | Общий обходчик `urfin_vfs::walk` (work-stealing поверх `Vfs::list`) — владелец S4; используют S4 `dir_size`, S5 (не-локальные источники), S8 | S8-2 |
| 15 | Trait `Vfs` расширяется: `resolve(base, item)`, `extract_plan` + `ExtractStream`, `Caps::{READ_ONLY, SEQUENTIAL, VIRTUAL}`; фаза 3 — `writer() -> VfsWriteTxn`. Владелец trait — S4 | S8-3 |
| 16 | S5: `Engine::Vfs` для не-локального источника (dest в M5a — только local); `ops.move` из `READ_ONLY` → черновик с предложением копирования | S8-4 |
| 17 | `viewer.open { pos?, line? }`; файлы из архивов — через `open_read` или спилл во временный файл | S8-8 |

## UI / keymap
| # | Изменение | Откуда |
|---|---|---|
| 18 | S2: inline-формы `search.draft`, `arc.draft`; для VIRTUAL-панелей имена `dir\name`; колонка статуса сравнения; индикаторы `in_search`, `in_archive` | S8-7 |
| 19 | Команды утверждены: `viewer.follow` (Shift+F), `viewer.copy`, `viewer.select_all`, `viewer.dock`, `viewer.quick`, `viewer.search_cancel`, `editor.eol`, `editor.reload`, `editor.input`; `search.*`, `arc.*`, `ops.compare { recursive? }`. Привязки: Alt+F7, Ctrl+Shift+PgDn (войти в файл как в архив), Shift+F1/F2, Ctrl+PgUp в результатах поиска — к каталогу файла | S7-8, S8-10 |

## Зависимости и политика
- Одобрено: memmap2 0.9, encoding_rs 0.8, chardetng 1.0, memchr 2.8, regex-automata 0.4, regex-syntax 0.8, ropey 1.6, regex-cursor 0.1.5, unicode-segmentation 1.12, zip 9, sevenz-rust2 0.24, grep-searcher/grep-regex, ignore (только gitignore), tracing-tracy, winresource. deny.toml: allow BSD-3-Clause. `unrar` — под feature, исключение `LicenseRef-UnRAR` — после решения владельца №37.
- Датасеты S7 V1–V6 добавляются в `urfin-testdata` (S9).
- **Пороги регрессий (единые, отменяют расхождения S2/S5):** CI — 3% по инструкциям (gungraun) и аллокациям, 20% A/B по времени на одном раннере; стенд — 10% при CV ≤ 5%. Правило `perf-evidence`: PR в горячие пути vfs/ops/term/ui-render требует JSON прогона стенда в `docs/perf/runs/`.
- Бюджет «бинарь < 15 МБ» — только `urfin.exe`; DLL ConPTY/OpenConsole не входят.
- Заголовки PR — Conventional Commits (`feat(core): …`).
- До RC закрыть решения №5 (a11y) и №15 ($Recycle.Bin). Идентификаторы (издатель, winget ID, bundle id, UpgradeCode) — до S9-17, см. решение владельца.

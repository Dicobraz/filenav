# S0 — изменения контрактов v6

Статус: принято S0, 2026-10-23 (владелец: «вноси, делай»). Источник: «Запросы к S0» `specs/F01.md` §6, `specs/F02.md` §6. Дополняет v2–v5. Действует для пакета `docs/TASKS-F.md` (после M3).

## Общее
| # | Изменение | Откуда |
|---|---|---|
| 1 | **Колонки, задаваемые схемой** для `Caps::VIRTUAL`: `Vfs::columns() -> &[ColumnSpec]` (`id`, заголовок через i18n, ширина, выравнивание, вид: `Text`/`Size`/`Date`/`Bar`), данные строки — `Entry.ext` (провайдер ячеек схемы). Один механизм для `du:` и `pkg:`; колонка `share` (полоса + %) — вид `Bar`, значение `size / parent_total` (родитель — через `Done{ total }`) | F01 №5, F02 №4 |
| 2 | **Вид пассивной панели от провайдера** активной вкладки — по образцу quick view (v5 №3): `ui::du::treemap` для `du:`; фокус в неё — Ctrl+Tab в контексте `in_du` | F01 №6 |
| 3 | **Ext-состояние** вкладки для виртуальных схем (фильтр, сводка, баннеры) — механизм S5 запрос №4; используют `DuState`, `PkgState` | F01 №9 |
| 4 | Нумерация S0-decisions исправлена: решения F-01/F-02 — №81–84, политики — №86 | F02 №8 |

## VFS
| # | Изменение | Откуда |
|---|---|---|
| 5 | Схема **`du:`** — `Location { backend: "du", path: "/<snap_id>/<rel>" }`, query `f=` (фильтр); `caps = VIRTUAL | READ_ONLY_DIR`; в сессии не сохраняется (как `search:`/`cmp:`, v5 №8) | F01 №3 |
| 6 | Схема **`pkg:`** — `Location { backend: "pkg", path: "/installed" \| "/updates" \| "/search?q=…&m=…" }`; `caps = VIRTUAL`; в сессии не сохраняется | F02 №5 |
| 7 | **`Attrs::SIZE_PARTIAL`** — размер каталога ещё досчитывается («≥ X»), рядом с `SIZE_KNOWN` | F01 №4 |
| 8 | **`walk_with(&dyn DirLister)`** — расширение публичного `urfin_vfs::walk` (v5 №14); `walk` остаётся | F01 №8 |
| 9 | **Формат кэша `%LOCALAPPDATA%\Urfin\state\du\*.du` v1** — фиксирован как формат на диске; смена — только с повышением `ver`, старые файлы удаляются | F01 №7 |

## Команды и клавиатура
| # | Изменение | Откуда |
|---|---|---|
| 10 | Namespace **`du`**: `du.open`, `du.refresh`, `du.stop`, `du.filter`, `du.map`, `du.verify_hash`, `du.old_age`; **`du.elevate`** — регистрируется, но выключен до принятия ADR-0021. Handler — `urfin_vfs::du` | F01 №1 |
| 11 | Keymap-флаг **`in_du`** (панель на `du:`), по образцу `in_search` | F01 №2 |
| 12 | Namespace **`pkg`** (`pkg.open`, `pkg.refresh`, `pkg.search`, `pkg.install`, `pkg.upgrade`, `pkg.upgrade_all`, `pkg.uninstall`, `pkg.output`) и контекст keymap **`pkg`**; привязки из №84 действуют только в контекстах `in_du` / `pkg` и не конфликтуют с пресетами (проверка `commands_doc`) | F02 №5 |
| 13 | `ui.utilities` — Ctrl+F9 (№83), меню F9 → «Утилиты» | F01 №1 |

## Крейты, зависимости, процессы
| # | Изменение | Откуда |
|---|---|---|
| 14 | Новый крейт **`urfin-pkg`**, без `unsafe` | F02 №1 |
| 15 | **`urfin-term` — публичные безопасные API** (`unsafe` остаётся в `term::sys`): `term::proc::spawn_piped(argv, cwd, env, flags) -> PipedChild` (job object, `kill_tree()`, `wait_timeout()`); `term::proc::session_has_process(name, since) -> bool`; ленточный сеанс `TermApi::feed_session(title) -> FeedSink` (без PTY, только чтение, Ctrl+O и список сеансов) | F02 №2 |
| 16 | Зависимость **`windows-registry`** (MIT/Apache-2.0, семейство windows-rs) — чтение ARP, `OEMCP` и политик `HKLM\Software\Policies\Aahz\Urfin` (ADR-0020) | F02 №3 |
| 17 | Флаг app **`--pkg-check`** — ранний выход из `boot.rs` до GPUI, без окна; IPC единственного экземпляра на него не распространяется. **`--pkg-elevated` — отклонено**: помощника для choco нет (F02 §8 — choco через UAC в своём окне, ход по журналу choco) | F02 №7 |
| 18 | S9: деинсталлятор удаляет задачу планировщика `\Urfin\PkgCheck`; установщик создаёт `%ProgramData%\Urfin` с ACL «запись — администраторы» (ADR-0020) | F02 №6, №86 |

## Наблюдаемость
| # | Изменение | Откуда |
|---|---|---|
| 19 | Спаны tracing (v5 №10): `du.scan`, `du.first_rows`, `du.cache_load`, `du.layout`; `pkg.detect`, `pkg.list`, `pkg.search`, `pkg.action` | F01 №10, F02 |

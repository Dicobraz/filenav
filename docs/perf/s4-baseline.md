# Перф-база листинга (S4-11)

Снято 2026-10-18 на стенде владельца, release, warm (повторные прогоны, кэш ОС тёплый). Цели — S4 §7; стенд и тулчейн — как в [baseline.md](baseline.md). JSON прогонов — `docs/perf/runs/s4-11-*.json` (правило `perf-evidence`).

## Данные

`gen_tree --n 100000 --seed 1` (крейт `urfin-testdata`): 99 462 записи в одном каталоге — 1% каталогов, 2% скрытых, 10% имён длиннее 24 байт, 3% Unicode, 70% пустых файлов. Ссылки (0,5%) пропущены: без режима разработчика Windows не даёт создать symlink. Навигация — каталог из 100 записей (`--nav`). Генерация 100k на NTFS — 21 с.

## Windows 11, NTFS, NVMe

| Метрика | 100k | Цель §7 | | 1M |
|---|---|---|---|---|
| `t_first`, stream (первый батч) | 0,97–1,11 мс | < 5 мс | ✓ | — |
| `t_first`, через core (первые строки в табе) | 1,15–1,45 мс | < 5 мс | ✓ | — |
| `t_done`, stream | 42–55 мс | < 60 мс | ✓ | — |
| `t_sorted` (natural, dirs_first), через core | 79–89 мс | < 90 мс | ✓, запас мал | — |
| arrange: Name / Ext / Size | 7,0 / 6,3 / 5,3 мс | < 10 мс | ✓ | 63 / 71 / 33 мс (< 90) ✓ |
| фильтр по маске `*.rs,*.toml` | 3,2 мс | < 4 мс | ✓ | 34,4 мс (< 35) ✓ |
| пик WS листинга (`--core-only`) | 17,5–18,0 МБ | < 15 МБ | ✗ [#70](https://github.com/Dicobraz/filenav/issues/70) | — |
| пик кучи листинга (считающий аллокатор) | 21,7 МБ | — | | — |
| навигация: каталог 100 записей, до `Complete` | 0,32 мс | < 1 мс до чанка | ✓ (без кэша; кэш — S4-16) | — |

1M не снимали: генерация положит миллион файлов в `%TEMP%` (≈ 1 ГБ MFT), а цель «peak RSS 1M < 110 МБ» упирается в бюджет процесса ≤ 200 МБ при базе ~162 МБ — вопрос владельцу (TASKS-M1, S4-11). Arrange и фильтр на 1M — из criterion на синтетических записях.

### Режимы чтения каталога (criterion `list.rs`, 100k пустых `file-NNNNNN.txt`)

| Режим | `t_done` |
|---|---|
| `nt` — `NtQueryDirectoryFile` (по умолчанию) | 34,6 мс |
| `find` — `FindFirstFileExW` (`FIND_FIRST_EX_LARGE_FETCH`) | 48,6 мс |
| `std` — `std::fs::read_dir` + метаданные | 48,6 мс |
| через `VfsListing` и core до `Complete` | 47,4 мс |

На сгенерированном дереве (длинные и Unicode-имена, скрытые) те же режимы: 47 / 53 / 61 мс.

### CLI-базлайны (тот же каталог 100k, тёплые прогоны)

| Команда | До конца вывода | Urfin `t_sorted` быстрее |
|---|---|---|
| `cmd /c dir /b /a > nul` | 400 мс | ×4,7 |
| `Get-ChildItem -Force` | 1737 мс | ×20 |

Сравнение с Explorer (ручной протокол с записью экрана) — S4-19, `docs/perf/s4-vs-explorer.md`.

## Профиль памяти

Сервис листинга (`vfs::service::stream`) держит полную копию чанков для геометрических пересортов и финального `Order`, а core — свои `entries`: две копии по 48 Б × 100k плюс переходные пики роста `Vec`. Эксперимент без копии в воркере: пик кучи 13,2 МБ, WS 10,1 МБ, `t_sorted` 47 мс (без сортировки). Исправление и решение по промежуточным пересортам — S4-19, [#70](https://github.com/Dicobraz/filenav/issues/70).

Ext-сортировка в этом PR ускорена с 12,7 / 170 мс до 6,3 / 71 мс (100k / 1M): вторичный ключ — префикс имени в `SortRec.aux`, флаг `EXT_EXACT` пропускает полное сравнение расширений, когда префикс расширения точный (ASCII, ≤ 7 байт, без цифр при natural).

## Другие ОС и ФС

- **ReFS / Dev Drive:** на стенде нет тома Dev Drive — строка пустая до появления тома.
- **SMB:** — (S4-19).
- **macOS APFS:** пассив (решение владельца); `vfs_bench` и `gen_tree` кроссплатформенные, `diag::peak_memory` на macOS даёт `None`.
- **Linux ext4/btrfs:** tier 2; `diag::peak_memory` — `VmHWM` из `/proc/self/status`, private — нет.

## Как повторить

```
cargo run -p urfin-testdata --example gen_tree --release -- --n 100000 --seed 1 --out %TEMP%\urfin-bench-gen-100k
cargo run -p urfin-vfs --example vfs_bench --release -- %TEMP%\urfin-bench-gen-100k --runs 7 --json run.json
cargo run -p urfin-vfs --example vfs_bench --release -- %TEMP%\urfin-bench-gen-100k --runs 7 --core-only
cargo bench -p urfin-vfs --bench list
cargo bench -p urfin-vfs --bench sort
```

`vfs_bench`: первый прогон — прогрев, в медиану не входит; `listing_peak_*` — пик процесса минус пик до первого листинга (рантайм и пустой `Core`). Пики Windows (`PeakWorkingSetSize`, `PeakPagefileUsage`) не сбрасываются, поэтому память листинга — отдельным запуском с `--core-only`.

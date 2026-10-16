# FileNav — архитектура (ведёт S0 «Джарвис»)

## Решения (ADR-0)
- Rust stable (edition 2024), egui/eframe, рендер **wgpu** (Windows — DX12, запасной Vulkan; Linux — Vulkan/GLES; macOS — Metal; ADR-0012, было glow); код UI не зависит от бэкенда.
- Платформы: **Windows — приоритет**, macOS — вторая. Linux — tier 2 (сборка в CI, без гарантий).
- Лицензия: **MIT OR Apache-2.0** (стандарт Rust-экосистемы).
- Ноль модальных окон. Все «диалоги» — inline-оверлеи в панели/cmdline. UI-поток не делает I/O.
- «Перекрыть системные» = свои алгоритмически быстрые реализации операций (каталог — S5). Системные хоткеи ОС не перехватываем.
- Хоткеи: пресеты DN и Far, пользовательский keymap.

## Крейты (`crates/*`, пакеты `filenav-*`)
| Крейт | Ответственность | Зависит |
|---|---|---|
| core | AppState (панели, табы, выделение, история), Command bus, события, job registry | — |
| vfs | trait Vfs, локальный бэкенд с platform fast paths, watcher, сортировка | core |
| ops | движок операций, очередь, прогресс, конфликты | vfs |
| term | PTY, VT-эмуляция, shells, история, автокомплит | core |
| keymap | реестр команд → сочетания, пресеты, конфиг | core |
| ui | виджеты egui | все выше |
| view | вьювер/hex/редактор | vfs |
| app | бинарь eframe | все |

## Потоки
- UI: egui, события из crossbeam-каналов, repaint только по событию (`request_repaint`).
- io-pool: N blocking-потоков (настраиваемо) — vfs/ops.
- cpu-pool: rayon — сортировка, хэши, поиск.
- term: поток-читатель на каждый PTY.
- Любое действие пользователя → `Command` → bus → handler в core. UI только рисует снапшот состояния.
- Отмена: токен в каждой задаче. Прогресс: атомики + троттлинг событий ≤30/с.

## Бюджеты
- Первый кадр <250 мс (своё время старта <20 мс, остальное — окно и устройство D3D12; S0-decisions 2026-10-16); каталог 100k записей <200 мс (первые строки <16 мс, стриминг).
- Кадр ≤2 мс при 100k строк (виртуализация), ввод ≤1 кадр.
- Простой: 0% CPU, своя память <50 МБ, процесс ≤200 МБ private (S0-decisions 2026-10-16). Бинарь <15 МБ.

## Платформенный слой
Модули `sys/{windows,macos,linux}.rs` внутри vfs/ops, fallback на std. `unsafe` — только в `sys`.

## Конвенции
- Ошибки: thiserror в либах, anyhow в app. Логи: tracing.
- Тесты: cargo-nextest; бенчи: criterion + сценарные (hyperfine).
- Clippy pedantic = warn, rustfmt по умолчанию.
- Ветки `sX/<задача>`, PR на задачу.
- Документы в репо: `docs/specs/Sx.md`, `docs/adr/NNNN-*.md`.

## Стенд
Ноут Windows (нативно — основное), WSL2 Ubuntu (Linux-сборка/тесты, репо в ~/), VMware Linux Desktop (GUI X11/Wayland, матрица ФС), Docker (headless CI). Для macOS нужен реальный Mac (Apple Silicon) — сборка, тесты, подпись/notarization. Репо и target на Dev Drive (ReFS) + исключения Defender.

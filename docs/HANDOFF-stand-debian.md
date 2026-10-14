# Handoff: стенд Debian 13 (ВМ) для Claude Code

Claude Code работает на ВМ Debian 13. Целевая платформа продукта — **Windows** (единственная активная), поэтому Windows-часть пишется здесь, а проверяется лестницей L0–L2 (ниже).

## 1. ВМ
| Ресурс | Значение |
|---|---|
| ОС | Debian 13 (trixie) amd64, рабочий стол GNOME (сессии Wayland и X11) |
| vCPU / RAM | 8 / 16 ГБ (лучше 24) |
| Диск | 150 ГБ системный; тест-ФС — loop-образы (`scripts/stand/fslab.sh`) |
| 3D | желательно включить 3D-ускорение VMware; без него работает llvmpipe (OpenGL 4.5, медленно — только функциональные проверки) |
| Сеть | нужна для crates.io, GitHub, первичной загрузки MSVC SDK (cargo-xwin) |

Подготовка: `bash scripts/stand/setup-debian.sh` (идемпотентно, нужен sudo). Затем `gh auth login`, установить Claude Code по официальной документации, клонировать репо в `~/src/filenav`.

## 2. Что проверяется где
| Уровень | Где | Что ловит | Кто |
|---|---|---|---|
| L0 | стенд: `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` | компиляция Windows-кода, cfg, windows-sys API | Code, перед каждым PR |
| L0 | стенд: Linux — fmt, clippy, nextest, `--smoke`, ручной запуск GUI (Wayland и X11) | вся кроссплатформенная логика | Code |
| L1 | GitHub Actions `windows-latest` (+ ubuntu, macos) | тесты на настоящей Windows, release-сборка, размер бинаря, `--smoke` | CI, автоматически |
| L2 | ноут владельца, артефакт CI | GUI, ConPTY, хоткеи/раскладки, Mica, перф-бюджеты | владелец по чек-листу в PR |

Правила:
- Windows-код (`sys/windows.rs`, ConPTY, DWM, `KeyNotes`, IFileOperation) пишется на стенде; PR не считается готовым, пока L0 (xwin) и L1 (CI windows) не зелёные.
- Задачи с критериями, проверяемыми только в живой Windows (GUI, ввод, ConPTY, перф), помечай в PR меткой `win-manual` и добавляй чек-лист для владельца: что запустить, что нажать, что должно получиться. Мерж — после «ОК» владельца.
- Перф-бюджеты (первый кадр, кадр ≤2 мс, CPU 0%, RAM) — **только Windows** (L2 / `xtask perf` на ноуте). Цифры со стенда — справочные, в `docs/perf/runs/` не коммитить как эталон.
- Тесты, требующие Windows, — `#[cfg(windows)]`; на стенде они не компилируются в тест-бинарь, но проходят L0 через xwin.
- Wine для проверки не используем (нет ConPTY, ненадёжный GL).

## 3. Команды
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --locked -- -D warnings
cargo run -p filenav-app -- --smoke
cargo run -p filenav-app            # GUI, проверить в Wayland и X11 (WAYLAND_DISPLAY= для X11)
scripts/stand/fslab.sh up           # тест-ФС: /mnt/fl-{ext4,btrfs,xfs,ntfs3}
```

## 4. Поток работы
1. Задача из `docs/TASKS-M0.md` (далее — следующие пакеты) → ветка `sX/NN-кратко`.
2. Реализация + тесты → L0 локально → push → `gh pr create` (заголовок — Conventional Commits).
3. Ждать L1. Красный CI windows → чинить, не мержить.
4. `win-manual` → чек-лист владельцу в PR, ждать ответа.
5. Мерж (squash). Следующая задача.

## 5. Ограничения стенда
- `cargo-xwin` при первом запуске скачивает MSVC CRT и Windows SDK: это означает принятие лицензии Microsoft — владелец подтверждает один раз перед первым запуском.
- macOS — только CI (в пассиве).
- Секреты (gh-токен) — только в `gh auth`, не в репо и не в логах.

# Handoff: стенд Windows 11 для Claude Code

Claude Code работает нативно на Windows 11 владельца. Это и целевая платформа продукта, и стенд: Windows-код собирается, проверяется и запускается здесь же, без ВМ и кросс-компиляции.

## 1. Окружение
| Компонент | Значение |
|---|---|
| ОС | Windows 11 Pro x64 |
| Rust | rustup, версия из `rust-toolchain.toml`, таргет `x86_64-pc-windows-msvc` |
| Линковщик | Visual Studio 2022 Build Tools, ворклоад «C++ build tools» + Windows SDK |
| Инструменты | cargo-nextest, cargo-deny (готовые бинари с GitHub Releases), gh, Python 3 |
| Доп. таргеты | `x86_64-unknown-linux-gnu` (только std, для `cargo clippy --target`) |

Подготовка: `powershell -ExecutionPolicy Bypass -File scripts/stand/setup-windows.ps1` (идемпотентно; Build Tools ставятся с принятием лицензии Microsoft и могут запросить UAC и перезагрузку). Затем `gh auth login`.

`~/.cargo/bin` добавляется в PATH пользователя. В уже открытых шеллах его может не быть: тогда `export PATH="$HOME/.cargo/bin:$PATH"` (Git Bash) или перезапуск терминала.

## 2. Что проверяется где
| Уровень | Где | Что ловит | Кто |
|---|---|---|---|
| L0 | стенд: fmt, clippy, nextest, release-сборка, `--smoke` | вся логика, Windows-код (`sys/windows.rs`, windows-sys API), размер бинаря | Code, перед каждым PR |
| L0 | стенд: `cargo clippy --lib --bins --target x86_64-unknown-linux-gnu` | `cfg`-ошибки unix-кода, пока зависимости собираются без C-тулчейна цели (dev-зависимости вроде criterion его требуют, поэтому без тестов и бенчей) | Code, если задача трогает не-Windows код |
| L1 | GitHub Actions: windows-latest, ubuntu-latest (блокирующие), macos-latest (неблокирующий) | тесты, Linux/macOS-сборка, deny, размер бинаря | CI, автоматически |
| L2 | стенд, владелец | GUI, ConPTY, хоткеи и раскладки, Mica, перф-бюджеты | владелец по чек-листу в PR |

Правила:
- PR не считается готовым, пока L0 и L1 не зелёные.
- Задачи с критериями, которые проверяются только руками в живом GUI (ввод, ConPTY, отрисовка), помечай меткой `win-manual` и добавляй чек-лист для владельца: что запустить, что нажать, что должно получиться. Если критерий можно проверить автоматически на стенде (запуск exe, замер, скриншот), проверь сам и приложи результат.
- Перф-бюджеты (первый кадр, кадр ≤2 мс, CPU 0%, RAM) меряются на этой машине в release. Эталонные прогоны (`docs/perf/runs/`) — только с `xtask perf` по правилам S9.
- Linux — tier 2: проверяется CI и, при необходимости, WSL2 (`scripts/stand/fslab.sh` — матрица ФС на loop-образах). macOS — в пассиве: время на него не тратим, падения macOS-джоба не чиним, а записываем в `docs/macos-notes.md`.
- Тесты, требующие Windows, — `#[cfg(windows)]`.

## 3. Команды
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked
cargo test --doc --workspace --locked
cargo build --release -p filenav-app --locked
cargo clippy --workspace --lib --bins --locked --target x86_64-unknown-linux-gnu -- -D warnings
cargo deny check
cargo run -p filenav-app
```

## 4. Поток работы
1. Задача из `docs/TASKS-M0.md` (далее — следующие пакеты) → ветка `sX/NN-кратко` от актуального `main`.
2. Реализация + тесты → L0 → push → `gh pr create` (заголовок — Conventional Commits).
3. Ждать L1. Красный CI → чинить, не мержить.
4. `win-manual` → чек-лист владельцу в PR, ждать ответа.
5. Мерж (squash) — делает владелец. Следующая задача.

## 5. Ограничения
- Мерж PR и любые действия с репозиторием, кроме push в свои ветки и создания PR, — с согласия владельца.
- Секреты (gh-токен) — только в `gh auth`, не в репо и не в логах.

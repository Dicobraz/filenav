# Urfin — инструкции для Claude Code

Быстрый двухпанельный файловый менеджер (функции и раскладка DN/Far, визуал Windows Fluent). Rust stable, edition 2024, egui/eframe 0.36 (wgpu: DX12, запасной Vulkan — ADR-0012). Windows — единственная активная платформа; macOS — в пассиве (код кроссплатформенный, CI собирает, mac-специфичное — в конце очереди); Linux tier 2.

## Где правда (по убыванию приоритета)
1. docs/specs/S0-decisions.md — решения владельца
2. docs/specs/S0-contracts-v5.md → v4 → v3 → v2 — изменения контрактов
3. docs/specs/Sx.md — спецификации сессий
4. docs/00-ARCHITECTURE.md, docs/01-ROADMAP.md
Задачи: docs/TASKS-M1.md (текущий пакет; M0 — docs/TASKS-M0.md, закрыт). Читай только спеки, нужные задаче.

## Правила
- Одна задача = ветка `sX/NN-кратко` = один PR. Выполняй критерии приёмки задачи дословно; тесты — cargo nextest; `cargo clippy --workspace --all-targets -- -D warnings` и `cargo fmt --check` зелёные перед PR.
- Межкрейтовые контракты (публичные типы core, Command id, Focus, ListingUpdate и т.п.) не менять самостоятельно. Если нужно — остановись, опиши предложение в docs/adr/NNNN-*.md и спроси владельца.
- `unsafe` только в модулях `sys` (vfs, ops, term, ui, view, app). Ошибки: thiserror в либах, anyhow в app. Логи — tracing.
- UI-поток не делает I/O и не блокируется. Тяжёлое — через Runtime (полосы Interactive/Bulk, rayon, spawn_dedicated).
- Офлайн: никаких сетевых запросов в рантайме, телеметрии, CDN. Ресурсы вшиты (кроме системных шрифтов Segoe — их читаем с диска ОС).
- Строки UI — только через Fluent (крейт urfin-i18n), EN+RU.
- Реестр команд (функций Urfin) — docs/COMMANDS.md, генерируется тестом `commands_doc_is_current`; новая или изменённая команда → `URFIN_UPDATE_DOCS=1 cargo nextest run -p urfin-ui commands_doc` в том же PR.
- Реестр ручных проверок — docs/MANUAL-TESTS.md: PR с `win-manual` добавляет раздел и строку в «Статус разделов»; итоги прогонов владельца — в «Результат».
- Хоткеи с буквой работают на любой раскладке/языке (S0-contracts-v4 №5).
- Целевые бюджеты: первый кадр <250 мс (своё время старта <20 мс), кадр ≤2 мс при 100k строк, 0% CPU в простое, своя память <50 МБ (процесс ≤200 МБ private), бинарь <15 МБ — S0-decisions 2026-10-16.

## Стенд
Ты работаешь нативно на **Windows 11** владельца — прочитай docs/HANDOFF-stand-windows.md до первой задачи. Проверка лестницей: L0 на стенде (fmt, clippy, nextest, release-сборка; для не-Windows кода — `cargo clippy --lib --bins --target x86_64-unknown-linux-gnu`) → L1 CI (windows/ubuntu; macos — неблокирующий, проблемы в docs/macos-notes.md) → L2 ручная приёмка владельцем GUI (метка `win-manual` + чек-лист в PR). Перф-бюджеты меряются на этой машине в release. Мерж PR — владелец.

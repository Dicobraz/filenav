# S0 — изменения контрактов v3 (ADR-0006)

Статус: принято S0, 2026-10-14. Источник: «Запросы к S0» из specs/S2.md, S3.md, S6.md. Дополняет v2, имеет приоритет над specs/S1–S6.

## Core (реализуется в задачах S1)
| # | Изменение | Откуда | Задача |
|---|---|---|---|
| 1 | `Focus { Panel, Cmdline, Prompt(PromptId), Viewer, Editor, Terminal, Overlay(OverlayKind) }`, `OverlayKind { Menu, QuickSearch, InlineEdit, Draft, Palette }` | S2-1, S3-1 | S1-06 |
| 2 | **Ввод — единая точка `App::raw_input_hook` (S3).** Keymap резолвит по контексту текущего Focus; поглощённые события удаляются из RawInput; непоглощённые идут в egui → оверлеи S2 читают их в `Ui::show`. В `Overlay`/`Cmdline`/`Terminal` keymap поглощает только биндинги контекстов `overlay`/`cmdline`/`terminal` + `global` | S2-1 vs S3 — конфликт снят | S1-13, S3 |
| 3 | `panel.viewport { side, rows, cols, top }`; `top` — позиция прокрутки, хранится в Cursor, меняет `rev`; `panel.cursor.left/right` | S2-2, S3-2 | S1-08 |
| 4 | Команды: `panel.cursor.match {mask, dir}`, `panel.root`, `panel.mode {files\|info\|qview\|tree}` (поле `Tab.mode`, в сессии), `panel.bookmark.set/go {n}` (в сессии), `sel.move {motion}`, `sel.invert {dirs?}`, `sel.same {by, on}`, маска опциональна в `sel.add/remove` | S3-2 | S1-08 |
| 5 | Аргумент `side` принимает `left\|right\|active\|passive` | S3-3 | S1-04 |
| 6 | `ViewMode::Custom(u8)` — наборы колонок, M2+ | S2-4 | S1-06 (enum), S2 |
| 7 | `MsgSink::poke()` и `Runtime::waker()` — разбудить UI без Msg (терминал читает общее состояние напрямую) | S6-1 | S1-05 |
| 8 | Новый листовой крейт **`filenav-i18n`** (Fluent, вшитые en/ru, fallback en). Зависят: core, vfs, ops, ui, term. Тосты/промпты из любых крейтов — только через ключи | S2-3 | S1-01 (заглушка), S2 |
| 9 | Цепочка Enter по файлу: `panel.enter` → `file.open`; резолв: S8 (архив как VFS) → пользовательские ассоциации `[file.assoc]` → `term.open` (ассоциации ОС). Аргументы `file.open { alt?, internal?, elevated? }` | S6-3, S3-8/9 | S1-08, S5, S6 |
| 10 | App: `raw_input_hook` отдан keymap; `event_loop_builder` — хук сообщений Windows (`KeyNotes`, numpad/Ctrl+Ins/Shift+Del) и отключение стандартного меню macOS; версия winit — строго та, что тянет eframe 0.36 (без дубля) | S3-6 | S1-13 |

## UI / слои (S2, S6)
| # | Изменение | Откуда |
|---|---|---|
| 11 | Слои окна снизу вверх: терминал → панели → cmdline (1–3 строки) → оверлеи. Видимость панелей не меняет размер PTY. Модуль `filenav-ui::term` — владелец S6 | S6-2 |
| 12 | **Один вшитый моноширинный шрифт для панелей и терминала** — выбор S2 (JetBrains Mono / Terminus, OFL). Cascadia Mono не берём. Параметры `ui.font_mono`, `ui.font_mono_size`; терминал рисует рамки/Powerline процедурно | S6-4 vs S2 — конфликт снят |
| 13 | Ввод при фокусе на панели: печатный текст → `cmdline.insert`; IME включён только в Cmdline/Terminal; в панелях нет фокусируемых egui-виджетов; `zoom_with_keyboard = false`; `Focus::Terminal` → не поглощённое keymap уходит в `term.key/term.input`; имена клавиш — `egui::Key::name()` | S3-4/5, S6-5 |
| 14 | Подписи keybar: Fluent `keybar-<id>`, fallback `cmd-<id>`. Поле в `CommandMeta` не добавляем | S3-10 |

## Межсессионные (S2/S3 ↔ S5)
| # | Изменение | Откуда |
|---|---|---|
| 15 | `ops.delete/copy/move` без `confirmed=true` → `UiEvent::Ext { id: "ops.draft", args }` (inline-черновик операции); с `confirmed` — запуск job | S2-5 |
| 16 | В конфликте: «Прервать задачу» = `Choice { id: "abort", key: 'q' }`; Esc = «позже» (вопрос сворачивается в `?` в job-баре, job идёт) | S2-5 |
| 17 | S5 добавляет: `ops.copy/move/delete { current? }` (Shift+F5/F6/F8 — только элемент под курсором), имя в `ops.mkdir` опционально, `file.clip.copy/cut/paste` (файлы через системный буфер) | S3-8 |
| 18 | Id `viewer.*`, `editor.*`, `search.*`, `arc.*` из S3 — вход для S7/S8, подтверждаются при их запуске | S3-9 |

## Зависимости (одобрено)
alacritty_terminal =0.26.0, rustix 1.1, rustix-openpty 0.2, base64 0.22, unicode-width 0.2, fluent-bundle, unic-langid. deny.toml: `skip` для дубля windows-sys 0.59/0.61. Upstream-PR в egui-winit (numpad) — задача S3.

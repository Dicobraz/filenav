# ADR-0013: аргумент `ui.draft` — `command`, а не `cmd`

Статус: принято (владелец, 2026-10-17, при S2-02).

## Контекст

S2 §3 задаёт команду строки ввода аргумента: `ui.draft {cmd:Str, arg:Str, init?:Str}` (F7 → `ui.draft cmd=ops.mkdir arg=name`).

Но имя `cmd` занято. S3 §2.2 резервирует ключи `cmd` и `_repeat` в таблице привязки keymap (`{ cmd = "panel.sort", key = "mtime" }`), и core это уже проверяет: `valid_key` в `filenav-core` (S1) отвергает аргумент `cmd` у любой команды. Поэтому команду в виде из S2 §3 нельзя ни разобрать из текста (`Command::parse`), ни описать в keymap.

## Решение

Целевая команда передаётся аргументом `command`:

```text
ui.draft command=ops.mkdir arg=name init=New
```

Имена `arg` и `init` остаются. В `UiState.draft` поле по-прежнему `Draft.cmd`: это внутреннее поле, а не ключ аргумента.

## Последствия

- В S2-02 handler `ui` реализует `ui.draft` с аргументом `command`.
- S3 (пресеты Far/DN, привязка F7 и Gray+) пишет `command=` вместо `cmd=`.
- Текст S2 §3 читать с этой поправкой. Другой вариант — снять резерв `cmd` в S3 §2.2 и в core — меняет формат keymap, поэтому не предлагается.

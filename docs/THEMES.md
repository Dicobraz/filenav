# Темы оформления (утверждено 2026-10-20)

Решения — S0-decisions №59–№61, задача — `docs/TASKS-THEMES.md`. Эталон — холст «Урфин — темы оформления» (шаблон окна, экраны F3/F4/F5/F8, поиск, настройки, конфликт, job-бар). Принцип: спокойно, почти ч/б, без чистого белого. Правка палитры — сразу во всех темах. Высокая контрастность — только авто из Windows, в списке тем нет. По умолчанию — «как в ОС» (light/dark + акцент ОС). Mica не используется: фон каркаса сплошной.

**Одна информация — одно место** (№61): имя файла — во вкладке; путь — в заголовке экрана; размер, кодировка, переводы строк, тип — в статус-строке; переключатели режимов — в заголовке; действия по клавишам — в панели клавиш; действия встроенной строки (поиск, подтверждение, конфликт) — на её кнопках с подписью клавиши, в панели клавиш не повторяются.

Роли: frame (каркас/заголовок), panel, header (статус-строка, плашки, подложка номеров строк), text, secondary, accent (акцент ОС в light/dark), cursor, cursor_text, border, current_line (строка редактора), highlight (совпадения поиска), modified, exec, archive, context_admin, context_remote; syntax: keyword, string, number, comment. Контраст text-ролей и syntax к panel ≥4,5:1; exec не совпадает по тону с accent.

```toml
[system-light]
frame="#E9E9E9" panel="#F4F4F4" header="#EEEEEE" text="#1B1B1B" secondary="#5F5F5F"
accent="#0067C0" cursor="#D3E3F1" cursor_text="#1B1B1B" border="#DCDCDC"
current_line="#ECECEC" highlight="#F2E3A0"
modified="#B5402F" exec="#2E7D32" archive="#7B4FB0" context_admin="#B5402F" context_remote="#A86A12"
syntax = { keyword="#1F5FA8", string="#8A4B12", number="#6A3FA0", comment="#5F6B76" }

[system-dark]
frame="#1C1C1C" panel="#262626" header="#2B2B2B" text="#F2F2F2" secondary="#B8B8B8"
accent="#6CC4F2" cursor="#24557A" cursor_text="#FFFFFF" border="#3C3C3C"
current_line="#2E2E2E" highlight="#5C4A1C"
modified="#FFB86C" exec="#8FD98A" archive="#C9B4FF" context_admin="#FF8A7A" context_remote="#FFC46B"
syntax = { keyword="#7FB8F0", string="#E3B07A", number="#C9B4FF", comment="#949494" }

[blue]
frame="#E1E9F1" panel="#EEF3F8" header="#E8EEF4" text="#1E2A36" secondary="#5E6B78"
accent="#3A6EA5" cursor="#D4E3F2" cursor_text="#1E2A36" border="#D3DDE8"
current_line="#E6EDF4" highlight="#F0E2A6"
modified="#B0452F" exec="#2E7D4A" archive="#7A4FA8" context_admin="#B0452F" context_remote="#A86A12"
syntax = { keyword="#2F5F96", string="#8A4B12", number="#6A3FA0", comment="#5E6B78" }

[turquoise]
frame="#DFEDEB" panel="#EDF5F4" header="#E6F1EF" text="#1D2E2D" secondary="#5A7270"
accent="#2C8C86" cursor="#CFE9E6" cursor_text="#1D2E2D" border="#CFE1DE"
current_line="#E4F0EE" highlight="#F0E2A6"
modified="#B0502F" exec="#3A7D2E" archive="#7A4FA8" context_admin="#B0502F" context_remote="#A86A12"
syntax = { keyword="#1F6F6A", string="#8A4B12", number="#6A3FA0", comment="#5A7270" }

[grass]
frame="#E3ECDD" panel="#EEF4EA" header="#E8F0E3" text="#222D1C" secondary="#5F6D57"
accent="#4E8A3A" cursor="#DCEBCF" cursor_text="#222D1C" border="#D5E0CC"
current_line="#E6EEE1" highlight="#F0E2A6"
modified="#A84A2C" exec="#2C6E8F" archive="#7A4FA8" context_admin="#A84A2C" context_remote="#A86A12"
syntax = { keyword="#3E6E2E", string="#8A4B12", number="#2C5F8F", comment="#5F6D57" }
# exec не зелёный — не сливается с акцентом

[sepia]
frame="#E7DECF" panel="#F2ECE1" header="#ECE4D7" text="#3A3128" secondary="#7A6C5C"
accent="#9A6A3A" cursor="#E8DCC6" cursor_text="#3A3128" border="#DACFBC"
current_line="#EBE3D6" highlight="#EAD49A"
modified="#A8432F" exec="#4F7A2E" archive="#7A4F8F" context_admin="#A8432F" context_remote="#9A6212"
syntax = { keyword="#7A4A1F", string="#5E6A2E", number="#6A3F7A", comment="#7A6C5C" }
```

Плотность строк 20/24/28 px (умолч. 24). Иконки файлов: нет / Segoe Fluent моно (умолч.) / цветные. Отмеченные файлы — цвет accent + полужирный.

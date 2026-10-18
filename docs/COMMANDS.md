# Реестр команд Urfin

> Генерируется из реестра команд (core + ui) тестом `commands_doc_is_current`
> (`crates/ui/src/tests.rs`). Руками не править. Обновить:
> `URFIN_UPDATE_DOCS=1 cargo nextest run -p urfin-ui commands_doc`.

Команда — имя функции Urfin. Её принимают меню F9 (`menu.toml`), keymap (S3) и командная
строка в одном виде: `id арг=значение …`, например `panel.sort key=size` или
`panel.go path="C:\Program Files"`.

Значения аргументов:
- `bool` — `true` / `false`;
- `int` — целое;
- `str` — слово или строка в кавычках, кавычка внутри — `""`;
- `loc` — путь или uri (`local:C:\work`, архив — `local:C:\a.zip>>zip:/in`);
- `list` — `[a, b]`.

`?` — необязательный аргумент. `side` — `left | right | active | passive`, по умолчанию активная панель.

**Проверка в runtime.** При загрузке `menu.toml` каждая команда проверяется по этому реестру. Id должен быть известен, аргументы — описаны и нужного типа, обязательные — заданы. Иначе пункт скрыт, а предупреждение показывается тостом.

**Клавиши** — пресет Far (заглушка до keymap S3). **Меню F9** — где команда стоит во вшитом `menu.toml`.

## `panel` (29)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `panel.cursor.up` | `n?:int` | Cursor up | Курсор вверх | — | — |
| `panel.cursor.down` | `n?:int` | Cursor down | Курсор вниз | — | — |
| `panel.cursor.page_up` | `n?:int` | Page up | Страница вверх | — | — |
| `panel.cursor.page_down` | `n?:int` | Page down | Страница вниз | — | — |
| `panel.cursor.left` | `n?:int` | Column left | Колонка влево | — | — |
| `panel.cursor.right` | `n?:int` | Column right | Колонка вправо | — | — |
| `panel.cursor.home` | — | First item | Первый элемент | — | — |
| `panel.cursor.end` | — | Last item | Последний элемент | — | — |
| `panel.cursor.to` | `name:str` | Go to name | Перейти к имени | — | — |
| `panel.cursor.match` | `mask:str`, `dir?:str` | Find by mask | Найти по маске | — | — |
| `panel.viewport` | `side?:str`, `rows:int`, `cols?:int`, `top?:int` | Viewport | Область просмотра | — | — |
| `panel.enter` | — | Open | Открыть | — | — |
| `panel.up` | — | Parent folder | Родительская папка | — | — |
| `panel.root` | — | Root folder | Корень диска | — | — |
| `panel.back` | — | Back | Назад | — | commands |
| `panel.forward` | — | Forward | Вперёд | — | commands |
| `panel.go` | `path:loc`, `side?:str` | Go to folder | Перейти в папку | — | — |
| `panel.refresh` | `tab?:int` | Refresh | Обновить | — | left, right |
| `panel.switch` | `side?:str` | Switch panel | Другая панель | Tab | — |
| `panel.swap` | — | Swap panels | Поменять панели местами | Ctrl+U | commands |
| `panel.focus` | `side?:str` | Focus panel | Перейти в панель | — | — |
| `panel.toggle` | `side:str` | Show or hide panel | Показать или скрыть панель | Ctrl+F1 (side=left), Ctrl+F2 (side=right) | — |
| `panel.sort` | `key:str`, `desc?:bool` | Sort | Сортировка | Ctrl+F3 (key=name), Ctrl+F4 (key=ext), Ctrl+F5 (key=mtime), Ctrl+F6 (key=size), Ctrl+F7 (key=unsorted) | left, right |
| `panel.view_mode` | `mode:str` | View mode | Вид | Ctrl+1 (mode=brief), Ctrl+2 (mode=medium), Ctrl+3 (mode=full), Ctrl+4 (mode=wide) | left, right |
| `panel.mode` | `mode:str`, `side?:str` | Panel mode | Режим панели | — | — |
| `panel.filter` | `mask?:str` | Filter | Фильтр | — | — |
| `panel.hidden.toggle` | — | Show hidden files | Показывать скрытые файлы | — | left, right |
| `panel.bookmark.set` | `n:int` | Set bookmark | Поставить закладку | — | — |
| `panel.bookmark.go` | `n:int` | Go to bookmark | Перейти к закладке | — | — |

## `selection` (9)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `sel.toggle` | `advance?:bool` | Toggle selection | Пометить | Ins | — |
| `sel.move` | `motion:str` | Select while moving | Пометить с движением | — | — |
| `sel.all` | — | Select all files | Пометить все файлы | — | files |
| `sel.none` | — | Clear selection | Снять пометку | — | files |
| `sel.invert` | `dirs?:bool` | Invert selection | Инвертировать пометку | Gray * | files |
| `sel.restore` | — | Restore selection | Вернуть пометку | — | files |
| `sel.add` | `mask?:str` | Select by mask | Пометить по маске | — | — |
| `sel.remove` | `mask?:str` | Deselect by mask | Снять пометку по маске | — | — |
| `sel.same` | `by:str`, `on?:bool` | Select same | Пометить такие же | — | — |

## `tab` (7)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `tab.new` | `side?:str` | New tab | Новая вкладка | Ctrl+Shift+T | commands |
| `tab.dup` | `side?:str` | Duplicate tab | Копия вкладки | — | commands |
| `tab.close` | `side?:str` | Close tab | Закрыть вкладку | Ctrl+Shift+W | commands |
| `tab.next` | `side?:str` | Next tab | Следующая вкладка | — | — |
| `tab.prev` | `side?:str` | Previous tab | Предыдущая вкладка | — | — |
| `tab.goto` | `n:int`, `side?:str` | Go to tab | Перейти на вкладку | — | — |
| `tab.move` | `n:int`, `side?:str` | Move tab | Переместить вкладку | — | — |

## `app` (4)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `app.quit` | — | Quit | Выход | F10 | — |
| `app.dismiss_banner` | — | Dismiss banner | Скрыть уведомление | — | — |
| `app.reload_config` | — | Reload configuration | Перечитать настройки | — | options |
| `app.save_session` | — | Save session | Сохранить сеанс | Shift+F9 | options |

## `job` (5)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `job.cancel` | `id:int` | Cancel task | Отменить задачу | — | — |
| `job.pause` | `id:int` | Pause task | Приостановить задачу | — | — |
| `job.resume` | `id:int` | Resume task | Продолжить задачу | — | — |
| `job.retry` | `id:int` | Retry task | Повторить задачу | — | — |
| `job.priority` | `id:int`, `p:int` | Task priority | Приоритет задачи | — | — |

## `prompt` (2)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `prompt.reply` | `id:int`, `choice:str`, `input?:str`, `all?:bool` | Answer | Ответить | — | — |
| `prompt.cancel` | `id:int` | Dismiss | Закрыть | — | — |

## `ui` (16)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `ui.menu` | `menu?:str`, `side?:str`, `path?:loc` | Menu | Меню | F9 | — |
| `ui.qsearch` | `text?:str` | Quick search | Быстрый поиск | — | — |
| `ui.rename` | — | Rename in place | Переименовать на месте | — | files |
| `ui.path_edit` | `side?:str`, `path?:str` | Edit path | Ввести путь | — | left, right |
| `ui.draft` | `command:str`, `arg:str`, `init?:str` | Command argument | Аргумент команды | Gray + (command=sel.add arg=mask), Gray − (command=sel.remove arg=mask) | files |
| `ui.cancel` | — | Close | Закрыть | — | — |
| `ui.prompt.hide` | `id?:int` | Hide question | Свернуть вопрос | — | — |
| `ui.prompt.show` | `id?:int` | Show question | Развернуть вопрос | — | — |
| `ui.jobs.toggle` | — | Expand tasks | Развернуть задачи | — | — |
| `ui.theme` | `mode:str` | Theme | Тема | — | options |
| `ui.zoom` | `delta?:int` | Zoom | Масштаб | — | options |
| `ui.split` | `delta?:int`, `reset?:bool` | Panel split | Раздел панелей | — | — |
| `ui.lang` | `lang:str` | Language | Язык | — | options |
| `ui.columns` | `set:str` | Columns | Колонки | — | — |
| `ui.panels.toggle_all` | — | Hide or show panels | Скрыть или показать панели | Ctrl+O | commands |
| `ui.about` | — | About | О программе | — | options |

## `keymap` (11)

| Команда | Аргументы | EN | RU | Клавиши (Far) | Меню F9 |
|---|---|---|---|---|---|
| `keymap.reload` | — | Reload key bindings | Перечитать раскладку клавиш | — | options |
| `keymap.preset` | `name:str` | Key bindings preset | Пресет раскладки клавиш | — | — |
| `keymap.show` | — | Key bindings | Раскладка клавиш | — | — |
| `keymap.dump` | `path?:str` | Export key bindings | Выгрузить раскладку клавиш | — | options |
| `keymap.qs.start` | `text?:str` | Quick search | Быстрый поиск | — | — |
| `keymap.qs.input` | `text:str` | Quick search: type | Быстрый поиск: ввод | — | — |
| `keymap.qs.next` | — | Quick search: next match | Быстрый поиск: следующее | — | — |
| `keymap.qs.prev` | — | Quick search: previous match | Быстрый поиск: предыдущее | — | — |
| `keymap.qs.backspace` | — | Quick search: erase a character | Быстрый поиск: стереть символ | — | — |
| `keymap.qs.cancel` | — | Close quick search | Закрыть быстрый поиск | — | — |
| `keymap.pending` | `chord?:str` | Pending key chord | Начатое сочетание клавиш | — | — |

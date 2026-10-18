# UI texts (S2).
ui-footer-selected = { $count } selected · { $bytes }
ui-footer-items = { $count } { $count ->
    [one] item
   *[other] items
} · { $bytes }
ui-delete-trash = Move { $count } { $count ->
    [one] item
   *[other] items
} ({ $bytes }) to the Recycle Bin?
ui-conflict-head = Conflict { $index } of { $total } · { $job }
ui-loading = Loading…
ui-listing-failed = Cannot read the folder: { $error }
ui-lang-unavailable = Language { $lang } is not available, using English
ui-warn-no-system-font = No system interface font found, using the built-in one
ui-type-folder = Folder
ui-type-file = File
ui-type-file-ext = { $ext } File
ui-type-app = Application
ui-type-archive = { $ext } Archive
ui-type-link = Shortcut
ui-col-name = Name
ui-col-size = Size
ui-col-mtime = Modified
ui-folder-empty = This folder is empty
ui-retry = Retry
ui-loading-count = Loading… { $count }

# Breadcrumb (S2-12)
ui-menu-loading = Loading…
ui-menu-empty = No folders
ui-path-not-found = Cannot open { $path }: { $error }

ui-renderer-fallback = Graphics driver is failing — switched to software rendering

## About / splash (R-01). The screens are not built yet: TODO S2-14 (F9 menu).
ui-about-name = Úrfionn
ui-about-aka = a.k.a. Oorfene Deuce

## Меню F9 (S2-14). `.menu-key` — буква выбора пункта.
menu-left = Left
menu-files = Files
menu-commands = Commands
menu-options = Options
menu-right = Right
menu-view-brief = Brief
    .menu-key = b
menu-view-medium = Medium
    .menu-key = m
menu-view-full = Full
    .menu-key = f
menu-view-wide = Wide
    .menu-key = w
menu-sort-name = Sort by name
    .menu-key = n
menu-sort-ext = Sort by extension
    .menu-key = x
menu-sort-mtime = Sort by time
    .menu-key = t
menu-sort-size = Sort by size
    .menu-key = z
menu-sel-add = Select group…
    .menu-key = g
menu-sel-remove = Deselect group…
    .menu-key = d
menu-theme-system = System theme
    .menu-key = y
menu-theme-light = Light theme
    .menu-key = l
menu-theme-dark = Dark theme
    .menu-key = d
menu-zoom-in = Zoom in
    .menu-key = i
menu-zoom-out = Zoom out
    .menu-key = o
menu-zoom-reset = Reset zoom
    .menu-key = z
menu-lang-en = English
    .menu-key = e
menu-lang-ru = Русский
    .menu-key = u
menu-about = About Úrfionn
    .menu-key = a
# Быстрый поиск и начатый аккорд в подвале панели (S3-06)
ui-qs-matches = { $count } found
ui-chord-pending = { $chord } … (waiting for the next key)
# Баннер-вопрос, конфликт и черновики (S2-15)
ui-prompt-more = { $count ->
    [one] { $count } more question
   *[other] { $count } more questions
}
ui-prompt-all = For all
ui-prompt-new-name = New name
ui-button-apply = Apply
ui-button-back = Back
ui-button-later = Later
ui-button-cancel = Cancel
ui-button-delete = Delete
ui-button-permanent = Permanently
ui-conflict-name = Name
ui-conflict-size = Size
ui-conflict-mtime = Modified
ui-conflict-new = New
ui-conflict-old = Existing
ui-conflict-newer = ▲ newer by { $span }
ui-conflict-older = ▼ older by { $span }
ui-span-min = { $n } min
ui-span-hours = { $n } h
ui-span-days = { $n } d
ui-draft-delete = Move { $count } { $count ->
    [one] item
   *[other] items
} ({ $size }) to the Recycle Bin?
ui-draft-delete-permanent = Delete { $count } { $count ->
    [one] item
   *[other] items
} ({ $size }) PERMANENTLY? This cannot be undone.
ui-draft-copy = Copy { $count } { $count ->
    [one] item
   *[other] items
} ({ $size }) to
ui-draft-move = Move { $count } { $count ->
    [one] item
   *[other] items
} ({ $size }) to
ui-draft-ops-mkdir = New folder
ui-draft-mkdir-hint = a\b\c is fine
ui-draft-sel-add = Select by mask
ui-draft-sel-remove = Deselect by mask
ui-draft-panel-filter = Filter
# Переименование по месту (S2-16)
ui-rename-hint = Enter — rename · Esc — cancel

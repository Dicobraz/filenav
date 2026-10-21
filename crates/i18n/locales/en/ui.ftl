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
menu-sync-lr = Synchronize →
    .menu-key = y
menu-sync-rl = Synchronize ←
    .menu-key = i
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
    .menu-key = e
menu-theme-system = System theme
    .menu-key = y
menu-theme-next = Next theme
    .menu-key = n
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
ui-draft-ops-multi_rename = New names:
ui-mr-pattern-error = Pattern: { $error }
ui-mr-problem-invalid = invalid name
ui-mr-problem-duplicate = duplicate
ui-mr-problem-exists = name taken
ui-mr-summary = { $changed } will change, { $problems } with problems (skipped) · { $shown } shown · [N] name, [E] extension, [C:1:1:3] counter, [Y][M][D] date, [U]/[L] case, “find/replace” — regex
ui-draft-ops-link = Link in:
ui-draft-ops-attrs = Attributes (+R −H +S +A):
ui-draft-ops-touch = Modified time:
ui-prompt-all = For all
ui-prompt-all-hint = Space; Shift+key answers for all
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
ui-draft-sel-add = Select by mask
ui-draft-sel-remove = Deselect by mask
ui-draft-panel-filter = Filter
# Переименование по месту (S2-16)
ui-rename-hint = Enter — rename · Esc — cancel
# Job-бар (S2-17)
ui-job-speed = { $size }/s
ui-job-phase-scan = Counting
ui-job-phase-verify = Verifying
ui-job-phase-finalize = Finishing
ui-job-phase-recover = Recovering
ui-job-eta-sec = ≈ { $s } s
ui-job-eta-min = ≈ { $m } min
ui-job-eta-hour = ≈ { $h } h { $m } min
ui-job-questions = { $count } { $count ->
    [one] question
   *[other] questions
}
ui-jobs-count = Tasks { $count }
# Режимы панели: дерево, сведения, быстрый просмотр (S2-18)
ui-tree-title = Tree · { $root }
ui-tree-title-empty = Tree
ui-tree-hint = ← → collapse/expand · Enter — open
ui-info-title = Information
ui-info-hint = Ctrl+L — close · updates live
ui-info-volume = Volume
ui-info-fs = File system
ui-info-caps = Features
ui-info-free = Free
ui-info-free-value = { $free } of { $total } ({ $pct } %)
ui-info-media-net = network
ui-info-media-usb = removable
ui-info-cap-trash = recycle bin
ui-info-cap-ads = ADS
ui-info-cap-compression = compression
ui-info-cap-clone = block cloning
ui-info-cap-case = case-sensitive
ui-info-path = Path
ui-info-content = Contents
ui-info-content-value = folders: { $dirs } · files: { $files }
ui-info-bytes = Files size
ui-info-selected = Selected
ui-info-cursor = Under cursor
ui-info-name = Name
ui-info-type = Type
ui-info-size = Size
ui-info-mtime = Modified
ui-info-attrs = Attributes
ui-info-kind-dir = folder
ui-info-kind-file = file
ui-info-kind-link = link
ui-info-kind-other = other
ui-qview-hint = F3 — viewer
ui-qview-binary = binary
ui-qview-dir = A folder — its contents are in the panel
ui-info-volume-local = Local Disk

## Темы (T-01, №59)
ui-theme-auto = As in Windows
ui-theme-system-light = System light
ui-theme-system-dark = System dark
ui-theme-blue = Blue
ui-theme-turquoise = Turquoise
ui-theme-grass = Grass
ui-theme-sepia = Sepia
ui-theme-locked = The theme is set by your organization’s policy
ui-theme-unknown = Unknown theme “{ $id }”
ui-theme-error = Theme not loaded: { $error }
ui-context-admin = Administrator
ui-context-remote = Remote session

## Каркас окна (D-02, №61)
ui-panel-count = { $count } { $count ->
    [one] item
   *[other] items
}
ui-status-selected = Selected { $count } of { $total }
ui-status-selected-bytes = Selected { $count } of { $total } · { $bytes }
ui-status-free = { $root } { $bytes } free
ui-status-dir = { $name } · { $count } { $count ->
    [one] item
   *[other] items
}

## Диски (D-03)
ui-drives-section-drives = Drives
ui-drives-section-network = Network
ui-drives-section-places = Places
ui-drives-free = { $bytes } free
ui-drives-offline = no connection
ui-place-home = Home
ui-place-desktop = Desktop
ui-place-documents = Documents
ui-place-downloads = Downloads
ui-size-dir = <DIR>

## Состояния папки (D-03)
ui-state-empty = Folder is empty
ui-state-denied = No access to the folder
ui-state-denied-text = Only administrators can read this folder.
ui-state-offline = No connection to { $path }
ui-state-offline-text = The network resource does not respond. Other work does not wait.
ui-state-failed = Cannot read the folder
ui-state-back = Back
ui-state-retry = Retry now

ui-tree-label = tree
ui-qv-title = Quick view
ui-info-summary = Folder { $path }: { $files } files, { $dirs } folders · { $bytes }
ui-info-free-of = { $free } free of { $total }

## Встроенные строки (D-04)
ui-strip-ifexists = If the file exists
ui-strip-ifexists-ask = Ask
ui-strip-ifexists-overwrite = Replace
ui-strip-ifexists-skip = Skip
ui-strip-ifexists-newer = Replace if newer
ui-strip-keep-dates = Keep dates
ui-strip-verify = Verify hash after copying
ui-strip-verify-move = Verify hash before deleting the source
ui-strip-background = In background
ui-strip-matches = { $count } match
ui-strip-mr-summary = will change { $changed } · problems { $problems }
ui-strip-conflict-of = { $name } · conflict { $index } of { $total }
ui-strip-newer = newer
ui-strip-mkdir = New folder
ui-strip-mkdir-many = Several, separated by ;
ui-strip-mkdir-nested = nested — with \
ui-strip-filter = Show
ui-strip-filter-matches = matches { $count } of { $total }
ui-strip-attrs = { $count ->
    [one] Attributes of { $count } item
   *[other] Attributes of { $count } items
}
ui-strip-attrs-mtime = Modified
ui-strip-attrs-now = now
ui-strip-attrs-varies = various
ui-attr-readonly = Read-only
ui-attr-hidden = Hidden
ui-attr-system = System
ui-attr-archive = Archive
ui-filter-chip = filter { $mask }
ui-panel-count-filtered = { $count } of { $total }
ui-button-create = Create
ui-button-reset = Reset
ui-keys-apply = Apply
ui-keys-cancel = Cancel
ui-keys-name-ext = Name ↔ extension
ui-button-copy = Copy
ui-button-move = Move
ui-button-mark = Select
ui-button-unmark = Deselect
ui-button-rename = Rename
ui-button-ok = OK
ui-keys-qs-next = Next
ui-keys-qs-prev = Previous
ui-keys-close = Close

## Pouches and menus (D-05)
ui-sort-name = Name
ui-sort-ext = Extension
ui-sort-size = Size
ui-sort-mtime = Date modified
ui-sort-unsorted = Unsorted
ui-sort-desc = Descending
menu-copy-path = Copy path
menu-copy-name = Copy name
ui-places-current = current folder
ui-places-find-bookmark = Find a bookmark
ui-places-find-folder = Find a folder
ui-places-empty = Empty
ui-places-add = Add current
ui-places-root = root
ui-keys-menu = Menu
ui-keys-item = Item
ui-keys-choose = Choose
ui-keys-toggle = Toggle
ui-keys-go = Go
ui-keys-other-panel = Open in the other panel

## Field completion (No. 70)
ui-complete-history = history
ui-complete-folder = folder
ui-complete-here = in folder
ui-keys-complete-next = Next
ui-keys-insert = Insert
ui-keys-field-history = Field history

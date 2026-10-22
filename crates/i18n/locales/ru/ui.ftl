# Тексты интерфейса (S2).
ui-footer-selected = { $count } { $count ->
    [one] выбран
   *[other] выбрано
} · { $bytes }
ui-footer-items = { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} · { $bytes }
ui-delete-trash = В корзину { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $bytes })?
ui-conflict-head = Конфликт { $index } из { $total } · { $job }
ui-loading = Загрузка…
ui-listing-failed = Не удалось прочитать папку: { $error }
ui-lang-unavailable = Языка { $lang } нет, используется английский
ui-warn-no-system-font = Системный шрифт интерфейса не найден, используется встроенный
ui-type-folder = Папка
ui-type-file = Файл
ui-type-file-ext = Файл { $ext }
ui-type-app = Приложение
ui-type-archive = Архив { $ext }
ui-type-link = Ярлык
ui-col-name = Имя
ui-col-size = Размер
ui-col-mtime = Изменён
ui-folder-empty = Папка пуста
ui-retry = Повторить
ui-loading-count = Загрузка… { $count }

# Breadcrumb (S2-12)
ui-menu-loading = Загрузка…
ui-menu-empty = Папок нет
ui-path-not-found = Не открыть { $path }: { $error }

ui-renderer-fallback = Графический драйвер сбоит — включён программный рендер

## О программе / сплэш (R-01). Экранов ещё нет: TODO S2-14 (меню F9).
ui-about-name = Úrfionn
ui-about-aka = он же Урфин Джюс

## Меню F9 (S2-14). `.menu-key` — буква выбора пункта.
menu-left = Левая
menu-files = Файлы
menu-commands = Команды
menu-options = Параметры
menu-right = Правая
menu-sync-lr = Синхронизировать →
    .menu-key = и
menu-sync-rl = Синхронизировать ←
    .menu-key = х
menu-view-brief = Список
    .menu-key = с
menu-view-medium = Средний
    .menu-key = р
menu-view-full = Таблица
    .menu-key = т
menu-view-wide = Широкий
    .menu-key = ш
menu-sort-name = По имени
    .menu-key = и
menu-sort-ext = По расширению
    .menu-key = а
menu-sort-mtime = По времени
    .menu-key = в
menu-sort-size = По размеру
    .menu-key = з
menu-sel-add = Выделить группу…
    .menu-key = г
menu-sel-remove = Снять выделение…
    .menu-key = н
menu-theme-system = Тема как в системе
    .menu-key = с
menu-theme-next = Следующая тема
    .menu-key = д
menu-theme-light = Светлая тема
    .menu-key = е
menu-theme-dark = Тёмная тема
    .menu-key = т
menu-zoom-in = Крупнее
    .menu-key = к
menu-zoom-out = Мельче
    .menu-key = м
menu-zoom-reset = Обычный масштаб
    .menu-key = о
menu-lang-en = English
    .menu-key = e
menu-lang-ru = Русский
    .menu-key = р
menu-about = О программе Úrfionn
    .menu-key = п
# Быстрый поиск и начатый аккорд в подвале панели (S3-06)
ui-qs-matches = Найдено: { $count }
ui-chord-pending = { $chord } … (ждёт следующую клавишу)
# Баннер-вопрос, конфликт и черновики (S2-15)
ui-prompt-more = ещё { $count } { $count ->
    [one] вопрос
    [few] вопроса
   *[many] вопросов
}
ui-draft-ops-multi_rename = Новые имена:
ui-mr-pattern-error = Шаблон: { $error }
ui-mr-problem-invalid = недопустимое имя
ui-mr-problem-duplicate = повтор
ui-mr-problem-exists = имя занято
ui-mr-summary = Изменится { $changed }, с проблемами { $problems } (их пропустим) · показано { $shown } · [N] имя, [E] расширение, [C:1:1:3] счётчик, [Y][M][D] дата, [U]/[L] регистр, «найти/заменить» — regex
ui-draft-ops-link = Ссылка в:
ui-draft-ops-attrs = Атрибуты (+R −H +S +A):
ui-draft-ops-touch = Время изменения:
ui-prompt-all = Для всех
ui-prompt-all-hint = Пробел; Shift+клавиша — ответ для всех
ui-prompt-new-name = Новое имя
ui-button-apply = Применить
ui-button-back = Назад
ui-button-later = Позже
ui-button-cancel = Отмена
ui-button-delete = Удалить
ui-button-permanent = Насовсем
ui-conflict-name = Имя
ui-conflict-size = Размер
ui-conflict-mtime = Изменён
ui-conflict-new = Новый
ui-conflict-old = Есть
ui-conflict-newer = ▲ новее на { $span }
ui-conflict-older = ▼ старше на { $span }
ui-span-min = { $n } мин
ui-span-hours = { $n } ч
ui-span-days = { $n } д
ui-draft-delete = В корзину { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $size })?
ui-draft-delete-permanent = Удалить НАВСЕГДА { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $size })? Отменить нельзя.
ui-draft-copy = Копировать { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $size }) в
ui-draft-move = Перенести { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $size }) в
ui-draft-ops-mkdir = Новая папка
ui-draft-sel-add = Пометить по маске
ui-draft-sel-remove = Снять пометку по маске
ui-draft-panel-filter = Фильтр
# Переименование по месту (S2-16)
ui-rename-hint = Enter — переименовать · Esc — отмена
# Job-бар (S2-17)
ui-job-speed = { $size }/с
ui-job-phase-scan = Подсчёт
ui-job-phase-verify = Проверка
ui-job-phase-finalize = Завершение
ui-job-phase-recover = Восстановление
ui-job-eta-sec = ≈ { $s } с
ui-job-eta-min = ≈ { $m } мин
ui-job-eta-hour = ≈ { $h } ч { $m } мин
ui-job-questions = { $count } { $count ->
    [one] вопрос
    [few] вопроса
   *[many] вопросов
}
ui-jobs-count = Задачи { $count }
# Режимы панели: дерево, сведения, быстрый просмотр (S2-18)
ui-tree-title = Дерево · { $root }
ui-tree-title-empty = Дерево
ui-tree-hint = ← → свернуть/развернуть · Enter — открыть
ui-info-title = Сведения
ui-info-hint = Ctrl+L — закрыть · обновляется на лету
ui-info-volume = Том
ui-info-fs = Файловая система
ui-info-caps = Возможности
ui-info-free = Свободно
ui-info-free-value = { $free } из { $total } ({ $pct } %)
ui-info-media-net = сеть
ui-info-media-usb = съёмный
ui-info-cap-trash = корзина
ui-info-cap-ads = ADS
ui-info-cap-compression = сжатие
ui-info-cap-clone = клонирование блоков
ui-info-cap-case = регистр букв
ui-info-path = Путь
ui-info-content = Содержимое
ui-info-content-value = папок: { $dirs } · файлов: { $files }
ui-info-bytes = Объём файлов
ui-info-selected = Выделено
ui-info-cursor = Под курсором
ui-info-name = Имя
ui-info-type = Тип
ui-info-size = Размер
ui-info-mtime = Изменён
ui-info-attrs = Атрибуты
ui-info-kind-dir = папка
ui-info-kind-file = файл
ui-info-kind-link = ссылка
ui-info-kind-other = другое
ui-qview-hint = F3 — просмотр
ui-qview-binary = двоичный
ui-qview-dir = Папка — её содержимое в панели
ui-info-volume-local = Локальный диск

## Темы (T-01, №59)
ui-theme-auto = Как в Windows
ui-theme-system-light = Системная светлая
ui-theme-system-dark = Системная тёмная
ui-theme-blue = Голубая
ui-theme-turquoise = Бирюза
ui-theme-grass = Травяная
ui-theme-sepia = Сепия
ui-theme-locked = Тема задана политикой организации
ui-theme-unknown = Неизвестная тема «{ $id }»
ui-theme-error = Тема не загружена: { $error }
ui-context-admin = Администратор
ui-context-remote = Удалённый сеанс

## Каркас окна (D-02, №61)
ui-panel-count = { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
}
ui-status-selected = Выделено { $count } из { $total }
ui-status-selected-bytes = Выделено { $count } из { $total } · { $bytes }
ui-status-free = { $root } { $bytes } свободно
ui-status-dir = { $name } · { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
}

## Диски (D-03)
ui-drives-section-drives = Диски
ui-drives-section-network = Сеть
ui-drives-section-places = Места
ui-drives-free = свободно { $bytes }
ui-drives-offline = нет связи
ui-place-home = Домашняя папка
ui-place-desktop = Рабочий стол
ui-place-documents = Документы
ui-place-downloads = Загрузки
ui-size-dir = <DIR>

## Состояния папки (D-03)
ui-state-empty = Папка пуста
ui-state-denied = Нет доступа к папке
ui-state-denied-text = Читать эту папку могут только администраторы.
ui-state-offline = Нет связи с { $path }
ui-state-offline-text = Сетевой ресурс не отвечает. Остальная работа не ждёт.
ui-state-failed = Не удалось прочитать папку
ui-state-back = Назад
ui-state-retry = Повторить сейчас

ui-tree-label = дерево
ui-qv-title = Быстрый просмотр
ui-info-summary = Папка { $path }: файлов { $files }, папок { $dirs } · { $bytes }
ui-info-free-of = { $free } свободно из { $total }

## Встроенные строки (D-04)
ui-strip-ifexists = Если файл есть
ui-strip-ifexists-ask = Спросить
ui-strip-ifexists-overwrite = Заменить
ui-strip-ifexists-skip = Пропустить
ui-strip-ifexists-newer = Заменить, если новее
ui-strip-keep-dates = Сохранять даты
ui-strip-verify = Проверить хэш после копирования
ui-strip-verify-move = Проверить хэш перед удалением исходных
ui-strip-background = В фоне
ui-strip-matches = совпадает { $count }
ui-strip-mr-summary = изменится { $changed } · проблем { $problems }
ui-strip-conflict-of = { $name } · конфликт { $index } из { $total }
ui-strip-newer = новее
ui-strip-mkdir = Новая папка
ui-strip-mkdir-many = Несколько через ;
ui-strip-mkdir-nested = вложенные — через \
ui-strip-filter = Показывать
ui-strip-filter-matches = совпадает { $count } из { $total }
ui-strip-attrs = { $count ->
    [one] Атрибуты { $count } объекта
   *[other] Атрибуты { $count } объектов
}
ui-strip-attrs-mtime = Изменён
ui-strip-attrs-now = сейчас
ui-strip-attrs-varies = разные
ui-attr-readonly = Только чтение
ui-attr-hidden = Скрытый
ui-attr-system = Системный
ui-attr-archive = Архивный
ui-filter-chip = фильтр { $mask }
ui-panel-count-filtered = { $count } из { $total }
ui-button-create = Создать
ui-button-reset = Сбросить
ui-keys-apply = Применить
ui-keys-cancel = Отмена
ui-keys-name-ext = Имя ↔ расширение
ui-button-copy = Копировать
ui-button-move = Перенести
ui-button-mark = Отметить
ui-button-unmark = Снять отметку
ui-button-rename = Переименовать
ui-button-ok = ОК
ui-keys-qs-next = Следующее
ui-keys-qs-prev = Предыдущее
ui-keys-close = Закрыть

## «Мешочки» и меню (D-05)
ui-sort-name = Имя
ui-sort-ext = Расширение
ui-sort-size = Размер
ui-sort-mtime = Дата изменения
ui-sort-unsorted = Без сортировки
ui-sort-desc = По убыванию
menu-copy-path = Копировать путь
menu-copy-name = Копировать имя
ui-places-current = текущая папка
ui-places-find-bookmark = Найти закладку
ui-places-find-folder = Найти папку
ui-places-empty = Пусто
ui-places-add = Добавить текущую
ui-places-root = корень
ui-keys-menu = Меню
ui-keys-item = Пункт
ui-keys-choose = Выбрать
ui-keys-toggle = Переключить
ui-keys-go = Перейти
ui-keys-other-panel = Открыть в другой панели

## Дополнение полей (№70)
ui-complete-history = история
ui-complete-folder = папка
ui-complete-here = в папке
ui-keys-complete-next = Следующий
ui-keys-insert = Вставить
ui-keys-field-history = История поля

## Job-бар и список задач (D-06)
ui-job-copy = Копирование
ui-job-move = Перемещение
ui-job-delete = Удаление
ui-job-trash = Удаление
ui-job-link = Ссылка
ui-job-rename = Переименование
ui-job-waiting = ждёт ответа
ui-job-paused = пауза
ui-job-queued = в очереди
ui-job-failed = ошибка: { $error }
ui-job-cancelled = отменено
ui-job-done = готово · { $n }
ui-job-of = { $done } из { $total }
ui-job-rate = { $rate }/с
ui-job-eta-s = ещё { $s } с
ui-job-eta-m = ещё { $m } мин
ui-job-eta-h = ещё { $h } ч { $m } мин
ui-job-pause = Пауза
ui-job-resume = Продолжить
ui-job-answer = Ответить
ui-job-retry = Повторить
ui-job-run-now = Без очереди
ui-jobs-title = Задачи { $n }
ui-jobs-collapse = Свернуть
ui-jobs-done = Готово { $n }
ui-jobs-clear = Очистить
ui-keys-job = Задача
ui-keys-pause-resume = Пауза или продолжить
ui-keys-cancel-job = Отменить задачу
ui-keys-collapse = Свернуть
ui-jobs-with-errors = с ошибками
ui-jobs-expand = Развернуть
ui-key-space = Пробел

## Строки операций (D-04): ссылка, время, переименование, F7, сравнение, синхронизация
ui-strip-link = Ссылка
ui-strip-link-to = на
ui-strip-link-kind = Тип
ui-strip-link-hint = Без режима разработчика нужны права администратора; для папки подойдёт Junction
ui-link-sym = Символическая
ui-link-hard = Жёсткая
ui-link-junction = Junction
ui-strip-touch = Время изменения
ui-strip-touch-deep = Для вложенных
ui-strip-touch-hint = создание и доступ — в «Атрибутах»
ui-strip-mr = Новые имена
ui-mr-tok-name = имя
ui-mr-tok-ext = расширение
ui-mr-tok-counter = счётчик
ui-mr-tok-date = дата
ui-mr-tok-replace = найти/замена через /
ui-mkdir-exists = есть
ui-mkdir-new = новая
ui-strip-sync = Синхронизировать:
ui-strip-sync-what = { $count } { $count ->
    [one] файл
    [few] файла
   *[many] файлов
} · { $size }
ui-strip-sync-mirror = Удалять лишнее
ui-strip-sync-go = Синхронизировать
ui-compare-title = Сравнение папок:
ui-compare-only-left = только слева { $n }
ui-compare-only-right = только справа { $n }
ui-compare-newer-left = новее слева { $n }
ui-compare-newer-right = новее справа { $n }
ui-compare-differs = различаются { $n }
ui-compare-equal = одинаковых { $n }
ui-compare-sync-lr = Синхронизировать →
ui-compare-sync-rl = Синхронизировать ←
ui-button-close = Закрыть
ui-tag-newer = новее
ui-tag-only-here = только здесь
ui-prompt-skip-all = Пропустить все такие
ui-job-waiting-for = ждёт ответа: { $what }

## Каркас: табы, приветствие, ссылки (D-02, D-03)
ui-tabs-more = Скрытые вкладки
ui-keys-tab = Вкладка
ui-welcome-title = Добро пожаловать в Урфин
ui-welcome-keys = Клавиши
ui-welcome-hint = Alt+буква — поиск, F10 — выход
ui-welcome-theme = Тема как в Windows
ui-welcome-done = Готово
ui-welcome-later = Позже
ui-welcome-updated = Урфин обновлён до { $to } · с { $from }
ui-welcome-news = Что нового
ui-keys-preset = Far ↔ DN
ui-keys-flag = Флажок
ui-keys-next-field = Следующее поле
ui-link-broken = цель не найдена
ui-size-symlink = <SYMLINK>
ui-size-junction = <JUNCTION>
ui-size-link = <LINK>

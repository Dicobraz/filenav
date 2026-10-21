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
ui-draft-move = Переместить { $count } { $count ->
    [one] объект
    [few] объекта
   *[many] объектов
} ({ $size }) в
ui-draft-ops-mkdir = Новая папка
ui-draft-mkdir-hint = можно a\b\c
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

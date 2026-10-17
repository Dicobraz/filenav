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

## О программе / сплэш (R-01). Экранов ещё нет: TODO S2-14 (меню F9).
ui-about-name = Úrfionn
ui-about-aka = он же Урфин Джюс

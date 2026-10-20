# Тексты core (urfin-core, crate::text::tr).
core-unknown-command = Неизвестная команда: { $id }
core-command-failed = { $id }: { $error }
core-task-panicked = Задача { $task } неожиданно завершилась с ошибкой
core-not-found = Не найдено: { $name }
core-no-match = Ничего не подходит под { $mask }
core-no-bookmark = Закладка { $n } пуста
core-last-tab = Последнюю вкладку закрыть нельзя
core-no-tab = Нет вкладки { $n }
core-config-warnings = В файле настроек ошибки ({ $count }): { $first }
core-crashed = Прошлый запуск Урфина завершился сбоем. Подробности: { $file }
core-action-show-logs = Открыть логи
core-action-dismiss = Скрыть
vfs-job-dir-size = Размер папок
vfs-dir-size-none = Нечего считать: встаньте на папку или выделите папки
ops-not-yet = Пока не сделано: { $what }
ops-nothing = Нечего делать: встаньте на файл или выделите файлы
ops-dest-empty = Не указано, куда
ops-dest-file = Назначение — файл: { $path }
ops-dest-mkdir-failed = Не создать { $path }: { $error }
ops-conflict-bad = Неизвестная политика конфликтов: { $what }
ops-filter-bad = Неверная маска файлов: { $mask }
ops-job-copy = { $n ->
    [one] Копировать { $name }
    [few] Копировать { $n } объекта
   *[many] Копировать { $n } объектов
}
ops-job-move = { $n ->
    [one] Перенести { $name }
    [few] Перенести { $n } объекта
   *[many] Перенести { $n } объектов
}
ops-copy-same = Нельзя скопировать файл сам в себя
ops-copy-into-self = Нельзя скопировать папку внутрь неё самой
ops-copy-type = Файл и папка с одним именем
ops-copy-no-name = Нет свободного имени для копии
ops-move-src-left = Скопировано, но источник не удалён: { $error }
ops-copy-job-failed = Операция не выполнена: { $error }
ops-copy-errors = { $n ->
    [one] Не удалось: { $n } объект
    [few] Не удалось: { $n } объекта
   *[many] Не удалось: { $n } объектов
} — { $first }
ops-conflict-exists = Файл уже есть
ops-conflict-other = Не удалось выполнить для файла
ops-conflict-body = Конфликт { $index } из { $total }: { $name } — { $cmp }
ops-conflict-body-short = Конфликт { $index } из { $total }: { $name }
ops-conflict-newer = новее
ops-conflict-older = старше
ops-conflict-same-time = то же время
ops-conflict-larger = больше на { $size }
ops-conflict-smaller = меньше на { $size }
ops-conflict-same-size = тот же размер
ops-choice-overwrite = Заменить
ops-choice-skip = Пропустить
ops-choice-rename = Переименовать
ops-choice-newer = Заменить, если новее
ops-choice-cancel = Отмена
ops-copy-skipped = { $n ->
    [one] Пропущен { $n } объект
    [few] Пропущено { $n } объекта
   *[many] Пропущено { $n } объектов
}
ops-open-failed = Не открыть { $name }: { $error }
core-settings-not-saved = Настройки не сохранены в { $file }: { $error }
ops-mkdir-failed = Не создать { $name }: { $error }
ops-rename-failed = Не переименовать { $name }: { $error }
ops-exists = { $name } уже есть
ops-rename-bad = В имени не может быть \ и / — перенести в другую папку можно через F6
ops-job-trash = В корзину
ops-job-delete = Удаление
ops-delete-failed = Не удалено: { $error }

# Раскладка клавиш (S3-10): баннер и keymap.dump
keymap-warnings = Раскладка клавиш: { $count } { $count ->
    [one] проблема
    [few] проблемы
   *[many] проблем
}. Первая: { $first }
keymap-kept = Раскладка клавиш не перечитана, осталась прежняя: { $first }
keymap-dumped = Раскладка клавиш сохранена в { $path }
keymap-dump-failed = Не удалось сохранить раскладку в { $path }: { $error }

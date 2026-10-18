# ADR-0016: кто владеет быстрым поиском — keymap (S3) или ui (S2)

Статус: принято (владелец, 2026-10-18, при S3-06).

## Контекст

Две спецификации описывают быстрый поиск (Alt+буква в Far, одиночный Alt в DN) по-разному:
- **S2 §576, §652, §664.** Команда `ui.qsearch {text?}` открывает `Focus::Overlay(QuickSearch)`, состояние — `UiState.qsearch`, поле поиска — виджет S2 в подвале панели, он сам читает остаток ввода egui. Курсор ставит `panel.cursor.match {mask, dir}`.
- **S3 §1.3, §3.6, приёмка S3-06.** Handler `keymap` с командами `keymap.qs.{start,input,next,prev,backspace,cancel}`, состояние `QuickSearch { side, mask }` в `AppState.ext` (`KeymapView`), меняет его только keymap; S2 только рисует.

В коде сейчас есть только `ui.qsearch` (открывает оверлей и кладёт текст в `UiState.qsearch`), поля поиска и его ввода ещё нет.

## Решение

Логика — у keymap (S3), отрисовка — у ui (S2):
- Состояние `KeymapView { qs: Option<QuickSearch>, pending: Option<CompactString>, warnings }` в `AppState.ext`, handler `keymap` (`keymap.qs.*`). Фокус `Overlay(QuickSearch)` ставит и снимает он же.
- Ввод поиска — командами из конвейера `KeyInput` (`keymap.qs.input {text}` из `Event::Text`), без egui-виджета: в панели по-прежнему нет фокусируемых виджетов (контракт v3 №13).
- Совпадения ищет handler `keymap` сам (без тоста `Rejected` на каждый символ), курсор ставит `panel.cursor.to {name}`. Нет совпадения — символ не принимается.
- `ui.qsearch {text?}` остаётся (id в реестре, меню) и делает то же, что `keymap.qs.start`. `UiState.qsearch` убирается.
- Поле поиска в подвале панели рисует ui по `KeymapView.qs` (S2 §576); в S3-06 — минимальная версия: маска и число совпадений вместо подвала.

## Альтернатива

Владелец — ui (S2): keymap только превращает Alt+букву в `ui.qsearch {text}`, дальше ввод читает egui-поле. Минусы: фокусируемый виджет в панели (против v3 №13), ввод поиска в обход keymap (Up/Down/Ctrl+Enter/Esc — двойная обработка), нельзя проверить без окна.

## Последствия

- S2 §576/§652: состояние поиска — `KeymapView.qs`, не `UiState.qsearch`; S2 рисует, не обрабатывает ввод.
- `ui` зависит от `urfin-keymap` (тип `KeymapView`) — зависимость в одну сторону, keymap от ui не зависит.

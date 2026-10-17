# ADR-0011: UI-фронтенд получает корневой `egui::Ui`

Статус: принято (владелец, 2026-10-15, при S1-13).

## Контекст

S1 §4 и S2 фиксируют сигнатуру фронтенда `Ui::show(&mut self, ctx: &egui::Context, s: &AppState, ev: &[UiEvent], out: &mut Vec<Command>)` (и `DebugUi::show` в S1-13 так же). Она написана под API egui ≤ 0.35, где панели верхнего уровня рисовались прямо от `Context` (`CentralPanel::show(ctx, …)`).

В egui/eframe 0.36 (версия из S1 §3) так нельзя: eframe сам создаёт корневую область окна и передаёт её в `App::ui(&mut self, ui: &mut egui::Ui, …)`, а `CentralPanel`/`Panel` рисуются внутри переданного `Ui`. Сохранить `ctx` можно только ручной сборкой корневого `Ui` — дублем того, что делает eframe.

## Решение

Фронтенды (`urfin_ui::DebugUi` сейчас, `Ui` S2 потом) получают корневой `Ui` от eframe:

```rust
pub fn show(&mut self, root: &mut egui::Ui, s: &AppState, ev: &[UiEvent], out: &mut Vec<Command>);
```

`Context` доступен как `root.ctx()` (ввод, шрифты, `request_repaint*`). Остальной контракт (только чтение `&AppState`, события `UiEvent`, действия командами в `out`) не меняется.

## Последствия

- Для пользователя ничего не меняется.
- S2 пишет UI внутри переданного `Ui`; в тексте S1 §4 и S2 параметр `ctx` читать как `root: &mut egui::Ui`.
- `App::logic` (до `ui`) остаётся местом для pump Core и работы с `Context` без рисования.

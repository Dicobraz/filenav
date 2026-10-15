# ADR-0010: поле поколения называется `generation`

Статус: принято (владелец, 2026-10-15, при S1-06).

## Контекст

Спеки называют поле и параметр поколения `gen: Gen`: `Listing.gen` (S1 §2.4), `LoadReq.gen`, `ArrangeReq.gen`, `listing_msg(tab, gen, …)` (S1 §2.6, S4), `CellKey.gen` (S2). В Rust edition 2024 `gen` — зарезервированное слово: так поле не компилируется, нужен `r#gen`.

## Решение

Во всех контрактах поле и параметр поколения называются `generation`; тип остаётся `Gen`.

| Было (спеки) | Стало |
|---|---|
| `Listing.gen` | `Listing.generation` |
| `LoadReq.gen`, `ArrangeReq.gen` | `.generation` |
| `listing_msg(tab, gen, u)` | `listing_msg(tab, generation, u)` |
| `CellKey { tab, gen, … }` (S2) | `CellKey { tab, generation, … }` |

В прозе спек «gen» (политика gen/cancel, «старые gen отброшены») по-прежнему означает поколение. Тексты спек не правятся: этот ADR имеет приоритет.

## Последствия

- Никаких `r#gen` в коде S1/S2/S4/S8.
- Сессии, которые реализуют перечисленные типы, используют новое имя.

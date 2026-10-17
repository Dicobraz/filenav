# Urfin

Быстрый двухпанельный файловый менеджер: функции и раскладка DN/Far, визуальный язык Windows Fluent.
Rust, egui/eframe (wgpu, ADR-0012). Активная платформа — Windows; код кроссплатформенный, macOS и Linux собираются в CI.

Статус: веха M0, каркас. Пока ничего пользовательского нет.

## Сборка

Нужен Rust из `rust-toolchain.toml` (rustup подтянет его сам).

```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo run -p urfin-app
```

## Документы

- [docs/00-ARCHITECTURE.md](docs/00-ARCHITECTURE.md) — архитектура и крейты.
- [docs/01-ROADMAP.md](docs/01-ROADMAP.md) — вехи.
- [docs/specs/](docs/specs/) — спецификации сессий S0–S9.
- [docs/adr/](docs/adr/) — архитектурные решения.

## Лицензия

Под любой из лицензий на ваш выбор:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE));
- MIT ([LICENSE-MIT](LICENSE-MIT)).

Любой вклад, намеренно отправленный в проект, лицензируется так же, без дополнительных условий.

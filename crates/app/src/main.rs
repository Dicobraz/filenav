//! FileNav — бинарь `filenav`.
//!
//! Заглушка S1-01: печатает версию. Окно eframe и цикл кадра — S1-13.
//!
//! `--smoke` — headless-прогон для CI (S1 §3): выход с кодом 0. Пока core
//! пуст, прогон сводится к проверке, что бинарь запускается.

/// Версия приложения из `Cargo.toml`.
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() {
    println!("filenav {}", version());
    if std::env::args().skip(1).any(|a| a == "--smoke") {
        println!("smoke ok");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}

//! FileNav — бинарь `filenav`.
//!
//! Заглушка S1-01: печатает версию. Окно eframe и цикл кадра — S1-13.

/// Версия приложения из `Cargo.toml`.
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn main() {
    println!("filenav {}", version());
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}

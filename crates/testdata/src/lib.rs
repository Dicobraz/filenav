//! Urfin — генераторы тестовых данных. Только `[dev-dependencies]`, в граф `urfin-app` не входит.
//!
//! Заглушка S1-01, владелец — S9.

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }
}

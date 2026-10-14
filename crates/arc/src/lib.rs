//! FileNav — архивы как VFS.
//!
//! Заглушка S1-01, владелец — S8.

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert!(!super::version().is_empty());
    }
}

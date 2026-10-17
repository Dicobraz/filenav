//! FileNav — VFS (S4): адрес `Location`, trait `Vfs`, реестр backend'ов,
//! локальная ФС. Листинг для core (`ListingService`), сортировка, watcher и кэш —
//! S4-03 и дальше.

mod api;
mod local;
pub mod name;
pub mod path;

pub use api::*;
pub use local::LocalFs;

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

//! Urfin — VFS (S4): адрес `Location`, trait `Vfs`, реестр backend'ов,
//! локальная ФС, листинг для core (`VfsListing`), сортировка, иконки. Watcher и
//! кэш — S4-12…16.

mod api;
pub mod batch;
pub mod diag;
pub mod icons;
mod local;
pub mod name;
pub mod path;
pub mod service;
pub mod sort;
mod sys;
pub mod volume;

pub use api::*;
pub use local::{ListMode, LocalFs, ROOTS_TIMEOUT};
pub use service::{VfsCfg, VfsListing};

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

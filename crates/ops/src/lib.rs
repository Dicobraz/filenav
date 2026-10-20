//! Urfin — движок файловых операций (S5): копирование, перенос, удаление,
//! создание каталогов, переименование; очередь, прогресс, немодальные конфликты.
//!
//! S5-01 — типы и команды. Движок job, обход и сами операции — S5-03…S5-12
//! (`docs/TASKS-M2.md`).

pub mod api;
pub mod config;
pub mod dest;
// Подключается к командам в S5-06…S5-10; до тех пор — только тесты.
#[cfg_attr(not(test), allow(dead_code))]
mod engine;
mod handler;
pub mod names;
pub mod ops;
pub mod sys;

pub use api::{
    AttrChange, CloudPolicy, CompareMode, ConflictPolicy, DestMode, DirPolicy, Durability, Engine,
    EngineOverride, ErrorPolicy, FailedItem, HashAlgo, IoClass, Issue, IssueId, IssueKind,
    ItemMeta, ItemOp, LinkKind, LinkMode, OpKind, OpOptions, OpRequest, Preserve, Priority,
    QueueMode, RenameScheme, RenameSpec, Report, Resolution, SyncSpec, Verify,
};
pub use config::{DeviceJobs, OpsConfig};
pub use dest::{DestError, parse_dest, resolve_dest};
pub use handler::{FileHandler, OpsHandler};

use urfin_core::Core;

/// Регистрирует handlers `ops` и `file`. Конфиг — `config.section("ops")`.
pub fn register(core: &mut Core, cfg: OpsConfig) {
    core.register(Box::new(OpsHandler::new(cfg)));
    core.register(Box::new(FileHandler));
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_smoke() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
        assert_ne!(super::version(), "");
    }

    /// Запрос переживает serde туда и обратно (журнал, S5-12).
    #[test]
    fn request_roundtrip() {
        let base = urfin_core::Location::local(if cfg!(windows) { r"C:\a" } else { "/a" });
        let req = OpRequest {
            kind: OpKind::Rename { to: "b.txt".into() },
            base: base.clone(),
            items: vec!["a.txt".into()],
            dest: Some(base),
            opts: OpOptions {
                conflict: ConflictPolicy::RenameNew(RenameScheme::Counter),
                verify: Verify::Hash(HashAlgo::Xxh3_128),
                preserve: Preserve::TIMES,
                ..OpOptions::default()
            },
        };
        let s = toml::to_string(&req).unwrap();
        let back: OpRequest = toml::from_str(&s).unwrap();
        assert_eq!(back, req);
    }

    /// Секция `[ops]` по умолчанию — снапшот (изменение умолчаний видно в ревью).
    #[test]
    fn default_config_snapshot() {
        insta::assert_snapshot!(toml::to_string(&OpsConfig::default()).unwrap());
    }

    /// Пустая и частичная секция дают умолчания (`#[serde(default)]`).
    #[test]
    fn partial_config() {
        let c: OpsConfig = toml::from_str("max_threads = 8\n[device_jobs]\nhdd = 2").unwrap();
        assert_eq!(c.max_threads, 8);
        assert_eq!(c.device_jobs.hdd, 2);
        assert_eq!(c.device_jobs.ssd, 3);
        assert!(c.delete_to_trash);
    }
}

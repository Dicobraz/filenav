//! FileNav — локализация (Fluent, EN+RU; S2 §7). Листовой крейт: не зависит от других
//! `filenav-*`.
//!
//! Глобальное состояние — активный [`I18n`] за `RwLock<Arc<_>>`: [`current`] даёт снимок,
//! [`set_language`] подменяет его целиком, [`epoch`] растёт при каждой смене (сброс
//! кэшей текста). I/O — только в [`init`] (чтение `config_dir/i18n/`).

mod bundle;
pub mod fmt;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

pub use bundle::{Catalog, FALLBACK, I18n, I18nWarning};
pub use fluent_bundle::{FluentArgs, FluentValue};
pub use unic_langid::LanguageIdentifier;

struct Global {
    catalog: Arc<Catalog>,
    active: Arc<I18n>,
}

/// До [`init`] — вшитые ресурсы, en.
static GLOBAL: LazyLock<RwLock<Global>> = LazyLock::new(|| {
    let catalog = Catalog::embedded();
    let (active, _) = catalog.build(&FALLBACK.parse().expect("fallback tag"));
    RwLock::new(Global {
        catalog: Arc::new(catalog),
        active: Arc::new(active),
    })
});

static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Загружает вшитые ресурсы и `config_dir/i18n/<lang>/*.ftl`, включает `language`
/// (BCP-47 или `auto` — язык ОС).
pub fn init(language: &str, config_dir: Option<&Path>) -> Vec<I18nWarning> {
    let (catalog, mut warnings) = Catalog::load(config_dir);
    warnings.extend(activate(Arc::new(catalog), language));
    warnings
}

/// Меняет язык на лету (без I/O). Потокобезопасно: читатели видят старый или новый язык
/// целиком.
pub fn set_language(language: &str) -> Vec<I18nWarning> {
    let catalog = Arc::clone(&read().catalog);
    activate(catalog, language)
}

fn activate(catalog: Arc<Catalog>, language: &str) -> Vec<I18nWarning> {
    let (lang, unknown) = catalog.resolve(language);
    let (active, mut warnings) = catalog.build(&lang);
    warnings.extend(unknown);
    for w in &warnings {
        tracing::warn!(%w, "i18n");
    }
    {
        let mut global = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
        global.catalog = catalog;
        global.active = Arc::new(active);
        EPOCH.fetch_add(1, Ordering::Release);
    }
    warnings
}

fn read() -> std::sync::RwLockReadGuard<'static, Global> {
    GLOBAL.read().unwrap_or_else(PoisonError::into_inner)
}

/// Активный язык (снимок).
pub fn current() -> Arc<I18n> {
    Arc::clone(&read().active)
}

/// Номер смены языка: кэши текста сравнивают его со своим.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Языки с ресурсами (вшитые и из `config_dir`).
pub fn available() -> Vec<LanguageIdentifier> {
    read().catalog.available()
}

/// Текст по ключу активного языка: `tr!("ui-key")`, `tr!("ui-key", count = 3, bytes = s)`.
/// Числа в аргументах форматируются по языку и выбирают плюраль.
#[macro_export]
macro_rules! tr {
    ($key:expr $(,)?) => {
        $crate::current().get($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = $crate::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::current().fmt($key, &args)
    }};
}

/// Версия крейта из `Cargo.toml`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

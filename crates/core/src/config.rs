//! `config.toml` (S1 §2.8, ADR-0005 «Изменения задач»): пользователь хозяин файла,
//! приложение только читает. Ошибки не мешают старту: дефолты + предупреждения.

use std::fmt;
use std::fs;
use std::io;

use compact_str::CompactString;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::paths::{Paths, write_atomic};
use crate::runtime::RuntimeConfig;
use crate::tab::{SortSpec, ViewMode};

pub const CONFIG_VERSION: u32 = 1;

/// Миграция сырого конфига с версии `.0` на следующую.
pub(crate) type Migration = (u32, fn(&mut toml::Table) -> Result<(), String>);

/// v1 — первая версия: миграций пока нет.
static MIGRATIONS: &[Migration] = &[];

/// Секции core, остальные (`[ui]`, `[keymap]`, `[vfs]`, …) — в `ext`.
const CORE_SECTIONS: [&str; 4] = ["general", "panels", "runtime", "log"];

/// Настройки приложения.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub panels: PanelsCfg,
    pub runtime: RuntimeConfig,
    pub log: LogCfg,
    /// Секции других крейтов, разбираются лениво через [`Config::section`].
    #[serde(flatten)]
    pub ext: toml::Table,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            general: General::default(),
            panels: PanelsCfg::default(),
            runtime: RuntimeConfig::default(),
            log: LogCfg::default(),
            ext: toml::Table::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// Восстанавливать табы и пути прошлого запуска (решение владельца №2).
    pub restore_session: bool,
    pub show_hidden: bool,
    /// `"auto"` (язык ОС) или тег BCP-47: `"en"`, `"ru"`, `"pt-BR"`.
    pub language: CompactString,
}

impl Default for General {
    fn default() -> Self {
        General {
            restore_session: true,
            show_hidden: false,
            language: "auto".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelsCfg {
    pub sort: SortSpec,
    pub view_mode: ViewMode,
    pub history_cap: u16,
    pub visited_cap: u16,
}

impl Default for PanelsCfg {
    fn default() -> Self {
        PanelsCfg {
            sort: SortSpec::default(),
            view_mode: ViewMode::Full,
            history_cap: 100,
            visited_cap: 200,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogCfg {
    /// `error | warn | info | debug | trace` или фильтр `tracing` (`URFIN_LOG` важнее).
    pub level: CompactString,
    /// Сколько дневных файлов лога хранить.
    pub keep_files: u16,
}

impl Default for LogCfg {
    fn default() -> Self {
        LogCfg {
            level: "info".into(),
            keep_files: 7,
        }
    }
}

/// Проблема конфига; показывается баннером.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigWarning {
    /// Ключ или секция (`general.language`, `ui`), пусто — файл целиком.
    pub key: String,
    pub msg: String,
}

impl ConfigWarning {
    fn new(key: impl Into<String>, msg: impl fmt::Display) -> Self {
        ConfigWarning {
            key: key.into(),
            msg: msg.to_string(),
        }
    }
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            f.write_str(&self.msg)
        } else {
            write!(f, "{}: {}", self.key, self.msg)
        }
    }
}

/// Шаблон `config.toml` по умолчанию; разбирается ровно в `Config::default()`.
pub const DEFAULT_TEMPLATE: &str = r#"# Urfin settings. The app only reads this file; edit it and run
# "Reload configuration" (app.reload_config) or restart.
# Unknown sections are kept for other components ([ui], [keymap], [vfs], [ops], [term], [viewer]).

version = 1

[general]
# Reopen last tabs and folders on start.
restore_session = true
# Show hidden and system files in new tabs.
show_hidden = false
# "auto" follows the OS language; otherwise a BCP-47 tag such as "en" or "ru".
language = "auto"

[panels]
# Column set for new tabs: "brief" | "medium" | "full" | "wide".
view_mode = "full"
# Back/forward steps kept per tab.
history_cap = 100
# Folders remembered in the folder history (Alt+F12).
visited_cap = 200

[panels.sort]
# "name" | "ext" | "size" | "mtime" | "unsorted".
key = "name"
desc = false
dirs_first = true

[runtime]
# Threads for listing folders and for short file-operation steps.
interactive_threads = 4
bulk_threads = 2
# Threads for sorting and hashing; 0 = number of CPU cores minus one.
cpu_threads = 0
msg_capacity = 1024

[log]
# error | warn | info | debug | trace. The URFIN_LOG environment variable wins.
level = "info"
# Daily log files to keep.
keep_files = 7
"#;

impl Config {
    /// Никогда не падает: ошибки → дефолты + предупреждения (показываются баннером).
    ///
    /// - нет файла — дефолты без предупреждений;
    /// - битый TOML — дефолты, предупреждение, файл не трогается;
    /// - старая `version` — миграции, бэкап `config.toml.v{N}.bak`, перезапись
    ///   (комментарии при перезаписи теряются; запись с сохранением — S2);
    /// - будущая `version` — чтение как есть, предупреждение, файл не трогается;
    /// - ошибка в секции core — дефолт этой секции и предупреждение.
    ///
    /// Файл без `version` считается текущей версии.
    pub fn load(paths: &Paths) -> (Config, Vec<ConfigWarning>) {
        load_with(paths, MIGRATIONS, CONFIG_VERSION)
    }

    /// Секция другого крейта из `ext`. Нет секции — `T::default()`; ошибка —
    /// `T::default()` и предупреждение.
    pub fn section<T: DeserializeOwned + Default>(&self, name: &str) -> (T, Option<ConfigWarning>) {
        match self.ext.get(name) {
            None => (T::default(), None),
            Some(v) => match v.clone().try_into() {
                Ok(t) => (t, None),
                Err(e) => (T::default(), Some(ConfigWarning::new(name, e))),
            },
        }
    }

    /// Записать шаблон с комментариями, если файла нет. `Ok(true)` — записан.
    pub fn write_default_if_missing(paths: &Paths) -> io::Result<bool> {
        let file = paths.config_file();
        if file.exists() {
            return Ok(false);
        }
        write_atomic(&file, DEFAULT_TEMPLATE.as_bytes())?;
        Ok(true)
    }
}

pub(crate) fn load_with(
    paths: &Paths,
    migrations: &[Migration],
    current: u32,
) -> (Config, Vec<ConfigWarning>) {
    let file = paths.config_file();
    let mut warnings = Vec::new();
    let text = match fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Config::default(), warnings),
        Err(e) => {
            warnings.push(ConfigWarning::new("", format!("{}: {e}", file.display())));
            return (Config::default(), warnings);
        }
    };
    let mut table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            warnings.push(ConfigWarning::new("", e.message()));
            return (Config::default(), warnings);
        }
    };
    let version = table.get("version").map_or(current, |v| {
        v.as_integer()
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or_else(|| {
                warnings.push(ConfigWarning::new(
                    "version",
                    "expected a non-negative integer",
                ));
                current
            })
    });
    if version > current {
        warnings.push(ConfigWarning::new(
            "version",
            format!("version {version} is newer than {current}: read as is, file left untouched"),
        ));
    } else if version < current {
        migrate(
            &file,
            &text,
            &mut table,
            version,
            migrations,
            current,
            &mut warnings,
        );
    }
    (typed(table, current, &mut warnings), warnings)
}

/// Миграции `from → current`; при успехе — бэкап и перезапись.
fn migrate(
    file: &std::path::Path,
    original: &str,
    table: &mut toml::Table,
    from: u32,
    migrations: &[Migration],
    current: u32,
    warnings: &mut Vec<ConfigWarning>,
) {
    let mut migrated = table.clone();
    for v in from..current {
        let Some((_, step)) = migrations.iter().find(|(f, _)| *f == v) else {
            warnings.push(ConfigWarning::new(
                "version",
                format!("no migration from version {v}"),
            ));
            return;
        };
        if let Err(e) = step(&mut migrated) {
            warnings.push(ConfigWarning::new(
                "version",
                format!("migration from {v}: {e}"),
            ));
            return;
        }
    }
    migrated.insert("version".into(), toml::Value::Integer(current.into()));
    *table = migrated;
    let backup = file.with_file_name(format!(
        "{}.v{from}.bak",
        file.file_name()
            .map_or_else(|| "config.toml".into(), |n| n.to_string_lossy())
    ));
    let written = fs::write(&backup, original).and_then(|()| {
        let text = toml::to_string(table).map_err(io::Error::other)?;
        write_atomic(file, text.as_bytes())
    });
    if let Err(e) = written {
        warnings.push(ConfigWarning::new(
            "",
            format!("cannot save migrated config: {e}"),
        ));
    }
}

/// Таблица → `Config` по секциям: ошибка в одной не губит остальные.
fn typed(mut table: toml::Table, current: u32, warnings: &mut Vec<ConfigWarning>) -> Config {
    fn take<T: DeserializeOwned + Default>(
        t: &mut toml::Table,
        name: &str,
        warnings: &mut Vec<ConfigWarning>,
    ) -> T {
        match t.remove(name) {
            None => T::default(),
            Some(v) => v.try_into().unwrap_or_else(|e| {
                warnings.push(ConfigWarning::new(name, e));
                T::default()
            }),
        }
    }
    let version = table
        .remove("version")
        .and_then(|v| v.as_integer())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(current);
    let mut general: General = take(&mut table, "general", warnings);
    if !valid_language(&general.language) {
        warnings.push(ConfigWarning::new(
            "general.language",
            format!("'{}' is not \"auto\" or a BCP-47 tag", general.language),
        ));
        general.language = "auto".into();
    }
    let panels = take(&mut table, "panels", warnings);
    let runtime = take(&mut table, "runtime", warnings);
    let log = take(&mut table, "log", warnings);
    debug_assert!(CORE_SECTIONS.iter().all(|s| !table.contains_key(*s)));
    Config {
        version,
        general,
        panels,
        runtime,
        log,
        ext: table,
    }
}

/// `"auto"` или синтаксически корректный тег BCP-47 (язык 2–3 или 5–8 букв,
/// подтеги 1–8 букв и цифр).
pub(crate) fn valid_language(s: &str) -> bool {
    if s == "auto" {
        return true;
    }
    let mut parts = s.split('-');
    let lang_ok = parts.next().is_some_and(|l| {
        matches!(l.len(), 2 | 3 | 5..=8) && l.bytes().all(|b| b.is_ascii_alphabetic())
    });
    lang_ok
        && s.len() <= 35
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::{
        CONFIG_VERSION, Config, ConfigWarning, DEFAULT_TEMPLATE, Migration, load_with,
        valid_language,
    };
    use crate::paths::Paths;
    use crate::tab::{SortKey, ViewMode};
    use std::fs;

    fn paths() -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let p = Paths::in_dir(tmp.path());
        (tmp, p)
    }

    #[test]
    fn missing_file_gives_defaults_silently() {
        let (_t, p) = paths();
        assert_eq!(Config::load(&p), (Config::default(), vec![]));
    }

    #[test]
    fn broken_toml_gives_defaults_warning_untouched_file() {
        let (_t, p) = paths();
        let bad = "version = = 1\n[general\n";
        fs::write(p.config_file(), bad).unwrap();
        let (c, w) = Config::load(&p);
        assert_eq!(c, Config::default());
        assert_eq!(w.len(), 1);
        assert_eq!(fs::read_to_string(p.config_file()).unwrap(), bad);
    }

    #[test]
    fn unknown_sections_kept_in_ext_and_sections_typed() {
        #[derive(serde::Deserialize, Default, PartialEq, Debug)]
        #[serde(default)]
        struct UiCfg {
            font_size: u32,
        }
        let (_t, p) = paths();
        fs::write(
            p.config_file(),
            "[general]\nshow_hidden = true\n[ui]\nfont_size = 14\n[keymap]\npreset = \"far\"\n[vfs.cache]\nsize = 1\n",
        )
        .unwrap();
        let (c, w) = Config::load(&p);
        assert_eq!(w, vec![]);
        assert!(c.general.show_hidden);
        let mut keys: Vec<&str> = c.ext.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["keymap", "ui", "vfs"]);
        assert_eq!(c.section::<UiCfg>("ui"), (UiCfg { font_size: 14 }, None));
        assert_eq!(c.section::<UiCfg>("ops"), (UiCfg::default(), None));
        let (bad, warn) = c.section::<UiCfg>("keymap");
        assert_eq!(bad, UiCfg::default());
        assert!(warn.is_none(), "unknown keys are ignored");
        fs::write(p.config_file(), "[ui]\nfont_size = \"big\"\n").unwrap();
        let (c, _) = Config::load(&p);
        let (v, warn) = c.section::<UiCfg>("ui");
        assert_eq!(v, UiCfg::default());
        assert_eq!(warn.map(|w| w.key), Some("ui".to_owned()));
    }

    #[test]
    fn bad_core_section_falls_back_alone() {
        let (_t, p) = paths();
        fs::write(
            p.config_file(),
            "[general]\nrestore_session = \"yes\"\n[panels]\nview_mode = \"brief\"\n",
        )
        .unwrap();
        let (c, w) = Config::load(&p);
        assert_eq!(c.general, super::General::default());
        assert_eq!(c.panels.view_mode, ViewMode::Brief);
        // Частичная вложенная секция дополняется дефолтами.
        fs::write(
            p.config_file(),
            "[panels.sort]
key = \"size\"
",
        )
        .unwrap();
        let (c, w2) = Config::load(&p);
        assert_eq!(w2, vec![]);
        assert_eq!(c.panels.sort.key, SortKey::Size);
        assert!(c.panels.sort.dirs_first);
        assert_eq!(
            w.iter().map(|w| w.key.as_str()).collect::<Vec<_>>(),
            ["general"]
        );
    }

    #[test]
    fn language_is_bcp47() {
        for ok in [
            "auto",
            "en",
            "ru",
            "pt-BR",
            "zh-Hans-CN",
            "sr-Latn",
            "es-419",
        ] {
            assert!(valid_language(ok), "{ok}");
        }
        for bad in ["", "english!", "e", "ru_RU", "en--US", "toolonglanguage"] {
            assert!(!valid_language(bad), "{bad}");
        }
        let (_t, p) = paths();
        fs::write(p.config_file(), "[general]\nlanguage = \"ru_RU\"\n").unwrap();
        let (c, w) = Config::load(&p);
        assert_eq!(c.general.language, "auto");
        assert_eq!(w[0].key, "general.language");
    }

    #[test]
    fn migration_v0_to_v1_renames_key_and_backs_up() {
        fn rename_hidden(t: &mut toml::Table) -> Result<(), String> {
            let g = t
                .get_mut("general")
                .and_then(toml::Value::as_table_mut)
                .ok_or("no [general]")?;
            if let Some(v) = g.remove("hidden") {
                g.insert("show_hidden".into(), v);
            }
            Ok(())
        }
        static TEST: &[Migration] = &[(0, rename_hidden)];
        let (_t, p) = paths();
        let original = "version = 0\n[general]\nhidden = true\n";
        fs::write(p.config_file(), original).unwrap();
        let (c, w) = load_with(&p, TEST, 1);
        assert_eq!(w, vec![]);
        assert!(c.general.show_hidden);
        assert_eq!(c.version, 1);
        let backup = p.config_dir.join("config.toml.v0.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), original);
        let rewritten = fs::read_to_string(p.config_file()).unwrap();
        assert!(rewritten.contains("version = 1"), "{rewritten}");
        assert!(rewritten.contains("show_hidden = true"), "{rewritten}");
        assert!(!rewritten.contains("hidden = true\n[") && !rewritten.contains("\nhidden"));
        // Повторная загрузка — уже без миграции.
        assert_eq!(load_with(&p, TEST, 1).1, vec![]);
    }

    #[test]
    fn failed_migration_reads_best_effort_without_writing() {
        static FAIL: &[Migration] = &[(0, |_| Err("boom".into()))];
        let (_t, p) = paths();
        let original = "version = 0\n[general]\nshow_hidden = true\n";
        fs::write(p.config_file(), original).unwrap();
        let (c, w) = load_with(&p, FAIL, 1);
        assert!(c.general.show_hidden);
        assert_eq!(w.len(), 1);
        assert_eq!(fs::read_to_string(p.config_file()).unwrap(), original);
        assert!(!p.config_dir.join("config.toml.v0.bak").exists());
    }

    #[test]
    fn future_version_is_read_only() {
        let (_t, p) = paths();
        let text = "version = 99\n[panels]\nview_mode = \"wide\"\n[future]\nx = 1\n";
        fs::write(p.config_file(), text).unwrap();
        let (c, w) = Config::load(&p);
        assert_eq!(c.panels.view_mode, ViewMode::Wide);
        assert_eq!(c.version, 99);
        assert!(c.ext.contains_key("future"));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].key, "version");
        assert_eq!(fs::read_to_string(p.config_file()).unwrap(), text);
        assert_eq!(fs::read_dir(&p.config_dir).unwrap().count(), 1);
    }

    #[test]
    fn template_is_defaults_and_written_once() {
        let parsed: Config = toml::from_str(DEFAULT_TEMPLATE).unwrap();
        assert_eq!(parsed, Config::default());
        assert_eq!(parsed.version, CONFIG_VERSION);
        let (_t, p) = paths();
        assert!(Config::write_default_if_missing(&p).unwrap());
        assert!(!Config::write_default_if_missing(&p).unwrap());
        assert_eq!(Config::load(&p), (Config::default(), vec![]));
        insta::assert_snapshot!("config_template", DEFAULT_TEMPLATE);
    }

    #[test]
    fn defaults() {
        let c = Config::default();
        assert!(c.general.restore_session);
        assert_eq!(c.general.language, "auto");
        assert_eq!((c.panels.history_cap, c.panels.visited_cap), (100, 200));
        assert_eq!(c.panels.sort.key, SortKey::Name);
        assert_eq!((c.log.level.as_str(), c.log.keep_files), ("info", 7));
        let w = ConfigWarning {
            key: "general.language".into(),
            msg: "bad".into(),
        };
        assert_eq!(w.to_string(), "general.language: bad");
    }
}

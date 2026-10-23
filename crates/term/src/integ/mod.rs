//! Shell integration (S6 §5, S6-11…S6-13): как запустить shell, чтобы он размечал
//! промпт маркерами 633 и сообщал cwd. Скрипты вшиты; bash, zsh и fish читают их с
//! диска — [`ensure_files`] пишет их в `state_dir/term/shell/<версия>/` (в задаче
//! запуска, не в UI-потоке). PowerShell получает скрипт в `-EncodedCommand`, cmd —
//! в переменной `PROMPT`: им файлы не нужны.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use crate::shells::{PathStyle, ShellKind, ShellSpec};

const PS1: &str = include_str!("urfin.ps1");
const BASH: &str = include_str!("urfin.bash");
const ZSH: &str = include_str!("urfin.zsh");
const ZSHRC: &str = include_str!("zshrc");
const FISH: &str = include_str!("urfin.fish");
const BOOT: &str = include_str!("boot.sh");

/// Файлы каталога скриптов: путь относительно него и текст.
const FILES: [(&str, &str); 8] = [
    ("urfin.bash", BASH),
    ("urfin.zsh", ZSH),
    ("urfin.fish", FISH),
    ("boot.sh", BOOT),
    ("zsh/.zshenv", ZSHRC),
    ("zsh/.zprofile", ZSHRC),
    ("zsh/.zshrc", ZSHRC),
    ("zsh/.zlogin", ZSHRC),
];

/// Как запустить shell с интеграцией.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Launch {
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    /// Нужен каталог скриптов на диске.
    pub needs_files: bool,
}

/// Каталоги интеграции сеанса.
#[derive(Clone, Debug)]
pub(crate) struct Dirs {
    /// `state_dir/term/shell/<версия>`.
    pub shell: PathBuf,
    /// Файл тихого cd сеанса: `state_dir/term/run/<сеанс>/cwd`.
    pub cwd_file: PathBuf,
}

impl Dirs {
    pub fn new(term_dir: &Path, session: u32) -> Dirs {
        Dirs {
            shell: term_dir.join("shell").join(crate::version()),
            cwd_file: term_dir.join("run").join(session.to_string()).join("cwd"),
        }
    }
}

/// Запуск `spec` с интеграцией; `None` — без неё (выключена, вид без поддержки, нет
/// каталога для скриптов). `nonce` — секрет сеанса (cmd получает его в `PROMPT`).
pub(crate) fn launch(spec: &ShellSpec, dirs: Option<&Dirs>, nonce: &str) -> Option<Launch> {
    if !spec.integration {
        return None;
    }
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    if let Some(d) = dirs {
        env.push((
            "URFIN_CWD_FILE".into(),
            shell_path(&d.cwd_file, spec).into(),
        ));
    }
    let mut args: Vec<OsString> = spec.args.iter().map(OsString::from).collect();
    let needs_files = match &spec.kind {
        ShellKind::Pwsh | ShellKind::WinPowerShell => {
            args.extend([
                "-NoExit".into(),
                "-EncodedCommand".into(),
                encode_ps(PS1).into(),
            ]);
            false
        }
        ShellKind::Cmd => {
            let user = std::env::var("PROMPT").unwrap_or_else(|_| "$P$G".into());
            env.push(("PROMPT".into(), cmd_prompt(&user, nonce).into()));
            false
        }
        ShellKind::Bash => {
            let d = dirs?;
            args = vec![
                "--rcfile".into(),
                shell_path(&d.shell.join("urfin.bash"), spec).into(),
                "-i".into(),
            ];
            true
        }
        ShellKind::Wsl { distro } => {
            let d = dirs?;
            args = vec![
                "-d".into(),
                distro.as_str().into(),
                "-e".into(),
                "sh".into(),
                "-c".into(),
                ". \"$URFIN_SHELL_DIR/boot.sh\"".into(),
            ];
            env.push(("URFIN_SHELL_DIR".into(), d.shell.clone().into()));
            let mut wslenv = std::env::var("WSLENV").unwrap_or_default();
            if !wslenv.is_empty() {
                wslenv.push(':');
            }
            wslenv.push_str("URFIN_SHELL_DIR/p:URFIN_CWD_FILE/p:URFIN_NONCE:URFIN_SESSION");
            env.push(("WSLENV".into(), wslenv.into()));
            true
        }
        // zsh и fish на Windows — только в WSL (S0 №57); свои — без инъекции.
        ShellKind::Zsh | ShellKind::Fish | ShellKind::Other => return None,
    };
    Some(Launch {
        args,
        env,
        needs_files,
    })
}

/// Путь для shell: MSYS (Git Bash) понимает `C:/x`; WSL переводит сам (`WSLENV /p`).
fn shell_path(p: &Path, spec: &ShellSpec) -> String {
    let s = p.display().to_string();
    match spec.path_style {
        PathStyle::Posix => s.replace('\\', "/"),
        _ => s,
    }
}

/// `PROMPT` cmd с маркерами (S6 §5): `D` → `A` → `FnCwd` (`$P`) → промпт
/// пользователя → `B`. `PROMPT` раскрывает только свои `$`-коды, не `%VAR%`, поэтому
/// секрет вписывается в строку при запуске.
fn cmd_prompt(user: &str, nonce: &str) -> String {
    format!("$e]633;D$e\\$e]633;A$e\\$e]633;P;FnCwd={nonce};$P$e\\{user}$e]633;B$e\\")
}

/// Скрипт для `-EncodedCommand`: UTF-16LE в base64.
fn encode_ps(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Записать скрипты в каталог, если их там нет или они другие (атомарно: во
/// временный файл и переименованием). Переводы строк — LF.
pub(crate) fn ensure_files(dir: &Path) -> std::io::Result<()> {
    for (rel, text) in FILES {
        let path = dir.join(rel);
        let text = text.replace("\r\n", "\n");
        if std::fs::read(&path).is_ok_and(|b| b == text.as_bytes()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp~");
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(())
}

/// Путь панели → путь для shell (файл тихого cd): WSL — `/mnt/c/x` или путь внутри
/// дистрибутива из `\\wsl.localhost\<D>\x`; Git Bash — `C:/x`; прочие — как есть.
pub fn to_shell(path: &Path, style: &PathStyle, mount_root: Option<&str>) -> String {
    let s = path.display().to_string();
    match style {
        PathStyle::Windows => s,
        PathStyle::Posix => s.replace('\\', "/"),
        PathStyle::Wsl {
            distro,
            mount_root: root,
        } => {
            let root = mount_root.unwrap_or(root);
            for prefix in [r"\\wsl.localhost\", r"\\wsl$\"] {
                if let Some(rest) = s.strip_prefix(prefix)
                    && let Some((d, tail)) = rest.split_once('\\')
                    && d.eq_ignore_ascii_case(distro)
                {
                    return format!("/{}", tail.replace('\\', "/"));
                }
            }
            let b = s.as_bytes();
            if b.len() >= 2 && b[1] == b':' {
                return format!(
                    "{root}{}{}",
                    s[..1].to_ascii_lowercase(),
                    s[2..].replace('\\', "/")
                );
            }
            s
        }
    }
}

/// Путь от shell → путь Windows для панели (S6 §5.1): WSL `/mnt/c/x` → `C:\x`, прочее
/// — `\\wsl.localhost\<D>\x`; Git Bash `/c/x` → `C:\x`. `None` — не отобразить.
pub fn to_windows(path: &str, style: &PathStyle, mount_root: Option<&str>) -> Option<PathBuf> {
    let drive = |rest: &str| -> Option<PathBuf> {
        let mut it = rest.splitn(2, '/');
        let d = it.next()?;
        if d.len() != 1 || !d.as_bytes()[0].is_ascii_alphabetic() {
            return None;
        }
        let tail = it.next().unwrap_or("").replace('/', "\\");
        Some(PathBuf::from(format!(
            "{}:\\{tail}",
            d.to_ascii_uppercase()
        )))
    };
    match style {
        PathStyle::Windows => Some(PathBuf::from(path)),
        PathStyle::Posix => {
            if path.len() >= 2 && path.as_bytes()[1] == b':' {
                return Some(PathBuf::from(path.replace('/', "\\")));
            }
            drive(path.strip_prefix('/')?)
        }
        PathStyle::Wsl {
            distro,
            mount_root: root,
        } => {
            let root = mount_root.unwrap_or(root);
            if let Some(rest) = path.strip_prefix(root)
                && let Some(p) = drive(rest)
            {
                return Some(p);
            }
            let tail = path.trim_start_matches('/').replace('/', "\\");
            Some(PathBuf::from(format!(r"\\wsl.localhost\{distro}\{tail}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: ShellKind, style: PathStyle, args: &[&str]) -> ShellSpec {
        ShellSpec {
            id: "x".into(),
            title: "x".into(),
            kind,
            program: "x.exe".into(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            env: Vec::new(),
            integration: true,
            path_style: style,
        }
    }

    #[test]
    fn paths_from_shells() {
        let wsl = PathStyle::Wsl {
            distro: "Ubuntu".into(),
            mount_root: "/mnt/".into(),
        };
        assert_eq!(
            to_windows("/mnt/c/Windows", &wsl, None),
            Some(PathBuf::from(r"C:\Windows"))
        );
        assert_eq!(
            to_windows("/home/u", &wsl, None),
            Some(PathBuf::from(r"\\wsl.localhost\Ubuntu\home\u"))
        );
        assert_eq!(
            to_windows("/c/x/y", &PathStyle::Posix, None),
            Some(PathBuf::from(r"C:\x\y"))
        );
        assert_eq!(to_windows("/usr/bin", &PathStyle::Posix, None), None);
        assert_eq!(
            to_windows("/win/d/x", &wsl, Some("/win/")),
            Some(PathBuf::from(r"D:\x"))
        );
    }

    #[test]
    fn launches() {
        let dirs = Dirs::new(Path::new(r"C:\st\term"), 7);
        let ps = launch(
            &spec(ShellKind::WinPowerShell, PathStyle::Windows, &["-NoLogo"]),
            Some(&dirs),
            "n0nce",
        )
        .unwrap();
        assert_eq!(ps.args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(ps.args[3].to_str().unwrap())
            .unwrap();
        let u16s: Vec<u16> = decoded
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&u16s).unwrap(), PS1);
        assert!(!ps.needs_files);

        let bash = launch(
            &spec(ShellKind::Bash, PathStyle::Posix, &["--login", "-i"]),
            Some(&dirs),
            "n0nce",
        )
        .unwrap();
        assert_eq!(bash.args[0], "--rcfile");
        assert!(bash.args[1].to_str().unwrap().ends_with("/urfin.bash"));
        assert!(!bash.args[1].to_str().unwrap().contains('\\'));
        assert!(launch(&spec(ShellKind::Bash, PathStyle::Posix, &[]), None, "n").is_none());

        let cmd = launch(
            &spec(ShellKind::Cmd, PathStyle::Windows, &[]),
            None,
            "n0nce",
        )
        .unwrap();
        let prompt = cmd.env.iter().find(|(k, _)| k == "PROMPT").unwrap();
        assert!(prompt.1.to_str().unwrap().ends_with("$e]633;B$e\\"));

        let mut off = spec(ShellKind::Cmd, PathStyle::Windows, &[]);
        off.integration = false;
        assert!(launch(&off, None, "n").is_none());
    }

    #[test]
    fn files_are_written_once_with_lf() {
        let d = tempfile::tempdir().unwrap();
        ensure_files(d.path()).unwrap();
        let rc = std::fs::read_to_string(d.path().join("zsh/.zshrc")).unwrap();
        assert!(!rc.contains('\r'));
        let t = std::fs::metadata(d.path().join("urfin.bash"))
            .unwrap()
            .modified()
            .unwrap();
        ensure_files(d.path()).unwrap();
        let t2 = std::fs::metadata(d.path().join("urfin.bash"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(t, t2, "unchanged files are not rewritten");
    }
}

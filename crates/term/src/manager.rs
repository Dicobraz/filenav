//! Менеджер сеансов (S6 §4.2, S6-10): пул сеансов по shells, активный — один,
//! остальные живут в фоне с урезанным скроллбэком.
//!
//! Тяжёлое — не в UI-потоке: поиск shells и запуск процесса (`CreateProcess`,
//! `ConPTY`) идут задачами полосы Interactive, готовый [`Session`] приходит `Msg`.
//! Shell по умолчанию прогревается через [`WARMUP`] после первого pump — уже после
//! первого кадра. Выбор по умолчанию (№52): настройка → статистика выбора → pwsh →
//! cmd.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use compact_str::CompactString;
use urfin_core::{
    Anchor, CancelToken, Choice, CmdError, Cx, Lane, Level, Location, Msg, PromptSpec,
    QuitDecision, Runtime,
};

use crate::TermState;
use crate::config::TermConfig;
use crate::session::{CoreSink, Session, SessionEvent, SessionId, SessionOpts};
use crate::shells::{self, ShellSpec, SystemProbe};
use crate::sys::conpty::{PtySize, SpawnSpec};
use crate::vt::input::{self, Mods};
use crate::vt::shared::TermShared;

/// Через сколько после первого pump прогреть shell по умолчанию.
pub const WARMUP: Duration = Duration::from_millis(500);

/// Размер PTY до первой раскладки окна (S6-08 ресайзит под область терминала).
const START_SIZE: PtySize = PtySize {
    cols: 120,
    rows: 30,
};

/// Состояние слота.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Starting,
    Running,
    /// Процесс вышел; транскрипт остаётся, ввод перезапускает.
    Exited(Option<u32>),
    Failed,
}

/// Сеанс в пуле.
#[derive(Debug)]
pub struct Slot {
    pub id: SessionId,
    pub shell: CompactString,
    pub state: SlotState,
    /// Заголовок от процесса (OSC 0/2).
    pub title: Option<String>,
    session: Option<Session>,
    /// Сетка живёт и после выхода процесса.
    shared: Option<Arc<TermShared>>,
    /// Ввод до старта процесса.
    queued: Vec<u8>,
}

impl Slot {
    pub fn shared(&self) -> Option<&Arc<TermShared>> {
        self.shared.as_ref()
    }

    /// Кроме shell есть процессы (без интеграции — по job object).
    pub fn busy(&self) -> bool {
        self.session.as_ref().is_some_and(Session::busy)
    }
}

/// Отложенный до готовности списка shells запуск.
#[derive(Debug)]
enum Deferred {
    /// `term.shell`/`term.new`: id shell (`None` — по умолчанию), выбор пользователя.
    Start {
        shell: Option<CompactString>,
        reuse: bool,
        cwd: Option<PathBuf>,
    },
}

/// Пул сеансов.
#[derive(Debug, Default)]
pub struct Manager {
    shells: Option<Vec<ShellSpec>>,
    slots: Vec<Slot>,
    active: Option<SessionId>,
    next_id: SessionId,
    /// Сколько раз пользователь выбирал shell (№52), `state_dir/term/usage.toml`.
    usage: BTreeMap<CompactString, u32>,
    term_dir: Option<PathBuf>,
    born: Option<Instant>,
    discovering: bool,
    warmed: bool,
    deferred: Vec<Deferred>,
}

impl Manager {
    pub fn new(term_dir: Option<PathBuf>) -> Manager {
        Manager {
            term_dir,
            next_id: 1,
            ..Manager::default()
        }
    }

    pub fn shells(&self) -> &[ShellSpec] {
        self.shells.as_deref().unwrap_or_default()
    }

    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    pub fn active(&self) -> Option<&Slot> {
        let id = self.active?;
        self.slots.iter().find(|s| s.id == id)
    }

    fn slot_mut(&mut self, id: SessionId) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.id == id)
    }

    /// Shell по умолчанию (№52): `term.default_shell`, иначе чаще всего
    /// выбираемый, иначе pwsh 7, иначе cmd, иначе первый найденный.
    pub fn default_shell(&self, cfg: &TermConfig) -> Option<&ShellSpec> {
        let shells = self.shells();
        let by_id = |id: &str| shells.iter().find(|s| s.id == id);
        if cfg.default_shell != "auto"
            && let Some(s) = by_id(&cfg.default_shell)
        {
            return Some(s);
        }
        let used = shells
            .iter()
            .filter_map(|s| self.usage.get(&s.id).map(|n| (*n, s)))
            .filter(|(n, _)| *n > 0)
            // Равные — первый в списке shells.
            .fold(None::<(u32, &ShellSpec)>, |best, (n, s)| match best {
                Some((b, _)) if b >= n => best,
                _ => Some((n, s)),
            });
        used.map(|(_, s)| s)
            .or_else(|| by_id("pwsh"))
            .or_else(|| by_id("cmd"))
            .or_else(|| shells.first())
    }

    /// Каждый pump: в первый — таймер прогрева, через [`WARMUP`] — поиск shells и старт shell по умолчанию. Без каталога
    /// состояния (тесты, `--smoke`, `--screens`) — ничего: shells ищутся по первой
    /// команде, прогрева нет.
    pub(crate) fn pump(&mut self, cfg: &TermConfig, rt: &Runtime) {
        if self.term_dir.is_none() {
            return;
        }
        let Some(born) = self.born else {
            // Поиск shells — тоже после первого кадра: полоса Interactive нужна
            // листингу на старте.
            self.born = Some(Instant::now());
            rt.waker().wake_after(WARMUP);
            return;
        };
        if born.elapsed() >= WARMUP {
            self.discover(cfg, rt);
        }
        self.maybe_warm(cfg, rt);
    }

    fn discover(&mut self, cfg: &TermConfig, rt: &Runtime) {
        if !self.discovering {
            self.discovering = true;
            spawn_discovery(rt, cfg, self.term_dir.clone());
        }
    }

    fn maybe_warm(&mut self, cfg: &TermConfig, rt: &Runtime) {
        if self.warmed || self.shells.is_none() || self.born.is_none_or(|b| b.elapsed() < WARMUP) {
            return;
        }
        self.warmed = true;
        if self.slots.is_empty()
            && let Some(id) = self.default_shell(cfg).map(|s| s.id.clone())
        {
            let _ = self.start(cfg, rt, &id, None, true);
        }
    }

    /// Список shells и статистика готовы.
    fn on_shells(
        &mut self,
        cfg: &TermConfig,
        rt: &Runtime,
        shells: Vec<ShellSpec>,
        usage: BTreeMap<CompactString, u32>,
    ) -> Vec<String> {
        self.shells = Some(shells);
        for (k, v) in usage {
            *self.usage.entry(k).or_default() += v;
        }
        let mut errors = Vec::new();
        for d in std::mem::take(&mut self.deferred) {
            let Deferred::Start { shell, reuse, cwd } = d;
            if let Err(e) = self.open(cfg, rt, shell.as_deref(), reuse, cwd) {
                errors.push(e.to_string());
            }
        }
        self.maybe_warm(cfg, rt);
        errors
    }

    /// `term.shell` (`reuse`) / `term.new`: сделать активным сеанс shell, запустив
    /// при необходимости. До готовности списка shells — отложить.
    pub(crate) fn open(
        &mut self,
        cfg: &TermConfig,
        rt: &Runtime,
        shell: Option<&str>,
        reuse: bool,
        cwd: Option<PathBuf>,
    ) -> Result<(), CmdError> {
        if self.shells.is_none() {
            self.deferred.push(Deferred::Start {
                shell: shell.map(Into::into),
                reuse,
                cwd,
            });
            self.discover(cfg, rt);
            return Ok(());
        }
        let id: CompactString = match shell {
            Some(s) => s.into(),
            None => self
                .default_shell(cfg)
                .map(|s| s.id.clone())
                .ok_or_else(|| CmdError::Rejected(tr("term-no-shells", &[]).into()))?,
        };
        if !self.shells().iter().any(|s| s.id == id) {
            return Err(arg_err("id", tr("term-unknown-shell", &[("id", &id)])));
        }
        *self.usage.entry(id.clone()).or_default() += 1;
        self.save_usage(rt);
        if reuse && let Some(s) = self.slots.iter().find(|s| s.shell == id) {
            let sid = s.id;
            self.set_active(cfg, sid);
            return Ok(());
        }
        self.start(cfg, rt, &id, cwd, true).map(|_| ())
    }

    /// Запустить сеанс shell в фоне; слот появляется сразу (`Starting`).
    fn start(
        &mut self,
        cfg: &TermConfig,
        rt: &Runtime,
        shell: &str,
        cwd: Option<PathBuf>,
        activate: bool,
    ) -> Result<SessionId, CmdError> {
        let spec = self
            .shells()
            .iter()
            .find(|s| s.id == shell)
            .cloned()
            .ok_or_else(|| arg_err("id", tr("term-unknown-shell", &[("id", shell)])))?;
        let id = self.next_id;
        self.next_id += 1;
        self.slots.push(Slot {
            id,
            shell: spec.id.clone(),
            state: SlotState::Starting,
            title: None,
            session: None,
            shared: None,
            queued: Vec::new(),
        });
        if activate {
            self.set_active(cfg, id);
        }
        let nonce = nonce();
        let spawn = spawn_spec(&spec, cwd, id, &nonce);
        let opts = SessionOpts {
            size: START_SIZE,
            scrollback: if activate {
                cfg.scrollback
            } else {
                cfg.background_scrollback
            },
            osc52: cfg.osc52.to_string(),
            nonce: Some(nonce),
        };
        rt.spawn(Lane::Interactive, "term.start", rt.token(), move |t| {
            let sink = Arc::new(CoreSink {
                id,
                sink: t.sink.clone(),
                cancel: CancelToken::new(),
            });
            let result = Session::start(id, &spawn, &opts, sink).map_err(|e| e.to_string());
            // Shutdown: сеанс закроется в Drop.
            let _ = t.sink.send(Box::new(StartedMsg { id, result }), &t.cancel);
        });
        Ok(id)
    }

    /// Сеанс стал активным: прежний — в фон (скроллбэк урезается).
    pub(crate) fn set_active(&mut self, cfg: &TermConfig, id: SessionId) {
        if self.active == Some(id) {
            return;
        }
        if let Some(old) = self.active
            && let Some(sh) = self.slot_mut(old).and_then(|s| s.shared.clone())
        {
            sh.set_scrollback(cfg.background_scrollback, &cfg.osc52);
        }
        self.active = Some(id);
        if let Some(sh) = self.slot_mut(id).and_then(|s| s.shared.clone()) {
            sh.set_scrollback(cfg.scrollback, &cfg.osc52);
            sh.dirty.store(true, std::sync::atomic::Ordering::Release);
        }
    }

    /// `term.next` / `term.prev`.
    pub(crate) fn cycle(&mut self, cfg: &TermConfig, step: isize) -> Result<(), CmdError> {
        let n = self.slots.len();
        if n == 0 {
            return Err(no_session());
        }
        let cur = self
            .active
            .and_then(|a| self.slots.iter().position(|s| s.id == a))
            .unwrap_or(0);
        let next = (cur.cast_signed() + step)
            .rem_euclid(n.cast_signed())
            .cast_unsigned();
        let id = self.slots[next].id;
        self.set_active(cfg, id);
        Ok(())
    }

    /// `term.close`: сеанс из пула; активным становится соседний.
    pub(crate) fn close(
        &mut self,
        cfg: &TermConfig,
        id: Option<SessionId>,
    ) -> Result<(), CmdError> {
        let id = id.or(self.active).ok_or_else(no_session)?;
        let pos = self
            .slots
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| arg_err("id", "no such session".into()))?;
        // Drop сеанса — закрытие через writer, без ожидания.
        self.slots.remove(pos);
        if self.active == Some(id) {
            self.active = None;
            if let Some(s) = self.slots.get(pos.min(self.slots.len().saturating_sub(1))) {
                let next = s.id;
                self.set_active(cfg, next);
            }
        }
        Ok(())
    }

    /// `term.restart`: тот же shell заново на месте активного сеанса.
    pub(crate) fn restart(&mut self, cfg: &TermConfig, rt: &Runtime) -> Result<(), CmdError> {
        let old = self.active.ok_or_else(no_session)?;
        let pos = self
            .slots
            .iter()
            .position(|s| s.id == old)
            .ok_or_else(no_session)?;
        let shell = self.slots[pos].shell.clone();
        let id = self.start(cfg, rt, &shell, None, false)?;
        let new = self.slots.pop().expect("slot just pushed");
        self.slots[pos] = new;
        self.active = None;
        self.set_active(cfg, id);
        Ok(())
    }

    /// `term.kill`: убить процесс активного сеанса и его дерево.
    pub(crate) fn kill(&mut self) -> Result<(), CmdError> {
        let s = self.active().ok_or_else(no_session)?;
        match &s.session {
            Some(sess) => sess
                .kill()
                .map_err(|e| CmdError::Rejected(e.to_string().into())),
            None => Ok(()),
        }
    }

    /// Байты в активный сеанс. Вышедший процесс перезапускается: Enter после
    /// «процесс завершён» — новый сеанс (сам Enter не уходит).
    pub(crate) fn write(
        &mut self,
        cfg: &TermConfig,
        rt: &Runtime,
        bytes: Vec<u8>,
        restart_only: bool,
    ) -> Result<(), CmdError> {
        let id = self.active.ok_or_else(no_session)?;
        let state = self.slot_mut(id).map(|s| s.state).ok_or_else(no_session)?;
        if matches!(state, SlotState::Exited(_) | SlotState::Failed) {
            self.restart(cfg, rt)?;
            if restart_only {
                return Ok(());
            }
        }
        let id = self.active.ok_or_else(no_session)?;
        let slot = self.slot_mut(id).ok_or_else(no_session)?;
        match &slot.session {
            Some(s) => s.write(bytes),
            None => slot.queued.extend_from_slice(&bytes),
        }
        Ok(())
    }

    /// Режимы ввода активного сеанса.
    pub(crate) fn input_modes(&self) -> input::Modes {
        self.active()
            .and_then(Slot::shared)
            .map(|s| s.input_modes())
            .unwrap_or_default()
    }

    fn on_started(
        &mut self,
        cfg: &TermConfig,
        id: SessionId,
        result: Result<Session, String>,
    ) -> Option<String> {
        let active = self.active == Some(id);
        let Some(slot) = self.slot_mut(id) else {
            return None; // закрыт до старта: Drop закроет сеанс
        };
        match result {
            Ok(s) => {
                if !slot.queued.is_empty() {
                    s.write(std::mem::take(&mut slot.queued));
                }
                if active {
                    s.shared().set_scrollback(cfg.scrollback, &cfg.osc52);
                }
                slot.shared = Some(s.shared().clone());
                slot.session = Some(s);
                slot.state = SlotState::Running;
                None
            }
            Err(e) => {
                slot.state = SlotState::Failed;
                let shell = slot.shell.clone();
                Some(tr("term-start-failed", &[("shell", &shell), ("error", &e)]))
            }
        }
    }

    fn on_event(&mut self, id: SessionId, ev: SessionEvent) {
        let Some(slot) = self.slot_mut(id) else {
            return;
        };
        match ev {
            SessionEvent::Exited(code) => {
                slot.state = SlotState::Exited(code);
                slot.session = None;
                let line = match code {
                    Some(c) => tr("term-exited", &[("code", &c.to_string())]),
                    None => tr("term-exited-unknown", &[]),
                };
                if let Some(sh) = &slot.shared {
                    sh.inject(format!("\r\n\x1b[0;2m{line}\x1b[0m\r\n").as_bytes());
                }
            }
            SessionEvent::Title(t) => slot.title = t,
            // Буфер обмена — S6-07, события интеграции — S6-11…S6-14.
            SessionEvent::Clipboard(_) | SessionEvent::Bell | SessionEvent::Osc { .. } => {}
        }
    }

    /// Вето выхода: есть сеансы с работающими процессами.
    pub(crate) fn quit_decision(&mut self) -> QuitDecision {
        let n = self.slots.iter().filter(|s| s.busy()).count();
        if n == 0 {
            // Закрыть все: потоки сеансов кончатся сами.
            self.slots.clear();
            self.active = None;
            return QuitDecision::Allow;
        }
        let i18n = urfin_i18n::current();
        let choice = |id: &str, key: &str, k: char| Choice {
            id: id.into(),
            label: i18n.get(key).into(),
            key: k,
        };
        QuitDecision::Veto(PromptSpec {
            anchor: Anchor::Cmdline,
            title: tr("term-quit-title", &[("n", &n.to_string())]),
            body: i18n.get("term-quit-body"),
            choices: [
                choice("cancel", "term-quit-cancel", 'c'),
                choice("quit", "term-quit-terminate", 'q'),
            ]
            .into_iter()
            .collect(),
            default: 0,
            input: None,
            allow_all: false,
            detail: None,
            secret: false,
        })
    }

    /// Статистика выбора — на диск (полоса Bulk).
    fn save_usage(&self, rt: &Runtime) {
        let Some(dir) = self.term_dir.clone() else {
            return;
        };
        let usage = self.usage.clone();
        rt.spawn(Lane::Bulk, "term.usage", rt.token(), move |_| {
            if let Err(e) = save_usage(&dir.join(USAGE_FILE), &usage) {
                tracing::warn!(error = %e, "term: usage not saved");
            }
        });
    }
}

const USAGE_FILE: &str = "usage.toml";

fn save_usage(path: &Path, usage: &BTreeMap<CompactString, u32>) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut t = toml::Table::new();
    for (k, v) in usage {
        t.insert(k.to_string(), toml::Value::Integer(i64::from(*v)));
    }
    std::fs::write(path, toml::to_string(&t).unwrap_or_default())
}

fn load_usage(path: &Path) -> BTreeMap<CompactString, u32> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(t) = text.parse::<toml::Table>() else {
        return BTreeMap::new();
    };
    t.iter()
        .filter_map(|(k, v)| Some((k.as_str().into(), u32::try_from(v.as_integer()?).ok()?)))
        .collect()
}

fn spawn_discovery(rt: &Runtime, cfg: &TermConfig, term_dir: Option<PathBuf>) {
    let user = cfg.shells.clone();
    rt.spawn(Lane::Interactive, "term.shells", rt.token(), move |t| {
        let shells = shells::discover(&SystemProbe, &user);
        let mut usage = BTreeMap::new();
        if let Some(dir) = &term_dir {
            let cache = dir.join("shells.toml");
            if shells::load_cache(&cache).as_ref() != Some(&shells)
                && let Err(e) = shells::save_cache(&cache, &shells)
            {
                tracing::warn!(error = %e, "term: shells cache");
            }
            usage = load_usage(&dir.join(USAGE_FILE));
        }
        let _ = t
            .sink
            .send(Box::new(ShellsMsg { shells, usage }), &t.cancel);
    });
}

/// Что запустить: shell, cwd и окружение ребёнка (S6 §4.2).
fn spawn_spec(spec: &ShellSpec, cwd: Option<PathBuf>, id: SessionId, nonce: &str) -> SpawnSpec {
    let mut env: Vec<(OsString, OsString)> = vec![
        ("TERM_PROGRAM".into(), "Urfin".into()),
        ("TERM_PROGRAM_VERSION".into(), crate::version().into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("URFIN_SESSION".into(), id.to_string().into()),
        ("URFIN_NONCE".into(), nonce.into()),
    ];
    env.extend(spec.env.iter().map(|(k, v)| (k.into(), v.into())));
    SpawnSpec {
        program: spec.program.clone(),
        args: spec.args.iter().map(OsString::from).collect(),
        cwd,
        env,
    }
}

/// Секрет интеграции, 128 бит (S6 §1.3). Ключи `RandomState` — из генератора ОС.
fn nonce() -> String {
    let s = RandomState::new();
    let t = Instant::now();
    let a = s.hash_one((t, 0u8));
    let b = s.hash_one((t, 1u8));
    format!("{a:016x}{b:016x}")
}

/// Каталог сеанса: локальный путь вкладки или ближайший локальный предок
/// (архив — каталог, где он лежит).
pub fn session_cwd(loc: &Location) -> Option<PathBuf> {
    if loc.is_local() {
        return Some(loc.path.clone());
    }
    let outer = loc.outer.as_deref()?;
    if outer.is_local() {
        return outer.path.parent().map(Path::to_path_buf);
    }
    session_cwd(outer)
}

/// Клавиша по имени и модификаторам `ctrl+alt+shift` (`term.key`).
pub fn key_bytes(key: &str, mods: Option<&str>, term: input::Modes) -> Option<Vec<u8>> {
    let mut m = Mods::NONE;
    for part in mods.unwrap_or("").split('+').filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "alt" => m.alt = true,
            "shift" => m.shift = true,
            _ => return None,
        }
    }
    let key = key.to_ascii_lowercase();
    let text = (key.chars().count() == 1).then_some(key.as_str());
    let mut out = Vec::new();
    input::key(&key, m, text, term, &mut out).then_some(out)
}

fn arg_err(name: &str, msg: String) -> CmdError {
    CmdError::Arg {
        name: name.into(),
        msg: msg.into(),
    }
}

fn no_session() -> CmdError {
    CmdError::Rejected(tr("term-no-session", &[]).into())
}

fn tr(key: &str, args: &[(&str, &str)]) -> String {
    let mut a = urfin_i18n::FluentArgs::new();
    for (k, v) in args {
        a.set(*k, (*v).to_owned());
    }
    urfin_i18n::current().fmt(key, &a)
}

/// Готовый (или не запустившийся) сеанс из фоновой задачи.
struct StartedMsg {
    id: SessionId,
    result: Result<Session, String>,
}

impl Msg for StartedMsg {
    fn name(&self) -> &'static str {
        "term.started"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        let Some(st) = cx.state.ext.get_mut::<TermState>() else {
            return;
        };
        let TermState {
            config, sessions, ..
        } = st;
        if let Some(err) = sessions.on_started(config, self.id, self.result) {
            cx.toast(Level::Error, err);
        }
    }
}

/// Найденные shells и статистика выбора.
struct ShellsMsg {
    shells: Vec<ShellSpec>,
    usage: BTreeMap<CompactString, u32>,
}

impl Msg for ShellsMsg {
    fn name(&self) -> &'static str {
        "term.shells"
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        let rt = cx.rt;
        let Some(st) = cx.state.ext.get_mut::<TermState>() else {
            return;
        };
        let TermState {
            config, sessions, ..
        } = st;
        let errors = sessions.on_shells(config, rt, self.shells, self.usage);
        for e in errors {
            cx.toast(Level::Error, e);
        }
    }
}

/// Событие сеанса (`SessionMsg`).
pub(crate) fn on_session_event(cx: &mut Cx<'_>, id: SessionId, ev: SessionEvent) {
    if let Some(st) = cx.state.ext.get_mut::<TermState>() {
        st.sessions.on_event(id, ev);
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;

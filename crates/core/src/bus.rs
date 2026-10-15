//! Шина Core (ADR-0001, ADR-0002): владеет состоянием в UI-потоке, роутит команды
//! по namespace, применяет `Msg` в `pump` с бюджетом времени.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::cmd::{CmdError, Command, CommandMeta, CommandRegistry};
use crate::ids::TabId;
use crate::loc::Location;
use crate::msg::{Cx, MsgInbox, WakeFlag, Waker};
use crate::prompt::PromptSpec;
use crate::runtime::{Runtime, RuntimeConfig};
use crate::services::Services;
use crate::state::{AppState, Level, Panel, UiEvent};
use crate::tab::Tab;
use crate::text::tr;

/// Обработчик namespace. Живёт в UI-потоке, не делает I/O и не блокируется.
pub trait Handler: 'static {
    fn namespace(&self) -> &'static str;
    /// Команды handler; id каждой — в его namespace.
    fn commands(&self) -> &'static [CommandMeta];
    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError>;

    /// Опрос перед выходом (S0 v2 №13); `app.quit` опрашивает все handlers до shutdown.
    fn on_quit(&mut self, _cx: &mut Cx<'_>) -> QuitDecision {
        QuitDecision::Allow
    }

    /// Начало каждого pump, до команд и `Msg` — например, разбор `ViewInbox` S7
    /// (S0 v5 №4). `rev` шина здесь не поднимает: при изменениях — `cx.state.touch()`.
    fn on_pump(&mut self, _cx: &mut Cx<'_>) {}
}

/// Ответ handler на выход.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "форма из S0 v2 №13; значение возвращается раз за сеанс, при выходе"
)]
pub enum QuitDecision {
    Allow,
    /// Нельзя выйти молча: показать запрос (например, «идёт копирование»).
    Veto(PromptSpec),
}

/// Начальные данные Core. Пути, конфиг, предупреждения конфига, сессию и признак
/// падения (S1 §2.5) добавят S1-10…S1-12.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Boot {
    pub runtime: RuntimeConfig,
    pub left: Location,
    pub right: Location,
}

impl Boot {
    pub fn new(left: Location, right: Location) -> Self {
        Boot {
            runtime: RuntimeConfig::default(),
            left,
            right,
        }
    }
}

/// Итог `pump`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pump {
    /// Бюджет кончился, а работа осталась: нужен ещё кадр.
    pub more: bool,
    /// `state.rev` изменился.
    pub changed: bool,
}

/// Канал закрыт: Core завершён.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("command channel closed")]
pub struct Closed;

/// Отправка команд из не-UI потоков (term, watcher, IPC). Clone + Send.
#[derive(Clone)]
pub struct CommandSender {
    tx: Sender<Command>,
    flag: Arc<WakeFlag>,
}

impl CommandSender {
    /// Не блокирует; будит UI через общий с `MsgSink` флаг.
    pub fn send(&self, c: Command) -> Result<(), Closed> {
        self.tx.send(c).map_err(|_| Closed)?;
        self.flag.raise();
        Ok(())
    }
}

impl fmt::Debug for CommandSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSender")
            .field("queued", &self.tx.len())
            .finish_non_exhaustive()
    }
}

/// Handler дольше этого в debug-сборке даёт `tracing::warn!` (S1 §0).
const SLOW_HANDLER: Duration = Duration::from_millis(1);

/// Состояние и шина. Живёт в UI-потоке.
pub struct Core {
    state: AppState,
    rt: Runtime,
    inbox: MsgInbox,
    services: Services,
    handlers: Vec<Box<dyn Handler>>,
    by_ns: FxHashMap<&'static str, usize>,
    registry: CommandRegistry,
    cmd_tx: Sender<Command>,
    cmd_rx: Receiver<Command>,
    queue: VecDeque<Command>,
}

impl Core {
    /// Создаёт Runtime и состояние с одним табом на панель. Вызывать в UI-потоке.
    pub fn new(boot: Boot, services: Services, waker: Arc<dyn Waker>) -> Self {
        let (rt, inbox) = Runtime::new(&boot.runtime, waker);
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let state = AppState::new(
            Panel::new(Tab::new(TabId(1), boot.left)),
            Panel::new(Tab::new(TabId(2), boot.right)),
        );
        Core {
            state,
            rt,
            inbox,
            services,
            handlers: Vec::new(),
            by_ns: FxHashMap::default(),
            registry: CommandRegistry::new(),
            cmd_tx,
            cmd_rx,
            queue: VecDeque::new(),
        }
    }

    /// Паника при дубле namespace, невалидном или повторном id команды и при
    /// команде не из своего namespace — это ошибки сборки приложения.
    pub fn register(&mut self, h: Box<dyn Handler>) {
        let ns = h.namespace();
        assert!(
            !self.by_ns.contains_key(ns),
            "handler namespace '{ns}' registered twice"
        );
        if let Some(m) = h
            .commands()
            .iter()
            .find(|m| m.id.split_once('.').map(|(n, _)| n) != Some(ns))
        {
            panic!("command '{}' is outside handler namespace '{ns}'", m.id);
        }
        if let Err(e) = self.registry.register(h.commands()) {
            panic!("handler '{ns}': {e}");
        }
        self.by_ns.insert(ns, self.handlers.len());
        self.handlers.push(h);
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn registry(&self) -> &CommandRegistry {
        &self.registry
    }

    pub fn sender(&self) -> CommandSender {
        CommandSender {
            tx: self.cmd_tx.clone(),
            flag: self.rt.wake_flag(),
        }
    }

    /// Синхронно выполняет команду и всё, что handlers поставили через
    /// `cx.dispatch` (FIFO). Ошибка — тост, не паника.
    pub fn dispatch(&mut self, cmd: Command) {
        self.queue.push_back(cmd);
        while let Some(c) = self.queue.pop_front() {
            self.exec(&c, None);
        }
    }

    /// Кадр: хуки `on_pump`, затем команды (из `CommandSender` и `cx.dispatch`),
    /// затем `Msg` — пока не кончится работа или `budget` (хотя бы один шаг
    /// выполняется всегда). `panel.refresh` с одним и тем же `tab` выполняется за
    /// pump один раз (S0 v2 №14).
    pub fn pump(&mut self, budget: Duration) -> Pump {
        let start = Instant::now();
        let rev0 = self.state.rev;
        // Сброс WakeFlag до чтения каналов: всё, что придёт позже, разбудит снова.
        self.inbox.begin_pump();
        for h in &mut self.handlers {
            h.on_pump(&mut Cx::new(
                &mut self.state,
                &self.rt,
                &self.services,
                &mut self.queue,
            ));
        }
        self.queue.extend(self.cmd_rx.try_iter());
        let mut refreshed = FxHashSet::default();
        loop {
            if let Some(c) = self.queue.pop_front() {
                self.exec(&c, Some(&mut refreshed));
            } else if let Some(m) = self.inbox.try_recv() {
                let name = m.name();
                let mut cx = Cx::new(&mut self.state, &self.rt, &self.services, &mut self.queue);
                m.apply(&mut cx);
                let kept = cx.rev_kept();
                if !kept {
                    self.state.touch();
                }
                tracing::trace!(msg = name, "applied");
            } else {
                break;
            }
            if start.elapsed() >= budget {
                break;
            }
        }
        Pump {
            more: !self.queue.is_empty() || !self.inbox.is_empty() || !self.cmd_rx.is_empty(),
            changed: self.state.rev != rev0,
        }
    }

    pub fn take_ui_events(&mut self) -> Vec<UiEvent> {
        self.state.take_ui_events()
    }

    /// Опрос handlers перед выходом: первый `Veto` в порядке регистрации.
    pub fn quit_decision(&mut self) -> QuitDecision {
        for h in &mut self.handlers {
            let mut cx = Cx::new(&mut self.state, &self.rt, &self.services, &mut self.queue);
            if let QuitDecision::Veto(p) = h.on_quit(&mut cx) {
                return QuitDecision::Veto(p);
            }
        }
        QuitDecision::Allow
    }

    /// Останавливает Runtime (см. [`Runtime::shutdown`]); `CommandSender` после
    /// этого получает `Closed`. Возвращает число отпущенных потоков. Финальную
    /// запись сессии добавит S1-11.
    pub fn shutdown(self, timeout: Duration) -> usize {
        let Core { rt, inbox, .. } = self;
        rt.shutdown(inbox, timeout)
    }

    fn exec(&mut self, cmd: &Command, refreshed: Option<&mut FxHashSet<Option<i64>>>) {
        if cmd.id.as_str() == "panel.refresh"
            && let Some(seen) = refreshed
            && !seen.insert(cmd.args.int("tab"))
        {
            return;
        }
        if let Err(e) = self.registry.validate(cmd) {
            self.error_toast(cmd, &e);
            return;
        }
        let Some(&i) = self.by_ns.get(cmd.id.namespace()) else {
            // Реестр знает только команды зарегистрированных handlers.
            unreachable!("validated command without handler: {}", cmd.id);
        };
        let t0 = Instant::now();
        let mut cx = Cx::new(&mut self.state, &self.rt, &self.services, &mut self.queue);
        let r = self.handlers[i].handle(cmd, &mut cx);
        let kept = cx.rev_kept();
        let dt = t0.elapsed();
        if cfg!(debug_assertions) && dt > SLOW_HANDLER {
            tracing::warn!(
                cmd = cmd.id.as_str(),
                ?dt,
                "slow handler (> 1 ms): move work to Runtime"
            );
        }
        match r {
            Ok(()) if !kept => self.state.touch(),
            Ok(()) => {}
            Err(e) => self.error_toast(cmd, &e),
        }
    }

    fn error_toast(&mut self, cmd: &Command, e: &CmdError) {
        let id = cmd.id.as_str();
        let text = match e {
            CmdError::Unknown(_) => tr("core-unknown-command", &[("id", id)]),
            CmdError::Rejected(msg) => msg.to_string(),
            e => tr(
                "core-command-failed",
                &[("id", id), ("error", &e.to_string())],
            ),
        };
        tracing::debug!(cmd = id, error = %e, "command failed");
        let level = if matches!(e, CmdError::Rejected(_)) {
            Level::Warn
        } else {
            Level::Error
        };
        self.state.push_ui(UiEvent::Toast { level, text });
    }
}

impl fmt::Debug for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Core")
            .field("handlers", &self.by_ns.keys().collect::<Vec<_>>())
            .field("queued", &self.queue.len())
            .field("rev", &self.state.rev)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Boot, Core, Handler, Pump, QuitDecision};
    use crate::cmd::{ArgSpec, CmdError, Command, CommandMeta, ValueKind};
    use crate::ids::Side;
    use crate::loc::Location;
    use crate::msg::{Cx, Waker, msg};
    use crate::prompt::{Anchor, PromptSpec};
    use crate::services::Services;
    use crate::state::{Level, UiEvent};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct TestWaker {
        wakes: AtomicUsize,
        pending: Mutex<bool>,
        cv: Condvar,
    }

    impl Waker for TestWaker {
        fn wake(&self) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
            *self.pending.lock().unwrap() = true;
            self.cv.notify_all();
        }
        fn wake_after(&self, _d: Duration) {
            self.wake();
        }
    }

    impl TestWaker {
        fn wait(&self, timeout: Duration) -> bool {
            let g = self.pending.lock().unwrap();
            let (mut g, _) = self.cv.wait_timeout_while(g, timeout, |p| !*p).unwrap();
            std::mem::take(&mut *g)
        }
    }

    const fn meta(id: &'static str, args: &'static [ArgSpec]) -> CommandMeta {
        CommandMeta {
            id,
            title: id,
            category: "test",
            args,
            repeatable: false,
        }
    }

    static PANEL: [CommandMeta; 4] = [
        meta(
            "panel.log",
            &[ArgSpec {
                name: "n",
                kind: ValueKind::Int,
                required: false,
            }],
        ),
        meta("panel.chain", &[]),
        meta("panel.fail", &[]),
        meta(
            "panel.refresh",
            &[ArgSpec {
                name: "tab",
                kind: ValueKind::Int,
                required: false,
            }],
        ),
    ];

    /// Пишет id выполненных команд в общий журнал.
    struct Recorder {
        log: Rc<RefCell<Vec<String>>>,
        veto: bool,
    }

    impl Handler for Recorder {
        fn namespace(&self) -> &'static str {
            "panel"
        }
        fn commands(&self) -> &'static [CommandMeta] {
            &PANEL
        }
        fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
            let entry = match cmd.args.int("n").or(cmd.args.int("tab")) {
                Some(n) => format!("{}:{n}", cmd.id),
                None => cmd.id.to_string(),
            };
            self.log.borrow_mut().push(entry);
            match cmd.id.as_str() {
                "panel.chain" => {
                    cx.dispatch(Command::new("panel.log").arg("n", 1));
                    cx.dispatch(Command::new("panel.log").arg("n", 2));
                    Ok(())
                }
                "panel.fail" => Err(CmdError::Rejected("not now".into())),
                _ => Ok(()),
            }
        }
        fn on_quit(&mut self, _cx: &mut Cx<'_>) -> QuitDecision {
            if self.veto {
                QuitDecision::Veto(PromptSpec {
                    anchor: Anchor::JobBar,
                    title: String::new(),
                    body: String::new(),
                    choices: smallvec::SmallVec::new(),
                    default: 0,
                    input: None,
                    allow_all: false,
                    detail: None,
                    secret: false,
                })
            } else {
                QuitDecision::Allow
            }
        }
    }

    struct Other;
    static OTHER: [CommandMeta; 1] = [meta("tab.new", &[])];
    impl Handler for Other {
        fn namespace(&self) -> &'static str {
            "tab"
        }
        fn commands(&self) -> &'static [CommandMeta] {
            &OTHER
        }
        fn handle(&mut self, _: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
            cx.keep_rev();
            Ok(())
        }
    }

    fn core() -> (Core, Arc<TestWaker>, Rc<RefCell<Vec<String>>>) {
        let w = Arc::new(TestWaker::default());
        let p = if cfg!(windows) { r"C:\" } else { "/" };
        let mut c = Core::new(
            Boot::new(Location::local(p), Location::local(p)),
            Services::null(),
            w.clone(),
        );
        let log = Rc::new(RefCell::new(Vec::new()));
        c.register(Box::new(Recorder {
            log: log.clone(),
            veto: false,
        }));
        c.register(Box::new(Other));
        (c, w, log)
    }

    fn toasts(c: &mut Core) -> Vec<(Level, String)> {
        c.take_ui_events()
            .into_iter()
            .filter_map(|e| match e {
                UiEvent::Toast { level, text } => Some((level, text)),
                _ => None,
            })
            .collect()
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn duplicate_namespace_panics() {
        let (mut c, _, log) = core();
        c.register(Box::new(Recorder { log, veto: false }));
    }

    #[test]
    #[should_panic(expected = "outside handler namespace")]
    fn foreign_command_panics() {
        struct Bad;
        static BAD: [CommandMeta; 1] = [meta("panel.x", &[])];
        impl Handler for Bad {
            fn namespace(&self) -> &'static str {
                "ops"
            }
            fn commands(&self) -> &'static [CommandMeta] {
                &BAD
            }
            fn handle(&mut self, _: &Command, _: &mut Cx<'_>) -> Result<(), CmdError> {
                Ok(())
            }
        }
        core().0.register(Box::new(Bad));
    }

    #[test]
    fn unknown_command_is_a_toast() {
        let (mut c, _, log) = core();
        let rev = c.state().rev;
        c.dispatch(Command::new("ops.copy"));
        c.dispatch(Command::new("panel.log").arg("n", "x"));
        let t = toasts(&mut c);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], (Level::Error, "Unknown command: ops.copy".to_owned()));
        assert!(t[1].1.starts_with("panel.log: arg n"), "{t:?}");
        assert!(log.borrow().is_empty());
        assert!(c.state().rev > rev, "toast must trigger a repaint");
    }

    #[test]
    fn rejected_is_a_warning_toast() {
        let (mut c, _, _) = core();
        c.dispatch(Command::new("panel.fail"));
        assert_eq!(toasts(&mut c), [(Level::Warn, "not now".to_owned())]);
    }

    #[test]
    fn cx_dispatch_is_fifo_in_same_dispatch_and_pump() {
        let (mut c, _, log) = core();
        c.dispatch(Command::new("panel.chain"));
        c.dispatch(Command::new("panel.log").arg("n", 3));
        assert_eq!(
            *log.borrow(),
            ["panel.chain", "panel.log:1", "panel.log:2", "panel.log:3"]
        );
        log.borrow_mut().clear();
        let s = c.sender();
        s.send(Command::new("panel.chain")).unwrap();
        s.send(Command::new("panel.log").arg("n", 3)).unwrap();
        let p = c.pump(Duration::from_secs(1));
        assert_eq!(
            p,
            Pump {
                more: false,
                changed: true
            }
        );
        assert_eq!(
            *log.borrow(),
            ["panel.chain", "panel.log:3", "panel.log:1", "panel.log:2"]
        );
    }

    #[test]
    fn rev_bumps_unless_kept() {
        let (mut c, _, _) = core();
        let r0 = c.state().rev;
        c.dispatch(Command::new("panel.log"));
        assert_eq!(c.state().rev, r0 + 1);
        c.dispatch(Command::new("tab.new"));
        assert_eq!(c.state().rev, r0 + 1);
        assert_eq!(
            c.pump(Duration::from_secs(1)),
            Pump {
                more: false,
                changed: false
            }
        );
    }

    #[test]
    fn pump_stops_on_budget_and_reports_more() {
        let (mut c, _, log) = core();
        let s = c.sender();
        for n in 0..5 {
            s.send(Command::new("panel.log").arg("n", n)).unwrap();
        }
        let p = c.pump(Duration::ZERO);
        assert_eq!(
            p,
            Pump {
                more: true,
                changed: true
            }
        );
        assert_eq!(log.borrow().len(), 1);
        let mut pumps = 1;
        while c.pump(Duration::ZERO).more {
            pumps += 1;
        }
        assert_eq!(log.borrow().len(), 5);
        assert_eq!(pumps, 4);
    }

    #[test]
    fn commands_before_msgs_and_msgs_can_dispatch() {
        let (mut c, w, log) = core();
        let sink = c.rt.sink();
        let t = std::thread::spawn(move || {
            sink.try_send(msg("listed", |cx| {
                cx.dispatch(Command::new("panel.log").arg("n", 9));
            }))
            .unwrap();
        });
        t.join().unwrap();
        c.sender()
            .send(Command::new("panel.log").arg("n", 1))
            .unwrap();
        assert!(w.wait(Duration::from_secs(1)));
        c.pump(Duration::from_secs(1));
        assert_eq!(*log.borrow(), ["panel.log:1", "panel.log:9"]);
    }

    #[test]
    fn sender_from_other_thread_wakes_and_delivers() {
        let (mut c, w, log) = core();
        let s = c.sender();
        std::thread::spawn(move || {
            for n in 0..100 {
                s.send(Command::new("panel.log").arg("n", n)).unwrap();
            }
        })
        .join()
        .unwrap();
        assert!(w.wait(Duration::from_secs(1)));
        assert_eq!(w.wakes.load(Ordering::SeqCst), 1);
        assert!(!c.pump(Duration::from_secs(1)).more);
        assert_eq!(log.borrow().len(), 100);
        let s = c.sender();
        assert_eq!(c.shutdown(Duration::from_secs(5)), 0);
        assert!(s.send(Command::new("panel.log")).is_err());
    }

    #[test]
    fn refresh_deduplicated_per_tab_within_pump() {
        let (mut c, _, log) = core();
        let s = c.sender();
        for cmd in [
            Command::new("panel.refresh").arg("tab", 1),
            Command::new("panel.refresh").arg("tab", 2),
            Command::new("panel.refresh").arg("tab", 1),
            Command::new("panel.refresh"),
            Command::new("panel.refresh"),
        ] {
            s.send(cmd).unwrap();
        }
        c.pump(Duration::from_secs(1));
        assert_eq!(
            *log.borrow(),
            ["panel.refresh:1", "panel.refresh:2", "panel.refresh"]
        );
        s.send(Command::new("panel.refresh").arg("tab", 1)).unwrap();
        c.pump(Duration::from_secs(1));
        assert_eq!(log.borrow().len(), 4);
    }

    #[test]
    fn on_pump_runs_first_without_rev_bump() {
        struct Inbox(Rc<RefCell<Vec<String>>>);
        static VIEW: [CommandMeta; 0] = [];
        impl Handler for Inbox {
            fn namespace(&self) -> &'static str {
                "viewer"
            }
            fn commands(&self) -> &'static [CommandMeta] {
                &VIEW
            }
            fn handle(&mut self, _: &Command, _: &mut Cx<'_>) -> Result<(), CmdError> {
                Ok(())
            }
            fn on_pump(&mut self, _cx: &mut Cx<'_>) {
                self.0.borrow_mut().push("on_pump".into());
            }
        }
        let (mut c, _, log) = core();
        c.register(Box::new(Inbox(log.clone())));
        c.sender().send(Command::new("panel.log")).unwrap();
        c.pump(Duration::from_secs(1));
        assert_eq!(*log.borrow(), ["on_pump", "panel.log"]);
        assert!(!c.pump(Duration::from_secs(1)).changed);
    }

    #[test]
    fn quit_decision_polls_handlers() {
        let (mut c, _, _) = core();
        assert_eq!(c.quit_decision(), QuitDecision::Allow);
        let w = Arc::new(TestWaker::default());
        let mut c = Core::new(
            Boot::new(Location::local("/"), Location::local("/")),
            Services::null(),
            w,
        );
        c.register(Box::new(Other));
        c.register(Box::new(Recorder {
            log: Rc::default(),
            veto: true,
        }));
        assert!(matches!(c.quit_decision(), QuitDecision::Veto(_)));
    }

    #[test]
    fn task_panic_becomes_toast() {
        let (mut c, w, _) = core();
        c.rt.spawn(crate::runtime::Lane::Bulk, "boom", c.rt.token(), |_| {
            panic!("test panic")
        });
        assert!(w.wait(Duration::from_secs(5)));
        c.pump(Duration::from_secs(1));
        let t = toasts(&mut c);
        assert_eq!(
            t,
            [(Level::Error, "Task boom failed unexpectedly".to_owned())]
        );
    }

    #[test]
    fn initial_state() {
        let (c, _, _) = core();
        assert_eq!(c.state().active, Side::Left);
        assert_eq!(c.state().panel(Side::Right).tab().id, crate::ids::TabId(2));
        assert_eq!(c.registry().iter().count(), 5);
    }
}

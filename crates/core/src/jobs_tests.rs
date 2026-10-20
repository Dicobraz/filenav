//! Интеграционные тесты задач и запросов на реальном Runtime (таймауты ≤ 1 с).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use smallvec::smallvec;

use crate::bus::{Boot, Core, Handler, QuitDecision};
use crate::cmd::{ArgSpec, CmdError, Command, CommandMeta, ValueKind};
use crate::ids::{JobId, PromptId};
use crate::jobs::{JobSpec, JobState, JobSummary, Phase};
use crate::loc::Location;
use crate::msg::{Cx, Waker};
use crate::prompt::{Anchor, Choice, PromptSpec, Reply};
use crate::runtime::Lane;
use crate::services::Services;
use crate::state::{Focus, UiEvent};

const T: Duration = Duration::from_secs(1);

#[derive(Default)]
struct TestWaker {
    pending: Mutex<bool>,
    cv: Condvar,
}

impl Waker for TestWaker {
    fn wake(&self) {
        *self.pending.lock().unwrap() = true;
        self.cv.notify_all();
    }
    /// Отложенное пробуждение (как `request_repaint_after`): тесты качают pump сами.
    fn wake_after(&self, _d: Duration) {}
}

impl TestWaker {
    fn wait(&self, timeout: Duration) -> bool {
        let g = self.pending.lock().unwrap();
        let (mut g, _) = self.cv.wait_timeout_while(g, timeout, |p| !*p).unwrap();
        std::mem::take(&mut *g)
    }
}

fn spec(title: &str) -> PromptSpec {
    PromptSpec {
        anchor: Anchor::JobBar,
        title: title.into(),
        body: String::new(),
        choices: smallvec![
            Choice {
                id: "yes".into(),
                label: "Yes".into(),
                key: 'y',
            },
            Choice {
                id: "no".into(),
                label: "No".into(),
                key: 'n',
            },
        ],
        default: 0,
        input: None,
        allow_all: true,
        detail: None,
        secret: false,
    }
}

static TEST: [CommandMeta; 1] = [CommandMeta {
    id: "test.job",
    title: "Test job",
    category: "test",
    args: &[ArgSpec {
        name: "mode",
        kind: ValueKind::Str,
        required: true,
    }],
    repeatable: false,
}];

/// Владелец тестовых задач: запускает воркеры разных видов, принимает
/// `job.retry`/`job.priority`, вето на выход по флагу.
struct Owner {
    log: Rc<RefCell<Vec<String>>>,
    veto: bool,
}

impl Handler for Owner {
    fn namespace(&self) -> &'static str {
        "test"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &TEST
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        if cmd.id.namespace() == "job" {
            self.log.borrow_mut().push(cmd.to_string());
            return Ok(());
        }
        let mode = cmd.args.str("mode").unwrap_or_default().to_owned();
        let h = cx.state.jobs.start(
            JobSpec {
                kind: "test",
                title: mode.clone(),
                lane: Lane::Bulk,
                priority: 1,
                owner_ns: "test",
            },
            cx.rt.sink(),
        );
        let cancel = h.cancel_token().clone();
        cx.rt
            .spawn_dedicated(format!("job-{}", h.id().0), cancel, move |_| {
                worker(&mode, h);
            });
        Ok(())
    }

    fn on_quit(&mut self, _cx: &mut Cx<'_>) -> QuitDecision {
        if self.veto {
            QuitDecision::Veto(spec("Tasks are running"))
        } else {
            QuitDecision::Allow
        }
    }
}

fn worker(mode: &str, h: crate::jobs::JobHandle) {
    match mode {
        "loop" => {
            h.progress().set_phase(Phase::Transfer);
            let r = (|| {
                for _ in 0..10_000 {
                    h.checkpoint().map_err(|_| "cancelled".to_owned())?;
                    h.progress().items_done.fetch_add(1, Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(JobSummary::default())
            })();
            h.finish(r);
        }
        "ask" => {
            // При отмене — без finish: Drop отменённой задачи даёт Cancelled.
            if let Ok(r) = h.ask(spec("Overwrite?")) {
                h.finish(Ok(JobSummary {
                    items: if r.choice == "yes" { 1 } else { 2 },
                    ..JobSummary::default()
                }));
            }
        }
        "drop" => drop(h),
        "panic" => {
            let _keep = h;
            panic!("job failed");
        }
        _ => unreachable!(),
    }
}

struct H {
    core: Core,
    w: Arc<TestWaker>,
    log: Rc<RefCell<Vec<String>>>,
    /// Каталоги Core (`shutdown` пишет сессию): удаляются вместе с `H`.
    _dir: tempfile::TempDir,
}

impl H {
    fn new(veto: bool) -> H {
        let w = Arc::new(TestWaker::default());
        let p = if cfg!(windows) { r"C:\" } else { "/" };
        let dir = tempfile::Builder::new()
            .prefix("urfin-jobs-")
            .tempdir()
            .unwrap();
        let mut boot = Boot::new(Location::local(p), Location::local(p));
        boot.paths = crate::Paths::in_dir(dir.path().to_path_buf());
        let mut core = Core::new(boot, Services::null(), w.clone());
        let log = Rc::new(RefCell::new(Vec::new()));
        core.register(Box::new(Owner {
            log: log.clone(),
            veto,
        }));
        H {
            core,
            w,
            log,
            _dir: dir,
        }
    }

    /// Качать pump, пока не выполнится условие (не дольше `T`).
    fn until(&mut self, what: &str, f: impl Fn(&Core) -> bool) {
        let deadline = Instant::now() + T;
        loop {
            self.core.pump(T);
            if f(&self.core) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for: {what}");
            self.w.wait(Duration::from_millis(20));
        }
    }

    fn start(&mut self, mode: &str) -> JobId {
        self.core
            .dispatch(Command::new("test.job").arg("mode", mode));
        self.core.state().jobs.iter().last().unwrap().id
    }

    fn state(&self, id: JobId) -> JobState {
        self.core.state().jobs.get(id).unwrap().state.clone()
    }

    fn items(&self, id: JobId) -> u64 {
        self.core
            .state()
            .jobs
            .get(id)
            .unwrap()
            .progress
            .snapshot()
            .items_done
    }

    fn wait_state(&mut self, id: JobId, st: &JobState) {
        let what = format!("{st:?}");
        self.until(&what, |c| c.state().jobs.get(id).unwrap().state == *st);
    }

    fn waiting_prompt(&mut self, id: JobId) -> PromptId {
        self.until("Waiting", |c| {
            matches!(c.state().jobs.get(id).unwrap().state, JobState::Waiting(_))
        });
        let JobState::Waiting(p) = self.state(id) else {
            unreachable!()
        };
        let active = self.core.state().prompts.active().unwrap();
        assert_eq!((active.id, active.job), (p, Some(id)));
        p
    }
}

#[test]
fn pause_resume_cancel() {
    let mut h = H::new(false);
    let id = h.start("loop");
    assert!(h.core.state().jobs.any_active());
    h.until("progress", |c| {
        c.state()
            .jobs
            .get(id)
            .unwrap()
            .progress
            .snapshot()
            .items_done
            > 5
    });
    assert_eq!(
        h.core.state().jobs.get(id).unwrap().progress.phase(),
        Phase::Transfer
    );
    h.core.dispatch(Command::new("job.pause").arg("id", 1));
    assert_eq!(h.state(id), JobState::Paused);
    thread::sleep(Duration::from_millis(30));
    let n1 = h.items(id);
    thread::sleep(Duration::from_millis(100));
    assert!(h.items(id) <= n1 + 1, "paused job keeps going");
    h.core.dispatch(Command::new("job.resume").arg("id", 1));
    assert_eq!(h.state(id), JobState::Running);
    h.until("progress after resume", |c| {
        c.state()
            .jobs
            .get(id)
            .unwrap()
            .progress
            .snapshot()
            .items_done
            > n1 + 5
    });
    h.core.dispatch(Command::new("job.cancel").arg("id", 1));
    h.wait_state(id, &JobState::Cancelled);
    assert!(!h.core.state().jobs.any_active());
    assert!(h.core.state().jobs.get(id).unwrap().finished.is_some());
}

#[test]
fn cancel_while_paused() {
    let mut h = H::new(false);
    let id = h.start("loop");
    h.core.dispatch(Command::new("job.pause").arg("id", 1));
    h.core.dispatch(Command::new("job.cancel").arg("id", 1));
    h.wait_state(id, &JobState::Cancelled);
}

#[test]
fn ask_then_reply_continues() {
    let mut h = H::new(false);
    let id = h.start("ask");
    let p = h.waiting_prompt(id);
    assert!(h.core.state().jobs.any_active());
    h.core.dispatch(
        Command::new("prompt.reply")
            .arg("id", i64::try_from(p.0).unwrap())
            .arg("choice", "yes"),
    );
    assert!(h.core.state().prompts.is_empty());
    h.wait_state(
        id,
        &JobState::Done(JobSummary {
            items: 1,
            ..JobSummary::default()
        }),
    );
    assert!(!h.core.state().jobs.any_active());
}

#[test]
fn ask_then_prompt_cancel_is_cancelled() {
    let mut h = H::new(false);
    let id = h.start("ask");
    let p = h.waiting_prompt(id);
    h.core
        .dispatch(Command::new("prompt.cancel").arg("id", i64::try_from(p.0).unwrap()));
    h.wait_state(id, &JobState::Cancelled);
    assert!(h.core.state().prompts.is_empty());
}

#[test]
fn ask_then_job_cancel_is_cancelled() {
    let mut h = H::new(false);
    let id = h.start("ask");
    h.waiting_prompt(id);
    h.core.dispatch(Command::new("job.cancel").arg("id", 1));
    assert!(h.core.state().prompts.is_empty());
    h.wait_state(id, &JobState::Cancelled);
}

#[test]
fn drop_without_finish_fails_aborted() {
    let mut h = H::new(false);
    let id = h.start("drop");
    h.wait_state(id, &JobState::Failed("aborted".into()));
}

#[test]
fn panic_in_job_fails() {
    let mut h = H::new(false);
    let id = h.start("panic");
    h.wait_state(id, &JobState::Failed("panicked".into()));
    // Тост Runtime отправляет после раскрутки стека, то есть после `Failed`.
    let deadline = Instant::now() + T;
    loop {
        h.core.pump(T);
        let events = h.core.take_ui_events();
        if events.iter().any(|e| matches!(e, UiEvent::Toast { .. })) {
            break;
        }
        assert!(Instant::now() < deadline, "no panic toast");
        h.w.wait(Duration::from_millis(20));
    }
}

#[test]
fn retry_and_priority_go_to_owner() {
    let mut h = H::new(false);
    let id = h.start("loop");
    h.core.dispatch(Command::new("job.retry").arg("id", 1));
    h.core
        .dispatch(Command::new("job.priority").arg("id", 1).arg("p", 7));
    assert_eq!(*h.log.borrow(), ["job.retry id=1", "job.priority id=1 p=7"]);
    assert_eq!(h.core.state().jobs.get(id).unwrap().priority, 7);
    h.core.dispatch(Command::new("job.retry").arg("id", 99));
    assert_eq!(h.log.borrow().len(), 2);
    h.core.dispatch(Command::new("job.cancel").arg("id", 1));
    h.wait_state(id, &JobState::Cancelled);
}

#[test]
fn quit_veto_opens_prompt() {
    let mut h = H::new(true);
    let id = h.start("loop");
    h.core.dispatch(Command::new("app.quit"));
    assert!(!h.core.state().quit);
    let p = h.core.state().prompts.active().unwrap().id;
    assert_eq!(h.core.state().focus, Focus::Prompt(p));
    let reply = |choice: &str, p: PromptId| {
        Command::new("prompt.reply")
            .arg("id", i64::try_from(p.0).unwrap())
            .arg("choice", choice)
    };
    h.core.dispatch(reply("no", p));
    assert!(!h.core.state().quit);
    assert_eq!(h.core.state().focus, Focus::Panel);
    assert!(h.core.state().jobs.any_active());
    h.core.dispatch(Command::new("app.quit"));
    let p = h.core.state().prompts.active().unwrap().id;
    h.core.dispatch(reply("quit", p));
    assert!(h.core.state().quit);
    h.wait_state(id, &JobState::Cancelled);
}

#[test]
fn shutdown_cancels_jobs() {
    let mut h = H::new(false);
    h.start("loop");
    h.until("running", |c| {
        c.state()
            .jobs
            .get(JobId(1))
            .unwrap()
            .progress
            .snapshot()
            .items_done
            > 0
    });
    let _ = h.core.shutdown(T);
}

#[test]
fn retire_keeps_recent_finished() {
    let mut h = H::new(false);
    let ids: Vec<JobId> = (0..3).map(|_| h.start("drop")).collect();
    for &id in &ids {
        h.wait_state(id, &JobState::Failed("aborted".into()));
    }
    let active = h.start("loop");
    h.core.dispatch(Command::new("job.cancel").arg("id", 4));
    h.wait_state(active, &JobState::Cancelled);
    let mut jobs = std::mem::take(&mut h.core.state_mut_for_tests().jobs);
    jobs.retire(1);
    let left: Vec<JobId> = jobs.iter().map(|j| j.id).collect();
    assert_eq!(left, [active]);
}

#[test]
fn finished_jobs_are_retired_automatically() {
    let mut h = H::new(false);
    let n = crate::jobs::JobRegistry::KEEP_FINISHED + 3;
    let ids: Vec<JobId> = (0..n).map(|_| h.start("drop")).collect();
    // Ранние задачи уже убраны — ждём, пока не останется активных.
    h.until("all finished", |c| !c.state().jobs.any_active());
    let left: Vec<JobId> = h.core.state().jobs.iter().map(|j| j.id).collect();
    assert_eq!(left, ids[3..], "остались последние KEEP_FINISHED");
}

#[test]
fn prompts_open_reply_cancel() {
    let mut ps = crate::prompt::Prompts::default();
    let (a, ra) = ps.open(spec("A"), None);
    let (b, rb) = ps.open(spec("B"), None);
    assert_eq!(ps.active().map(|p| p.id), Some(a));
    assert!(ps.reply(
        a,
        Reply {
            choice: "yes".into(),
            input: Some("secret".into()),
            all: true,
        }
    ));
    let r = ra.recv_timeout(T).unwrap();
    assert!(r.all);
    assert!(
        !format!("{r:?}").contains("secret"),
        "input must not be logged"
    );
    assert!(!ps.reply(a, r));
    assert_eq!(ps.active().map(|p| p.id), Some(b));
    ps.cancel(b);
    assert!(rb.recv_timeout(T).is_err());
    assert!(ps.is_empty());
}

#[test]
fn progress_snapshot() {
    let p = crate::jobs::Progress::default();
    p.bytes_total.store(100, Ordering::Relaxed);
    p.total_final.store(true, Ordering::Relaxed);
    p.parked.fetch_add(2, Ordering::Relaxed);
    p.set_current("a.txt");
    p.set_phase(Phase::Verify);
    let s = p.snapshot();
    assert_eq!(
        (s.bytes_total, s.total_final, s.parked, s.phase),
        (100, true, 2, Phase::Verify)
    );
    assert_eq!(s.current.as_deref(), Some("a.txt"));
}

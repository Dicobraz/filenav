//! Задачи для сцен `--screens` (D-06): job-бару нужны идущие задачи, а демо-дерево
//! копируется мгновенно. Handler регистрирует только app под `--screens`, поэтому в
//! реестре команд и `docs/COMMANDS.md` его нет. `screens.jobs` — запустить три задачи,
//! которые медленно «идут» сами, `stop=true` — отменить их.

use std::sync::atomic::Ordering;
use std::time::Duration;

use urfin_core::{
    ArgSpec, CmdError, Command, CommandMeta, Cx, Handler, JobId, JobSpec, Lane, ValueKind,
};
use urfin_i18n::FluentArgs;

static COMMANDS: &[CommandMeta] = &[CommandMeta {
    id: "screens.jobs",
    title: "Demo tasks (--screens)",
    category: "screens",
    args: &[ArgSpec {
        name: "stop",
        kind: ValueKind::Bool,
        required: false,
    }],
    repeatable: false,
}];

/// Демо-задачи сцен.
#[derive(Default)]
pub struct DemoJobs {
    ids: Vec<JobId>,
}

/// Зарегистрировать демо-задачи (только `--screens`).
pub fn register_demo(core: &mut urfin_core::Core) {
    core.register(Box::new(DemoJobs::default()));
}

/// (вид, текст «что → куда», всего байт, всего штук, сделано, шаг за 100 мс).
const DEMO: [(&str, u64, u64, u64, u64); 3] = [
    ("copy", 3_650_722_201, 48, 1_288_490_188, 5_000_000),
    ("trash", 0, 12_870, 9_012, 3),
    ("move", 1_073_741_824, 214, 644_245_094, 0),
];

impl Handler for DemoJobs {
    fn namespace(&self) -> &'static str {
        "screens"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        if cmd.args.bool("stop").unwrap_or(false) {
            for id in self.ids.drain(..) {
                cx.state.jobs.cancel(id);
            }
            return Ok(());
        }
        let i18n = urfin_i18n::current();
        for (n, (kind, bytes, items, done, step)) in DEMO.into_iter().enumerate() {
            let mut a = FluentArgs::new();
            a.set("n", items);
            a.set("name", "");
            a.set(
                "dest",
                if n == 0 {
                    "\\\\fs01\\share\\presale"
                } else {
                    "D:\\backup\\clients"
                },
            );
            let title = match kind {
                "trash" => i18n.fmt("ops-job-trash-what", &a),
                _ => i18n.fmt("ops-job-transfer", &a),
            };
            let job = cx.state.jobs.start(
                JobSpec {
                    kind,
                    title,
                    lane: Lane::Bulk,
                    priority: 0,
                    owner_ns: "screens",
                },
                cx.rt.sink(),
            );
            let id = job.id();
            self.ids.push(id);
            if n == 2 {
                // Третья — на паузе (кнопка «Продолжить»).
                cx.state.jobs.pause(id);
            }
            cx.rt
                .spawn_dedicated("screens.job".into(), cx.rt.token(), move |tcx| {
                    let p = job.progress();
                    let total = if bytes > 0 { bytes } else { items };
                    p.bytes_total.store(bytes, Ordering::Relaxed);
                    p.items_total.store(items, Ordering::Relaxed);
                    p.total_final.store(true, Ordering::Relaxed);
                    let done_at = if bytes > 0 {
                        &p.bytes_done
                    } else {
                        &p.items_done
                    };
                    done_at.store(done, Ordering::Relaxed);
                    while !tcx.cancel.is_cancelled() && job.checkpoint().is_ok() {
                        if done_at.load(Ordering::Relaxed) + step < total {
                            done_at.fetch_add(step, Ordering::Relaxed);
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    job.finish(Err(String::new()));
                });
        }
        Ok(())
    }
}

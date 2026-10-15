//! `job.*`. `job.retry` и `job.priority` шина отдаёт handler `owner_ns` задачи
//! (S0 v2 №8); сюда они не приходят.

use super::{arg, meta};
use crate::bus::Handler;
use crate::cmd::{ArgSpec, CmdError, Command, CommandMeta, ValueKind};
use crate::ids::JobId;
use crate::msg::Cx;

pub(crate) struct JobHandler;

const ID: ArgSpec = arg("id", ValueKind::Int, true);

static COMMANDS: [CommandMeta; 5] = [
    meta("job.cancel", "Cancel task", &[ID], false),
    meta("job.pause", "Pause task", &[ID], false),
    meta("job.resume", "Resume task", &[ID], false),
    meta("job.retry", "Retry task", &[ID], false),
    meta(
        "job.priority",
        "Task priority",
        &[ID, arg("p", ValueKind::Int, true)],
        false,
    ),
];

/// Аргумент `id` задачи.
pub(crate) fn job_id(cmd: &Command) -> Result<JobId, CmdError> {
    cmd.args
        .int("id")
        .and_then(|v| u64::try_from(v).ok())
        .map(JobId)
        .ok_or_else(|| CmdError::arg("id", "expected a job id"))
}

impl Handler for JobHandler {
    fn namespace(&self) -> &'static str {
        "job"
    }

    fn commands(&self) -> &'static [CommandMeta] {
        &COMMANDS
    }

    fn handle(&mut self, cmd: &Command, cx: &mut Cx<'_>) -> Result<(), CmdError> {
        let id = job_id(cmd)?;
        if cx.state.jobs.get(id).is_none() {
            return Err(CmdError::arg("id", "no such job"));
        }
        match cmd.id.as_str() {
            "job.cancel" => {
                cx.state.jobs.cancel(id);
                // Открытые вопросы задачи закрываются: ждущий `ask` получит Cancelled.
                let open: Vec<_> = cx
                    .state
                    .prompts
                    .iter()
                    .filter(|p| p.job == Some(id))
                    .map(|p| p.id)
                    .collect();
                for p in open {
                    cx.state.prompts.cancel(p);
                }
            }
            "job.pause" => cx.state.jobs.pause(id),
            "job.resume" => cx.state.jobs.resume(id),
            _ => unreachable!("routed to owner handler: {}", cmd.id),
        }
        Ok(())
    }
}

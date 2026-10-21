//! Поток writer (S6 §1.1): запись в PTY, resize, закрытие псевдоконсоли (не из
//! reader — иначе дедлок) и `stop_sync` DEC 2026 по таймауту.

use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};

use super::{PtyPort, SessionSink, WriterCmd};
use crate::vt::shared::TermShared;

/// После выхода процесса ждать EOF столько, затем закрыть псевдоконсоль самим
/// (без `ReleasePseudoConsole` EOF без закрытия не придёт).
pub const EXIT_GRACE: Duration = Duration::from_millis(300);

pub(crate) fn run(
    rx: &Receiver<WriterCmd>,
    mut port: impl PtyPort,
    shared: &TermShared,
    sink: &dyn SessionSink,
) {
    let mut broken = false;
    let mut sync: Option<Instant> = None;
    let mut close_at: Option<Instant> = None;
    loop {
        let deadline = match (sync, close_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let cmd = match deadline {
            Some(d) => rx.recv_deadline(d),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match cmd {
            Ok(WriterCmd::Bytes(b)) => {
                if !broken && let Err(e) = port.write(&b) {
                    tracing::debug!(error = %e, "term: write");
                    broken = true;
                }
            }
            Ok(WriterCmd::Resize(size)) => port.resize(size),
            Ok(WriterCmd::Sync(at)) => sync = Some(at),
            Ok(WriterCmd::ChildExited) => {
                close_at.get_or_insert_with(|| Instant::now() + EXIT_GRACE);
            }
            Ok(WriterCmd::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if close_at.is_some_and(|c| c <= now) {
                    break;
                }
                if sync.is_some_and(|s| s <= now) {
                    sync = stop_sync(shared, now);
                    if sync.is_none() && shared.mark_dirty() {
                        sink.poke();
                    }
                }
            }
        }
    }
    port.close();
}

/// Прервать DEC 2026, если его срок истёк; иначе — новый срок.
fn stop_sync(shared: &TermShared, now: Instant) -> Option<Instant> {
    let mut vt = shared.vt.lock();
    let vt = &mut *vt;
    match vt.processor.sync_timeout().sync_timeout() {
        Some(at) if at > now => Some(at),
        Some(_) => {
            vt.processor.stop_sync(&mut vt.term);
            None
        }
        None => None,
    }
}

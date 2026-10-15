//! Сообщения воркеры → UI-поток: `Msg`, `MsgSink` (bounded, с бэкпрешером),
//! коалесцирующее пробуждение `WakeFlag` → `Waker` (ADR-0003).

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, ThreadId};
use std::time::Duration;

use crossbeam_channel::{Receiver, SendTimeoutError, Sender, TrySendError as ChanTrySendError};

use crate::cancel::CancelToken;
use crate::cmd::Command;
use crate::runtime::Runtime;
use crate::services::Services;
use crate::state::{AppState, Level, UiEvent};

/// Будит UI-поток (в app — `egui::Context::request_repaint*`).
pub trait Waker: Send + Sync + 'static {
    fn wake(&self);
    fn wake_after(&self, d: Duration);
}

/// Результат фоновой работы; применяется в UI-потоке в `Core::pump`.
pub trait Msg: Send + 'static {
    /// Для tracing.
    fn name(&self) -> &'static str;
    fn apply(self: Box<Self>, cx: &mut Cx<'_>);
}

/// Контекст выполнения handler и применения `Msg`. Создаёт только Core.
///
/// После handler или `Msg` шина поднимает `state.rev` (ключ кэшей UI и сигнал
/// перерисовки), если не вызван [`Cx::keep_rev`]. В `Handler::on_pump` — наоборот:
/// `rev` не трогается, при изменениях handler вызывает `cx.state.touch()` сам.
pub struct Cx<'a> {
    pub state: &'a mut AppState,
    pub rt: &'a Runtime,
    pub services: &'a Services,
    queue: &'a mut VecDeque<Command>,
    keep_rev: bool,
}

impl<'a> Cx<'a> {
    pub(crate) fn new(
        state: &'a mut AppState,
        rt: &'a Runtime,
        services: &'a Services,
        queue: &'a mut VecDeque<Command>,
    ) -> Self {
        Cx {
            state,
            rt,
            services,
            queue,
            keep_rev: false,
        }
    }

    /// В конец очереди текущего pump (FIFO): выполнится в этом же pump или `dispatch`.
    pub fn dispatch(&mut self, c: Command) {
        self.queue.push_back(c);
    }

    pub fn ui(&mut self, e: UiEvent) {
        self.state.push_ui(e);
    }

    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        self.ui(UiEvent::Toast {
            level,
            text: text.into(),
        });
    }

    /// Состояние не менялось: не поднимать `rev` (например, `panel.viewport`
    /// без изменения прокрутки).
    pub fn keep_rev(&mut self) {
        self.keep_rev = true;
    }

    pub(crate) fn rev_kept(&self) -> bool {
        self.keep_rev
    }
}

impl fmt::Debug for Cx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cx")
            .field("queued", &self.queue.len())
            .field("keep_rev", &self.keep_rev)
            .finish_non_exhaustive()
    }
}

/// `Msg` из замыкания.
pub fn msg(name: &'static str, f: impl FnOnce(&mut Cx<'_>) + Send + 'static) -> Box<dyn Msg> {
    Box::new(FnMsg { name, f })
}

struct FnMsg<F> {
    name: &'static str,
    f: F,
}

impl<F: FnOnce(&mut Cx<'_>) + Send + 'static> Msg for FnMsg<F> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn apply(self: Box<Self>, cx: &mut Cx<'_>) {
        (self.f)(cx);
    }
}

/// Коалесцирующий флаг пробуждения: `Waker::wake()` вызывается только на переходе
/// false→true; флаг сбрасывает [`MsgInbox::begin_pump`].
pub(crate) struct WakeFlag {
    raised: AtomicBool,
    closed: AtomicBool,
    waker: Arc<dyn Waker>,
    ui_thread: ThreadId,
}

impl WakeFlag {
    pub(crate) fn new(waker: Arc<dyn Waker>) -> Self {
        WakeFlag {
            raised: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            waker,
            ui_thread: thread::current().id(),
        }
    }

    pub(crate) fn raise(&self) {
        if !self.closed.load(Ordering::SeqCst) && !self.raised.swap(true, Ordering::SeqCst) {
            self.waker.wake();
        }
    }

    /// Сброс до чтения канала: всё, что отправлено после, либо прочитается в этом
    /// pump, либо снова поднимет флаг и разбудит UI.
    fn reset(&self) {
        self.raised.swap(false, Ordering::SeqCst);
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn waker(&self) -> Arc<dyn Waker> {
        self.waker.clone()
    }
}

/// Отправка `Msg` в UI-поток. Clone + Send.
#[derive(Clone)]
pub struct MsgSink {
    tx: Sender<Box<dyn Msg>>,
    flag: Arc<WakeFlag>,
}

/// Ошибка [`MsgSink::send`]; сообщение отбрасывается.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("send cancelled")]
    Cancelled,
    #[error("message channel closed")]
    Closed,
}

/// Ошибка [`MsgSink::try_send`]; сообщение возвращается.
pub enum TrySendError {
    Full(Box<dyn Msg>),
    Closed(Box<dyn Msg>),
}

impl fmt::Debug for TrySendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrySendError::Full(m) => f.debug_tuple("Full").field(&m.name()).finish(),
            TrySendError::Closed(m) => f.debug_tuple("Closed").field(&m.name()).finish(),
        }
    }
}

/// Шаг проверки отмены в блокирующем `send`. Меньше 50 мс из S1 §0, чтобы с учётом
/// планировщика разблокировка укладывалась в 60 мс (приёмка S1-05).
const CANCEL_CHECK: Duration = Duration::from_millis(40);

impl MsgSink {
    /// Блокирует при полном канале (бэкпрешер), проверяя `cancel` не реже раза в 40 мс.
    /// Уже отменённый `cancel` — `Err(Cancelled)` без отправки. Закрытый канал
    /// (shutdown) — `Err(Closed)` сразу, в том числе для уже заблокированного вызова.
    ///
    /// Не вызывать из UI-потока: на полном канале это deadlock (`debug_assert!`).
    /// Там — `try_send`, а внутри handler — `Cx`.
    pub fn send(&self, m: Box<dyn Msg>, cancel: &CancelToken) -> Result<(), SendError> {
        debug_assert_ne!(
            thread::current().id(),
            self.flag.ui_thread,
            "MsgSink::send from the UI thread"
        );
        let mut m = m;
        loop {
            if cancel.is_cancelled() {
                return Err(SendError::Cancelled);
            }
            match self.tx.send_timeout(m, CANCEL_CHECK) {
                Ok(()) => {
                    self.flag.raise();
                    return Ok(());
                }
                Err(SendTimeoutError::Timeout(back)) => m = back,
                Err(SendTimeoutError::Disconnected(_)) => return Err(SendError::Closed),
            }
        }
    }

    pub fn try_send(&self, m: Box<dyn Msg>) -> Result<(), TrySendError> {
        match self.tx.try_send(m) {
            Ok(()) => {
                self.flag.raise();
                Ok(())
            }
            Err(ChanTrySendError::Full(m)) => Err(TrySendError::Full(m)),
            Err(ChanTrySendError::Disconnected(m)) => Err(TrySendError::Closed(m)),
        }
    }

    /// Разбудить UI без `Msg` (S0 v3 №7): тот же `WakeFlag`, без аллокации и без
    /// места в канале. После shutdown — ничего не делает.
    pub fn poke(&self) {
        self.flag.raise();
    }
}

impl fmt::Debug for MsgSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgSink")
            .field("len", &self.tx.len())
            .finish_non_exhaustive()
    }
}

/// Приёмный конец канала `Msg`. Владеет UI-поток (Core).
pub struct MsgInbox {
    rx: Receiver<Box<dyn Msg>>,
    flag: Arc<WakeFlag>,
}

impl MsgInbox {
    /// Начало pump: сбросить `WakeFlag`. Вызывать до чтения сообщений.
    pub fn begin_pump(&self) {
        self.flag.reset();
    }

    pub fn try_recv(&self) -> Option<Box<dyn Msg>> {
        self.rx.try_recv().ok()
    }

    pub fn len(&self) -> usize {
        self.rx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }
}

impl fmt::Debug for MsgInbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MsgInbox")
            .field("len", &self.rx.len())
            .finish_non_exhaustive()
    }
}

pub(crate) fn channel(capacity: usize, flag: Arc<WakeFlag>) -> (MsgSink, MsgInbox) {
    let (tx, rx) = crossbeam_channel::bounded(capacity.max(1));
    (
        MsgSink {
            tx,
            flag: flag.clone(),
        },
        MsgInbox { rx, flag },
    )
}

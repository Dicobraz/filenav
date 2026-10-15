//! Inline-запросы (S1 §2.7): описание, очередь `Prompts`, ответ `Reply`.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use compact_str::CompactString;
use crossbeam_channel::{Receiver, Sender};
use smallvec::SmallVec;

use crate::entry::Entry;
use crate::ids::{JobId, PromptId, Side};

/// Где показывать запрос.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    Panel(Side),
    Cmdline,
    JobBar,
}

/// Вариант ответа.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: CompactString,
    pub label: CompactString,
    pub key: char,
}

/// Дополнительные данные запроса для UI (S0 v2 №16); расширение — через S0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDetail {
    Conflict {
        src: Entry,
        dst: Entry,
        index: u64,
        total: u64,
    },
}

/// Описание inline-запроса.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptSpec {
    pub anchor: Anchor,
    pub title: String,
    pub body: String,
    pub choices: SmallVec<[Choice; 4]>,
    /// Индекс варианта по умолчанию.
    pub default: u8,
    /// Поле ввода с начальным значением.
    pub input: Option<String>,
    /// Флажок «для всех».
    pub allow_all: bool,
    pub detail: Option<PromptDetail>,
    /// Ввод маскируется, не логируется и не попадает в историю и сессию (S0 v5 №7).
    pub secret: bool,
}

/// Ответ на запрос. `Debug` не показывает `input`: он может быть секретом.
#[derive(Clone, PartialEq, Eq)]
pub struct Reply {
    pub choice: CompactString,
    pub input: Option<String>,
    /// «Для всех».
    pub all: bool,
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reply")
            .field("choice", &self.choice)
            .field("input", &self.input.as_ref().map(|_| "<redacted>"))
            .field("all", &self.all)
            .finish()
    }
}

/// Открытый запрос.
pub struct Prompt {
    pub id: PromptId,
    pub spec: PromptSpec,
    /// Задача, которая ждёт ответа.
    pub job: Option<JobId>,
    reply: Sender<Reply>,
}

impl fmt::Debug for Prompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prompt")
            .field("id", &self.id)
            .field("job", &self.job)
            .field("title", &self.spec.title)
            .finish_non_exhaustive()
    }
}

static NEXT_PROMPT: AtomicU64 = AtomicU64::new(1);

/// Уникальный id запроса; воркер получает его до открытия запроса в UI.
pub(crate) fn next_prompt_id() -> PromptId {
    PromptId(NEXT_PROMPT.fetch_add(1, Ordering::Relaxed))
}

/// Очередь запросов: показывается первый.
#[derive(Debug, Default)]
pub struct Prompts {
    queue: VecDeque<Prompt>,
}

impl Prompts {
    /// Открыть запрос; ответ придёт в `Receiver` (`Disconnected` — отменён).
    pub fn open(&mut self, spec: PromptSpec, job: Option<JobId>) -> (PromptId, Receiver<Reply>) {
        let id = next_prompt_id();
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.open_with(id, spec, job, tx);
        (id, rx)
    }

    pub(crate) fn open_with(
        &mut self,
        id: PromptId,
        spec: PromptSpec,
        job: Option<JobId>,
        reply: Sender<Reply>,
    ) {
        self.queue.push_back(Prompt {
            id,
            spec,
            job,
            reply,
        });
    }

    pub fn active(&self) -> Option<&Prompt> {
        self.queue.front()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Prompt> {
        self.queue.iter()
    }

    pub fn get(&self, id: PromptId) -> Option<&Prompt> {
        self.queue.iter().find(|p| p.id == id)
    }

    /// Ответить и закрыть. `false` — такого запроса нет.
    pub fn reply(&mut self, id: PromptId, r: Reply) -> bool {
        let Some(i) = self.queue.iter().position(|p| p.id == id) else {
            return false;
        };
        let p = self.queue.remove(i).expect("index from position");
        // Ждущий мог уже уйти (отмена) — ответ просто теряется.
        let _ = p.reply.try_send(r);
        true
    }

    /// Закрыть без ответа: ждущий получает `Disconnected` → `Cancelled`.
    pub fn cancel(&mut self, id: PromptId) {
        self.queue.retain(|p| p.id != id);
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

//! Политика чанков для core (S4 §2.2): первый чанк — сразу после первого
//! системного батча (первые строки на экране), дальше — не меньше 2048 записей
//! или раз в 8 мс. Одна на все backend'ы и ОС. Промежуточных пересортов нет:
//! финальный порядок многочанкового листинга считает core (ADR-0017).

use std::time::{Duration, Instant};

/// Записей в чанке после первого.
pub const CHUNK: usize = 2048;
/// Чанк уходит не реже этого, даже если записей меньше [`CHUNK`].
pub const MAX_DELAY: Duration = Duration::from_millis(8);

#[derive(Debug)]
pub struct Batcher {
    sent_first: bool,
    last: Instant,
}

impl Default for Batcher {
    fn default() -> Self {
        Batcher {
            sent_first: false,
            last: Instant::now(),
        }
    }
}

impl Batcher {
    /// Пора ли отправить `pending` записей (`done` — поток кончился).
    pub fn should_flush(&self, pending: usize, done: bool) -> bool {
        pending > 0
            && (done || !self.sent_first || pending >= CHUNK || self.last.elapsed() >= MAX_DELAY)
    }

    /// Чанк отправлен.
    pub fn flushed(&mut self) {
        self.sent_first = true;
        self.last = Instant::now();
    }

    pub fn is_first(&self) -> bool {
        !self.sent_first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_chunk_goes_immediately_then_by_size_or_time() {
        let mut b = Batcher::default();
        assert!(!b.should_flush(0, false));
        assert!(b.should_flush(1, false), "первый — сразу");
        b.flushed();
        assert!(!b.should_flush(10, false));
        assert!(b.should_flush(CHUNK, false));
        assert!(b.should_flush(10, true), "конец потока");
        std::thread::sleep(MAX_DELAY + Duration::from_millis(2));
        assert!(b.should_flush(10, false), "по времени");
    }
}

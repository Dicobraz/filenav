//! Политика чанков для core (S4 §2.2): первый чанк — сразу после первого
//! системного батча (первые строки на экране), дальше — не меньше 2048 записей
//! или раз в 8 мс. Одна на все backend'ы и ОС.

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

/// Первая полная сортировка при стриминге — на стольких записях; дальше — на
/// каждом удвоении (S4 §2.2 п.5): верх списка перестраивается несколько раз, а не
/// на каждый чанк, суммарно O(n log n).
pub const RESORT_FIRST: usize = CHUNK;

/// Когда пересортировать загружаемый каталог.
#[derive(Debug)]
pub struct Resort {
    next: usize,
    sorted_for: usize,
}

impl Default for Resort {
    fn default() -> Self {
        Resort {
            next: RESORT_FIRST,
            sorted_for: 0,
        }
    }
}

impl Resort {
    /// Пора ли полной сортировки при `len` записях.
    pub fn due(&self, len: usize) -> bool {
        len >= self.next
    }

    /// Отсортировано при `len` записях.
    pub fn sorted(&mut self, len: usize) {
        self.sorted_for = len;
        while self.next <= len {
            self.next *= 2;
        }
    }

    /// Нужна ли финальная сортировка на конце (последняя была не на `len`).
    pub fn final_due(&self, len: usize) -> bool {
        self.sorted_for != len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn million_entries_sort_at_most_ten_times() {
        let mut r = Resort::default();
        let mut sorts = 0;
        let mut len = 0;
        // Первый чанк ~256, дальше по CHUNK.
        for add in std::iter::once(256).chain(std::iter::repeat(CHUNK)) {
            len = (len + add).min(1_000_000);
            if r.due(len) {
                r.sorted(len);
                sorts += 1;
            }
            if len == 1_000_000 {
                break;
            }
        }
        if r.final_due(len) {
            sorts += 1;
        }
        assert!(sorts <= 10, "{sorts}");
        assert!(sorts >= 8, "{sorts}");
    }

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

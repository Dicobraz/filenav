//! AIMD числа работающих воркеров (S5 §3.3, S5-19): каждые 500 мс по
//! пропускной способности `bytes + items × 4 КБ` в секунду. Рост больше 5 % —
//! на одного больше (до `2 × n0` и не больше запущенных потоков), падение
//! больше 15 % — `n × 3/4`. На HDD — всегда один. Лишние воркеры ждут в gate
//! операции, потоки не убиваются.

/// Вес файла в пропускной способности: мелкие файлы — тоже работа.
pub(crate) const ITEM_WEIGHT: u64 = 4 << 10;

/// Тиков паузы после неудачной пробы.
const HOLD: u32 = 10;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Aimd {
    n: usize,
    max: usize,
    fixed: bool,
    prev: Option<f64>,
    /// Направление пробы: +1 или −1.
    dir: i8,
    /// Прошлый шаг изменил `n` (его итог ещё не оценён).
    moved: bool,
    hold: u32,
}

impl Aimd {
    /// Старт с `n0` (по носителю); `limit` — запущено потоков; `fixed` — HDD.
    pub(crate) fn new(n0: usize, limit: usize, fixed: bool) -> Aimd {
        let n0 = n0.clamp(1, limit.max(1));
        Aimd {
            n: n0,
            max: (n0 * 2).min(limit.max(1)),
            fixed,
            prev: None,
            dir: 1,
            moved: false,
            hold: 0,
        }
    }

    fn shift(&mut self) {
        let m = if self.dir > 0 {
            (self.n + 1).min(self.max)
        } else {
            self.n.saturating_sub(1).max(1)
        };
        self.moved = m != self.n;
        if !self.moved {
            self.dir = -self.dir;
        }
        self.n = m;
    }

    /// Новый замер пропускной способности → сколько воркеров работает дальше.
    /// Сдвиг в сторону `dir`, пока он даёт больше 5 %; без выигрыша — шаг
    /// назад, пауза и проба в другую сторону; падение больше 15 % после роста —
    /// `n × 3/4` (S5 §3.3).
    pub(crate) fn step(&mut self, rate: f64) -> usize {
        if self.fixed {
            return self.n;
        }
        let Some(p) = self.prev else {
            self.prev = Some(rate);
            self.shift();
            return self.n;
        };
        if self.moved && self.dir > 0 && rate < p * 0.85 {
            self.n = (self.n * 3 / 4).max(1);
            self.dir = -1;
        } else if self.moved && rate > p * 1.05 {
            self.shift();
        } else if self.moved {
            // Сдвиг не окупился: назад и подождать.
            self.n = if self.dir > 0 { self.n - 1 } else { self.n + 1 }.clamp(1, self.max);
            self.dir = -self.dir;
            self.moved = false;
            self.hold = HOLD;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.shift();
        }
        self.prev = Some(rate);
        self.n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Устройство: линейный рост до `k` воркеров, дальше — потери на конкуренции.
    fn device(n: usize, k: usize) -> f64 {
        #[allow(clippy::cast_precision_loss, reason = "модель")]
        let (n, k) = (n as f64, k as f64);
        if n <= k {
            n * 100.0
        } else {
            (k * 100.0 * (1.0 - 0.08 * (n - k))).max(10.0)
        }
    }

    /// Приёмка S5-19 (симулятор): AIMD не хуже лучшего фиксированного N − 5 %.
    #[test]
    fn aimd_reaches_best_fixed_minus_5_percent() {
        for (n0, limit, k) in [(4, 16, 6), (8, 16, 12), (16, 16, 4), (2, 16, 2), (1, 16, 9)] {
            let best = (1..=limit).map(|n| device(n, k)).fold(0.0, f64::max);
            let mut a = Aimd::new(n0, limit, false);
            let mut n = n0;
            let mut sum = 0.0;
            let steps = 60;
            for i in 0..steps {
                let rate = device(n, k);
                n = a.step(rate);
                // Первые шаги — разгон; среднее — по установившемуся режиму.
                if i >= 20 {
                    sum += device(n, k);
                }
            }
            #[allow(clippy::cast_precision_loss, reason = "среднее")]
            let avg = sum / f64::from(steps - 20);
            let reach = (n0 * 2).min(limit);
            let best_reachable = (1..=reach).map(|m| device(m, k)).fold(0.0, f64::max);
            assert!(
                avg >= best_reachable * 0.95,
                "n0 {n0}, k {k}: avg {avg:.0} < 95% of best reachable {best_reachable:.0} (best {best:.0}), n {n}"
            );
        }
    }

    #[test]
    fn hdd_fixed_and_backoff() {
        let mut h = Aimd::new(1, 16, true);
        for r in [100.0, 500.0, 10.0] {
            assert_eq!(h.step(r), 1);
        }
        let mut a = Aimd::new(8, 16, false);
        assert_eq!(a.step(1000.0), 9, "first sample: probe up");
        assert_eq!(a.step(500.0), 6, "drop >15% after growth → ×3/4");
        let mut b = Aimd::new(8, 16, false);
        b.step(1000.0);
        assert_eq!(b.step(1200.0), 10, "growth >5% → one more");
        assert_eq!(b.step(1210.0), 9, "no gain → step back");
        assert_eq!(b.step(1205.0), 9, "then hold");
    }
}

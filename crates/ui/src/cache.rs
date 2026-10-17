//! `RowCache` (S2 §6.2, S2-09): разметка ячеек списка (`Arc<Galley>`) по ключу
//! `(таб, поколение, элемент, колонка, ширина)` и отпечатку записи. Повторный кадр —
//! только поиск по хэшу, без аллокаций; `Patch.update` меняет отпечаток — промах
//! только у изменённых ячеек. Цвет текста подставляется при отрисовке.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, FontId, Galley};
use filenav_core::{Entry, Gen, TabId};
use rustc_hash::{FxHashMap, FxHasher};

use crate::fmt_cell::Cell;

/// Ёмкость по умолчанию.
const CAPACITY: usize = 4096;

/// Ключ ячейки (ADR-0010: `generation`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub tab: TabId,
    pub generation: Gen,
    /// Индекс в `Listing.entries`.
    pub idx: u32,
    /// Колонка (`ColKind as u8`).
    pub col: u8,
    /// Ширина колонки в физических пикселях, квантованная по 4.
    pub width_px: u16,
}

/// Как размечать ячейку на промахе.
#[derive(Clone, Debug)]
pub struct CellSpec {
    pub font: FontId,
    /// Обрезка многоточием; `f32::INFINITY` — без обрезки.
    pub max_width: f32,
    pub group: u8,
    pub icon: u16,
    /// Эмуляция tabular-цифр: ширина слота цифры (S2 §0).
    pub digit_slot: Option<f32>,
}

/// Размеченная ячейка.
#[derive(Clone, Debug)]
pub struct CellEntry {
    pub fp: u64,
    pub galley: Arc<Galley>,
    pub group: u8,
    pub icon: u16,
    /// Сдвиг текста от начала ячейки (первая цифра в слоте) и ширина по слотам.
    pub lead: f32,
    pub width: f32,
    last_frame: u32,
}

/// Счётчики для тестов и перф-лога.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

#[derive(Debug)]
pub struct RowCache {
    map: FxHashMap<CellKey, CellEntry>,
    frame: u32,
    /// Обращений в текущем кадре — оценка числа видимых ячеек.
    used: usize,
    /// Эпохи, при которых собран кэш: язык, стиль (тема, шрифты, масштаб).
    epochs: (u64, u64),
    /// Иконки: общий galley на глиф и кегль, цвет — при отрисовке (S2 §6.3).
    icons: FxHashMap<(char, u32), Arc<Galley>>,
    pub stats: CacheStats,
}

impl Default for RowCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Отпечаток записи: меняется при `Patch.update` (размер, время, атрибуты, вид, имя).
pub fn fingerprint(e: &Entry) -> u64 {
    let mut h = FxHasher::default();
    e.size.hash(&mut h);
    e.mtime.hash(&mut h);
    e.attrs.0.hash(&mut h);
    (e.kind as u8).hash(&mut h);
    e.name.as_str().hash(&mut h);
    h.finish()
}

impl RowCache {
    pub fn new() -> Self {
        Self {
            map: FxHashMap::with_capacity_and_hasher(CAPACITY, rustc_hash::FxBuildHasher),
            frame: 0,
            used: 0,
            epochs: (0, 0),
            icons: FxHashMap::default(),
            stats: CacheStats::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Полный сброс при смене языка или стиля (тема, шрифты, `pixels_per_point`).
    pub fn sync_epochs(&mut self, i18n: u64, style: u64) {
        if self.epochs != (i18n, style) {
            self.epochs = (i18n, style);
            self.map.clear();
            self.icons.clear();
        }
    }

    /// Galley глифа иконки (≤ 40 штук на стиль).
    pub fn icon(&mut self, ctx: &egui::Context, glyph: char, font: &FontId) -> Arc<Galley> {
        let key = (glyph, font.size.to_bits());
        if let Some(g) = self.icons.get(&key) {
            return Arc::clone(g);
        }
        let mut buf = [0u8; 4];
        let text = glyph.encode_utf8(&mut buf).to_owned();
        let g = ctx.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), Color32::WHITE));
        self.icons.insert(key, Arc::clone(&g));
        g
    }

    /// Ячейка из кэша; на промахе `f` пишет текст в стековый [`Cell`] и даёт
    /// [`CellSpec`], разметка — через шрифты `ctx`.
    pub fn get_or_layout(
        &mut self,
        key: CellKey,
        fp: u64,
        ctx: &egui::Context,
        f: impl FnOnce(&mut Cell) -> CellSpec,
    ) -> &CellEntry {
        self.used += 1;
        let frame = self.frame;
        let hit = self.map.get(&key).is_some_and(|e| e.fp == fp);
        if hit {
            self.stats.hits += 1;
        } else {
            self.stats.misses += 1;
            let mut text = Cell::new();
            let spec = f(&mut text);
            let (galley, lead, width) = layout(ctx, text.as_str(), &spec);
            self.map.insert(
                key,
                CellEntry {
                    fp,
                    galley,
                    group: spec.group,
                    icon: spec.icon,
                    lead,
                    width,
                    last_frame: frame,
                },
            );
        }
        let e = self.map.get_mut(&key).expect("present after insert");
        e.last_frame = frame;
        e
    }

    /// Конец кадра: при `len > 4 × видимых` — выбросить не тронутое за 2 кадра.
    pub fn end_frame(&mut self) {
        if self.map.len() > 4 * self.used.max(1) {
            let keep_from = self.frame.saturating_sub(2);
            self.map.retain(|_, e| e.last_frame >= keep_from);
        }
        self.frame = self.frame.wrapping_add(1);
        self.used = 0;
    }
}

/// Разметка: одна секция с обрезкой `…`, либо по символу с интервалами слотов цифр.
fn layout(ctx: &egui::Context, text: &str, spec: &CellSpec) -> (Arc<Galley>, f32, f32) {
    // Цвет — заглушка: при отрисовке подменяется (`galley_with_override_text_color`).
    let color = Color32::WHITE;
    ctx.fonts_mut(|fonts| match spec.digit_slot {
        None => {
            let mut job = LayoutJob::single_section(
                text.to_owned(),
                TextFormat::simple(spec.font.clone(), color),
            );
            if spec.max_width.is_finite() {
                job.wrap = TextWrapping {
                    max_width: spec.max_width.max(0.0),
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
            }
            let g = fonts.layout_job(job);
            let w = g.size().x;
            (g, 0.0, w)
        }
        Some(slot) => {
            // Цифра — по центру слота: интервал после символа = правый отступ его слота
            // + левый отступ следующего; левый отступ первого — сдвигом `lead`.
            let chars: Vec<(char, f32)> = text
                .chars()
                .map(|c| {
                    let w = fonts.glyph_width(&spec.font, c);
                    let s = if c.is_ascii_digit() { slot } else { w };
                    (c, (s - w) / 2.0)
                })
                .collect();
            let mut job = LayoutJob::default();
            let mut buf = [0u8; 4];
            let mut width = 0.0;
            for (i, &(c, pad)) in chars.iter().enumerate() {
                let next = chars.get(i + 1).map_or(0.0, |&(_, p)| p);
                let w = fonts.glyph_width(&spec.font, c);
                width += w + 2.0 * pad;
                job.append(
                    c.encode_utf8(&mut buf),
                    0.0,
                    TextFormat {
                        extra_letter_spacing: pad + next,
                        ..TextFormat::simple(spec.font.clone(), color)
                    },
                );
            }
            let lead = chars.first().map_or(0.0, |&(_, p)| p);
            (fonts.layout_job(job), lead, width)
        }
    })
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

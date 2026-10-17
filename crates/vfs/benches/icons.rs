//! `icon_class` (S4-18, S9 L.10): < 20 нс на вызов.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use filenav_core::{Attrs, EntryKind};
use filenav_vfs::icons::icon_class;

fn bench(c: &mut Criterion) {
    let names: Vec<String> = (0..1024)
        .map(|i| {
            let ext = [
                "txt", "RS", "png", "zip", "exe", "pdf", "qwerty", "", "tar.gz", "JPEG",
            ][i % 10];
            if ext.is_empty() {
                format!("file{i}")
            } else {
                format!("file{i}.{ext}")
            }
        })
        .collect();
    let mut g = c.benchmark_group("icons");
    g.throughput(Throughput::Elements(names.len() as u64));
    g.bench_function("icon_class", |b| {
        b.iter(|| {
            for n in &names {
                black_box(icon_class(black_box(n), EntryKind::File, Attrs::default()));
            }
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);

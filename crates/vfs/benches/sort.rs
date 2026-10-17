//! Natural sort (S4-04): `natural_cmp` на ASCII-именах — цель ≥ 100 M сравнений/с.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use filenav_vfs::sort::{natural_cmp, prefix_key};

/// Имена как в реальных каталогах: общий префикс, расширения, номера.
fn names(n: u32) -> Vec<String> {
    let exts = ["txt", "rs", "toml", "md", "png", "zip", "exe", "log"];
    (0..n)
        .map(|i| {
            let x = i.wrapping_mul(2_654_435_761);
            format!(
                "{}-{:04}.{}",
                ["file", "report", "IMG", "data", "Notes"][(x % 5) as usize],
                x % 10_000,
                exts[(x >> 8) as usize % exts.len()]
            )
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let v = names(4096);
    let pairs: Vec<(&str, &str)> = v
        .iter()
        .zip(v.iter().rev())
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let mut g = c.benchmark_group("natural");
    g.throughput(Throughput::Elements(pairs.len() as u64));
    g.bench_function("natural_cmp ascii", |b| {
        b.iter(|| {
            for &(x, y) in &pairs {
                black_box(natural_cmp(black_box(x), black_box(y)));
            }
        });
    });
    g.bench_function("prefix_key", |b| {
        b.iter(|| {
            for &(x, _) in &pairs {
                black_box(prefix_key(black_box(x), true));
            }
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);

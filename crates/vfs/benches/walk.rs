//! Обход дерева (S4-17, S4 §7): 100k файлов в 5k каталогов — размер каталогов.
//! Дерево — датасет `walk-100k` (`%LOCALAPPDATA%\Pets\testdata\urfin`, H-01): создаётся
//! один раз и переиспользуется.

use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use urfin_core::{CancelToken, EntryKind, Location};
use urfin_testdata::{Layout, TreeSpec, dataset};
use urfin_vfs::{LocalFs, walk};

fn tree() -> Location {
    let spec = TreeSpec {
        n: 100_000,
        seed: 1,
        layout: Layout::Nav { dirs: 5000 },
    };
    Location::local(dataset("urfin", "walk-100k", &spec).expect("dataset walk-100k"))
}

fn size(threads: usize, root: &Location) -> u64 {
    let total = AtomicU64::new(0);
    walk::walk(
        &LocalFs::new(),
        std::slice::from_ref(root),
        threads,
        &CancelToken::new(),
        &|| false,
        &|_, _, e| {
            if e.kind == EntryKind::File {
                total.fetch_add(e.size, Ordering::Relaxed);
            }
        },
        &|_| {},
        || {},
        Duration::from_millis(100),
    );
    total.into_inner()
}

fn bench(c: &mut Criterion) {
    let root = tree();
    let mut g = c.benchmark_group("walk 100k/5k");
    g.sample_size(20);
    for threads in [1, 4] {
        g.bench_function(format!("{threads} thread(s)"), |b| {
            b.iter(|| black_box(size(threads, &root)));
        });
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);

//! Листинг каталога 100k (S4-07): `t_done` тёплый < 60 мс на стенде.
//! Каталог создаётся один раз в temp (`urfin-bench-100k`) и переиспользуется.

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use urfin_core::{Boot, CancelToken, Command, Core, ListingStatus, Location, Services, Waker};
use urfin_vfs::{ListMode, ListOpts, LocalFs, Vfs, VfsCfg, VfsListing, VfsRegistry};

struct NoWake;
impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

const N: usize = 100_000;

/// 100k пустых файлов — датасет `%LOCALAPPDATA%\Pets\testdata\urfin\list-100k`
/// (создаётся один раз, H-01).
fn dir() -> PathBuf {
    urfin_testdata::dataset_with("urfin", "list-100k", "empty files v1 n=100000", |d| {
        for i in 0..N {
            std::fs::write(d.join(format!("file-{i:06}.txt")), b"")?;
        }
        Ok(())
    })
    .expect("dataset list-100k")
}

fn drain(fs: &LocalFs, loc: &Location) -> usize {
    let mut s = fs.list(loc, &ListOpts::default()).unwrap();
    let mut out = Vec::with_capacity(N);
    let cancel = CancelToken::new();
    while s.next_batch(&mut out, &cancel).unwrap() {}
    out.len()
}

fn bench(c: &mut Criterion) {
    let loc = Location::local(dir());
    let mut g = c.benchmark_group("list 100k");
    g.sample_size(20);
    let modes = if cfg!(windows) {
        vec![
            ("nt", ListMode::Nt),
            ("find", ListMode::Find),
            ("std", ListMode::Std),
        ]
    } else {
        vec![("std", ListMode::Std)]
    };
    for (name, mode) in modes {
        let fs = LocalFs::with_mode(mode);
        assert_eq!(drain(&fs, &loc), N);
        g.bench_function(name, |b| b.iter(|| black_box(drain(&fs, &loc))));
    }
    g.finish();

    // t_done: от panel.go до Complete в core (чанки, пересорт, Order, Done).
    let empty_dir = tempfile::Builder::new()
        .prefix("urfin-bench-empty-")
        .tempdir()
        .unwrap();
    let empty = empty_dir.path().to_path_buf();
    let services = Services {
        listing: Arc::new(VfsListing::new(
            Arc::new(VfsRegistry::with_local()),
            VfsCfg::default(),
        )),
        open_resolvers: Vec::new(),
    };
    let start = Location::local(&empty);
    let mut core = Core::new(
        Boot::new(start.clone(), start.clone()),
        services,
        Arc::new(NoWake),
    );
    let mut g = c.benchmark_group("t_done 100k");
    g.sample_size(20);
    g.bench_function("vfs listing via core", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                core.dispatch(Command::new("panel.go").arg("path", start.clone()));
                core.pump(Duration::from_millis(50));
                let t0 = Instant::now();
                core.dispatch(Command::new("panel.go").arg("path", loc.clone()));
                loop {
                    core.pump(Duration::from_millis(5));
                    let s = core.state();
                    if matches!(
                        s.panel(s.active).tab().listing.status,
                        ListingStatus::Complete
                    ) {
                        break;
                    }
                    std::hint::spin_loop();
                }
                total += t0.elapsed();
            }
            total
        });
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);

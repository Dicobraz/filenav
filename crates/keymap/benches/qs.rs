//! Бюджет S3-06: шаг быстрого поиска на 100k записей <1 мс (команда целиком, через Core).

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use urfin_core::fake::FakeListing;
use urfin_core::{Boot, Command, Core, ListingStatus, Location, Services, Waker};

struct NoWake;

impl Waker for NoWake {
    fn wake(&self) {}
    fn wake_after(&self, _d: Duration) {}
}

fn core() -> Core {
    let root = Location::local(std::env::temp_dir().join("urfin-keymap-qs-bench"));
    let fake = FakeListing::new(1 << 16);
    fake.set_dir(
        root.clone(),
        (0..100_000)
            .map(|i| FakeListing::file(&format!("file{i:05}.txt"), 1))
            .collect(),
    );
    let mut core = Core::new(
        Boot::new(root.clone(), root),
        Services {
            listing: Arc::new(fake),
            ..Services::null()
        },
        Arc::new(NoWake),
    );
    let _ = urfin_keymap::register(
        &mut core,
        urfin_core::Paths::in_dir(std::env::temp_dir().join("urfin-keymap-qs-bench")),
        urfin_keymap::KeymapCfg::default(),
        urfin_keymap::FlagRegistry::with_builtin(),
    );
    while core.state().active_tab().listing.status != ListingStatus::Complete {
        core.pump(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(1));
    }
    core
}

fn bench(c: &mut Criterion) {
    let mut core = core();
    let cmd = |s: &str| Command::parse(s).unwrap();
    core.dispatch(cmd("keymap.qs.start text=file4"));
    // В кавычках: `text=2` разобралось бы как число.
    let (input, miss, back) = (
        cmd(r#"keymap.qs.input text="2""#),
        cmd(r#"keymap.qs.input text="z""#),
        cmd("keymap.qs.backspace"),
    );
    // Символ принят (1000 совпадений) — команда и перемещение курсора; стирание не в счёт.
    c.bench_function("qs step 100k (hit)", |b| {
        b.iter_custom(|n| {
            let mut total = Duration::ZERO;
            for _ in 0..n {
                let t = Instant::now();
                core.dispatch(black_box(input.clone()));
                total += t.elapsed();
                core.dispatch(back.clone());
            }
            total
        });
    });
    let names: Vec<String> = (0..100_000).map(|i| format!("file{i:05}.txt")).collect();
    let m = urfin_keymap::handler::Matcher::new("f").unwrap();
    c.bench_function("matcher 100k", |b| {
        b.iter(|| names.iter().filter(|n| m.matches(black_box(n))).count());
    });
    // Первый символ: кэша нет, подходят все 100k строк.
    let (start, first) = (cmd("keymap.qs.start"), cmd("keymap.qs.input text=f"));
    c.bench_function("qs first char 100k", |b| {
        b.iter_custom(|n| {
            let mut total = Duration::ZERO;
            for _ in 0..n {
                core.dispatch(start.clone());
                let t = Instant::now();
                core.dispatch(black_box(first.clone()));
                total += t.elapsed();
            }
            total
        });
    });
    core.dispatch(cmd("keymap.qs.start text=file4"));
    // Нет совпадения среди кандидатов.
    c.bench_function("qs step 100k (miss)", |b| {
        b.iter(|| core.dispatch(black_box(miss.clone())));
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);

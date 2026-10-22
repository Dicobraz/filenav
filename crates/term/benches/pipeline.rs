//! Бюджет S6-04: разбор вывода ≥ 100 МБ/с в release — `yes`-подобный поток
//! (строка за строкой со скроллом) и цветной вывод вида `ls --color`; рядом —
//! голый `alacritty_terminal` на том же потоке (накладные расходы pipeline). Бюджет
//! S6-06: снапшот экрана 200×60 для рендера — доля от кадра ≤ 2 мс.
//!
//! Гибридный CPU стенда: на E-ядрах в разы медленнее — мерить, привязав процесс
//! к P-ядрам (`docs/perf/s6-04-pipeline.md`).

use std::hint::black_box;
use std::io::Cursor;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use urfin_term::session::{SessionEvent, SessionOpts, SessionSink, parse_inline};
use urfin_term::sys::conpty::PtySize;

struct Null;

impl SessionSink for Null {
    fn poke(&self) {}
    fn event(&self, _ev: SessionEvent) {}
}

const MB: usize = 1 << 20;

fn opts() -> SessionOpts {
    SessionOpts {
        size: PtySize {
            cols: 200,
            rows: 60,
        },
        scrollback: 10_000,
        osc52: "copy".into(),
        nonce: None,
    }
}

fn stream(line: &[u8], bytes: usize) -> Vec<u8> {
    line.repeat(bytes / line.len())
}

fn bench(c: &mut Criterion) {
    let mut g = c.benchmark_group("parse");
    g.sample_size(10);
    let yes = stream(b"y\r\n", 16 * MB);
    g.throughput(Throughput::Bytes(yes.len() as u64));
    // Базовая линия: голый эмулятор без сканера, блокировок и событий.
    g.bench_function("yes_16mb_bare_alacritty", |b| {
        b.iter(|| {
            let o = opts();
            let size = TermSize::new(usize::from(o.size.cols), usize::from(o.size.rows));
            let config = Config {
                scrolling_history: o.scrollback as usize,
                ..Config::default()
            };
            let mut term = Term::new(config, &size, VoidListener);
            let mut p: Processor = Processor::new();
            for c in yes.chunks(64 * 1024) {
                p.advance(&mut term, c);
            }
            black_box(term.grid().cursor.point)
        });
    });
    g.bench_function("yes_16mb", |b| {
        b.iter(|| black_box(parse_inline(Cursor::new(&yes), &opts(), &Null)));
    });
    let ls = stream(
        b"\x1b[0m\x1b[01;34mdirectory-name\x1b[0m  \x1b[01;32mrun.sh\x1b[0m  plain-file.txt  \x1b[38;5;208mweird\x1b[0m  \xd1\x84\xd0\xb0\xd0\xb9\xd0\xbb.rs\r\n",
        16 * MB,
    );
    g.throughput(Throughput::Bytes(ls.len() as u64));
    g.bench_function("ls_color_16mb", |b| {
        b.iter(|| black_box(parse_inline(Cursor::new(&ls), &opts(), &Null)));
    });
    g.finish();
    // Снапшот экрана для рендера (S6-06): 200×60 цветного вывода, под замком эмулятора.
    let screen = parse_inline(Cursor::new(&ls[..256 * 1024]), &opts(), &Null);
    let d = urfin_term::vt::grid::Defaults {
        fg: [0xcc, 0xcc, 0xcc],
        bg: [0x0c, 0x0c, 0x0c],
        cursor: [0xff, 0xff, 0xff],
    };
    c.bench_function("grid/snapshot_200x60", |b| {
        b.iter(|| black_box(screen.snapshot(&d)));
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);

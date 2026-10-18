//! Дерево для бенчей листинга (S4 §7):
//! `cargo run -p urfin-testdata --example gen_tree --release -- --n 100000 --seed 1 --out <каталог> [--nav 1000]`.

use std::path::PathBuf;
use std::time::Instant;

use urfin_testdata::{Layout, TreeSpec, gen_tree};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut n, mut seed, mut out, mut layout) = (100_000usize, 1u64, None, Layout::Flat);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("value after flag");
        match a.as_str() {
            "--n" => n = val().parse().expect("--n <число>"),
            "--seed" => seed = val().parse().expect("--seed <число>"),
            "--out" => out = Some(PathBuf::from(val())),
            "--nav" => {
                layout = Layout::Nav {
                    dirs: val().parse().expect("--nav <каталогов>"),
                }
            }
            other => panic!("unknown argument {other}"),
        }
    }
    let out = out.expect("--out <каталог>");
    let t = Instant::now();
    let r = gen_tree(&out, &TreeSpec { n, seed, layout }).expect("gen_tree");
    println!(
        "{} in {:.1} s: {r:?}",
        out.display(),
        t.elapsed().as_secs_f64()
    );
}

//! Датасеты Urfin (H-01): `cargo run -p urfin-testdata --example testdata -- clean` —
//! удалить все (`%LOCALAPPDATA%\Pets\testdata\urfin`); `-- path` — показать корень.

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let root = urfin_testdata::datasets_root("urfin");
    match cmd.as_str() {
        "clean" => {
            let bytes = urfin_testdata::clean("urfin").expect("clean");
            println!("{}: removed {} MB", root.display(), bytes / 1_000_000);
        }
        "path" => println!("{}", root.display()),
        _ => {
            eprintln!("usage: testdata clean | path");
            std::process::exit(2);
        }
    }
}

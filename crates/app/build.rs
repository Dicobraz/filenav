//! Ресурсы exe на Windows (R-01): VERSIONINFO с именем продукта и издателем
//! (S0-decisions №45, №49). Версии файла и продукта winresource берёт из Cargo.
//! Иконка приложения — ресурс с id 1 (`set_icon`): её показывают Проводник,
//! ярлыки и панель задач; GPUI берёт её же для класса окна.

const ICON: &str = "../../assets/icon/urfin.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICON}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ICON);
        res.set("ProductName", "Urfin")
            .set("CompanyName", "Aahz")
            .set("FileDescription", "Urfin — files and settings")
            .set("InternalName", "urfin")
            .set("OriginalFilename", "urfin.exe");
        if let Err(e) = res.compile() {
            panic!("exe resources (rc.exe from Windows SDK): {e}");
        }
    }
}

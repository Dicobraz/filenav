use egui::{Context, FontId, RawInput};

use super::*;

/// Кадр без окна: после него шрифты контекста готовы к запросам.
fn frame(ctx: &Context) {
    let mut out = ctx.run_ui(RawInput::default(), |_| {});
    out.textures_delta.clear();
}

#[cfg(windows)]
fn width(ctx: &Context, id: &FontId, text: &str, slot: Option<f32>) -> f32 {
    match slot {
        Some(slot) => tabular_layout(text, slot, |c| ctx.fonts_mut(|f| f.glyph_width(id, c))).1,
        None => text
            .chars()
            .map(|c| ctx.fonts_mut(|f| f.glyph_width(id, c)))
            .sum(),
    }
}

#[test]
fn tabular_layout_centers_digits_in_slots() {
    let w = |c: char| if c == '1' { 4.0 } else { 8.0 };
    let (glyphs, total) = tabular_layout("1,8", 8.0, w);
    assert_eq!(total, 8.0 + 8.0 + 8.0);
    assert_eq!(glyphs, [('1', 2.0), (',', 8.0), ('8', 16.0)]);
    assert_eq!(
        tabular_layout("111", 8.0, w).1,
        tabular_layout("888", 8.0, w).1
    );
}

#[test]
fn fallback_to_egui_fonts_without_install() {
    let fs = FontSet::default();
    assert!(!fs.installed());
    assert_eq!(fs.id(Font::Strong, 14.0).family, FontFamily::Proportional);
    assert_eq!(fs.id(Font::Mono, 13.0).family, FontFamily::Monospace);
    assert!(fs.tabular_ok(Font::Ui), "до проверки — без эмуляции");
}

#[test]
fn has_table_reads_table_directory() {
    let mut font = vec![0u8; 12 + 32];
    font[4..6].copy_from_slice(&2u16.to_be_bytes());
    font[12..16].copy_from_slice(b"cmap");
    font[28..32].copy_from_slice(b"fvar");
    assert!(has_table(&font, *b"fvar"));
    assert!(!has_table(&font, *b"glyf"));
    assert!(!has_table(&[0; 3], *b"fvar"));
}

#[test]
fn user_path_keeps_system_values() {
    assert_eq!(user_path("system"), None);
    assert_eq!(
        user_path(" jetbrains-mono "),
        None,
        "прежнее значение по умолчанию"
    );
    assert_eq!(user_path(r"D:\f.ttf"), Some(PathBuf::from(r"D:\f.ttf")));
}

#[test]
fn missing_user_font_warns_and_falls_back() {
    let cfg = UiCfg {
        font_ui: "Z:/no/such.ttf".into(),
        ..UiCfg::default()
    };
    let (loaded, warnings) = load(&FontFiles::default(), &cfg);
    assert!(loaded.ui.is_none());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].key, "ui.font_ui");
    // Без файлов — шрифты egui, без паники.
    let ctx = Context::default();
    let fs = install(&ctx, &loaded);
    frame(&ctx);
    assert!(!fs.icons);
    assert!(ctx.fonts_mut(|f| f.has_glyphs(&fs.id(Font::Strong, 14.0), "Привет")));
}

#[cfg(windows)]
mod system {
    use super::*;
    use crate::icons;

    fn system_files() -> FontFiles {
        crate::sys::probe_system().fonts
    }

    fn face(file: &str) -> Face {
        let dir =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        Face::read(&dir.join("Fonts").join(file)).unwrap()
    }

    #[test]
    fn every_icon_glyph_in_both_system_icon_fonts() {
        for file in ["SegoeIcons.ttf", "segmdl2.ttf"] {
            let dir = std::env::var_os("WINDIR")
                .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
            if !dir.join("Fonts").join(file).is_file() {
                eprintln!("{file}: нет в системе (Server/Win10) — пропуск");
                continue;
            }
            let loaded = LoadedFonts {
                icons: Some(face(file)),
                ..LoadedFonts::default()
            };
            let ctx = Context::default();
            let fs = install(&ctx, &loaded);
            frame(&ctx);
            let id = fs.id(Font::Icons, 16.0);
            // Только сам иконочный шрифт, без фолбэков egui.
            let missing: Vec<String> = icons::ALL
                .iter()
                .filter(|c| !ctx.fonts_mut(|f| f.has_glyph(&id, **c)))
                .map(|c| format!("{:X}", u32::from(*c)))
                .collect();
            assert_eq!(missing, Vec::<String>::new(), "{file}");
        }
    }

    #[test]
    fn ui_families_cover_cyrillic_and_strong_is_semibold() {
        let (loaded, warnings) = load(&system_files(), &UiCfg::default());
        assert_eq!(warnings, []);
        let ctx = Context::default();
        let fs = install(&ctx, &loaded);
        frame(&ctx);
        // Mono не проверяем: в Cascadia Mono есть U+FFFD, а `has_glyph` egui сравнивает
        // с гранью символа замены и для такого шрифта всегда даёт `false` (TODO в epaint).
        for font in [Font::Ui, Font::Strong, Font::Caption, Font::Subtitle] {
            let id = fs.id(font, 14.0);
            assert!(
                ctx.fonts_mut(|f| f.has_glyphs(&id, "Съешь ещё этих мягких булок — Ёё…")),
                "{font:?}"
            );
        }
        // Semibold шире Regular того же кегля: ось wght (или статический Semibold) работает.
        let w = |font| width(&ctx, &fs.id(font, 14.0), "Wwm", None);
        assert!(
            w(Font::Strong) > w(Font::Ui) * 1.02,
            "{} vs {}",
            w(Font::Strong),
            w(Font::Ui)
        );
        fs.warm_up(&ctx, &[(Font::Ui, 14.0)], &icons::all_text());
        assert!(fs.tabular.get().is_some());
    }

    /// Эмуляция tabular на системном шрифте с нетабличными цифрами.
    #[test]
    fn tabular_emulation_on_proportional_digits() {
        let ctx = Context::default();
        let candidate = [
            "georgia.ttf",
            "segoepr.ttf",
            "comic.ttf",
            "gabriola.ttf",
            "pala.ttf",
        ]
        .into_iter()
        .find_map(|file| {
            let dir = std::env::var_os("WINDIR")
                .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
            dir.join("Fonts").join(file).is_file().then(|| face(file))
        });
        let Some(face) = candidate else {
            eprintln!("нет шрифта с нетабличными цифрами — пропуск");
            return;
        };
        let loaded = LoadedFonts {
            ui: Some(face),
            ..LoadedFonts::default()
        };
        let fs = install(&ctx, &loaded);
        frame(&ctx);
        let id = fs.id(Font::Ui, 14.0);
        let natural = (width(&ctx, &id, "111", None), width(&ctx, &id, "888", None));
        if (natural.0 - natural.1).abs() < 0.01 {
            eprintln!("цифры табличные — пропуск");
            return;
        }
        fs.warm_up(&ctx, &[(Font::Ui, 14.0)], "");
        assert!(!fs.tabular_ok(Font::Ui));
        let slot = FontSet::digit_slot(&ctx, &id);
        let emulated = (
            width(&ctx, &id, "111", Some(slot)),
            width(&ctx, &id, "888", Some(slot)),
        );
        assert!(
            (emulated.0 - emulated.1).abs() < 0.01,
            "{emulated:?} (natural {natural:?})"
        );
    }

    #[test]
    fn variable_segoe_detected() {
        let files = system_files();
        let (loaded, _) = load(&files, &UiCfg::default());
        if files.ui.len() == 1 {
            assert!(loaded.variable(), "{:?}", files.ui);
        }
        assert!(loaded.mono.is_some(), "Cascadia Mono или Consolas");
    }
}

use proptest::prelude::*;

use super::*;

const OSES: [Os; 3] = [Os::Windows, Os::Macos, Linux];
use Os::Linux;

/// Наборы модификаторов без противоречий: для Ctrl и Alt — нет / любая / левая / правая.
fn mod_sets() -> Vec<Mods> {
    let ctrl = [Mods::empty(), Mods::CTRL, Mods::LCTRL, Mods::RCTRL];
    let alt = [Mods::empty(), Mods::ALT, Mods::LALT, Mods::RALT];
    let mut out = Vec::new();
    for c in ctrl {
        for a in alt {
            for shift in [Mods::empty(), Mods::SHIFT] {
                for cmd in [Mods::empty(), Mods::CMD] {
                    out.push(c | a | shift | cmd);
                }
            }
        }
    }
    out
}

fn all_keys() -> Vec<KeyCode> {
    egui::Key::ALL
        .iter()
        .map(|k| KeyCode::Key(*k))
        .chain(OWN.iter().map(|(_, c)| *c))
        .collect()
}

/// `parse(display(x)) == x` для всех клавиш egui и своих × всех наборов модификаторов.
#[test]
fn roundtrip_all_keys_and_mods() {
    let mods = mod_sets();
    for key in all_keys() {
        for &m in &mods {
            let s = Stroke::new(key, m);
            for os in OSES {
                let text = s.display(os);
                assert_eq!(Stroke::parse(&text, os), Ok(s), "{text}");
            }
        }
    }
}

proptest! {
    /// Аккорд из случайных сочетаний: разбор канонической формы, в том числе в другом
    /// регистре и с лишними пробелами.
    #[test]
    fn chord_roundtrip(
        picks in prop::collection::vec((0usize..1000, 0usize..64), 1..4),
        upper in any::<bool>(),
    ) {
        let keys = all_keys();
        let mods = mod_sets();
        let chord = Chord(
            picks
                .iter()
                .map(|&(k, m)| Stroke::new(keys[k % keys.len()], mods[m % mods.len()]))
                .collect(),
        );
        let text = chord.display(Os::Windows);
        let text = if upper { text.to_uppercase() } else { text.to_lowercase() };
        let spaced = format!("  {}  ", text.replace(' ', "   "));
        prop_assert_eq!(Chord::parse(&spaced, Os::Windows), Ok(chord));
    }
}

#[test]
fn synonyms_and_case() {
    let p = |s| Stroke::parse(s, Os::Windows).unwrap();
    let ctrl_t = Stroke::new(KeyCode::Key(egui::Key::T), Mods::CTRL);
    assert_eq!(p("ctrl+t"), ctrl_t);
    assert_eq!(p("CONTROL+T"), ctrl_t);
    let cmd = Stroke::new(KeyCode::Key(egui::Key::K), Mods::CMD);
    assert_eq!(p("Cmd+K"), cmd);
    assert_eq!(p("meta+k"), cmd);
    assert_eq!(p("Super+K"), cmd);
    assert_eq!(
        p("esc"),
        Stroke::new(KeyCode::Key(egui::Key::Escape), Mods::empty())
    );
    assert_eq!(p("Escape").display(Os::Windows), "Esc");
    assert_eq!(p("pagedown"), p("PageDown"));
    assert_eq!(p("ArrowUp"), p("Up"));
    assert_eq!(p("Shift+F12").display(Os::Windows), "Shift+F12");
    assert_eq!(
        p("shift+alt+ctrl+cmd+a").display(Os::Windows),
        "Ctrl+Alt+Shift+Cmd+A"
    );
    assert_eq!(p("LCtrl+RAlt+X").display(Os::Windows), "LCtrl+RAlt+X");
    assert_eq!(
        p("["),
        Stroke::new(KeyCode::Key(egui::Key::OpenBracket), Mods::empty())
    );
}

/// `Mod` — Ctrl на Windows и Linux, Cmd на macOS.
#[test]
fn mod_follows_os() {
    let o = KeyCode::Key(egui::Key::O);
    assert_eq!(
        Stroke::parse("Mod+O", Os::Windows).unwrap().mods,
        Mods::CTRL
    );
    assert_eq!(Stroke::parse("Mod+O", Os::Linux).unwrap().mods, Mods::CTRL);
    assert_eq!(
        Stroke::parse("mod+o", Os::Macos).unwrap(),
        Stroke::new(o, Mods::CMD)
    );
}

/// Серый `+` цифрового блока и `+` основной клавиатуры — разные клавиши.
#[test]
fn gray_plus_is_not_plus() {
    let gray = Stroke::parse("GrayPlus", Os::Windows).unwrap();
    let plus = Stroke::parse("Plus", Os::Windows).unwrap();
    assert_eq!(gray.key, KeyCode::GrayPlus);
    assert_eq!(plus.key, KeyCode::Key(egui::Key::Plus));
    assert_ne!(gray, plus);
    assert_eq!(Stroke::parse("+", Os::Windows).unwrap(), plus);
    assert_eq!(
        Stroke::parse("Ctrl+Plus", Os::Windows).unwrap().key,
        plus.key
    );
    assert_ne!(
        Stroke::parse("Num5", Os::Windows).unwrap(),
        Stroke::parse("5", Os::Windows).unwrap()
    );
}

/// Неверные строки — `KeyError` с позицией в исходной строке.
#[test]
fn errors_have_positions() {
    let e = |s: &str| Chord::parse(s, Os::Windows).unwrap_err();
    let unknown = e("Ctrl+Nope");
    assert_eq!(unknown.pos, 5);
    assert_eq!(unknown.kind, KeyErrorKind::UnknownKey("Nope".into()));
    assert_eq!(e("Ctrl+").pos, 5);
    assert_eq!(e("Ctrl+").kind, KeyErrorKind::Empty);
    assert_eq!(e("A+B").kind, KeyErrorKind::NotModifier("A".into()));
    assert_eq!(e("Ctrl+ctrl+A").kind, KeyErrorKind::Repeated("ctrl".into()));
    assert_eq!(e("Ctrl+LCtrl+A").kind, KeyErrorKind::Sides("LCtrl".into()));
    assert_eq!(e("   ").kind, KeyErrorKind::Empty);
    // Второе сочетание аккорда: позиция — от начала всей строки.
    let second = e("Ctrl+K  Ctrl+Bogus");
    assert_eq!(second.pos, 13);
    assert_eq!(second.to_string(), "at 13: unknown key 'Bogus'");
}

#[test]
fn chord_parts() {
    let c = Chord::parse("Ctrl+K Ctrl+C", Os::Windows).unwrap();
    assert_eq!(c.0.len(), 2);
    assert_eq!(c.display(Os::Windows), "Ctrl+K Ctrl+C");
}

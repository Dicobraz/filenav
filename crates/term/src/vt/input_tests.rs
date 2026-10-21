//! Табличные эталоны xterm (приёмка S6-05: не меньше 150).

use super::*;

const S: Mods = Mods {
    shift: true,
    alt: false,
    ctrl: false,
};
const A: Mods = Mods {
    shift: false,
    alt: true,
    ctrl: false,
};
const C: Mods = Mods {
    shift: false,
    alt: false,
    ctrl: true,
};

fn m(shift: bool, alt: bool, ctrl: bool) -> Mods {
    Mods { shift, alt, ctrl }
}

fn enc(name: &str, mods: Mods, text: Option<&str>, term: Modes) -> Vec<u8> {
    let mut out = Vec::new();
    key(name, mods, text, term, &mut out);
    out
}

/// Эталонные строки (`\x1b` → `^[` для читаемости сообщений).
fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).replace('\x1b', "^[")
}

#[test]
#[allow(clippy::too_many_lines, reason = "таблица эталонов")]
fn xterm_reference_table() {
    let norm = Modes::default();
    let app = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    let mut cases: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    let mut push =
        |label: String, got: Vec<u8>, want: &[u8]| cases.push((label, got, want.to_vec()));

    // Стрелки, Home, End: normal / APP_CURSOR / 7 сочетаний модификаторов.
    for (k, c) in [
        ("up", 'A'),
        ("down", 'B'),
        ("right", 'C'),
        ("left", 'D'),
        ("home", 'H'),
        ("end", 'F'),
    ] {
        push(
            k.to_string(),
            enc(k, Mods::NONE, None, norm),
            format!("\x1b[{c}").as_bytes(),
        );
        push(
            format!("app {k}"),
            enc(k, Mods::NONE, None, app),
            format!("\x1bO{c}").as_bytes(),
        );
        for (s, a, ct) in [
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
            (true, false, true),
            (false, true, true),
            (true, true, true),
        ] {
            let p = 1 + u8::from(s) + 2 * u8::from(a) + 4 * u8::from(ct);
            push(
                format!("{k} {s}{a}{ct}"),
                enc(k, m(s, a, ct), None, app),
                format!("\x1b[1;{p}{c}").as_bytes(),
            );
        }
    }
    // F1–F4 и F5–F12 с модификаторами.
    for (k, c) in [("f1", 'P'), ("f2", 'Q'), ("f3", 'R'), ("f4", 'S')] {
        push(
            k.into(),
            enc(k, Mods::NONE, None, norm),
            format!("\x1bO{c}").as_bytes(),
        );
        for (mods, p) in [(S, 2), (A, 3), (C, 5), (m(true, false, true), 6)] {
            push(
                format!("{k}+{p}"),
                enc(k, mods, None, norm),
                format!("\x1b[1;{p}{c}").as_bytes(),
            );
        }
    }
    for (k, n) in [
        ("f5", 15),
        ("f6", 17),
        ("f7", 18),
        ("f8", 19),
        ("f9", 20),
        ("f10", 21),
        ("f11", 23),
        ("f12", 24),
        ("insert", 2),
        ("delete", 3),
        ("pageup", 5),
        ("pagedown", 6),
    ] {
        push(
            k.into(),
            enc(k, Mods::NONE, None, norm),
            format!("\x1b[{n}~").as_bytes(),
        );
        for (mods, p) in [
            (S, 2),
            (A, 3),
            (C, 5),
            (m(true, false, true), 6),
            (m(false, true, true), 7),
        ] {
            push(
                format!("{k}+{p}"),
                enc(k, mods, None, norm),
                format!("\x1b[{n};{p}~").as_bytes(),
            );
        }
    }
    // Ctrl+буква → C0, Alt → ESC-префикс.
    for c in 'a'..='z' {
        let s = c.to_string();
        push(
            format!("C-{c}"),
            enc(&s, C, Some(&s), norm),
            &[c as u8 - b'a' + 1],
        );
        push(
            format!("M-{c}"),
            enc(&s, A, Some(&s), norm),
            &[0x1b, c as u8],
        );
    }
    for (k, b) in [
        ("space", 0u8),
        ("[", 0x1b),
        ("\\", 0x1c),
        ("]", 0x1d),
        ("2", 0),
        ("6", 0x1e),
        ("-", 0x1f),
        ("8", 0x7f),
    ] {
        push(format!("C-{k}"), enc(k, C, None, norm), &[b]);
    }
    // Enter, Tab, Backspace, Escape.
    let simple: [(&str, Mods, &[u8]); 10] = [
        ("enter", Mods::NONE, b"\r"),
        ("enter", A, b"\x1b\r"),
        ("tab", Mods::NONE, b"\t"),
        ("tab", S, b"\x1b[Z"),
        ("tab", A, b"\x1b\t"),
        ("backspace", Mods::NONE, b"\x7f"),
        ("backspace", C, b"\x08"),
        ("backspace", A, b"\x1b\x7f"),
        ("escape", Mods::NONE, b"\x1b"),
        ("escape", A, b"\x1b\x1b"),
    ];
    for (k, mods, want) in simple {
        push(format!("{k} {mods:?}"), enc(k, mods, None, norm), want);
    }
    // Печатный текст, Shift уже в тексте; кириллица — UTF-8.
    push("text a".into(), enc("a", Mods::NONE, Some("a"), norm), b"a");
    push("text A".into(), enc("a", S, Some("A"), norm), b"A");
    push(
        "text ж".into(),
        enc("ж", Mods::NONE, Some("ж"), norm),
        "ж".as_bytes(),
    );
    push(
        "M-ж".into(),
        enc("ж", A, Some("ж"), norm),
        &[&[0x1b][..], "ж".as_bytes()].concat(),
    );
    // AltGr (Ctrl+Alt с текстом) — сам символ.
    push(
        "AltGr @".into(),
        enc("q", m(false, true, true), Some("@"), norm),
        b"@",
    );
    push(
        "AltGr €".into(),
        enc("e", m(false, true, true), Some("€"), norm),
        "€".as_bytes(),
    );

    let total = cases.len();
    let bad: Vec<String> = cases
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(l, got, want)| format!("{l}: got {:?}, want {:?}", show(got), show(want)))
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {total} wrong:\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(total >= 150, "reference table: {total} < 150");
}

#[test]
fn paste_focus_mouse() {
    let mut out = Vec::new();
    paste("a\r\nb\nc", Modes::default(), &mut out);
    assert_eq!(out, b"a\rb\rc");
    out.clear();
    let br = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    paste("x\x1b[201~y", br, &mut out);
    assert_eq!(
        out, b"\x1b[200~x[201~y\x1b[201~",
        "ESC inside cannot close the brackets"
    );

    out.clear();
    focus(true, Modes::default(), &mut out);
    assert!(out.is_empty(), "no focus reports unless enabled");
    let f = Modes {
        focus_events: true,
        ..Modes::default()
    };
    focus(true, f, &mut out);
    focus(false, f, &mut out);
    assert_eq!(out, b"\x1b[I\x1b[O");

    let ev = |button, pressed, col, row| MouseEvent {
        button,
        pressed,
        motion: false,
        col,
        row,
        mods: Mods::NONE,
    };
    let sgr = Modes {
        sgr_mouse: true,
        ..Modes::default()
    };
    out.clear();
    mouse(ev(MouseButton::Left, true, 10, 5), sgr, &mut out);
    mouse(ev(MouseButton::Left, false, 10, 5), sgr, &mut out);
    mouse(ev(MouseButton::WheelUp, true, 300, 400), sgr, &mut out);
    assert_eq!(out, b"\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<64;300;400M");
    out.clear();
    assert!(mouse(
        ev(MouseButton::Right, true, 1, 1),
        Modes::default(),
        &mut out
    ));
    assert!(mouse(
        ev(MouseButton::Right, false, 1, 1),
        Modes::default(),
        &mut out
    ));
    assert_eq!(
        out,
        [0x1b, b'[', b'M', 34, 33, 33, 0x1b, b'[', b'M', 35, 33, 33]
    );
    assert!(
        !mouse(
            ev(MouseButton::Left, true, 300, 1),
            Modes::default(),
            &mut out
        ),
        "X10 limit"
    );
}
